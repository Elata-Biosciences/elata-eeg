//! Helper functions for the ADS1299 driver.

use crate::types::DriverError;
use log::debug;

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
    debug!("ch_raw_to_voltage: raw={}, vref={}, gain={}, calculated_voltage={}", raw, vref, gain, voltage);
    voltage
}

/// Helper function to get current timestamp in microseconds
pub fn current_timestamp_micros() -> Result<u64, DriverError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .map_err(|e| DriverError::Other(format!("Failed to get timestamp: {}", e)))
}

/// Helper function to read data from SPI in continuous mode (RDATAC)
pub fn read_data_from_spi<T: crate::ads1299::spi::SpiDevice + ?Sized>(spi: &mut T, num_channels: usize) -> Result<Vec<i32>, DriverError> {
    debug!("Reading data from ADS1299 via SPI for {} channels in continuous mode", num_channels);

    // Validate input parameters
    if num_channels == 0 {
        return Err(DriverError::ConfigurationError("Number of channels must be greater than 0".to_string()));
    }
    
    if num_channels > 8 {
        // ADS1299 has a maximum of 8 channels
        return Err(DriverError::ConfigurationError(format!("Invalid channel count: {}. ADS1299 supports max 8 channels", num_channels)));
    }

    // Calculate total bytes to read: 3 status bytes + (3 bytes per channel * num_channels)
    let total_bytes = 3 + (3 * num_channels);
    debug!("Reading {} total bytes (3 status + {} data bytes)", total_bytes, 3 * num_channels);

    // Prepare buffers for SPI transfer
    let mut read_buffer = vec![0u8; total_bytes];
    let write_buffer = vec![0u8; total_bytes];

    // Perform SPI transfer with retry logic
    const MAX_RETRIES: usize = 3;
    let mut retry_count = 0;
    let mut last_error = None;
    
    while retry_count < MAX_RETRIES {
        match spi.transfer(&mut read_buffer, &write_buffer) {
            Ok(_) => {
                debug!("SPI transfer successful on attempt {}, read {} bytes",
                      retry_count + 1, read_buffer.len());
                
                // Check if the data looks valid (simple validation)
                if read_buffer.iter().all(|&b| b == 0) {
                    // All zeros is suspicious, might be a failed read
                    log::warn!("SPI transfer returned all zeros, which may indicate a hardware issue");
                    // But continue processing anyway
                } else if read_buffer[0] == 0xFF && read_buffer[1] == 0xFF && read_buffer[2] == 0xFF {
                    // All 0xFF in status bytes is suspicious
                    log::warn!("SPI transfer returned 0xFF status bytes, which may indicate a hardware issue");
                    // But continue processing anyway
                }
                
                // Success, break out of retry loop
                break;
            },
            Err(e) => {
                retry_count += 1;
                last_error = Some(e);
                
                if retry_count < MAX_RETRIES {
                    log::warn!("SPI transfer error (attempt {}/{}): {}, retrying...",
                              retry_count, MAX_RETRIES, last_error.as_ref().unwrap());
                    // Small delay before retry
                    std::thread::sleep(std::time::Duration::from_millis(1));
                } else {
                    log::error!("SPI transfer failed after {} attempts: {}",
                               MAX_RETRIES, last_error.as_ref().unwrap());
                    return Err(DriverError::IoError(format!("SPI transfer error after {} retries: {}",
                                MAX_RETRIES, last_error.unwrap())));
                }
            }
        }
    }

    // Log raw data for debugging (only in debug mode to avoid excessive logging)
    debug!("Raw SPI data: {:02X?}", read_buffer);

    // Log status bytes
    debug!("Status bytes: [{:02X} {:02X} {:02X}]",
           read_buffer[0], read_buffer[1], read_buffer[2]);

    // Parse the data (skip the first 3 status bytes)
    let mut samples = Vec::with_capacity(num_channels);

    for ch in 0..num_channels {
        let start_idx = 3 + (ch * 3); // Skip 3 status bytes, then 3 bytes per channel
        
        // Bounds check to prevent panic
        if start_idx + 2 >= read_buffer.len() {
            return Err(DriverError::Other(format!(
                "Buffer underrun: expected at least {} bytes, got {}",
                start_idx + 3, read_buffer.len()
            )));
        }

        // Extract the 3 bytes for this channel
        let msb = read_buffer[start_idx];
        let mid = read_buffer[start_idx + 1];
        let lsb = read_buffer[start_idx + 2];

        // Log raw bytes BEFORE conversion (only in debug mode)
        debug!("Channel {}: raw bytes [{:02X} {:02X} {:02X}]", ch, msb, mid, lsb);

        // Convert to i32 using the ch_sample_to_raw function
        let value = ch_sample_to_raw(msb, mid, lsb);

        // Log converted value (only in debug mode)
        debug!("Channel {}: converted raw value = {}", ch, value);

        samples.push(value);
    }

    Ok(samples)
}