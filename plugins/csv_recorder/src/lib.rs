use chrono::{DateTime, Local};
use csv::Writer;
use pipeline::control::{ControlCommand, CustomCommand};
use pipeline::data::RtPacket;
use pipeline::error::StageError;
use pipeline::stage::{Stage, StageContext};
use std::fs::File;
use std::io;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum CsvRecorderCommand {
    StartRecording,
    StopRecording,
}

struct RecorderState {
    writer: Option<Writer<File>>,
    file_path: Option<String>,
    is_recording: bool,
}

#[derive(Clone)]
pub struct CsvRecorderPlugin {
    id: String,
    state: Arc<std::sync::Mutex<RecorderState>>,
    num_channels: usize,
    recordings_directory: String,
}

impl CsvRecorderPlugin {
    pub fn new(num_channels: usize, recordings_directory: &str) -> Self {
        let state = RecorderState {
            writer: None,
            file_path: None,
            is_recording: false,
        };
        Self {
            id: Uuid::new_v4().to_string(),
            state: Arc::new(std::sync::Mutex::new(state)),
            num_channels,
            recordings_directory: recordings_directory.to_string(),
        }
    }

    fn start_recording(&self, state: &mut std::sync::MutexGuard<RecorderState>) -> io::Result<()> {
        if state.is_recording {
            return Ok(());
        }

        std::fs::create_dir_all(&self.recordings_directory)?;

        let now: DateTime<Local> = Local::now();
        let filename = format!(
            "{}/{}_elata-v1.csv",
            self.recordings_directory,
            now.format("%Y-%m-%d_%H-%M"),
        );

        let file = File::create(&filename)?;
        let mut writer = csv::Writer::from_writer(file);

        let mut header = vec!["timestamp".to_string()];
        for i in 0..self.num_channels {
            header.push(format!("ch{}_voltage", i));
        }
        writer.write_record(&header)?;
        writer.flush()?;

        state.writer = Some(writer);
        state.file_path = Some(filename);
        state.is_recording = true;

        Ok(())
    }

    fn stop_recording(&self, state: &mut std::sync::MutexGuard<RecorderState>) -> io::Result<()> {
        if !state.is_recording {
            return Ok(());
        }

        if let Some(mut writer) = state.writer.take() {
            writer.flush()?;
        }

        state.is_recording = false;
        state.file_path = None;

        Ok(())
    }
}

impl Stage for CsvRecorderPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn control(&mut self, cmd: &ControlCommand, _ctx: &mut StageContext) -> Result<(), StageError> {
        if let ControlCommand::Custom(custom_cmd) = cmd {
            if let Some(recorder_cmd) = custom_cmd.as_any().downcast_ref::<CsvRecorderCommand>() {
                let mut state = self.state.lock().unwrap();
                match recorder_cmd {
                    CsvRecorderCommand::StartRecording => {
                        self.start_recording(&mut state)
                            .map_err(|e| StageError::Io(e.to_string()))?;
                    }
                    CsvRecorderCommand::StopRecording => {
                        self.stop_recording(&mut state)
                            .map_err(|e| StageError::Io(e.to_string()))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        match &*packet {
            RtPacket::Voltage(data) => {
                let mut state = self.state.lock().unwrap();
                if state.is_recording {
                    if let Some(writer) = state.writer.as_mut() {
                        let samples_per_channel = data.samples.len() / self.num_channels;
                        for i in 0..samples_per_channel {
                            let mut record = Vec::with_capacity(1 + self.num_channels);
                            record.push(data.header.ts_ns.to_string());
                            for ch in 0..self.num_channels {
                                let sample_idx = i * self.num_channels + ch;
                                record.push(data.samples[sample_idx].to_string());
                            }
                            writer
                                .write_record(&record)
                                .map_err(|e| StageError::Io(e.to_string()))?;
                        }
                    }
                }
            }
            // Ignore other packet types
            _ => {}
        }
        // Pass the packet through to the next stage
        Ok(vec![("out".to_string(), packet)])
    }
}