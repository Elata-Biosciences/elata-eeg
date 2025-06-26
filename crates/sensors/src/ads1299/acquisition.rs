//! Acquisition logic for the ADS1299 driver.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use log::{info, warn, debug, error, trace};
use std::collections::VecDeque;
use std::thread; // Add this import

use crate::types::{AdcConfig, AdcData, DriverStatus, DriverError, DriverEvent};
use super::helpers::{ch_raw_to_voltage, current_timestamp_micros, read_data_from_spi};
use super::registers::{CMD_SDATAC, CMD_RDATAC, CMD_START, CMD_STOP, CMD_WAKEUP};
use super::spi::{SpiDevice, InputPinDevice, send_command_to_spi};

/// Data passed from the interrupt handler to the processing task
pub enum InterruptData {
    /// Sample data with sample count
    Data(Vec<i32>, u64),
    /// Error message
    Error(String),
}

/// Type alias for SPI device
pub type SpiType = Box<dyn SpiDevice>;

/// Type alias for DRDY pin
pub type DrdyPinType = Box<dyn InputPinDevice>;

/// Type alias for ADS1299 configuration
pub type Ads1299Config = AdcConfig;

/// Start data acquisition from the ADS1299.
///
/// This method configures the device for continuous data mode, sets up the
/// interrupt handler, and starts the acquisition process.

