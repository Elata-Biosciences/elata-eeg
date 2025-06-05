use std::time::SystemTime;
use std::error::Error;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::mpsc;
use std::future::Future;
use std::pin::Pin;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use log::{info, warn, debug, trace, error};
use super::mock::driver::MockDriver;

// Driver events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DriverEvent {
    Data(Vec<AdcData>),
    Error(String),
    StatusChange(DriverStatus),
}

// Driver status
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverStatus {
    NotInitialized,
    Ok,
    Error,
    Stopped,
    Running,
}

// Fix DriverType enum to match create_driver usage
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverType {
    Ads1299,
    Mock,
}

// ADC configuration
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AdcConfig {
    pub sample_rate: u32,
    pub gain: f32,
    pub channels: Vec<usize>,
    pub board_driver: DriverType,
    pub batch_size: usize,  // Number of samples to collect in a batch
    pub Vref: f32,  // Vref of adc
}

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            sample_rate: 250,  // 250 Hz is a common EEG sampling rate
            gain: 1.0,
            channels: vec![0],
            board_driver: DriverType::Mock,
            batch_size: 32,    // Default batch size (typical SPI buffer size)
            Vref: 4.5,         // Vref for the ADC
        }
    }
}

// ADC data point
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdcData {
    pub timestamp: u64,
    pub raw_samples: Vec<Vec<i32>>,
    pub voltage_samples: Vec<Vec<f32>>,
}

// Driver error
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("Hardware not found")]
    HardwareNotFound(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Acquisition error: {0}")]
    AcquisitionError(String),
    
    #[error("Driver not initialized")]
    NotInitialized,
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Other error: {0}")]
    Other(String),
    
    #[error("Driver not configured")]
    NotConfigured,
}

// Remove the problematic From implementations that violate orphan rules
// Instead, create wrapper types for external errors
#[derive(Debug)]
pub struct SpiError(rppal::spi::Error);

#[derive(Debug)]
pub struct TimeError(std::time::SystemTimeError);

impl From<SpiError> for DriverError {
    fn from(err: SpiError) -> Self {
        DriverError::Other(err.0.to_string())
    }
}

impl From<TimeError> for DriverError {
    fn from(err: TimeError) -> Self {
        DriverError::Other(err.0.to_string())
    }
}


#[async_trait]
pub trait AdcDriver: Send + Sync + 'static {
    async fn start_acquisition(&mut self) -> Result<(), DriverError>;
    async fn stop_acquisition(&mut self) -> Result<(), DriverError>;
    
    async fn shutdown(&mut self) -> Result<(), DriverError>;

    async fn get_config(&self) -> Result<AdcConfig, DriverError>;
    async fn get_status(&self) -> DriverStatus;
}

// Factory function to create the appropriate driver and return the event channel
pub async fn create_driver(config: AdcConfig)
    -> Result<(Box<dyn AdcDriver>, mpsc::Receiver<DriverEvent>), DriverError> {
    
    match config.board_driver {
        DriverType::Ads1299 => {
            // Try to create the ADS1299 hardware driver
            match super::ads1299::driver::Ads1299Driver::new(config.clone(), 0) {
                Ok((driver, events)) => {
                    // Check if the driver is in error state after creation
                    if driver.get_status().await == DriverStatus::Error {
                        error!("ADS1299 driver is in error state after creation");
                        error!("Falling back to mock driver");
                        
                        // Fall back to mock driver
                        let mut mock_config = config.clone();
                        mock_config.board_driver = DriverType::Mock;
                        let (mock_driver, mock_events) = super::mock::driver::MockDriver::new(mock_config, 0)?;
                        Ok((Box::new(mock_driver), mock_events))
                    } else {
                        info!("ADS1299 driver created successfully");
                        Ok((Box::new(driver), events))
                    }
                },
                Err(e) => {
                    error!("Failed to create ADS1299 driver: {}", e);
                    error!("Falling back to mock driver");
                    
                    // Fall back to mock driver
                    let mut mock_config = config.clone();
                    mock_config.board_driver = DriverType::Mock;
                    let (mock_driver, mock_events) = super::mock::driver::MockDriver::new(mock_config, 0)?;
                    Ok((Box::new(mock_driver), mock_events))
                }
            }
        },
        DriverType::Mock => {
            info!("Creating mock driver");
            let (driver, events) = super::mock::driver::MockDriver::new(config, 0)?;
            Ok((Box::new(driver), events))
        }
    }
}
