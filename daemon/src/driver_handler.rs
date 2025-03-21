use eeg_driver::{AdcConfig, ProcessedData};
use std::sync::Arc;
use std::fs::File;
use std::io::{self, Write};
use chrono::{Local, DateTime};
use csv::Writer;
use std::time::Instant;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::config::DaemonConfig;


// We'll use ProcessedData directly instead of EegBatchData
#[derive(Clone, Serialize, Debug)]
pub struct EegBatchData {
    pub channels: Vec<Vec<f32>>,  // Each inner Vec represents a channel's data for the batch
    pub timestamp: u64,           // Timestamp for the start of the batch
}

/// Structure to hold CSV recording state
pub struct CsvRecorder {
    writer: Option<Writer<File>>,
    pub file_path: Option<String>,
    pub is_recording: bool,
    start_timestamp: Option<u64>,
    sample_rate: u32,
    last_flush_time: Instant,
    recording_start_time: Option<Instant>,
    config: Arc<DaemonConfig>,
    current_adc_config: AdcConfig,
}

impl CsvRecorder {
    pub fn new(sample_rate: u32, config: Arc<DaemonConfig>, adc_config: AdcConfig) -> Self {
        Self {
            writer: None,
            file_path: None,
            is_recording: false,
            start_timestamp: None,
            sample_rate,
            last_flush_time: Instant::now(),
            recording_start_time: None,
            config,
            current_adc_config: adc_config,
        }
    }
    
    /// Start recording to a new CSV file
    pub fn start_recording(&mut self) -> io::Result<String> {
        if self.is_recording {
            return Ok(format!("Already recording to {}", self.file_path.clone().unwrap_or_default()));
        }
        
        // Debug: Print the recordings directory path from config
        println!("DEBUG: Recordings directory from config: {}", self.config.recordings_directory);
        
        // Get absolute path of the current directory
        let current_dir = match std::env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                println!("ERROR: Failed to get current directory: {}", e);
                return Err(io::Error::new(io::ErrorKind::Other, "Failed to get current directory"));
            }
        };
        println!("DEBUG: Current directory: {:?}", current_dir);
        
        // Determine if we're running from the daemon directory
        let is_in_daemon_dir = current_dir.to_string_lossy().ends_with("/daemon");
        println!("DEBUG: Running from daemon directory: {}", is_in_daemon_dir);
        
        // Adjust the path if needed to ensure it's relative to the repo root
        let recordings_dir = if is_in_daemon_dir && self.config.recordings_directory.starts_with("./") {
            // Convert "./recordings/" to "../recordings/" when running from daemon directory
            let adjusted_path = format!("..{}", &self.config.recordings_directory[1..]);
            println!("DEBUG: Adjusted recordings path: {}", adjusted_path);
            adjusted_path
        } else {
            self.config.recordings_directory.clone()
        };
        
        // Debug: Print the absolute path of the recordings directory
        let absolute_recordings_path = current_dir.join(&recordings_dir);
        println!("DEBUG: Absolute recordings path: {:?}", absolute_recordings_path);
        
        // Create recordings directory if it doesn't exist
        std::fs::create_dir_all(&recordings_dir)?;
        
        // Create filename with current timestamp and parameters
        let now: DateTime<Local> = Local::now();
        let gain = self.current_adc_config.gain;
        let driver = format!("{:?}", self.current_adc_config.board_driver);
        let vref = self.current_adc_config.Vref;
        
        // Get session field from config
        let session_prefix = if self.config.session.is_empty() {
            "".to_string()
        } else {
            format!("session{}_", self.config.session)
        };

        let filename = format!(
            "{}/{}{}_gain{}_board{}_vref{}.csv",
            recordings_dir,
            session_prefix,
            now.format("%Y-%m-%d_%H-%M"),
            gain,
            driver,
            vref
        );
        println!("DEBUG: Creating recording file at: {}", filename);
        
        // Create CSV writer
        let file = File::create(&filename)?;
        let mut writer = csv::Writer::from_writer(file);
        
        // Write header row with both voltage and raw samples
        let mut header = vec!["timestamp".to_string()];
        
        // Add voltage channel headers
        for i in 1..=4 {
            header.push(format!("ch{}_voltage", i));
        }
        
        // Add raw channel headers
        for i in 1..=4 {
            header.push(format!("ch{}_raw_sample", i));
        }
        
        writer.write_record(&header)?;
        writer.flush()?;
        
        self.writer = Some(writer);
        self.file_path = Some(filename.clone());
        self.is_recording = true;
        self.start_timestamp = None; // Will be set when first data arrives
        self.recording_start_time = Some(Instant::now());
        
        Ok(format!("Started recording to {}", filename))
    }
    
    /// Stop recording and close the CSV file
    pub fn stop_recording(&mut self) -> io::Result<String> {
        if !self.is_recording {
            return Ok("Not currently recording".to_string());
        }
        
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }
        
        let file_path = self.file_path.take().unwrap_or_default();
        self.is_recording = false;
        self.start_timestamp = None;
        
        Ok(format!("Stopped recording to {}", file_path))
    }
    
    /// Write a batch of processed EEG data to the CSV file
    pub fn write_data(&mut self, data: &ProcessedData) -> io::Result<String> {
        if !self.is_recording || self.writer.is_none() {
            return Ok("Not recording".to_string());
        }
        
        // Set start timestamp if this is the first data
        if self.start_timestamp.is_none() {
            self.start_timestamp = Some(data.timestamp);
        }
        
        let writer = self.writer.as_mut().unwrap();
        let num_channels = data.processed_voltage_samples.len().min(4); // Limit to 4 channels
        let samples_per_channel = data.processed_voltage_samples[0].len();
        
        // Calculate microseconds per sample based on sample rate
        let us_per_sample = 1_000_000 / self.sample_rate as u64;
        
        // Write each sample as a row
        for i in 0..samples_per_channel {
            let sample_timestamp = data.timestamp + (i as u64 * us_per_sample);
            
            // Create a record with timestamp, voltage values, and raw values
            let mut record = Vec::with_capacity(1 + num_channels * 2); // timestamp + voltage channels + raw channels
            record.push(sample_timestamp.to_string());
            
            // Add voltage values
            for ch in 0..num_channels {
                record.push(data.processed_voltage_samples[ch][i].to_string());
            }
            
            // Pad with zeros if we have fewer than 4 voltage channels
            for _ in num_channels..4 {
                record.push("0.0".to_string());
            }
            
            // Add raw values
            for ch in 0..num_channels {
                record.push(data.raw_samples[ch][i].to_string());
            }
            
            // Pad with zeros if we have fewer than 4 raw channels
            for _ in num_channels..4 {
                record.push("0".to_string());
            }
            
            writer.write_record(&record)?;
        }
        
        // Check if it's time to flush (hardcoded to 5 seconds)
        let now = Instant::now();
        if now.duration_since(self.last_flush_time).as_secs() >= 5 {
            writer.flush()?;
            self.last_flush_time = now;
        }
        
        // Check if we've exceeded the maximum recording length
        if let Some(start_time) = self.recording_start_time {
            if now.duration_since(start_time).as_secs() >= (self.config.max_recording_length_minutes * 60) as u64 {
                // Stop current recording and start a new one
                let old_file = self.stop_recording()?;
                let new_file = self.start_recording()?;
                return Ok(format!("Maximum recording length reached. Stopped recording to {} and started new recording to {}", old_file, new_file));
            }
        }
        
        Ok("Data written successfully".to_string())
    }
}

