use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::File;
use std::io;
use chrono::{Local, DateTime};
use csv::Writer;
use std::time::Instant;
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, warn, error};

use eeg_types::{
    event::{SensorEvent, EegPacket, EventFilter},
    config::{DaemonConfig, DriverType},
};
use eeg_types::plugin::{EegPlugin, PluginConfig, EventBus};
use eeg_sensor::AdcConfig;

/// Configuration for the CSV Recorder Plugin
#[derive(Clone, Debug)]
pub struct CsvRecorderConfig {
    pub daemon_config: Arc<DaemonConfig>,
    pub adc_config: AdcConfig,
    pub is_recording_shared: Arc<AtomicBool>,
}

impl Default for CsvRecorderConfig {
    fn default() -> Self {
        Self {
            daemon_config: Arc::new(DaemonConfig::default()),
            adc_config: AdcConfig {
                board_driver: DriverType::MockEeg,
                channels: (0..8).collect(),
                sample_rate: 250,
                gain: 1.0,
                vref: 4.5,
                batch_size: 128,
            },
            is_recording_shared: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl PluginConfig for CsvRecorderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.adc_config.channels.is_empty() {
            return Err(anyhow::anyhow!("ADC config must have at least one channel"));
        }
        if self.adc_config.sample_rate == 0 {
            return Err(anyhow::anyhow!("Sample rate must be greater than 0"));
        }
        Ok(())
    }
    
    fn config_name(&self) -> &str {
        "csv_recorder_config"
    }
}

struct RecorderState {
    writer: Option<Writer<File>>,
    file_path: Option<String>,
    is_recording: bool,
    start_timestamp: Option<u64>,
    last_flush_time: Instant,
    recording_start_time: Option<Instant>,
}

/// CSV Recorder Plugin - handles recording EEG data to CSV files
pub struct CsvRecorderPlugin {
    config: CsvRecorderConfig,
    state: Arc<Mutex<RecorderState>>,
}

impl Clone for CsvRecorderPlugin {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: Arc::new(Mutex::new(RecorderState {
                writer: None,
                file_path: None,
                is_recording: false,
                start_timestamp: None,
                last_flush_time: Instant::now(),
                recording_start_time: None,
            })),
        }
    }
}

