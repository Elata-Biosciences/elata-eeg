pub mod ads1299;
pub mod mock;
pub mod types;

// Re-export types for convenience
pub use self::types::{AdcData, AdcConfig, DriverEvent, DriverStatus, DriverError, AdcDriver, DriverType};
pub use self::mock::driver::MockDriver;
pub use self::ads1299::driver::Ads1299Driver;
pub use self::types::create_driver;