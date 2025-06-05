use std::sync::Arc;
use std::collections::HashSet;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use async_trait::async_trait;
use log::{info, warn, debug, trace, error};
use lazy_static::lazy_static;
use super::super::types::{AdcConfig, DriverStatus, DriverError, DriverEvent, DriverType};
use super::mock_data_generator::{gen_realistic_eeg_data, current_timestamp_micros};

// Static hardware lock to simulate real hardware access constraints
lazy_static! {
    static ref HARDWARE_LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
}

/// A stubbed-out driver that does not access any hardware.
pub struct MockDriver {
    inner: Arc<Mutex<MockInner>>,
    task_handle: Option<JoinHandle<()>>,
    tx: mpsc::Sender<DriverEvent>,
    additional_channel_buffering: usize,
}

/// Internal state for the MockDriver.
struct MockInner {
    config: AdcConfig,
    running: bool,
    status: DriverStatus,
    // Base timestamp for calculating sample timestamps (microseconds since epoch)
    base_timestamp: Option<u64>,
    // Total samples generated since acquisition started
    sample_count: u64,
}

impl MockDriver {
    /// Create a new instance of the MockDriver.
    ///
    /// This constructor takes an ADC configuration and an optional additional channel buffering parameter.
    /// The additional_channel_buffering parameter determines how many extra batches can be buffered in the channel
    /// beyond the minimum required (which is the batch_size from the config). Setting this to 0 minimizes
    /// latency but may cause backpressure if the consumer can't keep up.
    ///
    /// Returns a tuple containing the driver instance and a receiver for driver events.
    /// Create a new instance of the MockDriver.
    ///
    /// This constructor takes an ADC configuration and an optional additional channel buffering parameter.
    /// The additional_channel_buffering parameter determines how many extra batches can be buffered in the channel
    /// beyond the minimum required (which is the batch_size from the config). Setting this to 0 minimizes
    /// latency but may cause backpressure if the consumer can't keep up.
    ///
    /// # Important
    /// Users should explicitly call `shutdown()` when done with the driver to ensure proper cleanup.
    /// While the Drop implementation provides some basic cleanup, it cannot perform the full async shutdown sequence.
    /// Don't start buffer data in new
    ///
    /// # Returns
    /// A tuple containing the driver instance and a receiver for driver events.
    ///
    /// # Errors
    /// Returns an error if:
    /// - config.board_driver is not DriverType::Mock
    /// - config.batch_size is 0 (batch size must be positive)
    /// - config.batch_size is less than the number of channels (need at least one sample per channel)
    pub fn new(
        config: AdcConfig,
        additional_channel_buffering: usize
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        // Try to acquire the hardware lock to simulate real hardware access constraints
        let mut hardware_in_use = HARDWARE_LOCK.lock()
            .map_err(|_| DriverError::Other("Failed to acquire hardware lock".to_string()))?;
            
        if *hardware_in_use {
            return Err(DriverError::HardwareNotFound(
                "Hardware already in use by another driver instance".to_string()
            ));
        }
        
        // Mark hardware as in use
        *hardware_in_use = true;
        
        // Validate config
        if config.board_driver != DriverType::Mock {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "MockDriver requires config.board_driver=DriverType::Mock".to_string()
            ));
        }
        
        // Validate channels
        if config.channels.is_empty() {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "At least one channel must be configured".to_string()
            ));
        }

        // Check for duplicate channels
        let mut unique_channels = std::collections::HashSet::new();
        for &channel in &config.channels {
            if !unique_channels.insert(channel) {
                // Release the lock if we're returning an error
                *hardware_in_use = false;
                return Err(DriverError::ConfigurationError(
                    format!("Duplicate channel detected: {}", channel)
                ));
            }
        }

        // For MockDriver, we can be more flexible with channel indices
        // but still validate they're reasonable
        for &channel in &config.channels {
            if channel > 31 {  // Allow more channels for mock testing
                // Release the lock if we're returning an error
                *hardware_in_use = false;
                return Err(DriverError::ConfigurationError(
                    format!("Invalid channel index: {}. MockDriver supports channels 0-31", channel)
                ));
            }
        }

        // Validate batch size
        if config.batch_size == 0 {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "Batch size must be greater than 0".to_string()
            ));
        }
        
        // Validate batch size relative to channel count
        if config.batch_size < config.channels.len() {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                format!("Batch size ({}) must be at least equal to the number of channels ({})",
                        config.batch_size, config.channels.len())
            ));
        }
        
        // Validate total buffer size (prevent excessive memory usage)
        const MAX_BUFFER_SIZE: usize = 10000; // Arbitrary limit to prevent excessive memory usage
        let channel_buffer_size = config.batch_size + additional_channel_buffering;
        if channel_buffer_size > MAX_BUFFER_SIZE {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                format!("Total buffer size ({}) exceeds maximum allowed ({})",
                        channel_buffer_size, MAX_BUFFER_SIZE)
            ));
        }
        
        let inner = MockInner {
            config: config.clone(),
            running: false,
            status: DriverStatus::Ok,
            base_timestamp: None,
            sample_count: 0,
        };
        
        // Create channel with validated buffer size
        let (tx, rx) = mpsc::channel(channel_buffer_size);
        
        let driver = MockDriver {
            inner: Arc::new(Mutex::new(inner)),
            task_handle: None,
            tx,
            additional_channel_buffering,
        };
        
        info!("MockDriver created with config: {:?}", config);
        info!("Channel buffer size: {} (batch_size: {} + additional_buffering: {})",
              channel_buffer_size, config.batch_size, additional_channel_buffering);
        
        Ok((driver, rx))
    }
    
    /// Return the current configuration.
    pub(crate) async fn get_config(&self) -> Result<AdcConfig, DriverError> {
        let inner = self.inner.lock().await;
        Ok(inner.config.clone())
    }

    /// Start a dummy acquisition task that sends fake data at regular intervals.
    ///
    /// This method validates the driver state and spawns a background task that
    /// generates synthetic data according to the configured parameters.
    pub(crate) async fn start_acquisition(&mut self) -> Result<(), DriverError> {
        // Check preconditions without holding the lock for too long
        {
            let inner = self.inner.lock().await;
                
            if inner.running {
                return Err(DriverError::ConfigurationError("Acquisition already running".to_string()));
            }
        }
        
        // Get the initial timestamp and update state to running
        let start_time = match current_timestamp_micros() {
            Ok(time) => time,
            Err(e) => {
                error!("Failed to get start timestamp: {:?}", e);
                return Err(DriverError::Other(format!("Failed to get start timestamp: {}", e)));
            }
        };

        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.status = DriverStatus::Running;
            inner.base_timestamp = Some(start_time);
            inner.sample_count = 0;
        }
        
        // Notify about the status change
        self.notify_status_change().await?;

        // Prepare for background task
        let inner_arc = self.inner.clone();
        let tx = self.tx.clone();
        
        // Spawn a task that periodically sends dummy data
        let handle = tokio::spawn(async move {
            // Get configuration and base timestamp without holding the lock for the entire task
            let (config, base_timestamp) = {
                let inner = inner_arc.lock().await;
                (inner.config.clone(), inner.base_timestamp.expect("Base timestamp should be set"))
            };
            
            // Get batch size from config
            let batch_size = config.batch_size;
            
            debug!("Starting acquisition with batch size: {}, sample rate: {} Hz",
                   batch_size, config.sample_rate);
            
            // Main acquisition loop
            loop {
                // Check if we should continue running and get current sample count
                let (should_continue, current_sample_count) = {
                    let mut inner = inner_arc.lock().await;
                    if !inner.running {
                        (false, 0)
                    } else {
                        let count = inner.sample_count;
                        // Update the sample count for the next batch
                        inner.sample_count += batch_size as u64;
                        (true, count)
                    }
                };
                
                if !should_continue {
                    break;
                }
                
                // Calculate timing parameters
                let mut batch = Vec::with_capacity(batch_size);
                let sample_interval = (1_000_000 / config.sample_rate) as u64; // microseconds between samples
                debug!("Sample interval: {} microseconds", sample_interval);
                
                // Generate a batch of samples with incrementing timestamps based on sample count
                for i in 0..batch_size {
                    let sample_number = current_sample_count + i as u64;
                    let timestamp = base_timestamp + sample_number * sample_interval;
                    trace!("Sample {}: absolute_time={} microseconds", sample_number, timestamp);
                    
                    // For data generation, we still use relative timestamps (time since acquisition started)
                    let relative_timestamp = sample_number * sample_interval;
                    
                    // Use gen_realistic_eeg_data for more realistic EEG data
                    let mut sample = gen_realistic_eeg_data(&config, relative_timestamp);
                    
                    // Override the timestamp with our calculated one
                    sample.timestamp = timestamp;
                    batch.push(sample);
                }
                
                // Send the batch of data
                if let Err(e) = tx.send(DriverEvent::Data(batch)).await {
                    warn!("MockDriver event channel closed: {}", e);
                    break;
                }
                
                // Sleep for the time it would take to collect this batch via SPI
                let sleep_time = (1000 * batch_size as u64) / config.sample_rate as u64;
                debug!("Sleeping for {} ms before next batch", sleep_time);
                sleep(Duration::from_millis(sleep_time)).await;
            }
            
            debug!("Acquisition task terminated");
        });
        
        self.task_handle = Some(handle);
        info!("MockDriver acquisition started");
        Ok(())
    }

    /// Stop the dummy data acquisition.
    ///
    /// This method signals the acquisition task to stop, waits for it to complete,
    /// and updates the driver status.
    pub(crate) async fn stop_acquisition(&mut self) -> Result<(), DriverError> {
        // Signal the acquisition task to stop
        {
            let mut inner = self.inner.lock().await;
            
            if !inner.running {
                debug!("Stop acquisition called, but acquisition was not running");
                return Ok(());
            }
            
            inner.running = false;
            debug!("Signaled acquisition task to stop");
        }
        
        // Wait for the task to complete
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(_) => debug!("Acquisition task completed successfully"),
                Err(e) => warn!("Acquisition task terminated with error: {}", e),
            }
        }
        
        // Update driver status and reset counters
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Stopped;
            inner.sample_count = 0;
            // Keep the base_timestamp as it is - we'll set a new one when acquisition starts again
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        info!("MockDriver acquisition stopped");
        Ok(())
    }

    /// Return the current driver status.
    ///
    /// This method returns the current status of the driver.
    pub(crate) async fn get_status(&self) -> DriverStatus {
        let inner = self.inner.lock().await;
        inner.status
    }

    /// Shut down the driver.
    ///
    /// This method stops any ongoing acquisition and resets the driver state.
    ///
    /// # Important
    /// This method should always be called before the driver is dropped to ensure
    /// proper cleanup of resources. The Drop implementation provides only basic cleanup
    /// and cannot perform the full async shutdown sequence.
    pub(crate) async fn shutdown(&mut self) -> Result<(), DriverError> {
        debug!("Shutting down MockDriver");
        
        // First check if running, but don't hold the lock
        let should_stop = {
            let inner = self.inner.lock().await;
            inner.running
        };
        
        // Stop acquisition if needed
        if should_stop {
            debug!("Stopping acquisition as part of shutdown");
            self.stop_acquisition().await?;
        }

        // Update final state
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::NotInitialized;
            inner.base_timestamp = None;
            inner.sample_count = 0;
            // Config is now static, so we don't need to reset it
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        info!("MockDriver shutdown complete");
        Ok(())
    }

    /// Internal helper to notify status changes over the event channel.
    ///
    /// This method sends a status change event to any listeners.
    async fn notify_status_change(&self) -> Result<(), DriverError> {
        // Get current status
        let status = {
            let inner = self.inner.lock().await;
            inner.status
        };
        
        debug!("Sending status change notification: {:?}", status);
        
        // Send the status change event
        self.tx
            .send(DriverEvent::StatusChange(status))
            .await
            .map_err(|e| DriverError::Other(format!("Failed to send status change: {}", e)))
    }
}

