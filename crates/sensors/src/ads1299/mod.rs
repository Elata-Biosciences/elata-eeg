//! ADS1299 driver for interfacing with the ADS1299EEG_FE board.
//!
//! This module provides a driver for the ADS1299 analog-to-digital converter,
//! which is commonly used in EEG applications.

// Re-export the driver module for backward compatibility
pub mod driver;

// Test module
pub mod test;

// New modules
pub mod acquisition;
pub mod builder;
pub mod error;
pub mod helpers;
pub mod registers;
pub mod spi;

// Re-export the main driver struct and builder for convenience
pub use driver::Ads1299Driver;
pub use builder::Ads1299DriverBuilder;