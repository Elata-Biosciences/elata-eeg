use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use flume::Receiver;
use log::{error, info, warn};
use rppal::gpio::{Gpio, InputPin, OutputPin, Trigger};
use rppal::spi::{Bus, Mode};
use thread_priority::ThreadPriority;
use eeg_types::SensorError;
use sensors::{
    ads1299::registers::{
        self, CH1SET_ADDR, CHN_REG, CMD_RDATAC, CMD_SDATAC, CMD_STANDBY, CMD_WAKEUP, CONFIG1_REG,
        CONFIG2_REG, CONFIG3_REG, CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, DAISY_DISABLE,
    BIASREF_INT , PD_BIAS, PD_REFBUF, BIAS_SENS_OFF_MASK, SRB1, MUX_NORMAL, POWER_OFF_CH},
    spi_bus::SpiBus,
    AdcConfig, AdcDriver, DriverError, DriverStatus,
    ads1299::driver::Ads1299Driver,
};
const START_PIN: u8 = 22; // GPIO for START pulse
pub struct ElataV2Driver {
    chip_drivers: Vec<Ads1299Driver>,
    bus: Arc<SpiBus>,
    gpio: Arc<Gpio>,
    status: Arc<Mutex<DriverStatus>>,
    config: AdcConfig,
    start_pin: Arc<Mutex<Option<OutputPin>>>,
    drdy_pin: Arc<Mutex<Option<InputPin>>>,
    sample_rx: Receiver<Vec<i32>>,
    acq_thread_handle: Option<JoinHandle<()>>,
    stop_acq_thread: Arc<AtomicBool>,
}
impl ElataV2Driver {
    pub fn new(config: AdcConfig) -> Result<Self, DriverError> {
        let gpio = Arc::new(Gpio::new()?);
        info!("GPIO initialized.");
        let mut chip_drivers = Vec::with_capacity(config.chips.len());
        let bus = Arc::new(SpiBus::new(
            Bus::Spi0,
            1_240_000,
            Mode::Mode1,
        )?);
        info!("SPI bus initialized.");
        for (i, chip_config) in config.chips.iter().enumerate() {
            // Define CS pins internally based on chip index for ElataV2 hardware
            let cs_pin_num = match i {
                0 => 7,  // Chip 0 uses CS pin 7
                1 => 8,  // Chip 1 uses CS pin 8
                _ => return Err(DriverError::ConfigurationError(
                    format!("ElataV2 driver only supports 2 chips, but chip {} was provided", i)
                )),
            };
            
            let mut cs_pin = gpio.get(cs_pin_num)?.into_output();
            cs_pin.set_high();
            info!("CS pin {} initialized for software control.", cs_pin_num);
            
            let driver = Ads1299Driver::new(chip_config.clone(), bus.clone(), cs_pin)?;
            chip_drivers.push(driver);
        }
        // The acquisition thread will be managed by `initialize` and `shutdown`
        // This channel will deliver completed samples from the acq thread to the consumer.
        let (sample_tx, sample_rx) = flume::bounded(4096);
        Ok(Self {
            chip_drivers,
            bus,
            gpio,
            status: Arc::new(Mutex::new(DriverStatus::Stopped)),
            config,
            start_pin: Arc::new(Mutex::new(None)),
            drdy_pin: Arc::new(Mutex::new(None)),
            sample_rx,
            acq_thread_handle: None,
            stop_acq_thread: Arc::new(AtomicBool::new(false)),
        })
    }
}
impl AdcDriver for ElataV2Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        // Ensure a fresh start for the acquisition thread
        self.stop_acq_thread.store(false, Ordering::Relaxed);
        
        info!("Initializing ElataV2 board with {} chips...", self.config.chips.len());
        // This board uses a single DRDY GPIO sourced from chip 0 by wiring convention.
        // If chip 0 has no active channels, DRDY will not toggle and acquisition will stall.
        if self.config.chips.get(0).map(|c| c.channels.is_empty()).unwrap_or(true) {
            return Err(DriverError::ConfigurationError(
                "Chip 0 has no active channels, but DRDY is sourced from chip 0. Enable at least one channel on chip 0 or rewire DRDY to the active chip.".to_string(),
            ));
        }
        // 1. Initialize chip registers first
        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            info!("Initializing Chip {}...", i);
            let chip_info = &self.config.chips[i];
            let gain_mask = registers::gain_to_reg_mask(self.config.gain)?;
            let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;
            let pd_bias = if i == 0 { PD_BIAS } else { 0x00 };
            let ch_settings: Vec<(u8, u8)> = (0..8)
                .map(|ch_idx| {
                    let setting = if chip_info.channels.contains(&ch_idx) {
                        CHN_REG | MUX_NORMAL | gain_mask
                    } else {
                        POWER_OFF_CH
                    };
                    (CH1SET_ADDR + ch_idx, setting)
                })
                .collect();
            let active_ch_mask = chip_info.channels.iter().fold(0, |acc, &ch| acc | (1 << (ch % 8)));
            // the setup is NOT daisy chained. running in cascade mode
            chip.initialize_chip(
                CONFIG1_REG | sps_mask | DAISY_DISABLE,
                CONFIG2_REG,
                CONFIG3_REG | BIASREF_INT | PD_REFBUF | pd_bias,
                CONFIG4_REG,
                LOFF_SESP_REG,
                MISC1_REG | SRB1,
                &ch_settings,
                active_ch_mask,
                BIAS_SENS_OFF_MASK
            )?;
            if chip_info.channels.is_empty() {
                info!("Chip {} initialized with 0 channels; will remain in standby and be skipped during acquisition.", i);
            } else {
                info!("Chip {} initialized and ready with {} active channel(s).", i, chip_info.channels.len());
            }
        }
        thread::sleep(Duration::from_millis(10));
        // This channel is for DRDY interrupts. It's local to the init block.
        let (drdy_tx, drdy_rx) = flume::bounded(128);
        // 2. Set up asynchronous DRDY interrupt
        let mut drdy_pin = self.gpio.get(self.config.drdy_pin)?.into_input_pullup();
        let initial_state = drdy_pin.is_high();
        info!(
            "DRDY pin {} initial state: {} (should be HIGH before data acquisition starts)",
            self.config.drdy_pin,
            if initial_state { "HIGH" } else { "LOW" }
        );
        let drdy_tx_clone = drdy_tx.clone();
        drdy_pin.set_async_interrupt(Trigger::FallingEdge, None, move |_| {
            let _ = drdy_tx_clone.send(());
        })?;
        info!(
            "Asynchronous DRDY interrupt handler registered on pin {}",
            self.config.drdy_pin
        );
        *self.drdy_pin.lock().unwrap() = Some(drdy_pin);
        // 3. Spawn the acquisition thread
        let stop_flag = self.stop_acq_thread.clone();
        let (sample_tx, sample_rx) = flume::bounded(4096); // This is the new channel for samples
        self.sample_rx = sample_rx; // Move the receiver to the struct
        let mut chip_drivers = self
            .chip_drivers
            .clone()
            .into_iter()
            .enumerate()
            .filter(|(idx, _)| !self.config.chips[*idx].channels.is_empty())
            .map(|(_, d)| d)
            .collect::<Vec<_>>();
        let total_active_channels: usize = self.config.chips.iter().map(|c| c.channels.len()).sum();
        let acq_thread = thread::Builder::new()
            .name("adc_acq".into())
            .spawn(move || {
                if let Err(e) = thread_priority::set_current_thread_priority(ThreadPriority::Max) {
                    warn!("Failed to set acquisition thread priority: {:?}", e);
                }
                info!("Acquisition thread started with high priority.");
                while !stop_flag.load(Ordering::Relaxed) {
                    match drdy_rx.recv_timeout(Duration::from_millis(1000)) {
                        Ok(_) => {
                            // Atomically read from all chips. If any fail, discard the entire sample.
                            let chip_data: Vec<Result<Vec<i32>, SensorError>> = chip_drivers
                                .iter_mut()
                                .map(|driver| driver.read_data_raw())
                                .collect();
                            // Check if all reads were successful
                            if chip_data.iter().all(|res| res.is_ok()) {
                                let frame: Vec<i32> = chip_data
                                    .into_iter()
                                    .flat_map(|res| res.unwrap())
                                    .collect();
                                if frame.len() == total_active_channels {
                                    if sample_tx.send(frame).is_err() {
                                        error!("Sample channel disconnected. Stopping acquisition thread.");
                                        return;
                                    }
                                } else {
                                    warn!(
                                        "Incorrect frame size. Expected {}, got {}. Discarding sample.",
                                        total_active_channels,
                                        frame.len()
                                    );
                                }
                            } else {
                                // Log errors for failed reads
                                for (i, res) in chip_data.iter().enumerate() {
                                    if let Err(e) = res {
                                        error!("[Chip {}] Failed to read data in acq thread: {}", i, e);
                                    }
                                }
                                warn!("Incomplete sample due to read errors. Discarding.");
                            }
                        }
                        Err(flume::RecvTimeoutError::Timeout) => {
                            warn!("Timeout waiting for DRDY. Discarding sample.");
                            continue;
                        }
                        Err(flume::RecvTimeoutError::Disconnected) => {
                            error!("DRDY channel disconnected. Stopping acquisition thread.");
                            return;
                        }
                    }
                }
                info!("Acquisition thread shutting down.");
            })
            .map_err(|e| DriverError::Other(format!("Failed to spawn thread: {}", e)))?;
        self.acq_thread_handle = Some(acq_thread);
        // 4. Now, start data acquisition on all chips
        // 4. Assert START pin HIGH after all chips are fully configured
        let mut start_pin = self.gpio.get(START_PIN)?.into_output();
        start_pin.set_high();
        thread::sleep(Duration::from_millis(1));
        
        // 5. Send RDATAC only to chips with active channels
        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            // Wake up all chips first
            chip.send_command(CMD_WAKEUP)?;
            thread::sleep(Duration::from_millis(10));
            
            // Only send RDATAC to chips with active channels
            if !self.config.chips[i].channels.is_empty() {
                chip.send_command(CMD_RDATAC)?;
                thread::sleep(Duration::from_millis(1));
            }
        }
        
        *self.start_pin.lock().unwrap() = Some(start_pin);
        info!("ElataV2 board initialized successfully and is acquiring data.");
        Ok(())
    }
    fn acquire_batched(
        &mut self,
        batch_size: usize,
        stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64, AdcConfig), SensorError> {
        *self.status.lock().unwrap() = DriverStatus::Running;
        let total_channels: usize = self.config.chips.iter().map(|c| c.channels.len()).sum();
        if total_channels == 0 {
            return Ok((Vec::new(), 0, self.config.clone()));
        }
        let mut batch_buffer: Vec<i32> = Vec::with_capacity(batch_size * total_channels);
        let first_sample_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        for i in 0..batch_size {
            if stop_flag.load(Ordering::Relaxed) {
                info!("Stop flag received, breaking batch acquisition loop.");
                break;
            }
            match self.sample_rx.recv_timeout(Duration::from_millis(1000)) {
                Ok(frame) => {
                    batch_buffer.extend(frame);
                }
                Err(flume::RecvTimeoutError::Timeout) => {
                    warn!("[Batch {}] Sample channel timeout. No data received from acquisition thread in 1000ms.", i);
                    // This might indicate a problem with the acquisition thread
                    continue;
                }
                Err(flume::RecvTimeoutError::Disconnected) => {
                    error!("Sample channel disconnected. Stopping acquisition.");
                    break;
                }
            }
        }
        *self.status.lock().unwrap() = DriverStatus::Stopped;
        Ok((batch_buffer, first_sample_timestamp, self.config.clone()))
    }
    fn get_status(&self) -> DriverStatus {
        self.status.lock().unwrap().clone()
    }
    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        Ok(self.config.clone())
    }
    fn reconfigure(&mut self, config: &AdcConfig) -> Result<(), DriverError> {
        // Pre-validate the incoming configuration BEFORE touching the running hardware.
        // ElataV2 boards always have exactly 2 chips, and require at least one active channel.
        if config.chips.len() != 2 {
            return Err(DriverError::ConfigurationError(
                "ElataV2Driver requires exactly 2 chip configurations".to_string(),
            ));
        }
        let total_channels: usize = config.chips.iter().map(|c| c.channels.len()).sum();
        if total_channels == 0 {
            return Err(DriverError::ConfigurationError(
                "At least one channel must be configured across both chips".to_string(),
            ));
        }
        info!("ElataV2 reconfigure: performing full shutdown + reinitialize");
        // Stop acquisition thread, clear IRQs and pins, and power down chips
        self.shutdown()?;
        
        // Recreate chip drivers with new configuration
        self.chip_drivers.clear();
        for (i, chip_config) in config.chips.iter().enumerate() {
            // Define CS pins internally based on chip index for ElataV2 hardware
            let cs_pin_num = match i {
                0 => 7,  // Chip 0 uses CS pin 7
                1 => 8,  // Chip 1 uses CS pin 8
                _ => return Err(DriverError::ConfigurationError(
                    format!("ElataV2 driver only supports 2 chips, but chip {} was provided", i)
                )),
            };
            
            let mut cs_pin = self.gpio.get(cs_pin_num)?.into_output();
            cs_pin.set_high();
            info!("CS pin {} initialized for software control.", cs_pin_num);
            
            let driver = Ads1299Driver::new(chip_config.clone(), self.bus.clone(), cs_pin)?;
            self.chip_drivers.push(driver);
        }
        
        // Update runtime configuration
        self.config = config.clone();
        
        // Re-run the known-good board-level initialization sequence
        self.initialize()
    }
    fn shutdown(&mut self) -> Result<(), DriverError> {
        info!("Shutting down ElataV2 board...");
        // 1. Signal the acquisition thread to stop
        self.stop_acq_thread.store(true, Ordering::Relaxed);
        // 2. Wait for the acquisition thread to finish
        if let Some(handle) = self.acq_thread_handle.take() {
            info!("Waiting for acquisition thread to join...");
            if let Err(e) = handle.join() {
                error!("Acquisition thread panicked: {:?}", e);
            }
            info!("Acquisition thread joined.");
        }
        // 3. Clear interrupt handlers and release DRDY pins
        if let Some(mut drdy_pin) = self.drdy_pin.lock().unwrap().take() {
            drdy_pin.clear_async_interrupt().unwrap();
        }
        // 4. Take START pin low to stop conversions
        if let Some(mut start_pin) = self.start_pin.lock().unwrap().take() {
            start_pin.set_low();
            info!("START pin set to low.");
        }
        // 5. Power down chips
        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            info!("Sending SDATAC and STANDBY to chip {}", i);
            chip.send_command(CMD_SDATAC)?;
            chip.send_command(CMD_STANDBY)?;
            chip.shutdown()?;
        }
        *self.status.lock().unwrap() = DriverStatus::NotInitialized;
        info!("ElataV2 board shut down.");
        Ok(())
    }
}
