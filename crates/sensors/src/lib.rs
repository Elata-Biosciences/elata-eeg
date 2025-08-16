pub mod types;
pub mod ads1299;
#[cfg(feature = "mock_eeg")]
pub mod mock_eeg;
pub mod spi_bus;

// Re-export the main types that users need
pub use types::{AdcConfig, DriverStatus, DriverError, AdcDriver};

// Optionally expose lower-level access through a raw module
pub mod raw {
    pub mod ads1299 {
        pub use crate::ads1299::driver::Ads1299Driver;
    }
    #[cfg(feature = "mock_eeg")]
    pub mod mock_eeg {
        pub use crate::mock_eeg::driver::MockDriver;
    }
}
use eeg_types::SensorError;

impl From<DriverError> for SensorError {
    fn from(e: DriverError) -> Self {
        SensorError::DriverError(e.to_string())
    }
}