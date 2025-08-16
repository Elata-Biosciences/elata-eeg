use eeg_types::SensorError;
use log::info;
use rppal::gpio::Gpio;
use rppal::spi::{Bus, Mode};
use sensors::{
    ads1299::{
        driver::Ads1299Driver,
        registers::{
            self, BIAS_SENSN_REG, CHN_OFF, CHN_REG, CONFIG1_REG, CONFIG2_REG, CONFIG3_REG,
            CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, CH1SET_ADDR,
BIASREF_INT , PD_BIAS , PD_REFBUF, BIAS_SENS_OFF_MASK, SRB1,MUX_NORMAL        },
    },
    spi_bus::SpiBus,
    AdcConfig, AdcDriver, DriverError, DriverStatus,
};
use std::sync::{atomic::AtomicBool, Arc};

pub struct ElataV1Driver {
    inner: Ads1299Driver,
    config: AdcConfig,
}

impl ElataV1Driver {
    pub fn new(config: AdcConfig) -> Result<Self, DriverError> {
        if config.chips.len() != 1 {
            return Err(DriverError::ConfigurationError(
                "ElataV1 driver only supports single-chip configurations".to_string(),
            ));
        }
        
        let chip_config = config.chips.get(0).ok_or_else(|| {
            DriverError::ConfigurationError("At least one chip must be configured for ElataV1".to_string())
        })?;

        let gpio = Gpio::new()?;
        let cs_pin = gpio.get(chip_config.cs_pin)?.into_output();

        let bus = Arc::new(SpiBus::new(
            Bus::Spi0,
            1_000_000,
            Mode::Mode1,
        )?);

        let inner = Ads1299Driver::new(chip_config.clone(), bus, cs_pin)?;
        Ok(Self { inner, config })
    }
}

impl AdcDriver for ElataV1Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV1 board...");
        let chip_config = self.config.chips.get(0).ok_or_else(|| {
            DriverError::ConfigurationError("At least one chip must be configured for ElataV1".to_string())
        })?;

        let gain_mask = registers::gain_to_reg_mask(self.config.gain)?;
        let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;
        let active_ch_mask = chip_config.channels.iter().fold(0, |acc, &ch| acc | (1 << ch));
        let ch_settings: Vec<(u8, u8)> = (0..8)
            .map(|i| {
                (
                    CH1SET_ADDR + i,
                    if chip_config.channels.contains(&i) {
                        CHN_REG | MUX_NORMAL | gain_mask
                    } else {
                        CHN_OFF
                    },
                )
            })
            .collect();
 
        self.inner.initialize_chip(
            CONFIG1_REG | sps_mask,
            CONFIG2_REG,
            CONFIG3_REG | BIASREF_INT | PD_BIAS | PD_REFBUF,
            CONFIG4_REG,
            LOFF_SESP_REG,
            MISC1_REG | SRB1,
            &ch_settings,
            active_ch_mask,
            BIAS_SENSN_REG & BIAS_SENS_OFF_MASK,
        )?;

        info!("ElataV1 board initialized successfully.");
        Ok(())
    }

    fn acquire_batched(
        &mut self,
        _batch_size: usize,
        _stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64, AdcConfig), SensorError> {
        // This is a placeholder for the actual sample acquisition logic
        // which would be driven by DRDY interrupts.
        // The `acquire_raw` method from Ads1299Driver would be adapted here.
        // For now, we'll just error out.
        Err(SensorError::HardwareFault(
            "Batched acquisition not yet fully implemented for ElataV1".to_string(),
        ))
    }

    fn get_status(&self) -> DriverStatus {
        self.inner.get_status()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        self.inner.get_config()
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        self.inner.shutdown()
    }
}