pub async fn start_acquisition(
    inner_arc: Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: mpsc::Sender<DriverEvent>,
    interrupt_running: Arc<AtomicBool>,
    mut spi: Option<SpiType>, // Make spi mutable
    drdy_pin: Option<DrdyPinType>,
) -> Result<(Option<thread::JoinHandle<()>>, Option<JoinHandle<()>>), DriverError> {
    // Note: spi and drdy_pin are moved into the tasks/threads now.
    // The caller (driver.rs) should set its copies to None.

    // Set base timestamp
    {
        let mut inner = inner_arc.lock().await;
        let timestamp = match current_timestamp_micros() {
            Ok(ts) => ts,
            Err(e) => {
                error!("Failed to get timestamp: {:?}", e);
                0
            }
        };
        inner.base_timestamp = Some(timestamp);
        inner.status = DriverStatus::Running;
    }
    
    // Wake up ADS1299 from standby before starting
    if let Some(spi_ref) = spi.as_mut() {
        if let Err(e) = send_command_to_spi(spi_ref, CMD_WAKEUP) {
            error!("Failed to send WAKEUP command: {:?}", e);
            return Err(e);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }
    // Send START command to ADS1299
    if let Some(spi_ref) = spi.as_mut() {
        if let Err(e) = send_command_to_spi(spi_ref, CMD_START) {
            error!("Failed to send START command: {:?}", e);
            return Err(e);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }
    
    debug!("Conversion started");
    
    // Set up continuous data mode
    if let Some(spi_ref) = spi.as_mut() {
        if let Err(e) = send_command_to_spi(spi_ref, CMD_SDATAC) {
            error!("Failed to send SDATAC command: {:?}", e);
            return Err(e);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        if let Err(e) = send_command_to_spi(spi_ref, CMD_RDATAC) {
            error!("Failed to send RDATAC command: {:?}", e);
            return Err(e);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }
    
    debug!("Starting acquisition using hardware interrupts in continuous data mode");
    
    // Get configuration
    let config = {
        let inner = inner_arc.lock().await;
        inner.config.clone()
    };
    
    let batch_size = config.batch_size;
    let num_channels = config.channels.len();
    
    info!(
        "Starting acquisition with batch size: {}, sample rate: {} Hz",
        batch_size, config.sample_rate
    );
    info!("Buffering {} samples before sending batches", batch_size);
    
    // Create channel for communication between interrupt handler and processing task
    let (data_tx, data_rx) = mpsc::channel::<InterruptData>(batch_size);
    
    // Set interrupt running flag
    interrupt_running.store(true, Ordering::SeqCst);
    
    // Take ownership of hardware interfaces
    let mut drdy_pin_unwrapped = drdy_pin.ok_or(DriverError::NotInitialized)?;
    let mut spi_unwrapped = spi.ok_or(DriverError::NotInitialized)?;
    
    // Spawn tasks/threads
    let (interrupt_thread_handle, processing_task_handle) = spawn_interrupt_and_processing_tasks(
        config,
        batch_size,
        num_channels,
        inner_arc.clone(),
        tx.clone(),
        data_tx,
        data_rx,
        interrupt_running.clone(),
        drdy_pin_unwrapped,
        spi_unwrapped,
    );

    info!("Ads1299Driver acquisition started");
    // Return the handles
    Ok((Some(interrupt_thread_handle), Some(processing_task_handle)))
}

/// Stop data acquisition from the ADS1299.
///
/// This method signals the acquisition task to stop, waits for it to complete,
/// and updates the driver status.
pub async fn stop_acquisition(
    inner_arc: Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: &mpsc::Sender<DriverEvent>,
    interrupt_running: &Arc<AtomicBool>,
    interrupt_thread: &mut Option<thread::JoinHandle<()>>, // Changed to mutable reference
    processing_task: &mut Option<JoinHandle<()>>,          // Changed to mutable reference
    spi: &mut Option<SpiType>,
) -> Result<(), DriverError> {
    debug!("Stopping acquisition");
    
    // First, update the inner state to signal stopping
    {
        let mut inner = inner_arc.lock().await;
        inner.running = false;
    }
    
    // Signal the interrupt handler to stop
    interrupt_running.store(false, Ordering::SeqCst);
    debug!("Interrupt thread signaled to stop");
    
    // Send STOP command to ADS1299 before waiting for threads to complete
    if let Some(spi_ref) = spi.as_mut() {
        if let Err(e) = send_command_to_spi(spi_ref, CMD_STOP) {
            error!("Failed to send STOP command: {:?}", e);
            // Continue with shutdown despite error
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        if let Err(e) = send_command_to_spi(spi_ref, CMD_SDATAC) {
            error!("Failed to send SDATAC command: {:?}", e);
            // Continue with shutdown despite error
        }
    }
    
    // Wait for the interrupt thread to complete with a timeout
    if let Some(handle) = interrupt_thread.take() {
        debug!("Waiting for interrupt thread to complete...");
        // Spawn a blocking task to join the thread with timeout
        match tokio::task::spawn_blocking(move || {
            // Use a timeout for joining the thread
            let join_result = std::thread::Builder::new()
                .name("thread-joiner".into())
                .spawn(move || handle.join())
                .expect("Failed to spawn thread joiner")
                .join();
                
            match join_result {
                Ok(Ok(_)) => debug!("Interrupt thread completed successfully"),
                Ok(Err(e)) => warn!("Interrupt thread panicked: {:?}", e),
                Err(e) => warn!("Failed to join interrupt thread: {:?}", e),
            }
        }).await {
            Ok(_) => debug!("Interrupt thread join completed"),
            Err(e) => warn!("Error joining interrupt thread: {}", e),
        }
    }
    
    // Now wait for the processing task to complete
    if let Some(task) = processing_task.take() {
        debug!("Waiting for processing task to complete...");
        // Use timeout to avoid blocking indefinitely
        match tokio::time::timeout(Duration::from_secs(2), task).await {
            Ok(Ok(_)) => debug!("Processing task completed successfully"),
            Ok(Err(e)) => warn!("Processing task error: {}", e),
            Err(_) => {
                warn!("Processing task did not complete within timeout, aborting it");
                // We can't really abort the task in a clean way, but it should
                // terminate on its own when it detects running=false
            }
        }
    }
    
    // SPI commands have already been sent above
    
    // Update driver status
    {
        let mut inner = inner_arc.lock().await;
        inner.status = DriverStatus::Stopped;
        inner.base_timestamp = None;
    }
    
    // Send status update event
    if let Err(e) = tx.send(DriverEvent::StatusChange(DriverStatus::Stopped)).await {
        error!("Failed to send status update: {}", e);
    }
    
    info!("Ads1299Driver acquisition stopped");
    Ok(())
}

/// Spawn the interrupt handler and async processing task
fn spawn_interrupt_and_processing_tasks(
    config: Ads1299Config,
    batch_size: usize,
    num_channels: usize,
    inner_arc: Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: mpsc::Sender<DriverEvent>,
    data_tx: mpsc::Sender<InterruptData>,
    mut data_rx: mpsc::Receiver<InterruptData>,
    interrupt_running: Arc<AtomicBool>,
    mut drdy_pin: DrdyPinType,
    mut spi: SpiType,
) -> (thread::JoinHandle<()>, JoinHandle<()>) { // Changed return type
    use rppal::gpio::Trigger;

    // Spawn a dedicated OS thread for interrupt handling
    let interrupt_thread_handle = thread::spawn(move || {
        let mut sample_count = 0;
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: usize = 5;

        // Configure the pin for interrupt
        match drdy_pin.set_interrupt(Trigger::FallingEdge, None) {
            Ok(_) => debug!("DRDY pin interrupt configured successfully"),
            Err(e) => {
                error!("Failed to configure DRDY pin interrupt: {:?}", e);
                // Use blocking_send as this is a thread, not async task
                let _ = data_tx.blocking_send(InterruptData::Error(format!("Failed to configure DRDY interrupt: {}", e)));
                return; // Exit thread if interrupt setup fails
            }
        }

        debug!("Interrupt handler thread started");

        // Main interrupt handling loop
        while interrupt_running.load(Ordering::SeqCst) {
            // Wait for the interrupt with a timeout (e.g., 1 second)
            match drdy_pin.poll_interrupt(true, Some(std::time::Duration::from_secs(1))) {
                Ok(Some(event)) if event.trigger == Trigger::FallingEdge => {
                    // DRDY pin went low, data is ready
                    debug!("DRDY interrupt received. Reading data from SPI...");
                    match read_data_from_spi(&mut spi, num_channels) {
                        Ok(samples) => {
                            trace!("Raw data from SPI: {:?}", samples);
                            // Reset error counter on successful read
                            consecutive_errors = 0;

                            // Send samples and current count through the channel
                            // Use blocking_send as try_send might drop data under backpressure
                            if let Err(e) = data_tx.blocking_send(InterruptData::Data(samples, sample_count)) {
                                error!("Failed to send samples to Tokio task (channel closed?): {}", e);
                                // If the channel is closed, the processing task likely panicked or exited.
                                // Check if we should still be running before breaking
                                if !interrupt_running.load(Ordering::SeqCst) {
                                    debug!("Channel closed but interrupt_running is false, normal shutdown");
                                } else {
                                    error!("Channel closed while interrupt_running is true, abnormal shutdown");
                                }
                                break; // Exit the interrupt loop
                            }
                            sample_count += 1;
                        },
                        Err(e) => {
                            error!("Error reading data in interrupt handler: {:?}", e);
                            consecutive_errors += 1;

                            // If we've had too many consecutive errors, signal a critical error
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                error!("Critical error: {} consecutive SPI failures", consecutive_errors);
                                // Send an error event through the data channel
                                let _ = data_tx.blocking_send(InterruptData::Error(format!(
                                    "Critical error: {} consecutive SPI failures", consecutive_errors
                                )));
                                break; // Exit the interrupt loop on persistent errors
                            }
                            // Continue and try again on next interrupt
                        }
                    }
                },
                Ok(Some(event)) => {
                    // This shouldn't happen as we're only triggering on falling edge
                    warn!("Unexpected interrupt event: {:?}", event);
                },
                Ok(None) => {
                    // Timeout occurred, no interrupt within 1 second. Check running flag.
                    if !interrupt_running.load(Ordering::SeqCst) {
                        break;
                    }
                    warn!("DRDY interrupt poll timed out after 1 second. No data ready from ADS1299.");
                },
                Err(e) => {
                    error!("Error polling for interrupt: {:?}", e);
                    // Send error and exit thread on poll error
                     let _ = data_tx.blocking_send(InterruptData::Error(format!(
                        "Error polling for interrupt: {}", e
                    )));
                    break; // Exit loop on poll error
                }
            }
            // No sleep needed here, poll_interrupt blocks until event or timeout
        }

        debug!("Interrupt handler thread terminated");

        // Clean up by disabling the interrupt
        if let Err(e) = drdy_pin.clear_interrupt() {
            error!("Failed to clear interrupt: {:?}", e);
        }
    }); // End of thread::spawn

    // Spawn a Tokio task to process the data from the interrupt thread
    let processing_task_handle = tokio::spawn(async move { // Renamed handle
        // Get base timestamp
        let base_timestamp = {
            let inner = inner_arc.lock().await;
            match inner.base_timestamp {
                Some(ts) => ts,
                None => {
                    error!("Base timestamp not set, using current time");
                    match super::helpers::current_timestamp_micros() {
                        Ok(ts) => ts,
                        Err(_) => {
                            error!("Failed to get current timestamp, using 0");
                            0
                        }
                    }
                }
            }
        };
        
        // Main acquisition loop
        // Buffer to accumulate raw samples and timestamps before creating AdcData
        // Each inner Vec will store batch_size samples for a single channel
        let mut raw_sample_buffer: Vec<VecDeque<i32>> = vec![VecDeque::with_capacity(batch_size); num_channels];
        let mut voltage_sample_buffer: Vec<VecDeque<f32>> = vec![VecDeque::with_capacity(batch_size); num_channels];
        let mut timestamps: VecDeque<u64> = VecDeque::with_capacity(batch_size);
        
        debug!("Starting Tokio task to process data from interrupt handler");
        
        // Use a timeout on the channel receive to periodically check if we should exit
        let mut exit_requested = false;
        while !exit_requested {
            // Check if we should continue running
            let should_continue = {
                let inner = inner_arc.lock().await;
                inner.running
            };
            
            if !should_continue {
                debug!("Processing task detected running=false, preparing to exit");
                exit_requested = true;
                // Don't break yet - process any remaining messages
            }
            
            // Use timeout to periodically check running state
            match tokio::time::timeout(Duration::from_millis(100), data_rx.recv()).await {
                Ok(Some(interrupt_msg)) => {
                    match interrupt_msg {
                        InterruptData::Data(raw_samples, sample_count) => {
                            // Normal data path - process even if exit requested to drain the channel

                    // Calculate timestamp
                    let sample_interval = (1_000_000 / config.sample_rate) as u64;
                    let timestamp = base_timestamp + sample_count * sample_interval;
                    timestamps.push_back(timestamp);

                    // Process raw samples and add to buffers (transposed structure)
                    for (i, &raw) in raw_samples.iter().enumerate() {
                        if i < num_channels {
                            // Add raw sample to buffer
                            raw_sample_buffer[i].push_back(raw);
                            
                            // Convert raw sample to voltage and add to buffer
                            // Removed redundant channel_idx lookup
                            let voltage = ch_raw_to_voltage(raw, config.vref as f32, config.gain as f32);
                            voltage_sample_buffer[i].push_back(voltage);
                        }
                    }

                    // Send the batch when we've collected batch_size samples
                    if timestamps.len() >= batch_size {
                        debug!("Sending batch of {} samples (sample_count: {})",
                            timestamps.len(), sample_count);

                        // Create AdcData for all samples in the batch
                        let mut data_vec = Vec::with_capacity(timestamps.len() * num_channels);
                        for i in 0..timestamps.len() {
                            let timestamp = timestamps[i];
                            for (channel_idx, &channel_num) in config.channels.iter().enumerate() {
                                if let (Some(raw_value), Some(voltage)) = (
                                    raw_sample_buffer.get(channel_idx).and_then(|q| q.get(i)),
                                    voltage_sample_buffer.get(channel_idx).and_then(|q| q.get(i)),
                                ) {
                                    data_vec.push(AdcData {
                                        channel: channel_num,
                                        raw_value: *raw_value,
                                        voltage: *voltage,
                                        timestamp,
                                    });
                                }
                            }
                        }

                        if let Err(e) = tx.send(DriverEvent::Data(data_vec)).await {
                            error!("Ads1299Driver event channel closed: {}", e);
                            break;
                        }

                        // Clear buffers for next batch
                        for channel in &mut raw_sample_buffer {
                            channel.clear();
                        }
                        for channel in &mut voltage_sample_buffer {
                            channel.clear();
                        }
                        timestamps.clear();
                    }
                        }
                        InterruptData::Error(msg) => {
                            error!("Received critical hardware error signal from interrupt thread: {}", msg);
                            if let Err(e) = tx.send(DriverEvent::Error(msg)).await {
                                error!("Failed to send error event: {}", e);
                            }
                            // Update driver status
                            let mut inner = inner_arc.lock().await;
                            inner.status = DriverStatus::Error("Acquisition error".to_string());
                            exit_requested = true; // Exit after this message
                        }
                    }
                },
                Ok(None) => {
                    // Channel closed by sender
                    debug!("Data channel closed by sender, exiting processing task");
                    break;
                },
                Err(_) => {
                    // Timeout - check if we should exit
                    if exit_requested {
                        // We've already processed any remaining messages, now we can exit
                        debug!("Exit requested and timeout reached, exiting processing task");
                        break;
                    }
                    // Otherwise continue waiting for messages
                }
            }
        }
        
        // Send any remaining samples in the buffer before exiting
        if !timestamps.is_empty() {
            debug!("Sending final batch of {} samples before stopping", timestamps.len());

            // Create AdcData for all samples in the batch
            let mut data_vec = Vec::with_capacity(timestamps.len() * num_channels);
            for i in 0..timestamps.len() {
                let timestamp = timestamps[i];
                for (channel_idx, &channel_num) in config.channels.iter().enumerate() {
                    if let (Some(raw_value), Some(voltage)) = (
                        raw_sample_buffer.get(channel_idx).and_then(|q| q.get(i)),
                        voltage_sample_buffer.get(channel_idx).and_then(|q| q.get(i)),
                    ) {
                        data_vec.push(AdcData {
                            channel: channel_num,
                            raw_value: *raw_value,
                            voltage: *voltage,
                            timestamp,
                        });
                    }
                }
            }

            if let Err(e) = tx.send(DriverEvent::Data(data_vec)).await {
                error!("Ads1299Driver event channel closed: {}", e);
            }
        }
        
        debug!("Tokio processing task terminated");
    });
    
    (interrupt_thread_handle, processing_task_handle)
}

// Dummy implementations for cloning
struct DummySpi {}

impl DummySpi {
    fn new() -> Self {
        Self {}
    }
}

impl SpiDevice for DummySpi {
    fn write(&mut self, _data: &[u8]) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Dummy SPI device"))
    }
    
    fn transfer(&mut self, _read: &mut [u8], _write: &[u8]) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Dummy SPI device"))
    }
}

struct DummyInputPin {}

impl DummyInputPin {
    fn new() -> Self {
        Self {}
    }
}

impl InputPinDevice for DummyInputPin {
    fn is_high(&self) -> bool {
        false
    }
    
    fn set_interrupt(&mut self, _trigger: rppal::gpio::Trigger, _timeout: Option<std::time::Duration>) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Dummy input pin"))
    }
    
    fn poll_interrupt(&mut self, _clear: bool, _timeout: Option<std::time::Duration>) -> Result<Option<rppal::gpio::Event>, std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Dummy input pin"))
    }
    
    fn clear_interrupt(&mut self) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Dummy input pin"))
    }
}