// Implement the AdcDriver trait
#[async_trait]
impl super::super::types::AdcDriver for MockDriver {
    async fn shutdown(&mut self) -> Result<(), DriverError> {
        self.shutdown().await
    }

    async fn start_acquisition(&mut self) -> Result<(), DriverError> {
        self.start_acquisition().await
    }

    async fn stop_acquisition(&mut self) -> Result<(), DriverError> {
        self.stop_acquisition().await
    }

    async fn get_status(&self) -> DriverStatus {
        self.get_status().await
    }

    async fn get_config(&self) -> Result<AdcConfig, DriverError> {
        self.get_config().await
    }
}

/// Implementation of Drop for MockDriver to handle cleanup when the driver is dropped.
///
/// Note: This provides only basic cleanup. For proper cleanup, users should explicitly
/// call `shutdown()` before letting the driver go out of scope. The Drop implementation
/// cannot perform the full async shutdown sequence because Drop is not async.
impl Drop for MockDriver {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning
        error!("MockDriver dropped without calling shutdown() first. This may lead to resource leaks.");
        error!("Always call driver.shutdown().await before dropping the driver.");
        
        // Note: We can't properly clean up in Drop because we can't use .await
        // This is why users should call shutdown() explicitly.
        
        // Note: We cannot await the task_handle here because Drop is not async.
        // This is why users should call shutdown() explicitly.
        if self.task_handle.is_some() {
            error!("Background task may still be running. Call shutdown() to properly terminate it.");
        }
        
        // Release the hardware lock
        if let Ok(mut lock) = HARDWARE_LOCK.lock() {
            *lock = false;
            debug!("Hardware lock released in Drop implementation");
        } else {
            error!("Failed to release hardware lock in Drop implementation");
        }
    }
}