// Function to process EEG data batches
pub async fn process_eeg_data(
    mut rx_data_from_adc: tokio::sync::mpsc::Receiver<ProcessedData>,
    tx_to_web_socket: tokio::sync::broadcast::Sender<EegBatchData>,
    csv_recorder: Arc<Mutex<CsvRecorder>>,
) {
    let mut count = 0;
    let mut last_time = std::time::Instant::now();
    let mut last_timestamp = None;
    
    while let Some(data) = rx_data_from_adc.recv().await {
        // Write to CSV if recording is active - write the full ProcessedData
        if let Ok(mut recorder) = csv_recorder.try_lock() {
            match recorder.write_data(&data) {
                Ok(msg) => {
                    // Only log if something interesting happened (like auto-rotating files)
                    if msg != "Data written successfully" && msg != "Not recording" {
                        println!("CSV Recording: {}", msg);
                    }
                },
                Err(e) => {
                    println!("Warning: Failed to write data to CSV: {}", e);
                }
            }
        }
        
        // Create smaller batches to send more frequently to WebSocket clients
        // Split the incoming data into chunks of 32 samples
        let batch_size = 32;
        let num_channels = data.processed_voltage_samples.len();
        let samples_per_channel = data.processed_voltage_samples[0].len();
        
        for chunk_start in (0..samples_per_channel).step_by(batch_size) {
            let chunk_end = (chunk_start + batch_size).min(samples_per_channel);
            
            // More efficient chunking - use slices instead of cloning when possible
            // If we need to send the data to multiple clients, we'll still need to clone
            let chunk_timestamp = data.timestamp + (chunk_start as u64 * 4000); // Adjust timestamp for each chunk
            
            // Create EegBatchData for WebSocket clients (they don't need raw samples)
            let mut chunk_channels = Vec::with_capacity(num_channels);
            for channel in &data.processed_voltage_samples {
                // More efficient: pre-allocate and use extend_from_slice
                let mut channel_chunk = Vec::with_capacity(chunk_end - chunk_start);
                channel_chunk.extend_from_slice(&channel[chunk_start..chunk_end]);
                chunk_channels.push(channel_chunk);
            }
            
            let eeg_batch_data = EegBatchData {
                channels: chunk_channels,
                timestamp: chunk_timestamp / 1000, // Convert to milliseconds
            };
            
            if let Err(e) = tx_to_web_socket.send(eeg_batch_data) {
                println!("Warning: Failed to send data chunk to WebSocket clients: {}", e);
            }
        }
        
        count += data.processed_voltage_samples[0].len();
        last_timestamp = Some(data.timestamp);
        
        if let Some(last_ts) = last_timestamp {
            let delta_us = data.timestamp - last_ts;
            let delta_ms = delta_us as f64 / 1000.0;  // Convert to milliseconds for display
            if delta_ms > 5.0 {
                println!("Large timestamp gap detected: {:.2}ms ({} µs)", delta_ms, delta_us);
                println!("Sample count: {}", count);
                println!("Expected time between batches: {:.2}ms", (32_000.0 / 250.0)); // For 32 samples at 250Hz
            }
        }
        
        // Print stats every 250 samples (about 1 second of data at 250Hz)
        if count % 250 == 0 {
            let elapsed = last_time.elapsed();
            let rate = 250.0 / elapsed.as_secs_f32();
            println!("Processing rate: {:.2} Hz", rate);
            println!("Total samples processed: {}", count);
            println!("Sample data (first 5 values from first channel):");
            println!("  1st Channel {:?}", &data.processed_voltage_samples[0]);
            last_time = std::time::Instant::now();
        }
    }
}