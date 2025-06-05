use std::error::Error;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex}; // Use Tokio Mutex
use tokio::task::JoinHandle;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::board_drivers::{
    create_driver, AdcConfig, AdcData, AdcDriver, DriverError, DriverEvent, DriverStatus, DriverType,
};
// use crate::dsp::filters::SignalProcessor; // Removed as per DSP refactor plan
use super::ProcessedData;

/// Helper function to process a batch of data
///
/// This is separated from the main task to improve readability
async fn process_data_batch(
    data_batch: &[AdcData],
    channel_count: usize,
    // processor: &Arc<Mutex<SignalProcessor>>, // This line was already commented, ensuring it stays so.
    tx: &mpsc::Sender<ProcessedData>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if data_batch.is_empty() {
        return Ok(());
    }

    // Pre-allocate with known capacity
    let batch_size = data_batch.len();
    let samples_per_channel = data_batch[0].voltage_samples[0].len();
    
    // Pre-allocate for voltage samples (previously processed_voltage_samples)
    let mut voltage_samples: Vec<Vec<f32>> = Vec::with_capacity(channel_count);
    for _ in 0..channel_count {
        voltage_samples.push(Vec::with_capacity(batch_size * samples_per_channel));
    }
    
    // Pre-allocate for raw samples
    let mut raw_samples: Vec<Vec<i32>> = Vec::with_capacity(channel_count);
    for _ in 0..channel_count {
        raw_samples.push(Vec::with_capacity(batch_size * samples_per_channel));
    }
    
    // // Single lock acquisition for the batch // Removed as per DSP refactor plan
    // let mut proc_guard = match processor.lock().await {
    //     guard => guard,
    //     // This would only happen if a thread panicked while holding the lock
    // };
    
    // Process all samples in the batch
    for data in data_batch {
        // Collect raw samples
        for (ch_idx, channel_raw_samples) in data.raw_samples.iter().enumerate() {
            if ch_idx < raw_samples.len() {
                raw_samples[ch_idx].extend(channel_raw_samples.iter().cloned());
            }
        }
        
        // Collect voltage samples directly (no filtering in driver)
        for (ch_idx, channel_voltage_s) in data.voltage_samples.iter().enumerate() {
            if ch_idx < channel_count {
                voltage_samples[ch_idx].extend(channel_voltage_s.iter().cloned());
            }
        }
    }
    // drop(proc_guard); // Removed as per DSP refactor plan

    // Send the processed data
    tx.send(ProcessedData {
        timestamp: data_batch.last().unwrap().timestamp,
        raw_samples,
        voltage_samples, // Renamed from processed_voltage_samples
        power_spectrums: None,
        frequency_bins: None,
        error: None,
    }).await.map_err(|e| format!("Failed to send processed data: {}", e).into())
}

pub struct EegSystem {
    driver: Box<dyn AdcDriver>,
    // processor: Arc<Mutex<SignalProcessor>>, // Removed as per DSP refactor plan
    processing_task: Option<JoinHandle<()>>,
    tx: mpsc::Sender<ProcessedData>,
    event_rx: Option<mpsc::Receiver<DriverEvent>>,
    cancel_token: CancellationToken,
}

impl EegSystem {
    /// Creates an EEG processing system without starting it
    pub async fn new(
        config: AdcConfig
    ) -> Result<(Self, mpsc::Receiver<ProcessedData>), Box<dyn Error>> {
        let (driver, event_rx) = create_driver(config.clone()).await?;
        // let processor = Arc::new(Mutex::new(SignalProcessor::new( // Removed as per DSP refactor plan
        //     config.sample_rate,
        //     config.channels.len(),
        //     config.dsp_high_pass_cutoff_hz,
        //     config.dsp_low_pass_cutoff_hz,
        //     config.powerline_filter_hz,
        // )));
        let (tx, rx) = mpsc::channel(100);
        let cancel_token = CancellationToken::new();

        let system = Self {
            driver,
            // processor, // Removed as per DSP refactor plan
            processing_task: None,
            tx,
            event_rx: Some(event_rx),
            cancel_token,
        };

        Ok((system, rx))
    }

    /// Starts the processing system with the given configuration
    pub async fn start(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        self.initialize_processing(config).await
    }

