//! CSV sink stage for recording voltage EEG data to files.

use crate::config::StageConfig;
use crate::control::ControlCommand;
use crate::data::RtPacket;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Drains, Stage, StageContext, StageInitCtx};
use chrono::Local;
use flume::Receiver;
use serde::Deserialize;
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A factory for creating `CsvSink` stages.
#[derive(Default)]
pub struct CsvSinkFactory;

impl StageFactory for CsvSinkFactory {
    fn create(
        &self,
        config: &StageConfig,
        _: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let params: CsvSinkParams = serde_json::from_value(serde_json::Value::Object(
            config.params.clone().into_iter().collect(),
        ))?;
        Ok((Box::new(CsvSink::new(config.name.clone(), params)?), None))
    }
}

/// A sink stage that writes incoming data to a CSV file.
pub struct CsvSink {
    id: String,
    params: CsvSinkParams,
    writer: Mutex<Option<BufWriter<File>>>,
    header_written: Mutex<bool>,
    is_recording: Mutex<bool>,
    start_time: Mutex<Option<Instant>>,
    last_frame_id: Mutex<Option<u64>>,
}

impl CsvSink {
    pub fn new(id: String, params: CsvSinkParams) -> Result<Self, StageError> {
        // Ensure the base directory exists
        if let Some(parent) = PathBuf::from(&params.path).parent() {
            if !parent.exists() {
                create_dir_all(parent).map_err(|e| {
                    StageError::Fatal(format!("Failed to create recordings directory: {}", e))
                })?;
            }
        }

        Ok(Self {
            id,
            params,
            writer: Mutex::new(None),
            header_written: Mutex::new(false),
            is_recording: Mutex::new(false),
            start_time: Mutex::new(None),
            last_frame_id: Mutex::new(None),
        })
    }

    fn create_new_writer(&self) -> Result<BufWriter<File>, StageError> {
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        let filename = format!(
            "{}_{}_{}.csv",
            self.params.path.trim_end_matches(".csv"),
            "0", // Hardcoded session
            timestamp
        );
        let recordings_dir = PathBuf::from("./recordings");
        if !recordings_dir.exists() {
            create_dir_all(&recordings_dir).map_err(|e| {
                StageError::Fatal(format!("Failed to create recordings directory: {}", e))
            })?;
        }
        let full_path = recordings_dir.join(filename);

        let file = File::create(&full_path).map_err(|e| {
            StageError::Fatal(format!("Failed to create CSV file {:?}: {}", full_path, e))
        })?;
        Ok(BufWriter::new(file))
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CsvSinkParams {
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub max_recording_length_minutes: Option<u32>,
}

fn default_path() -> String {
    "e2e_test_output.csv".to_string()
}

impl Stage for CsvSink {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        if let RtPacket::RawAndVoltage(packet) = &*packet {
            let is_recording = self.is_recording.lock().unwrap();
            if !*is_recording {
                return Ok(vec![]); // Not recording, so drop the packet.
            }

            let mut writer_opt = self.writer.lock().unwrap();
            let mut header_written = self.header_written.lock().unwrap();
            let mut start_time = self.start_time.lock().unwrap();
            let mut last_frame_id = self.last_frame_id.lock().unwrap();

            // Frame gap detection
            if let Some(last_id) = *last_frame_id {
                if packet.header.frame_id != last_id + 1 {
                    // Stop recording and return a fatal error
                    *self.is_recording.lock().unwrap() = false;
                    return Err(StageError::Fatal(format!(
                        "Data gap detected! Expected frame {}, but got {}. Recording stopped.",
                        last_id + 1,
                        packet.header.frame_id
                    )));
                }
            }
            *last_frame_id = Some(packet.header.frame_id);

            // Check for file rotation
            if let (Some(st), Some(max_mins)) =
                (*start_time, self.params.max_recording_length_minutes)
            {
                if st.elapsed() >= Duration::from_secs(max_mins as u64 * 60) {
                    *writer_opt = None; // Force creation of a new writer
                    *header_written = false;
                }
            }

            if writer_opt.is_none() {
                *writer_opt = Some(self.create_new_writer()?);
                *start_time = Some(Instant::now());
            }

            let writer = writer_opt.as_mut().unwrap();

            if !*header_written {
                let mut header = "timestamp".to_string();
                for i in 0..packet.samples.len() {
                    header.push_str(&format!(",ch{}_voltage,ch{}_raw_sample", i, i));
                }
                writeln!(writer, "{}", header)
                    .map_err(|e| StageError::Fatal(format!("Failed to write CSV header: {}", e)))?;
                *header_written = true;
            }

            let mut line = packet.header.ts_ns.to_string();
            for (raw, voltage) in &*packet.samples {
                line.push_str(&format!(",{},{}", voltage, raw));
            }
            writeln!(writer, "{}", line)
                .map_err(|e| StageError::Fatal(format!("Failed to write to CSV: {}", e)))?;
        }
        Ok(vec![])
    }

    fn is_locked(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }

    fn control(&mut self, cmd: &ControlCommand, _ctx: &mut StageContext) -> Result<(), StageError> {
        match cmd {
            ControlCommand::StartRecording => {
                let mut is_recording = self.is_recording.lock().unwrap();
                if !*is_recording {
                    *is_recording = true;
                    // Defer writer creation to the process method
                }
            }
            ControlCommand::StopRecording => {
                let mut is_recording = self.is_recording.lock().unwrap();
                if *is_recording {
                    *is_recording = false;
                    if let Some(mut writer) = self.writer.lock().unwrap().take() {
                        writer.flush().ok();
                    }
                    *self.start_time.lock().unwrap() = None;
                    *self.last_frame_id.lock().unwrap() = None;
                }
            }
            _ => {} // Ignore other commands
        }
        Ok(())
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        Some(self)
    }
}

impl Drains for CsvSink {
    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(writer) = self.writer.lock().unwrap().as_mut() {
            writer.flush()?;
        }
        Ok(())
    }
}