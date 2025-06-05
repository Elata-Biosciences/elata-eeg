use eeg_driver::{AdcConfig, ProcessedData};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::File;
use std::io;
use chrono::{Local, DateTime};
use csv::Writer;
use std::time::Instant;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use basic_voltage_filter::SignalProcessor; // Added for Phase 2
use crate::connection_manager::PipelineType; // For demand-based processing

use crate::config::DaemonConfig;


// Re-export EegBatchData from the driver crate to avoid duplication
pub use eeg_driver::EegBatchData;

/// Data structure for the new WebSocket endpoint (/ws/eeg/data__basic_voltage_filter)
/// This will contain data processed by basic_voltage_filter::SignalProcessor
#[derive(Clone, Serialize, Debug)]
pub struct FilteredEegData {
    pub timestamp: u64, // Timestamp for the start of the batch (milliseconds)
    pub raw_samples: Option<Vec<Vec<i32>>>, // Raw samples from the driver
    pub filtered_voltage_samples: Option<Vec<Vec<f32>>>, // Voltage samples after basic_voltage_filter
    pub error: Option<String>, // Optional error message
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
    is_recording_shared: Arc<AtomicBool>,
}

impl CsvRecorder {
    pub fn new(sample_rate: u32, config: Arc<DaemonConfig>, adc_config: AdcConfig, is_recording_shared: Arc<AtomicBool>) -> Self {
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
            is_recording_shared,
        }
    }
    
    /// Update the ADC configuration
    pub fn update_config(&mut self, new_config: AdcConfig) {
        self.current_adc_config = new_config;
    }
    
    /// Start recording to a new CSV file
    pub async fn start_recording(&mut self) -> io::Result<String> {
        if self.is_recording {
            return Ok(format!("Already recording to {}", self.file_path.clone().unwrap_or_default()));
        }
        
        // Update shared recording state
        self.is_recording_shared.store(true, Ordering::Relaxed);
        
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
        let driver = format!("{:?}", self.current_adc_config.board_driver);
        
        // Get session field from config
        let session_prefix = if self.config.session.is_empty() {
            "".to_string()
        } else {
            format!("session{}_", self.config.session)
        };

        let filename = format!(
            "{}/{}{}_board{}.csv",
            recordings_dir,
            session_prefix,
            now.format("%Y-%m-%d_%H-%M"),
            driver,
        );
        println!("DEBUG: Creating recording file at: {}", filename);
        
        // Create CSV writer
        let file = File::create(&filename)?;
        let mut writer = csv::Writer::from_writer(file);
        
        // Write header row with both voltage and raw samples
        let mut header = vec!["timestamp".to_string()];
        
        // Get the actual number of channels from the ADC configuration
        let _channel_count = self.current_adc_config.channels.len();
        
        // Add voltage channel headers using the actual channel indices
        for &channel_idx in &self.current_adc_config.channels {
            header.push(format!("ch{}_voltage", channel_idx));
        }
        
        // Add raw channel headers using the actual channel indices
        for &channel_idx in &self.current_adc_config.channels {
            header.push(format!("ch{}_raw_sample", channel_idx));
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
    pub async fn stop_recording(&mut self) -> io::Result<String> {
        if !self.is_recording {
            return Ok("Not currently recording".to_string());
        }
        
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }
        
        let file_path = self.file_path.take().unwrap_or_default();
        self.is_recording = false;
        self.start_timestamp = None;
        
        // Update shared recording state
        self.is_recording_shared.store(false, Ordering::Relaxed);
        
        Ok(format!("Stopped recording to {}", file_path))
    }
    
    /// Write a batch of processed EEG data to the CSV file
    pub async fn write_data(&mut self, data: &ProcessedData) -> io::Result<String> {
        if !self.is_recording || self.writer.is_none() {
            return Ok("Not recording".to_string());
        }
        
        // Set start timestamp if this is the first data
        if self.start_timestamp.is_none() {
            self.start_timestamp = Some(data.timestamp);
        }
        
        let writer = self.writer.as_mut().unwrap();
        let num_channels = data.voltage_samples.len();
        let samples_per_channel = data.voltage_samples[0].len();
        
        // Calculate microseconds per sample based on sample rate
        let us_per_sample = 1_000_000 / self.sample_rate as u64;
        
        // Write each sample as a row
        for i in 0..samples_per_channel {
            let sample_timestamp = data.timestamp + (i as u64 * us_per_sample);
            
            // Create a record with timestamp, voltage values, and raw values
            let mut record = Vec::with_capacity(1 + num_channels * 2); // timestamp + voltage channels + raw channels
            record.push(sample_timestamp.to_string());
            
            // Map the data to the correct channel indices
            // The data comes in as an array where the index is the position in the array
            // But we need to map it to the specific channel indices in the configuration
            
            // Add voltage values for each configured channel
            for (idx, &_channel_idx) in self.current_adc_config.channels.iter().enumerate() {
                if idx < num_channels {
                    // We have data for this channel
                    record.push(data.voltage_samples[idx][i].to_string());
                } else {
                    // No data for this channel, pad with zero
                    record.push("0.0".to_string());
                }
            }
            
            // Add raw values for each configured channel
            for (idx, &_channel_idx) in self.current_adc_config.channels.iter().enumerate() {
                if idx < num_channels {
                    // We have data for this channel
                    record.push(data.raw_samples[idx][i].to_string());
                } else {
                    // No data for this channel, pad with zero
                    record.push("0".to_string());
                }
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
                // Now that we're in an async function, we can properly handle rotation
                let old_file = self.stop_recording().await?;
                let new_file = self.start_recording().await?;
                return Ok(format!("Maximum recording length reached. Stopped recording to {} and started new recording to {}", old_file, new_file));
            }
        }
        
        Ok("Data written successfully".to_string())
    }
}

