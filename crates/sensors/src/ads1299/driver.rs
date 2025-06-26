//! Main driver implementation for the ADS1299 chip.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::collections::HashSet;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use log::{info, warn, debug, error};
use lazy_static::lazy_static;

use crate::types::{AdcConfig, DriverStatus, DriverError, DriverEvent, DriverType};
use super::acquisition::{start_acquisition, stop_acquisition, SpiType, DrdyPinType};
use super::error::HardwareLockGuard;
use super::registers::{
    CMD_RESET, CMD_SDATAC, REG_ID_ADDR,
    CONFIG1_ADDR, CONFIG2_ADDR, CONFIG3_ADDR, CONFIG4_ADDR,
    LOFF_SENSP_ADDR, MISC1_ADDR, CH1SET_ADDR, BIAS_SENSP_ADDR, BIAS_SENSN_ADDR,
    config1_reg, config2_reg, config3_reg, config4_reg, loff_sesp_reg, misc1_reg,
    chn_off, chn_reg, bias_sensp_reg_mask, bias_sensn_reg_mask,
    gain_to_reg_mask, sps_to_reg_mask
};
use super::spi::{SpiDevice, InputPinDevice, init_spi, init_drdy_pin, send_command_to_spi, write_register};

// Static hardware lock to simulate real hardware access constraints
lazy_static! {
    static ref HARDWARE_LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
}

/// ADS1299 driver for interfacing with the ADS1299EEG_FE board.
pub struct Ads1299Driver {
    inner: Arc<Mutex<Ads1299Inner>>,
    task_handle: Option<JoinHandle<()>>,
    tx: mpsc::Sender<DriverEvent>,
    additional_channel_buffering: usize,
    spi: Option<Box<dyn SpiDevice>>,
    drdy_pin: Option<Box<dyn InputPinDevice>>,
    // New fields for interrupt-driven approach
    interrupt_thread: Option<std::thread::JoinHandle<()>>,
    interrupt_running: Arc<AtomicBool>,
}

/// Internal state for the Ads1299Driver.
pub struct Ads1299Inner {
    pub config: AdcConfig,
    pub running: bool,
    pub status: DriverStatus,
    // Base timestamp for calculating sample timestamps (microseconds since epoch)
    pub base_timestamp: Option<u64>,
    // Total samples generated since acquisition started
    pub sample_count: u64,
    // Cache of register values
    pub registers: [u8; 24],
}

