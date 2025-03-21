use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use async_trait::async_trait;
use log::{info, warn, debug, trace, error};
use lazy_static::lazy_static;
use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use rppal::gpio::{Gpio, InputPin};
use super::types::{AdcConfig, AdcData, DriverStatus, DriverError, DriverEvent, DriverType};

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
    spi: Option<Spi>,
    drdy_pin: Option<InputPin>,
}

/// Internal state for the Ads1299Driver.
struct Ads1299Inner {
    config: AdcConfig,
    running: bool,
    status: DriverStatus,
    // Base timestamp for calculating sample timestamps (microseconds since epoch)
    base_timestamp: Option<u64>,
    // Total samples generated since acquisition started
    sample_count: u64,
    // Cache of register values
    registers: [u8; 24],
}

// ADS1299 Register Value Constants
const MUX_NORMAL: u8 = 0 << 0;
const PD_REFBUF: u8 = 1 << 7;      // 1 : Enable internal reference buffer
const BIAS_MEAS: u8 = 1 << 4;      // 1 : BIAS_IN signal is routed to the channel that has the MUX_Setting 010 (VREF)
const BIASREF_INT: u8 = 1 << 3;    // 1 : BIASREF signal (AVDD + AVSS) / 2 generated internally
const PD_BIAS: u8 = 1 << 2;        // 1 : BIAS buffer is enabled
const BIAS_LOFF_SENS: u8 = 1 << 1; // 1 : BIAS sense is enabled
const SRB1: u8 = 1 << 5;           // 1 : Switches closed.. This bit connects the SRB1 to all 4, 6, or 8 channels inverting inputs
const DC_TEST: u8 = 3 << 0;
const POWER_OFF_CH: u8 = 0x81;
const BIAS_SENS_OFF_MASK: u8 = 0x00;

// Register Setup
const REG_ID_ADDR    : u8 = 0x00;
const CONFIG1_ADDR   : u8 = 0x01; const config1_reg: u8 = 0x90;
const CONFIG2_ADDR   : u8 = 0x02; const config2_reg: u8 = 0xD0 | DC_TEST;
const CONFIG3_ADDR   : u8 = 0x03; const config3_reg: u8 = 0x60 | BIASREF_INT | PD_BIAS | PD_REFBUF;
const LOFF_ADDR      : u8 = 0x04;
const CH1SET_ADDR    : u8 = 0x05; const chn_reg    : u8 = 0x00 | MUX_NORMAL;
                                  const chn_off    : u8 = 0x00 | POWER_OFF_CH;
const BIAS_SENSP_ADDR: u8 = 0x0D; const bias_sensp_reg_mask : u8 = BIAS_SENS_OFF_MASK;
const BIAS_SENSN_ADDR: u8 = 0x0E; const bias_sensn_reg_mask : u8 = BIAS_SENS_OFF_MASK;
const LOFF_SENSP_ADDR: u8 = 0x0F; const loff_sesp_reg: u8 = 0x00;
const MISC1_ADDR     : u8 = 0x15; const misc1_reg   : u8 = 0x00 | SRB1;
const CONFIG4_ADDR   : u8 = 0x17; const config4_reg : u8 = 0x00;


// ADS1299 Commands
const CMD_WAKEUP: u8 = 0x02;
const CMD_STANDBY: u8 = 0x04;
const CMD_RESET: u8 = 0x06;
const CMD_START: u8 = 0x08;
const CMD_STOP: u8 = 0x0A;
const CMD_RDATAC: u8 = 0x10;
const CMD_SDATAC: u8 = 0x11;
const CMD_RDATA: u8 = 0x12;