// Function to process EEG data batches
pub async fn process_eeg_data(
    mut rx_data_from_adc: tokio::sync::mpsc::Receiver<ProcessedData>,
    tx_to_web_socket: tokio::sync::broadcast::Sender<EegBatchData>, // For existing /eeg endpoint (unfiltered driver output)
    tx_to_filtered_data_web_socket: tokio::sync::broadcast::Sender<FilteredEegData>, // For new filtered data endpoint
    csv_recorder: Arc<Mutex<CsvRecorder>>,
    _is_recording_shared_status: Arc<AtomicBool>, // Renamed, as direct is_recording check is on recorder
    connection_manager: Arc<crate::connection_manager::ConnectionManager>, // For demand-based processing
    cancellation_token: CancellationToken,
) {
    let mut count = 0;
    let mut last_time = std::time::Instant::now();
    // last_timestamp logic was not very effective, removing for now.

    // Initialize SignalProcessor once
    // We need sample_rate and num_channels from AdcConfig, and filter settings from DaemonConfig
    let (daemon_config_clone, adc_config_clone, sample_rate_u32, num_channels_usize) = {
        let recorder_guard = csv_recorder.lock().await;
        let adc_conf = recorder_guard.current_adc_config.clone(); // Clone AdcConfig
        let sample_r = adc_conf.sample_rate; // Keep as u32
        let num_ch = adc_conf.channels.len();
        (recorder_guard.config.clone(), adc_conf, sample_r, num_ch) // Clone DaemonConfig
    };

    let mut signal_processor = SignalProcessor::new(
        sample_rate_u32, // Use u32 directly
        num_channels_usize,
        daemon_config_clone.filter_config.dsp_high_pass_cutoff_hz,
        daemon_config_clone.filter_config.dsp_low_pass_cutoff_hz,
        daemon_config_clone.filter_config.powerline_filter_hz,
    );
    
    loop {
        tokio::select! {
            Some(data) = rx_data_from_adc.recv() => {
                // --- DEMAND-BASED PROCESSING CHECK ---
                // Check if any pipelines are active before processing
                let has_active_pipelines = connection_manager.has_active_pipelines().await;
                
                // CRITICAL FIX: Always process RawData pipeline when FFT feature is enabled
                // The FFT plugin runs on a separate server (port 8081) and can't register with connection_manager
                #[cfg(feature = "brain_waves_fft_feature")]
                let force_raw_data_processing = true;
                #[cfg(not(feature = "brain_waves_fft_feature"))]
                let force_raw_data_processing = false;
                
                if !has_active_pipelines && !force_raw_data_processing {
                    // IDLE STATE - 0% CPU usage
                    // Only handle CSV recording if needed, skip all other processing
                    if let Ok(mut recorder) = csv_recorder.try_lock() {
                        if recorder.is_recording {
                            match recorder.write_data(&data).await {
                                Ok(msg) => {
                                    if msg != "Data written successfully" && msg != "Not recording" {
                                        println!("CSV Recording (idle): {}", msg);
                                    }
                                },
                                Err(e) => println!("Warning: Failed to write data to CSV (idle): {}", e),
                            }
                        }
                    }
                    // Skip all WebSocket processing - no clients connected
                    continue;
                }
                
                // Get active pipelines for targeted processing
                let mut active_pipelines = connection_manager.get_active_pipelines().await;
                
                // CRITICAL FIX: Always include RawData pipeline when FFT feature is enabled
                #[cfg(feature = "brain_waves_fft_feature")]
                {
                    if !active_pipelines.contains(&PipelineType::RawData) {
                        active_pipelines.insert(PipelineType::RawData);
                        log::debug!("FFT Feature: Force-enabled RawData pipeline for FFT processing");
                    }
                }
                
                // --- CSV Recording ---
                // Uses data.voltage_samples (which are direct from driver, pre-SignalProcessor)
                // and data.raw_samples
                if let Ok(mut recorder) = csv_recorder.try_lock() {
                    if recorder.is_recording { // Check recorder's own flag
                        match recorder.write_data(&data).await {
                            Ok(msg) => {
                                if msg != "Data written successfully" && msg != "Not recording" {
                                    println!("CSV Recording: {}", msg);
                                }
                            },
                            Err(e) => println!("Warning: Failed to write data to CSV: {}", e),
                        }
                    }
                }

                // --- PIPELINE-AWARE DATA PROCESSING ---
                if let Some(error_msg) = &data.error {
                    println!("Error from EEG system: {}", error_msg);
                    
                    // Send error only to active pipelines
                    if active_pipelines.contains(&PipelineType::RawData) {
                        let error_batch_unfiltered = EegBatchData {
                            channels: Vec::new(),
                            timestamp: data.timestamp / 1000, // ms
                            power_spectrums: None,
                            frequency_bins: None,
                            error: Some(error_msg.clone()),
                        };
                        let _ = tx_to_web_socket.send(error_batch_unfiltered);
                    }

                    if active_pipelines.contains(&PipelineType::BasicVoltageFilter) {
                        let error_batch_filtered = FilteredEegData {
                            timestamp: data.timestamp / 1000, // ms
                            raw_samples: None,
                            filtered_voltage_samples: None,
                            error: Some(error_msg.clone()),
                        };
                        let _ = tx_to_filtered_data_web_socket.send(error_batch_filtered);
                    }

                } else if !data.voltage_samples.is_empty() && !data.voltage_samples[0].is_empty() {
                    // --- PIPELINE-SPECIFIC DATA PROCESSING ---
                    
                    // 1. Process RAW DATA pipeline (if active)
                    if active_pipelines.contains(&PipelineType::RawData) {
                        let batch_size_for_unfiltered_ws = daemon_config_clone.batch_size;
                        let num_channels_for_unfiltered = data.voltage_samples.len();
                        let samples_per_channel_unfiltered = data.voltage_samples[0].len();

                        for chunk_start in (0..samples_per_channel_unfiltered).step_by(batch_size_for_unfiltered_ws) {
                            let chunk_end = (chunk_start + batch_size_for_unfiltered_ws).min(samples_per_channel_unfiltered);
                            
                            let us_per_sample = 1_000_000 / adc_config_clone.sample_rate as u64;
                            let chunk_timestamp_us = data.timestamp + (chunk_start as u64 * us_per_sample);

                            let mut chunk_channels_unfiltered = Vec::with_capacity(num_channels_for_unfiltered);
                            for channel_samples in &data.voltage_samples {
                                chunk_channels_unfiltered.push(channel_samples[chunk_start..chunk_end].to_vec());
                            }
                            
                            let eeg_batch_data = EegBatchData {
                                channels: chunk_channels_unfiltered,
                                timestamp: chunk_timestamp_us / 1000, // Convert to milliseconds
                                power_spectrums: data.power_spectrums.clone(),
                                frequency_bins: data.frequency_bins.clone(),
                                error: None,
                            };
                            let _ = tx_to_web_socket.send(eeg_batch_data);
                        }
                    }

                    // 2. Process BASIC VOLTAGE FILTER pipeline (if active)
                    if active_pipelines.contains(&PipelineType::BasicVoltageFilter) {
                        // Create a mutable copy for in-place filtering
                        let mut samples_to_filter = data.voltage_samples.clone();
                        
                        for (channel_idx, channel_samples_vec) in samples_to_filter.iter_mut().enumerate() {
                            // Ensure channel_idx is within bounds for the signal_processor's configuration
                            if channel_idx < num_channels_usize {
                                // Create a copy of the input samples for processing
                                let input_samples = channel_samples_vec.clone();
                                match signal_processor.process_chunk(channel_idx, &input_samples, channel_samples_vec.as_mut_slice()) {
                                    Ok(_) => {} // Successfully processed
                                    Err(e) => {
                                        println!("Error processing chunk for channel {}: {}", channel_idx, e);
                                    }
                                }
                            } else {
                                println!("Warning: Channel index {} is out of bounds for signal_processor ({} channels configured). Skipping filtering for this channel.", channel_idx, num_channels_usize);
                            }
                        }

                        let filtered_eeg_data = FilteredEegData {
                            timestamp: data.timestamp / 1000, // ms, for the whole batch from driver
                            raw_samples: Some(data.raw_samples.clone()), // Include raw samples
                            filtered_voltage_samples: Some(samples_to_filter), // These are now filtered
                            error: None,
                        };
                        let _ = tx_to_filtered_data_web_socket.send(filtered_eeg_data);
                    }

                    // 3. Process FFT ANALYSIS pipeline (if active) - needs raw data like RawData pipeline
                    if active_pipelines.contains(&PipelineType::FftAnalysis) {
                        let batch_size_for_fft = daemon_config_clone.batch_size;
                        let num_channels_for_fft = data.voltage_samples.len();
                        let samples_per_channel_fft = data.voltage_samples[0].len();

                        for chunk_start in (0..samples_per_channel_fft).step_by(batch_size_for_fft) {
                            let chunk_end = (chunk_start + batch_size_for_fft).min(samples_per_channel_fft);
                            
                            let us_per_sample = 1_000_000 / adc_config_clone.sample_rate as u64;
                            let chunk_timestamp_us = data.timestamp + (chunk_start as u64 * us_per_sample);

                            let mut chunk_channels_fft = Vec::with_capacity(num_channels_for_fft);
                            for channel_samples in &data.voltage_samples {
                                chunk_channels_fft.push(channel_samples[chunk_start..chunk_end].to_vec());
                            }
                            
                            let eeg_batch_data_fft = EegBatchData {
                                channels: chunk_channels_fft,
                                timestamp: chunk_timestamp_us / 1000, // Convert to milliseconds
                                power_spectrums: data.power_spectrums.clone(),
                                frequency_bins: data.frequency_bins.clone(),
                                error: None,
                            };
                            let _ = tx_to_web_socket.send(eeg_batch_data_fft);
                        }
                    }

                    // --- Statistics --- (based on incoming data before filtering for consistency)
                    if let Some(first_channel_samples) = data.voltage_samples.get(0) {
                        count += first_channel_samples.len();
                    }
                    
                    if count % 250 == 0 { // Roughly every second for 250Hz
                        let elapsed = last_time.elapsed();
                        if !elapsed.is_zero() {
                             let rate = 250.0 / elapsed.as_secs_f32();
                             println!("Processing rate: {:.2} Samples/sec (for raw data handling)", rate * (data.voltage_samples.get(0).map_or(0, |s| s.len()) as f32 / 250.0) ); // Adjust for actual samples in batch
                        }
                        println!("Total samples processed (driver output): {}", count);
                        if let Some(first_channel_samples) = data.voltage_samples.get(0) {
                            if !first_channel_samples.is_empty() {
                                println!("  Unfiltered (driver output) 1st Chan (first 5): {:?}", first_channel_samples.iter().take(5).collect::<Vec<_>>());
                            }
                        }
                        last_time = std::time::Instant::now();
                    }
                } else {
                    // Handle case where voltage_samples might be empty but not an error
                    // e.g. if driver sends an empty data packet for some reason.
                    // println!("Warning: Received data packet with empty voltage_samples (and no error).");
                }
            },
            _ = cancellation_token.cancelled() => {
                println!("Processing task cancellation requested in driver_handler, cleaning up...");
                break;
            }
        }
    }
}