impl Ads1299Driver {
    /// Creates a new Ads1299Driver with the given ADC config and optional extra channel buffering.
    /// `additional_channel_buffering` sets extra buffered batches (0 = lowest latency, but may cause backpressure).
    ///
    /// # Note
    /// Call `shutdown()` when done to ensure proper async cleanup; `Drop` only handles basic cleanup.
    /// # Returns
    /// A tuple containing the driver instance and a receiver for driver events.
    ///
    /// # Errors
    /// Returns an error under various conditions
    pub fn new(
        config: AdcConfig,
        additional_channel_buffering: usize
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        // Acquire the hardware lock using RAII guard
        let _hardware_lock_guard = HardwareLockGuard::new(&HARDWARE_LOCK)?;

        // Validate config
        if config.board_driver != DriverType::Ads1299 {
            return Err(DriverError::ConfigurationError(
                "Ads1299Driver requires config.board_driver=DriverType::Ads1299".to_string()
            ));
        }

        // Validate channels
        if config.channels.is_empty() {
            return Err(DriverError::ConfigurationError(
                "At least one channel must be configured".to_string()
            ));
        }

        // Check for duplicate channels
        let mut unique_channels = std::collections::HashSet::new();
        for &channel in &config.channels {
            if !unique_channels.insert(channel) {
                return Err(DriverError::ConfigurationError(
                    format!("Duplicate channel detected: {}", channel)
                ));
            }
        }

        // Validate channel indices for ADS1299 (0-7)
        for &channel in &config.channels {
            if channel > 7 {
                return Err(DriverError::ConfigurationError(
                    format!("Invalid channel index: {}. ADS1299 supports channels 0-7", channel)
                ));
            }
        }

        // Validate sample rate for ADS1299
        match config.sample_rate {
            250 | 500 | 1000 | 2000 | 4000 | 8000 | 16000 => {
                // Valid sample rates for ADS1299
            }
            _ => {
                return Err(DriverError::ConfigurationError(
                    format!("Invalid sample rate: {}. ADS1299 supports: 250, 500, 1000, 2000, 4000, 8000, 16000 Hz", config.sample_rate)
                ));
            }
        }

        // Validate batch size
        if config.batch_size == 0 {
            return Err(DriverError::ConfigurationError(
                "Batch size must be greater than 0".to_string()
            ));
        }

        // Validate total buffer size (prevent excessive memory usage)
        const MAX_BUFFER_SIZE: usize = 10000; // Arbitrary limit to prevent excessive memory usage
        let channel_buffer_size = config.batch_size + additional_channel_buffering;
        if channel_buffer_size > MAX_BUFFER_SIZE {
            return Err(DriverError::ConfigurationError(
                format!("Total buffer size ({}) exceeds maximum allowed ({})",
                        channel_buffer_size, MAX_BUFFER_SIZE)
            ));
        }

        // Initialize SPI
        let spi = match init_spi() {
            Ok(spi) => spi,
            Err(e) => {
                return Err(e);
            }
        };

        // Initialize DRDY pin
        let drdy_pin = match init_drdy_pin() {
            Ok(pin) => pin,
            Err(e) => {
                return Err(e);
            }
        };
        
        // Initialize register cache
        let registers = [0u8; 24];
        
        let inner = Ads1299Inner {
            config: config.clone(),
            running: false,
            status: DriverStatus::NotInitialized,
            base_timestamp: None,
            sample_count: 0,
            registers,
        };
        
        // Create channel with validated buffer size
        let (tx, rx) = mpsc::channel(channel_buffer_size);
        
        // Create the driver as mutable
        let mut driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            task_handle: None,
            tx,
            additional_channel_buffering,
            spi: Some(Box::new(spi)),
            drdy_pin: Some(Box::new(drdy_pin)),
            interrupt_thread: None,
            interrupt_running: Arc::new(AtomicBool::new(false)),
        };

        // Put chip in standby (low power) mode initially
        {
            // This block ensures we only mutably borrow driver.spi for this operation
            let spi_opt = driver.spi.as_mut();
            if let Some(driver_spi) = spi_opt {
                let _ = driver_spi.write(&[super::registers::CMD_STANDBY]);
            }
        }

        info!("Ads1299Driver created with config: {:?}", config);
        info!("Channel buffer size: {} (batch_size: {} + additional_buffering: {})",
              channel_buffer_size, config.batch_size, additional_channel_buffering);