    /// Internal helper to initialize or reinitialize the driver and processing task
    async fn initialize_processing(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        // Add validation before proceeding
        if config.channels.is_empty() {
            return Err(Box::new(DriverError::ConfigurationError(
                "Cannot initialize with zero channels".into()
            )));
        }

        if config.sample_rate == 0 {
            return Err(Box::new(DriverError::ConfigurationError(
                "Sample rate must be greater than 0".into()
            )));
        }

        // Cancel any existing processing task gracefully
        if self.processing_task.is_some() {
            self.cancel_token.cancel();
            // Create a new token for the next task
            self.cancel_token = CancellationToken::new();
        }
 
        // // Reset the signal processor // Removed as per DSP refactor plan
        // {
        //     let mut proc_guard = match self.processor.lock().await {
        //         guard => guard,
        //         // This would only happen if a thread panicked while holding the lock
        //         // In a real system, we might want to recreate the processor entirely
        //     };
            
        //     proc_guard.reset(
        //         config.sample_rate,
        //         config.channels.len(),
        //         config.dsp_high_pass_cutoff_hz,
        //         config.dsp_low_pass_cutoff_hz,
        //         config.powerline_filter_hz
        //     );
        // }

        self.driver.start_acquisition().await?;

        // Take ownership of the event receiver
        let mut event_rx = self.event_rx.take().expect("Event receiver should exist");

        // Start the processing task
        // let processor: Arc<Mutex<SignalProcessor>> = Arc::clone(&self.processor); // Removed as per DSP refactor plan
        let tx = self.tx.clone();
        // Capture the channel count from the configuration
        let channel_count = config.channels.len();
        // Clone the cancellation token for the task
        let cancel_token = self.cancel_token.clone();

        self.processing_task = Some(tokio::spawn(async move {
            // Create a select future that will complete when either:
            // 1. We receive an event from the driver
            // 2. The cancellation token is triggered
            loop {
                tokio::select! {
                    // Check if cancellation was requested
                    _ = cancel_token.cancelled() => {
                        break;
                    }
                    // Process events from the driver
                    event_opt = event_rx.recv() => {
                        match event_opt {
                            Some(event) => {
                                match event {
                                    DriverEvent::Data(data_batch) => {
                                        if let Err(e) = process_data_batch(
                                            &data_batch,
                                            channel_count,
                                            // &processor, // This line was already commented, ensuring it stays so.
                                            &tx
                                        ).await {
                                            eprintln!("Error processing data batch: {}", e);
                                            // Send error event if possible
                                            let _ = tx.send(ProcessedData {
                                                timestamp: data_batch.last().map_or(0, |d| d.timestamp),
                                                raw_samples: Vec::new(),
                                                voltage_samples: Vec::new(), // Renamed
                                                power_spectrums: None, // Ensure all fields are present
                                                frequency_bins: None,  // Ensure all fields are present
                                                error: Some(format!("Processing error: {}", e)),
                                            }).await;
                                            
                                            // Continue processing - don't break on errors
                                        }
                                    }
                                    DriverEvent::StatusChange(status) => {
                                        if status == DriverStatus::Stopped {
                                            break;
                                        }
                                        
                                        // Log status changes but don't send them as errors
                                        println!("Driver status changed: {:?}", status);
                                        
                                        // Don't send status changes as ProcessedData errors since they're not errors
                                        // Status changes are normal operational events, not errors
                                    }
                                    DriverEvent::Error(err_msg) => {
                                        eprintln!("Driver error: {}", err_msg);
                                        
                                        // Forward the error to the processed data stream
                                        let _ = tx.send(ProcessedData {
                                            timestamp: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_micros() as u64,
                                            raw_samples: Vec::new(),
                                            voltage_samples: Vec::new(), // Renamed
                                            power_spectrums: None,
                                            frequency_bins: None,
                                            error: Some(format!("Driver error: {}", err_msg)),
                                        }).await;
                                    }
                                }
                            }
                            None => {
                                // Channel closed, exit the task
                                break;
                            }
                        }
                    }
                }
            }
        }));

        Ok(())
    }


    /// Stop the data acquisition & gracefully cancel the background task
    pub async fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        self.driver.stop_acquisition().await?;
        
        // Signal the task to stop gracefully
        if self.processing_task.is_some() {
            self.cancel_token.cancel();
            
            // Wait for a short time for the task to complete
            if let Some(task) = self.processing_task.take() {
                match tokio::time::timeout(Duration::from_millis(500), task).await {
                    Ok(_) => {
                        // Task completed gracefully
                    },
                    Err(_) => {
                        // Task didn't complete in time, force abort
                        // This is a fallback mechanism
                        eprintln!("Warning: Processing task didn't complete in time, forcing abort");
                    }
                }
            }
            
            // Create a new token for future tasks
            self.cancel_token = CancellationToken::new();
        }
        
        Ok(())
    }

    /// Reconfigure the driver with new settings, resetting the processor
    pub async fn reconfigure(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        // Stop the current driver and processing task
        self.stop().await?;
        
        // Create a new driver with the updated configuration
        let (new_driver, new_event_rx) = create_driver(config.clone()).await?;
        
        // Replace the driver and event_rx
        self.driver = new_driver;
        self.event_rx = Some(new_event_rx);
        
        // Initialize processing with the new configuration
        self.initialize_processing(config).await
    }

    /// Retrieve the current driver status
    pub async fn driver_status(&self) -> DriverStatus {
        self.driver.get_status().await
    }

    /// Retrieve the driver's configuration
    pub async fn driver_config(&self) -> Result<AdcConfig, DriverError> {
        self.driver.get_config().await
    }

    /// Optionally allow direct driver access
    pub fn driver(&mut self) -> &mut Box<dyn AdcDriver> {
        &mut self.driver
    }

    /// Completely shut down the EEG system and clean up resources
    pub async fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        // Add timeout for safety
        const SHUTDOWN_TIMEOUT_MS: u64 = 1000;
        
        let shutdown_future = async {
            // Convert the Box<dyn Error> to DriverError
            if let Err(e) = self.stop().await {
                return Err(DriverError::Other(e.to_string()));
            }
            self.driver.shutdown().await
        };
        
        match tokio::time::timeout(
            Duration::from_millis(SHUTDOWN_TIMEOUT_MS),
            shutdown_future
        ).await {
            Ok(result) => result.map_err(|e| Box::new(e) as Box<dyn Error>),
            Err(_) => Err(Box::new(DriverError::Other("Shutdown timed out".into())))
        }
    }
}

impl Drop for EegSystem {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning
        eprintln!("Warning: EegSystem dropped without calling shutdown() first");
        eprintln!("Always call system.shutdown().await before dropping the system");
    }
}
