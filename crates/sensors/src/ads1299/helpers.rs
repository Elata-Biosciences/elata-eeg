//! Helper functions for the ADS1299 driver.

use crate::types::DriverError;

/// Convert 24-bit SPI data to a signed 32-bit integer (sign-extended)
pub fn ch_sample_to_raw(msb: u8, mid: u8, lsb: u8) -> i32 {
    let raw_value = ((msb as u32) << 16) | ((mid as u32) << 8) | (lsb as u32);
    ((raw_value as i32) << 8) >> 8
}

/// Convert signed raw ADC value to voltage using VREF and gain
/// Formula: voltage = (raw * (VREF / Gain)) / 2^23
pub fn ch_raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
    let voltage = ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32;
    // Add detailed logging (using debug! instead of trace!)
    // debug!("ch_raw_to_voltage: raw={}, vref={}, gain={}, calculated_voltage={}", raw, vref, gain, voltage);
    voltage
}

/// Helper function to get current timestamp in microseconds
pub fn current_timestamp_micros() -> Result<u64, DriverError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .map_err(|e| DriverError::Other(format!("Failed to get timestamp: {}", e)))
}
