//! Builder pattern implementation for the ADS1299 driver.

use crate::types::{AdcConfig, DriverError, DriverEvent};
use tokio::sync::mpsc;

/// Builder for creating an Ads1299Driver with a fluent interface.
pub struct Ads1299DriverBuilder {
    config: Option<AdcConfig>,
    additional_channel_buffering: usize,
}

impl Ads1299DriverBuilder {
    /// Create a new Ads1299DriverBuilder with default values.
    pub fn new() -> Self {
        Self {
            config: None,
            additional_channel_buffering: 0,
        }
    }

    /// Set the ADC configuration.
    pub fn config(mut self, config: AdcConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set additional channel buffering.
    /// 
    /// This sets extra buffered batches (0 = lowest latency, but may cause backpressure).
    pub fn additional_channel_buffering(mut self, buffering: usize) -> Self {
        self.additional_channel_buffering = buffering;
        self
    }

    /// Build the Ads1299Driver with the configured parameters.
    /// 
    /// # Returns
    /// A tuple containing the driver instance and a receiver for driver events.
    ///
    /// # Errors
    /// Returns an error if the configuration is missing or invalid.
    pub fn build(self) -> Result<(super::driver::Ads1299Driver, mpsc::Receiver<DriverEvent>), DriverError> {
        let config = self.config.ok_or_else(|| DriverError::ConfigurationError("Missing AdcConfig".to_string()))?;
        super::driver::Ads1299Driver::new(config, self.additional_channel_buffering)
    }
}