impl CsvRecorderPlugin {
    pub fn new() -> Self {
        let state = RecorderState {
            writer: None,
            file_path: None,
            is_recording: false,
            start_timestamp: None,
            last_flush_time: Instant::now(),
            recording_start_time: None,
        };
        Self {
            config: CsvRecorderConfig::default(),
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Start recording to a new CSV file
    async fn start_recording(&self, state: &mut tokio::sync::MutexGuard<'_, RecorderState>) -> io::Result<String> {
        if state.is_recording {
            return Ok(format!("Already recording to {}", state.file_path.clone().unwrap_or_default()));
        }
        
        self.config.is_recording_shared.store(true, Ordering::Relaxed);
        
        let recordings_dir = &self.config.daemon_config.recordings_directory;
        std::fs::create_dir_all(recordings_dir)?;
        
        let now: DateTime<Local> = Local::now();
        let driver = format!("{:?}", self.config.adc_config.board_driver);
        let session_prefix = if self.config.daemon_config.session.is_empty() {
            "".to_string()
        } else {
            format!("session{}_", self.config.daemon_config.session)
        };

        let filename = format!(
            "{}/{}{}_board{}.csv",
            recordings_dir,
            session_prefix,
            now.format("%Y-%m-%d_%H-%M"),
            driver,
        );
        
        let file = File::create(&filename)?;
        let mut writer = csv::Writer::from_writer(file);
        
        let mut header = vec!["timestamp".to_string()];
        for &channel_idx in &self.config.adc_config.channels {
            header.push(format!("ch{}_voltage", channel_idx));
        }
        for &channel_idx in &self.config.adc_config.channels {
            header.push(format!("ch{}_raw_sample", channel_idx));
        }
        
        writer.write_record(&header)?;
        writer.flush()?;
        
        state.writer = Some(writer);
        state.file_path = Some(filename.clone());
        state.is_recording = true;
        state.start_timestamp = None;
        state.recording_start_time = Some(Instant::now());
        
        info!("Started recording to {}", filename);
        Ok(format!("Started recording to {}", filename))
    }
    
    /// Stop recording and close the CSV file
    async fn stop_recording(&self, state: &mut tokio::sync::MutexGuard<'_, RecorderState>) -> io::Result<String> {
        if !state.is_recording {
            return Ok("Not currently recording".to_string());
        }
        
        if let Some(mut writer) = state.writer.take() {
            writer.flush()?;
        }
        
        let file_path = state.file_path.take().unwrap_or_default();
        state.is_recording = false;
        state.start_timestamp = None;
        
        self.config.is_recording_shared.store(false, Ordering::Relaxed);
        
        info!("Stopped recording to {}", file_path);
        Ok(format!("Stopped recording to {}", file_path))
    }
    
    /// Write EEG packet data to the CSV file
    async fn write_eeg_packet(&self, state: &mut tokio::sync::MutexGuard<'_, RecorderState>, packet: &EegPacket) -> io::Result<String> {
        if !state.is_recording || state.writer.is_none() {
            return Ok("Not recording".to_string());
        }
        
        if state.start_timestamp.is_none() {
            state.start_timestamp = packet.timestamps.first().cloned();
        }
        
        let now = Instant::now();
        let should_flush = now.duration_since(state.last_flush_time).as_secs() >= 5;

        let writer = state.writer.as_mut().unwrap();
        let num_channels = self.config.adc_config.channels.len();
        if num_channels == 0 {
            return Ok("No channels configured".to_string());
        }
        let samples_per_channel = packet.voltage_samples.len() / num_channels;
        
        for i in 0..samples_per_channel {
            let mut record = Vec::with_capacity(1 + num_channels * 2);
            let timestamp_idx = i * num_channels;
            let sample_timestamp = packet.timestamps.get(timestamp_idx).cloned().unwrap_or(0);
            record.push(sample_timestamp.to_string());
            
            for channel_idx in 0..num_channels {
                let sample_idx = i * num_channels + channel_idx;
                record.push(packet.voltage_samples.get(sample_idx).cloned().unwrap_or(0.0).to_string());
            }
            
            for channel_idx in 0..num_channels {
                let sample_idx = i * num_channels + channel_idx;
                record.push(packet.raw_samples.get(sample_idx).cloned().unwrap_or(0).to_string());
            }
            
            writer.write_record(&record)?;
        }
        
        if should_flush {
            writer.flush()?;
            state.last_flush_time = now;
        }
        
        if let Some(start_time) = state.recording_start_time {
            if now.duration_since(start_time).as_secs() >= (self.config.daemon_config.max_recording_length_minutes * 60) as u64 {
                let old_file = self.stop_recording(state).await?;
                let new_file = self.start_recording(state).await?;
                return Ok(format!("Maximum recording length reached. {} and {}", old_file, new_file));
            }
        }
        
        Ok("Data written successfully".to_string())
    }
}

#[async_trait]
impl EegPlugin for CsvRecorderPlugin {
    fn name(&self) -> &'static str {
        "csv_recorder"
    }
    
    fn description(&self) -> &'static str {
        "Records EEG data to CSV files with automatic rotation"
    }

    fn clone_box(&self) -> Box<dyn EegPlugin> {
        Box::new(self.clone())
    }
    
    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::RawEegOnly]
    }

    async fn run(
        &mut self,
        _bus: Arc<dyn EventBus>,
        mut receiver: broadcast::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        info!("[{}] Starting CSV recorder plugin", self.name());

        loop {
            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    info!("[{}] Received shutdown signal", self.name());
                    let mut state = self.state.lock().await;
                    if state.is_recording {
                        if let Err(e) = self.stop_recording(&mut state).await {
                            error!("[{}] Error stopping recording during shutdown: {}", self.name(), e);
                        }
                    }
                    break;
                }
                event = receiver.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("[{}] Lagged by {} messages", self.name(), n);
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("[{}] Receiver channel closed.", self.name());
                            break;
                        }
                    };
                    match event {
                        SensorEvent::RawEeg(packet) => {
                            // Only record if recording is enabled
                            let mut state = self.state.lock().await;
                            if self.config.is_recording_shared.load(Ordering::Relaxed) {
                                if !state.is_recording {
                                    if let Err(e) = self.start_recording(&mut state).await {
                                        error!("[{}] Failed to start recording: {}", self.name(), e);
                                        continue;
                                    }
                                }

                                if let Err(e) = self.write_eeg_packet(&mut state, &packet).await {
                                    warn!("[{}] Failed to write data: {}", self.name(), e);
                                }
                            } else if state.is_recording {
                                if let Err(e) = self.stop_recording(&mut state).await {
                                    error!("[{}] Error stopping recording: {}", self.name(), e);
                                }
                            }
                        }
                        _ => {
                            // Ignore other event types
                        }
                    }
                }
            }
        }

        info!("[{}] CSV recorder plugin stopped", self.name());
        Ok(())
    }
}