impl Ads1299Driver {
    /// Create a new instance of the Ads1299Driver.
    ///
    /// This constructor takes an ADC configuration and an optional additional channel buffering parameter.
    /// The additional_channel_buffering parameter determines how many extra batches can be buffered in the channel
    /// beyond the minimum required (which is the batch_size from the config). Setting this to 0 minimizes
    /// latency but may cause backpressure if the consumer can't keep up.
    ///
    /// # Important
    /// Users should explicitly call `shutdown()` when done with the driver to ensure proper cleanup.
    /// While the Drop implementation provides some basic cleanup, it cannot perform the full async shutdown sequence.
    ///
    /// # Returns
    /// A tuple containing the driver instance and a receiver for driver events.
    ///
    /// # Errors
    /// Returns an error if:
    /// - config.board_driver is not DriverType::Ads1299
    /// - config.batch_size is 0 (batch size must be positive)
    /// - config.batch_size is less than the number of channels (need at least one sample per channel)
    /// - SPI or GPIO initialization fails
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
        if config.board_driver != DriverType::Ads1299 {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "Ads1299Driver requires config.board_driver=DriverType::Ads1299".to_string()
            ));
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
        
        // Initialize SPI
        let spi = match Self::init_spi() {
            Ok(spi) => spi,
            Err(e) => {
                // Release the lock if we're returning an error
                *hardware_in_use = false;
                return Err(e);
            }
        };
        
        // Initialize DRDY pin
        let drdy_pin = match Self::init_drdy_pin() {
            Ok(pin) => pin,
            Err(e) => {
                // Release the lock if we're returning an error
                *hardware_in_use = false;
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
        
        let driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            task_handle: None,
            tx,
            additional_channel_buffering,
            spi: Some(spi),
            drdy_pin: Some(drdy_pin),
        };
        
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
        // Check preconditions without holding the lock for too long
        {
            let inner = self.inner.lock().await;
                
            if inner.running {
                return Err(DriverError::ConfigurationError("Acquisition already running".to_string()));
            }
        }
        
        // Initialize the chip
        self.initialize_chip().await?;
        
        // Get the initial timestamp
        let start_time = current_timestamp_micros()?;
        
        // Update state to running
        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.status = DriverStatus::Running;
            inner.base_timestamp = Some(start_time);
            inner.sample_count = 0;
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        
        // Check DRDY pin state before starting
        if let Some(drdy_pin) = &self.drdy_pin {
            let drdy_state = if drdy_pin.is_high() { "HIGH" } else { "LOW" };
            debug!("DRDY pin state before starting conversion: {}", drdy_state);
            
            // DRDY should be high when idle
            if !drdy_pin.is_high() {
                warn!("DRDY pin is LOW before starting conversion, which is unexpected. This may indicate a hardware issue.");
            }
        }
        
        // Start conversion
        self.start_conversion()?;
        debug!("Conversion started");
        
        // Prepare for background task
        let inner_arc = self.inner.clone();
        let tx = self.tx.clone();
        let drdy_pin = self.drdy_pin.take().ok_or(DriverError::NotInitialized)?;
        let mut spi = self.spi.take().ok_or(DriverError::NotInitialized)?;
        
        // Spawn a task that monitors DRDY pin and reads data
        let handle = tokio::spawn(async move {
            // Get configuration and base timestamp
            let (config, base_timestamp) = {
                let inner = inner_arc.lock().await;
                (inner.config.clone(), inner.base_timestamp.expect("Base timestamp should be set"))
            };
            
            // Get batch size from config
            let batch_size = config.batch_size;
            let num_channels = config.channels.len();
            
            info!("Starting acquisition with batch size: {}, sample rate: {} Hz",
                   batch_size, config.sample_rate);
            info!("Buffering {} samples before sending batches", batch_size);
            
            // Make sure we're in SDATAC mode before starting
            if let Err(e) = send_command_to_spi(&mut spi, CMD_SDATAC) {
                error!("Failed to send SDATAC command: {:?}", e);
                return;
            }
            
            // Small delay after sending command
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            
            // Enter continuous data mode (RDATAC)
            if let Err(e) = send_command_to_spi(&mut spi, CMD_RDATAC) {
                error!("Failed to send RDATAC command: {:?}", e);
                return;
            }
            
            // Small delay after sending command
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            
            debug!("Starting acquisition loop using RDATAC (continuous data mode)");
            
            // Main acquisition loop
            let mut sample_buffer: Vec<AdcData> = Vec::with_capacity(batch_size);
            
            loop {
                // Check if we should continue running
                let (should_continue, current_sample_count) = {
                    let mut inner = inner_arc.lock().await;
                    if !inner.running {
                        (false, 0)
                    } else {
                        let count = inner.sample_count;
                        // Update the sample count for the next sample
                        inner.sample_count += 1;
                        (true, count)
                    }
                };
                
                if !should_continue {
                    // Send any remaining samples in the buffer before breaking
                    if !sample_buffer.is_empty() {
                        debug!("Sending final batch of {} samples before stopping", sample_buffer.len());
                        if let Err(e) = tx.send(DriverEvent::Data(sample_buffer)).await {
                            error!("Ads1299Driver event channel closed: {}", e);
                        }
                    }
                    break;
                }
                
                // Measure DRDY timing for debugging
                let drdy_start = std::time::Instant::now();
                
                // Wait for DRDY to go low (active low) with timeout
                let mut timeout = 1000; // Adjust based on sample rate
                while drdy_pin.is_high() && timeout > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
                    timeout -= 1;
                }
                
                let drdy_duration = drdy_start.elapsed();
                
                if timeout == 0 {
                    error!("DRDY timeout - pin never went low");
                    continue; // Skip this sample and try again
                } else {
                    // Only log timing occasionally to avoid flooding logs
                    if current_sample_count % 100 == 0 {
                        debug!("DRDY timing: {:?} (timeout count: {})", drdy_duration, 1000 - timeout);
                    }
                }
                
                // Read data from ADS1299
                let raw_samples = match read_data_from_spi(&mut spi, num_channels) {
                    Ok(samples) => samples,
                    Err(e) => {
                        error!("Error reading data: {:?}", e);
                        continue;
                    }
                };
                
                // Calculate timestamp
                let sample_interval = (1_000_000 / config.sample_rate) as u64;
                let timestamp = base_timestamp + current_sample_count * sample_interval;
                
                // Convert raw samples to voltage
                let mut voltage_samples = Vec::with_capacity(num_channels);
                for (i, &raw) in raw_samples.iter().enumerate() {
                    let channel_idx = config.channels[i];
                    let voltage = ch_raw_to_voltage(raw, config.Vref, config.gain);
                    voltage_samples.push(vec![voltage]);
                }
                
                // Create AdcData
                let data = AdcData {
                    timestamp,
                    raw_samples: raw_samples.iter().map(|&s| vec![s]).collect(),
                    voltage_samples,
                };
                
                // Add to buffer
                sample_buffer.push(data);
                
                // Send the batch when we've collected batch_size samples
                if sample_buffer.len() >= batch_size {
                    info!("Sending batch of {} samples (current_sample_count: {})",
                          sample_buffer.len(), current_sample_count);
                    if let Err(e) = tx.send(DriverEvent::Data(sample_buffer)).await {
                        error!("Ads1299Driver event channel closed: {}", e);
                        break;
                    }
                    // Create a new buffer for the next batch
                    sample_buffer = Vec::with_capacity(batch_size);
                }
            }
            
            // We're already in SDATAC mode, so no need to send it again
            debug!("Acquisition loop terminated");
            
            debug!("Acquisition task terminated");
        });
        
        self.task_handle = Some(handle);
        info!("Ads1299Driver acquisition started");
        Ok(())
    }

    /// Stop data acquisition from the ADS1299.
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
        
        // Exit continuous data mode and stop conversion
        if let Some(spi) = self.spi.as_mut() {
            // First exit RDATAC mode
            if let Err(e) = send_command_to_spi(spi, CMD_SDATAC) {
                warn!("Failed to send SDATAC command during stop_acquisition: {:?}", e);
                // Continue anyway to try to stop conversion
            }
            
            // Small delay after sending command
            std::thread::sleep(std::time::Duration::from_millis(5));
            
            // Then stop conversion
            self.stop_conversion()?;
        }
        
        // Wait for the task to complete
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(_) => debug!("Acquisition task completed successfully"),
                Err(e) => warn!("Acquisition task terminated with error: {}", e),
            }
        }
        
        // Update driver status
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Stopped;
            inner.sample_count = 0;
            // Keep the base_timestamp as it is - we'll set a new one when acquisition starts again
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        info!("Ads1299Driver acquisition stopped");
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
        debug!("Shutting down Ads1299Driver");
        
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
        info!("Ads1299Driver shutdown complete");
        Ok(())
    }

    /// Initialize SPI communication with the ADS1299.
    fn init_spi() -> Result<Spi, DriverError> {
        // Use SPI0 with CS0 at 500kHz (matching the working Python script)
        // ADS1299 datasheet specifies CPOL=0, CPHA=1 (Mode 1)
        let spi_speed = 500_000; // 500kHz - confirmed working with Python script
        info!("Initializing SPI with speed: {} Hz, Mode: Mode1 (CPOL=0, CPHA=1)", spi_speed);
        
        // For debugging, try to create a mock SPI device if the real one fails
        match Spi::new(
            Bus::Spi0,
            SlaveSelect::Ss0,
            spi_speed,
            Mode::Mode1,  // CPOL=0, CPHA=1 for ADS1299
        ) {
            Ok(spi) => {
                info!("SPI initialization successful");
                Ok(spi)
            },
            Err(e) => {
                error!("SPI initialization error: {}", e);
                error!("This could be because the SPI device is not available or the user doesn't have permission to access it.");
                error!("Make sure the SPI interface is enabled and the user has permission to access it.");
                
                // Return the error
                Err(DriverError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("SPI initialization error: {}", e)
                )))
            }
        }
    }

    /// Initialize the DRDY pin for detecting when new data is available.
    fn init_drdy_pin() -> Result<InputPin, DriverError> {
        info!("Initializing GPIO for DRDY pin (GPIO25)");
        
        match Gpio::new() {
            Ok(gpio) => {
                info!("GPIO initialization successful");
                
                // GPIO25 (Pin 22) is used for DRDY
                match gpio.get(25) {
                    Ok(pin) => {
                        info!("GPIO pin 25 acquired successfully");
                        Ok(pin.into_input_pullup())
                    },
                    Err(e) => {
                        error!("GPIO pin 25 error: {}", e);
                        error!("This could be because the GPIO pin is already in use or the user doesn't have permission to access it.");
                        error!("Make sure the GPIO interface is enabled and the user has permission to access it.");
                        
                        Err(DriverError::IoError(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("GPIO pin error: {}", e)
                        )))
                    }
                }
            },
            Err(e) => {
                error!("GPIO initialization error: {}", e);
                error!("This could be because the GPIO interface is not available or the user doesn't have permission to access it.");
                error!("Make sure the GPIO interface is enabled and the user has permission to access it.");
                
                Err(DriverError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("GPIO initialization error: {}", e)
                )))
            }
        }
    }

    /// Send a command to the ADS1299.
    fn send_command(&mut self, command: u8) -> Result<(), DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        send_command_to_spi(spi, command)
    }

    /// Read a register from the ADS1299.
    fn read_register(&mut self, register: u8) -> Result<u8, DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        
        // Command: RREG (0x20) + register address
        let command = 0x20 | (register & 0x1F);
        
        // First transfer: command and count (number of registers to read minus 1)
        let write_buffer = [command, 0x00];
        spi.write(&write_buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI write command error: {}", e)
        )))?;
        
        // Second transfer: read the data (send dummy byte to receive data)
        let mut read_buffer = [0u8];
        spi.transfer(&mut read_buffer, &[0u8]).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI transfer error: {}", e)
        )))?;
        
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
        
        spi.write(&write_buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI write error: {}", e)
        )))?;
        
        // Update register cache
        let mut inner = self.inner.try_lock().map_err(|_| DriverError::Other("Failed to lock inner state".to_string()))?;
        inner.registers[register as usize] = value;
        
        Ok(())
    }

    /// Read data from the ADS1299.
    fn read_data(&mut self) -> Result<Vec<i32>, DriverError> {
        // Get the number of channels
        let num_channels = {
            let inner = self.inner.try_lock().map_err(|_| DriverError::Other("Failed to lock inner state".to_string()))?;
            inner.config.channels.len()
        };
        
        // Get SPI and read data
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        read_data_from_spi(spi, num_channels)
    }

    /// Reset the ADS1299 chip.
    fn reset_chip(&mut self) -> Result<(), DriverError> {
        // Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // Wait for reset to complete (recommended 18 tCLK cycles, ~4.5µs at 4MHz)
        std::thread::sleep(std::time::Duration::from_micros(10));
        
        Ok(())
    }

    /// Start conversion on the ADS1299.
    fn start_conversion(&mut self) -> Result<(), DriverError> {
        // Send START command (0x08)
        self.send_command(CMD_START)?;
        Ok(())
    }

    /// Stop conversion on the ADS1299.
    fn stop_conversion(&mut self) -> Result<(), DriverError> {
        // Send STOP command (0x0A)
        self.send_command(CMD_STOP)?;
        Ok(())
    }

    /// Convert gain value to register mask.
    fn gain_to_reg_mask(&self, gain: f32) -> Result<u8, DriverError> {
        match gain as u8 {
            1 => Ok(0 << 4),
            2 => Ok(1 << 4),
            4 => Ok(2 << 4),
            6 => Ok(3 << 4),
            8 => Ok(4 << 4),
            12 => Ok(5 << 4),
            24 => Ok(6 << 4),
            _ => Err(DriverError::ConfigurationError(
                format!("Unsupported gain: {}. Supported gains: 1, 2, 4, 6, 8, 12, 24", gain)
            )),
        }
    }

    /// Convert samples per second value to register mask.
    fn sps_to_reg_mask(&self, sps: u32) -> Result<u8, DriverError> {
        match sps {
            250 => Ok(6 << 0),
            500 => Ok(5 << 0),
            1000 => Ok(4 << 0),
            2000 => Ok(3 << 0),
            4000 => Ok(2 << 0),
            8000 => Ok(1 << 0),
            16_000 => Ok(0 << 0),
            _ => Err(DriverError::ConfigurationError(
                format!("Unsupported samples per second: {}. Supported sps: 250, 500, 1000, 2000, 4000, 8000, 16000", sps)
            )),
        }
    }

    /// Initialize the ADS1299 chip with the current configuration.
    async fn initialize_chip(&mut self) -> Result<(), DriverError> {
        let config = {
            let inner = self.inner.lock().await;
            inner.config.clone()
        };
        let active_ch_mask = config.channels.iter().fold(0, |mask, &ch| mask | (1 << ch));
        let gain_mask = self.gain_to_reg_mask(config.gain)?;
        let sps_mask = self.sps_to_reg_mask(config.sample_rate)?;

        // Power-up sequence following the working Python script pattern:
        
        // 1. Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // 2. Send zeros
        if let Some(spi) = self.spi.as_mut() {
            spi.write(&[0x00, 0x00, 0x00]).map_err(|e| DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI write error: {}", e)
            )))?;
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
        write_register(&mut spi, CONFIG1_ADDR, config1_reg | sps_mask)?;
        write_register(&mut spi, CONFIG2_ADDR, config2_reg)?;
        write_register(&mut spi, CONFIG3_ADDR, config3_reg)?;
        write_register(&mut spi, CONFIG4_ADDR, config4_reg)?;
        write_register(&mut spi, LOFF_SENSP_ADDR, loff_sesp_reg)?;
        write_register(&mut spi, MISC1_ADDR, misc1_reg)?;
        for ch in 0..=7             { write_register(&mut spi, CH1SET_ADDR + ch, chn_off)?; }
        for &ch in &config.channels { write_register(&mut spi, CH1SET_ADDR + ch as u8, chn_reg | gain_mask)?; }
        write_register(&mut spi, BIAS_SENSP_ADDR, bias_sensp_reg_mask & active_ch_mask)?;
        write_register(&mut spi, BIAS_SENSN_ADDR, bias_sensn_reg_mask & active_ch_mask)?;
        
        // Wait for configuration to settle
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        println!("----Register Dump----");
        let names = ["ID", "CONFIG1", "CONFIG2", "CONFIG3", "LOFF", "CH1SET", "CH2SET", "CH3SET", "CH4SET", "CH5SET", "CH6SET", "CH7SET", "CH8SET", "BIAS_SENSP", "BIAS_SENSN", "LOFF_SENSP", "LOFF_SENSN", "LOFF_FLIP", "LOFF_STATP", "LOFF_STATN", "GPIO", "MISC1", "MISC2", "CONFIG4"];
        for reg in 0..=0x17 {println!("0x{:02X} - 0x{:02X} {}", reg, self.read_register(reg as u8)?, names[reg]);}
        println!("----Register Dump----");
        
        // Update status
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Ok;
        }
        
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

