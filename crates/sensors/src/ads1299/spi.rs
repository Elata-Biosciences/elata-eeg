use log::error;
use rppal::gpio::{InputPin, Trigger};
use std::time::Duration;

/// Waits for a falling edge interrupt on the DRDY pin with a specified timeout.
///
/// This function is a synchronous, blocking call that waits for the DRDY signal
/// from the ADS1299, indicating that new data is ready.
///
/// # Returns
/// - `Ok(true)` if the interrupt was received within the timeout.
/// - `Ok(false)` if the timeout occurred.
/// - `Err(DriverError)` if there was a GPIO error.
pub fn wait_irq(
    pin: &mut InputPin,
    timeout: Duration,
) -> Result<bool, crate::types::DriverError> {
    // Ensure the interrupt is configured for falling edge.
    // This might be redundant if set once at initialization, but it's safe.
    pin.set_interrupt(Trigger::FallingEdge, None)
?;

    match pin.poll_interrupt(true, Some(timeout)) {
        Ok(Some(_)) => Ok(true),
        Ok(None) => {
            // Timeout
            Ok(false)
        }
        Err(e) => {
            error!("DRDY pin poll_interrupt error: {}", e);
            Err(crate::types::DriverError::from(e))
        }
    }
}