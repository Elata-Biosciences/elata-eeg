//! Common types and traits for sensor drivers

use std::sync::atomic::AtomicBool;

use eeg_types::SensorError as EegSensorError;

/// Configuration for ADC/sensor drivers
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChipConfig {
    /// List of active channels for this chip (0-indexed for this chip)
    pub channels: Vec<u8>,
    /// SPI bus for this chip
    #[serde(default = "default_spi_bus")]
    pub spi_bus: u8,
    /// SPI chip select for this chip
    #[serde(default = "default_cs_pin")]
    pub cs_pin: u8,
}

fn default_spi_bus() -> u8 { 0 }
fn default_cs_pin() -> u8 { 0 }

impl Default for ChipConfig {
    fn default() -> Self {
        Self {
            channels: (0..8).collect(),
            spi_bus: default_spi_bus(),
            cs_pin: default_cs_pin(),
        }
    }
}

/// Configuration for ADC/sensor drivers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AdcConfig {
    /// Target sample rate in Hz
    pub sample_rate: u32,
    /// Reference voltage for ADC conversion
    pub vref: f32,
    /// Gain setting for all channels on all chips
    pub gain: f32,
    /// Data ready pin for the first chip in the daisy chain
    #[serde(default = "default_drdy_pin")]
    pub drdy_pin: u8,
    /// Configuration for each chip on the board.
    pub chips: Vec<ChipConfig>,
}

fn default_drdy_pin() -> u8 { 25 }

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            sample_rate: 250,
            vref: 4.5,
            gain: 1.0,
            chips: vec![ChipConfig::default()],
            drdy_pin: default_drdy_pin(),
        }
    }
}

/// Status of a sensor driver
#[derive(Debug, Clone, PartialEq)]
pub enum DriverStatus {
    /// Driver is not initialized
    NotInitialized,
    /// Driver is initialized but not running
    Stopped,
    /// Driver is actively acquiring data
    Running,
    /// Driver is OK/ready
    Ok,
    /// Driver encountered an error
    Error(String),
}


/// Errors that can occur in sensor drivers
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum DriverError {
    /// A sensor-specific error.
    #[error("Sensor error: {0}")]
    SensorError(#[from] EegSensorError),
    /// Hardware communication error
    #[error("Hardware error: {0}")]
    HardwareError(String),
    /// Invalid configuration
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    /// SPI communication error
    #[error("SPI error: {0}")]
    SpiError(String),
    /// GPIO error
    #[error("GPIO error: {0}")]
    GpioError(String),
    /// Timeout error
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    /// I/O error
    #[error("I/O error: {0}")]
    IoError(String),
    /// Driver not initialized
    #[error("Driver not initialized")]
    NotInitialized,
    /// Driver not configured
    #[error("Driver not configured")]
    NotConfigured,
    /// Hardware not found
    #[error("Hardware not found: {0}")]
    HardwareNotFound(String),
    /// Acquisition error
    #[error("Acquisition error: {0}")]
    AcquisitionError(String),
    /// Generic error
    #[error("Error: {0}")]
    Other(String),
    /// Feature not implemented
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

/// Trait that all sensor drivers must implement
pub trait AdcDriver: Send + Sync + 'static {
    /// Initialize the driver and underlying hardware.
    fn initialize(&mut self) -> Result<(), DriverError>;

    /// Acquire a batch of raw i32 samples from the sensor.
    /// This is a blocking call that should wait for data to be ready.
    ///
    /// # Arguments
    /// * `batch_size` - The number of samples to acquire for each channel.
    /// * `stop_flag` - An atomic bool to signal the acquisition loop to stop.
    ///
    /// # Returns
    /// A vector containing the raw i32 samples, interleaved by channel.
    fn acquire_batched(
        &mut self,
        batch_size: usize,
        stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64, AdcConfig), EegSensorError>;

    /// Get current driver status
    fn get_status(&self) -> DriverStatus;

    /// Get current configuration
    fn get_config(&self) -> Result<AdcConfig, DriverError>;

    /// Reconfigure the driver with new parameters.
    ///
    /// This method allows for dynamic reconfiguration of the driver while it is
    /// running. Not all parameters may be reconfigurable.
    fn reconfigure(&mut self, _config: &AdcConfig) -> Result<(), DriverError> {
        Err(DriverError::NotImplemented(
            "reconfigure is not implemented for this driver".to_string(),
        ))
    }

    /// Shutdown the driver and clean up resources
    fn shutdown(&mut self) -> Result<(), DriverError>;
}


impl From<rppal::spi::Error> for DriverError {
    fn from(err: rppal::spi::Error) -> Self {
        DriverError::SpiError(err.to_string())
    }
}

impl From<rppal::gpio::Error> for DriverError {
    fn from(err: rppal::gpio::Error) -> Self {
        DriverError::GpioError(err.to_string())
    }
}

impl From<std::io::Error> for DriverError {
    fn from(err: std::io::Error) -> Self {
        DriverError::IoError(err.to_string())
    }
}