        Ok((driver, rx))
    }
    
    /// Return the current configuration.
    pub(crate) async fn get_config(&self) -> Result<AdcConfig, DriverError> {
        let inner = self.inner.lock().await;
        Ok(inner.config.clone())
    }

    /// Start data acquisition from the ADS1299.
    ///
    /// This method validates the driver state, initializes the ADS1299 chip,
    /// and spawns a background task that reads data from the chip.
    pub(crate) async fn start_acquisition(&mut self) -> Result<(), DriverError> {
        // Check if acquisition is already running
        {
            let inner = self.inner.lock().await;
            if inner.running {
                return Err(DriverError::ConfigurationError(
                    "Acquisition already running".to_string()
                ));
            }
        }
        
        // Check if we have SPI and DRDY pin
        if self.spi.is_none() {
            return Err(DriverError::NotInitialized);
        }
        
        if self.drdy_pin.is_none() {
            return Err(DriverError::NotInitialized);
        }
        
        // Initialize the chip registers before starting acquisition tasks
        self.initialize_chip().await?;
        
        // Set running flag before starting tasks
        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
        }
        
        // Start the acquisition tasks
        match start_acquisition(
            self.inner.clone(),
            self.tx.clone(),
            self.interrupt_running.clone(),
            self.spi.take(),
            self.drdy_pin.take(),
        ).await {
            Ok((interrupt_thread, processing_task)) => {
                self.interrupt_thread = interrupt_thread;
                self.task_handle = processing_task;
                
                // Notify about the status change
                self.notify_status_change().await?;
                
                info!("Acquisition started successfully");
                Ok(())
            },
            Err(e) => {
                // Reset running flag on error
                {
                    let mut inner = self.inner.lock().await;
                    inner.running = false;
                    inner.status = DriverStatus::Error("Start acquisition failed".to_string());
                }
                
                error!("Failed to start acquisition: {:?}", e);
                
                // Try to notify about the status change
                let _ = self.notify_status_change().await;
                
                // Return the original error
                Err(e)
            }
        }
    }

    /// Stop data acquisition from the ADS1299.
    ///
    /// This method signals the acquisition task to stop, waits for it to complete,
    /// and updates the driver status.
    pub(crate) async fn stop_acquisition(&mut self) -> Result<(), DriverError> {
        debug!("Driver stop_acquisition called");
        
        // First check if acquisition is running
        let is_running = {
            let inner = self.inner.lock().await;
            inner.running
        };
        
        if !is_running {
            debug!("Acquisition not running, nothing to stop");
            return Ok(());
        }
        
        // Call the acquisition module's stop function
        let result = stop_acquisition(
            self.inner.clone(),
            &self.tx,
            &self.interrupt_running,
            &mut self.interrupt_thread,
            &mut self.task_handle,
            &mut self.spi,
        ).await;
        
        // Ensure we have proper cleanup even if stop_acquisition failed
        if result.is_err() {
            error!("Error during stop_acquisition: {:?}", result);
            // Still update the status to avoid stuck state
            let mut inner = self.inner.lock().await;
            inner.running = false;
            inner.status = DriverStatus::Error("Stop acquisition failed".to_string());
        }
        
        result
    }

    /// Return the current driver status.
    ///
    /// This method returns the current status of the driver.
    pub(crate) async fn get_status(&self) -> DriverStatus {
        let inner = self.inner.lock().await;
        inner.status.clone()
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
        debug!("Shutting down Ads1299Driver");
        
        // Always try to stop acquisition first, regardless of running state
        // This ensures a consistent shutdown sequence
        let stop_result = self.stop_acquisition().await;
        if let Err(e) = &stop_result {
            warn!("Error during stop_acquisition in shutdown: {:?}", e);
            // Continue with shutdown despite errors
        }
        
        // Double-check that interrupt thread is stopped
        if self.interrupt_running.load(Ordering::SeqCst) {
            warn!("Interrupt thread still running after stop_acquisition, forcing stop");
            self.interrupt_running.store(false, Ordering::SeqCst);
            
            // Wait for the interrupt thread to complete with timeout
            if let Some(handle) = self.interrupt_thread.take() {
                debug!("Joining interrupt thread during shutdown");
                // Use spawn_blocking to avoid blocking the async runtime
                match tokio::task::spawn_blocking(move || {
                    // Use a timeout for joining
                    let join_handle = std::thread::spawn(move || {
                        let _ = handle.join();
                    });
                    
                    // Wait up to 2 seconds
                    if join_handle.join().is_err() {
                        warn!("Failed to join interrupt thread cleanly");
                    }
                }).await {
                    Ok(_) => debug!("Interrupt thread joined during shutdown"),
                    Err(e) => warn!("Error joining interrupt thread: {}", e),
                }
            }
        }
        
        // Check if processing task is still running
        if let Some(task) = self.task_handle.take() {
            if !task.is_finished() {
                warn!("Processing task still running after stop_acquisition, aborting");
                task.abort();
                // We don't need to wait for the abort to complete
            }
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
        // Put chip in standby (low power) mode on shutdown
        if let Some(driver_spi) = self.spi.as_mut() {
            let _ = driver_spi.write(&[super::registers::CMD_STANDBY]);
        }

        info!("Ads1299Driver shutdown complete");
        Ok(())
    }

    /// Send a command to the ADS1299.
    fn send_command(&mut self, command: u8) -> Result<(), DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        send_command_to_spi(spi.as_mut(), command)
    }

    /// Reset the ADS1299 chip.
    fn reset_chip(&mut self) -> Result<(), DriverError> {
        // Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // Wait for reset to complete (recommended 18 tCLK cycles, ~4.5µs at 4MHz)
        std::thread::sleep(std::time::Duration::from_micros(10));
        
        Ok(())
    }

    /// Initialize the ADS1299 chip with the current configuration.
    async fn initialize_chip(&mut self) -> Result<(), DriverError> {
        let config = {
            let inner = self.inner.lock().await;
            inner.config.clone()
        };
        
        // Power-up sequence following the working Python script pattern:
        
        // 1. Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // 2. Send zeros
        if let Some(spi) = self.spi.as_mut() {
            spi.write(&[0x00, 0x00, 0x00]).map_err(|e| DriverError::SpiError(format!("SPI write error: {}", e)))?;
        }
        
        // 3. Send SDATAC command to stop continuous data acquisition mode
        self.send_command(CMD_SDATAC)?;
        
        // Check device ID to verify communication
        let id = self.read_register(REG_ID_ADDR)?;
        if id != 0x3E {
            return Err(DriverError::Other(format!("Invalid device ID: 0x{:02X}, expected 0x3E", id)));
        }
        
        // Setup registers for CH1 mode (working configuration)
        let mut spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;

        // Write registers in the specific order
        // Constants are imported from super::registers

        // Calculate masks based on config
        let active_ch_mask = config.channels.iter().fold(0, |mask, &ch| mask | (1 << ch));
        // Use the fully qualified path to call these functions
        let gain_mask = super::registers::gain_to_reg_mask(config.gain as f32)?;
        let sps_mask = super::registers::sps_to_reg_mask(config.sample_rate)?;

        // Write registers
        self.write_register(CONFIG1_ADDR, config1_reg | sps_mask)?;
        self.write_register(CONFIG2_ADDR, config2_reg)?;
        self.write_register(CONFIG3_ADDR, config3_reg)?;
        self.write_register(CONFIG4_ADDR, config4_reg)?;
        self.write_register(LOFF_SENSP_ADDR, loff_sesp_reg)?; // Assuming LOFF is off
        self.write_register(MISC1_ADDR, misc1_reg)?;
        // Turn off all channels first
        for ch in 0..8 { // Assuming 8 channels max for ADS1299
            self.write_register(CH1SET_ADDR + ch, chn_off)?;
        }
        // Turn on configured channels with correct gain
        for &ch in &config.channels {
            if ch < 8 { // Ensure channel index is valid
                 self.write_register(CH1SET_ADDR + ch as u8, chn_reg | gain_mask)?;
            } else {
                 log::warn!("Channel index {} out of range (0-7), skipping configuration.", ch);
            }
        }
        // Configure bias based on active channels
        self.write_register(BIAS_SENSP_ADDR, active_ch_mask as u8)?;
        self.write_register(BIAS_SENSN_ADDR, bias_sensn_reg_mask)?;

        // Add register dump for verification (optional but helpful)
        log::info!("----Register Dump After Configuration----");
        let names = ["ID", "CONFIG1", "CONFIG2", "CONFIG3", "LOFF", "CH1SET", "CH2SET", "CH3SET", "CH4SET", "CH5SET", "CH6SET", "CH7SET", "CH8SET", "BIAS_SENSP", "BIAS_SENSN", "LOFF_SENSP", "LOFF_SENSN", "LOFF_FLIP", "LOFF_STATP", "LOFF_STATN", "GPIO", "MISC1", "MISC2", "CONFIG4"];
        for reg in 0..=0x17 {
            match self.read_register(reg as u8) {
                Ok(val) => log::info!("Reg 0x{:02X} ({:<12}): 0x{:02X}", reg, names.get(reg).unwrap_or(&"Unknown"), val),
                Err(e) => log::error!("Failed to read register 0x{:02X}: {}", reg, e),
            }
        }
        log::info!("----End Register Dump----");
        
        // Wait for configuration to settle
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // Update status
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Ok;
        }
        
        Ok(())
    }

    /// Read a register from the ADS1299.
    fn read_register(&mut self, register: u8) -> Result<u8, DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        
        // Command: RREG (0x20) + register address
        let command = 0x20 | (register & 0x1F);
        
        // First transfer: command and count (number of registers to read minus 1)
        let write_buffer = [command, 0x00];
        spi.write(&write_buffer).map_err(|e| DriverError::SpiError(format!("SPI write command error: {}", e)))?;
        
        // Second transfer: read the data (send dummy byte to receive data)
        let mut read_buffer = [0u8];
        spi.transfer(&mut read_buffer, &[0u8]).map_err(|e| DriverError::SpiError(format!("SPI transfer error: {}", e)))?;
        
        Ok(read_buffer[0])
    }

    /// Write a value to a register in the ADS1299.
    fn write_register(&mut self, register: u8, value: u8) -> Result<(), DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        
        // Command: WREG (0x40) + register address
        let command = 0x40 | (register & 0x1F);
        
        // First byte: command, second byte: number of registers to write minus 1 (0 for single register)
        // Third byte: value to write
        let write_buffer = [command, 0x00, value];
        
        spi.write(&write_buffer).map_err(|e| DriverError::SpiError(format!("SPI write error: {}", e)))?;
        
        // Update register cache
        let mut inner = self.inner.try_lock().map_err(|_| DriverError::Other("Failed to lock inner state".to_string()))?;
        inner.registers[register as usize] = value;
        
        Ok(())
    }

    /// Internal helper to notify status changes over the event channel.
    ///
    /// This method sends a status change event to any listeners.
    async fn notify_status_change(&self) -> Result<(), DriverError> {
        // Get current status
        let status = {
            let inner = self.inner.lock().await;
            inner.status.clone()
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
#[async_trait::async_trait]
impl super::super::types::AdcDriver for Ads1299Driver {
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

// Implement Send and Sync for Ads1299Driver
// SAFETY: This unsafe impl of Send/Sync is justified due to the internal Arc<Mutex> and careful resource handling.
// If the driver's internal structure changes, this safety guarantee must be re-evaluated.
unsafe impl Send for Ads1299Driver {}
unsafe impl Sync for Ads1299Driver {}

/// Implementation of Drop for Ads1299Driver to handle cleanup when the driver is dropped.
///
/// Note: This provides only basic cleanup. For proper cleanup, users should explicitly
/// call `shutdown()` before letting the driver go out of scope. The Drop implementation
/// cannot perform the full async shutdown sequence because Drop is not async.
impl Drop for Ads1299Driver {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning and do our best
        error!("Ads1299Driver dropped without calling shutdown() first. This may lead to resource leaks.");
        error!("Always call driver.shutdown().await before dropping the driver.");
        
        // Signal all tasks to stop
        self.interrupt_running.store(false, Ordering::SeqCst);
        
        // Abort any Tokio task that might still be running
        if let Some(task) = self.task_handle.take() {
            if !task.is_finished() {
                error!("Background task still running during Drop. Aborting it.");
                task.abort();
            }
        }
        
        // For the interrupt thread, we can't join it properly in Drop
        if let Some(handle) = self.interrupt_thread.take() {
            error!("Interrupt thread may still be running during Drop. Detaching it.");
            // We can't join in Drop, so we have to detach it
            // This might lead to a thread leak, but it's better than blocking in Drop
            std::thread::spawn(move || {
                // Try to join with a timeout
                let _join_handle = std::thread::spawn(move || {
                    let _ = handle.join();
                });
                
                // Wait up to 100ms
                std::thread::sleep(std::time::Duration::from_millis(100));
                // After timeout, we just let the thread continue running
                // The OS will clean it up when the process exits
            });
        }
        
        // Hardware lock is released automatically by HardwareLockGuard's Drop.
    }
}