// Helper function to send a command to SPI
fn send_command_to_spi(spi: &mut Spi, command: u8) -> Result<(), DriverError> {
    let buffer = [command];
    spi.write(&buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("SPI write error: {}", e)
    )))?;
    Ok(())
}


/// Convert 24-bit SPI data to a signed 32-bit integer (sign-extended)
fn ch_sample_to_raw(msb: u8, mid: u8, lsb: u8) -> i32 {
    let raw_value = ((msb as u32) << 16) | ((mid as u32) << 8) | (lsb as u32);
    ((raw_value as i32) << 8) >> 8
}

/// Convert signed raw ADC value to voltage using VREF and gain
/// Formula: voltage = (raw * (VREF / Gain)) / 2^23
fn ch_raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
    ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
}

// Helper function to read data from SPI in continuous mode (RDATAC)
fn read_data_from_spi(spi: &mut Spi, num_channels: usize) -> Result<Vec<i32>, DriverError> {
    debug!("Reading data from ADS1299 via SPI for {} channels in continuous mode", num_channels);
    
    // In continuous mode (RDATAC), we don't need to send RDATA command before each read
    // We just read the data directly when DRDY goes low
    
    // Calculate total bytes to read: 3 status bytes + (3 bytes per channel * num_channels)
    let total_bytes = 3 + (3 * num_channels);
    debug!("Reading {} total bytes (3 status + {} data bytes)", total_bytes, 3 * num_channels);
    
    // Prepare buffers for SPI transfer
    let mut read_buffer = vec![0u8; total_bytes];
    let write_buffer = vec![0u8; total_bytes];
    
    // Perform SPI transfer
    match spi.transfer(&mut read_buffer, &write_buffer) {
        Ok(_) => debug!("SPI transfer successful, read {} bytes", read_buffer.len()),
        Err(e) => {
            error!("SPI transfer error: {}", e);
            return Err(DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI transfer error: {}", e)
            )));
        }
    }
    
    // Log raw data for debugging
    debug!("Raw SPI data: {:02X?}", read_buffer);
    
    // Log status bytes
    debug!("Status bytes: [{:02X} {:02X} {:02X}]",
           read_buffer[0], read_buffer[1], read_buffer[2]);
    
    // Parse the data (skip the first 3 status bytes)
    let mut samples = Vec::with_capacity(num_channels);
    
    for ch in 0..num_channels {
        let start_idx = 3 + (ch * 3); // Skip 3 status bytes, then 3 bytes per channel
        
        // Extract the 3 bytes for this channel
        let msb = read_buffer[start_idx];
        let mid = read_buffer[start_idx + 1];
        let lsb = read_buffer[start_idx + 2];
        
        // Convert to i32 using the ch_sample_to_raw function
        let value = ch_sample_to_raw(msb, mid, lsb);
        
        debug!("Channel {}: raw bytes [{:02X} {:02X} {:02X}] = {}",
               ch, msb, mid, lsb, value);
        
        samples.push(value);
    }
    
    Ok(samples)
}

