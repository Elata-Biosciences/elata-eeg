//! Error types for the ADS1299 driver.

use std::io;

/// Re-export the DriverError from the parent module for convenience
pub use crate::board_drivers::types::DriverError;

/// Convert an IO error to a DriverError
pub fn io_error_to_driver_error(e: io::Error, context: &str) -> DriverError {
    DriverError::IoError(io::Error::new(
        e.kind(),
        format!("{}: {}", context, e)
    ))
}

/// Create a configuration error with the given message
pub fn config_error(msg: &str) -> DriverError {
    DriverError::ConfigurationError(msg.to_string())
}

/// Create a hardware not found error with the given message
pub fn hardware_not_found(msg: &str) -> DriverError {
    DriverError::HardwareNotFound(msg.to_string())
}

/// Create an acquisition error with the given message
pub fn acquisition_error(msg: &str) -> DriverError {
    DriverError::AcquisitionError(msg.to_string())
}

/// Create a not initialized error
pub fn not_initialized() -> DriverError {
    DriverError::NotInitialized
}

/// Create a not configured error
pub fn not_configured() -> DriverError {
    DriverError::NotConfigured
}

/// Create a generic error with the given message
pub fn other_error(msg: &str) -> DriverError {
    DriverError::Other(msg.to_string())
}

/// RAII guard for the hardware lock.
/// Acquires the lock on creation, releases it on drop.
pub struct HardwareLockGuard<'a> {
    guard: std::sync::MutexGuard<'a, bool>,
}

impl<'a> HardwareLockGuard<'a> {
    /// Create a new hardware lock guard.
    /// 
    /// # Returns
    /// A new hardware lock guard if the lock was acquired successfully.
    /// 
    /// # Errors
    /// Returns an error if the lock could not be acquired or if the hardware is already in use.
    pub fn new(hardware_lock: &'a std::sync::Mutex<bool>) -> Result<Self, DriverError> {
        let mut guard = hardware_lock
            .lock()
            .map_err(|_| DriverError::Other("Failed to acquire hardware lock".to_string()))?;
        if *guard {
            return Err(DriverError::HardwareNotFound(
                "Hardware already in use by another driver instance".to_string(),
            ));
        }
        *guard = true;
        Ok(HardwareLockGuard { guard })
    }
}

impl<'a> Drop for HardwareLockGuard<'a> {
    fn drop(&mut self) {
        *self.guard = false;
    }
}