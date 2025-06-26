pub mod types;
pub mod ads1299;
pub mod mock_eeg;

// Re-export the main types that users need
pub use types::{AdcConfig, DriverType, DriverStatus, AdcData, DriverEvent, DriverError, AdcDriver, create_driver};

// Optionally expose lower-level access through a raw module
pub mod raw {
    pub use crate::ads1299::*;
    pub use crate::mock_eeg::*;
}