// Helper function to get current timestamp in microseconds
fn current_timestamp_micros() -> Result<u64, DriverError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .map_err(|e| DriverError::Other(format!("Failed to get timestamp: {}", e)))
}

// Helper function to write a value to a register in the ADS1299
fn write_register(spi: &mut Spi, register: u8, value: u8) -> Result<(), DriverError> {
    // Command: WREG (0x40) + register address
    let command = 0x40 | (register & 0x1F);
    
    // First byte: command, second byte: number of registers to write minus 1 (0 for single register)
    // Third byte: value to write
    let write_buffer = [command, 0x00, value];
    
    spi.write(&write_buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("SPI write error: {}", e)
    )))?;
    
    Ok(())
}


// Implement the AdcDriver trait
#[async_trait]
impl super::types::AdcDriver for Ads1299Driver {
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
// This is safe because we're using Arc<Mutex<>> for shared state
unsafe impl Send for Ads1299Driver {}
unsafe impl Sync for Ads1299Driver {}

/// Implementation of Drop for Ads1299Driver to handle cleanup when the driver is dropped.
///
/// Note: This provides only basic cleanup. For proper cleanup, users should explicitly
/// call `shutdown()` before letting the driver go out of scope. The Drop implementation
/// cannot perform the full async shutdown sequence because Drop is not async.
impl Drop for Ads1299Driver {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning
        error!("Ads1299Driver dropped without calling shutdown() first. This may lead to resource leaks.");
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
