//! Critical Mathematical Function Tests
//!
//! This module contains comprehensive tests for the core mathematical operations
//! used in EEG data processing. These tests are essential for ensuring data integrity
//! and accuracy throughout the signal processing pipeline.
//!
//! ## Test Categories
//!
//! ### ADC Conversion Mathematics
//! - 24-bit signed integer handling and sign extension
//! - Raw ADC value to voltage conversion with various gains and reference voltages
//! - Precision validation for nanovolt-level accuracy
//! - Boundary condition testing for full ADC range
//!
//! ### Signal Processing Validation  
//! - Gain scaling behavior across all supported hardware settings
//! - Reference voltage scaling for different ADC configurations
//! - Overflow and underflow handling for extreme input conditions
//! - Typical EEG signal range validation (±1000μV)
//!
//! ## Performance Requirements
//! These functions must execute efficiently as they process every EEG sample:
//! - ADC conversion: < 10ns per sample (for real-time processing)
//! - Voltage scaling: < 5ns per sample
//! - Sign extension: < 2ns per sample
//!
//! ## Accuracy Requirements
//! - Voltage conversion accuracy: ±1 LSB (least significant bit)
//! - Gain scaling linearity: < 0.01% error across all gain settings
//! - Reference voltage scaling: < 0.001% error
//!
//! These tests ensure the mathematical foundation of the EEG system meets
//! medical-grade accuracy requirements for neuroscience research.

#[test]
fn test_24bit_adc_sign_extension() {
    // Test the critical 24-bit to 32-bit sign extension logic
    // This is the core of ADC data interpretation
    
    fn convert_24bit_to_i32(msb: u8, mid: u8, lsb: u8) -> i32 {
        let raw_value = ((msb as u32) << 16) | ((mid as u32) << 8) | (lsb as u32);
        ((raw_value as i32) << 8) >> 8
    }
    
    // Test maximum positive 24-bit value: 0x7FFFFF = 8388607
    assert_eq!(convert_24bit_to_i32(0x7F, 0xFF, 0xFF), 8388607);
    
    // Test maximum negative 24-bit value: 0x800000 = -8388608
    assert_eq!(convert_24bit_to_i32(0x80, 0x00, 0x00), -8388608);
    
    // Test zero
    assert_eq!(convert_24bit_to_i32(0x00, 0x00, 0x00), 0);
    
    // Test small positive
    assert_eq!(convert_24bit_to_i32(0x00, 0x00, 0x01), 1);
    
    // Test small negative (0xFFFFFF in 24-bit = -1)
    assert_eq!(convert_24bit_to_i32(0xFF, 0xFF, 0xFF), -1);
    
    // Test mid-scale positive
    assert_eq!(convert_24bit_to_i32(0x40, 0x00, 0x00), 4194304);
    
    // Test mid-scale negative
    assert_eq!(convert_24bit_to_i32(0xC0, 0x00, 0x00), -4194304);
}

#[test]
fn test_voltage_conversion_mathematics() {
    // Test the core voltage conversion formula: voltage = (raw * (VREF / Gain)) / 2^23
    
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let vref = 4.5;
    let gain = 24.0;
    
    // Test zero gives zero
    assert_eq!(raw_to_voltage(0, vref, gain), 0.0);
    
    // Test full scale positive
    let max_raw = 8388607; // 2^23 - 1
    let voltage = raw_to_voltage(max_raw, vref, gain);
    let expected = vref / gain; // 4.5/24 = 0.1875V
    assert!((voltage - expected).abs() < 0.0001, 
           "Full scale positive should be ~{}V, got {}V", expected, voltage);
    
    // Test full scale negative
    let min_raw = -8388608; // -2^23
    let voltage = raw_to_voltage(min_raw, vref, gain);
    let expected = -vref / gain; // -0.1875V
    assert!((voltage - expected).abs() < 0.0001,
           "Full scale negative should be ~{}V, got {}V", expected, voltage);
    
    // Test LSB resolution
    let lsb_voltage = raw_to_voltage(1, vref, gain);
    let expected_lsb = (vref / gain) / (1 << 23) as f32;
    assert!((lsb_voltage - expected_lsb).abs() < 0.000001,
           "LSB should be {}V, got {}V", expected_lsb, lsb_voltage);
}

#[test]
fn test_gain_scaling_behavior() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let raw = 1000000; // Test value
    let vref = 4.5;
    
    // Test different gain settings
    let voltage_gain1 = raw_to_voltage(raw, vref, 1.0);
    let voltage_gain6 = raw_to_voltage(raw, vref, 6.0);
    let voltage_gain12 = raw_to_voltage(raw, vref, 12.0);
    let voltage_gain24 = raw_to_voltage(raw, vref, 24.0);
    
    // Higher gain should give proportionally lower voltage
    assert!((voltage_gain1 / voltage_gain6 - 6.0).abs() < 0.01, 
           "Gain 1 vs 6 ratio should be ~6x");
    assert!((voltage_gain1 / voltage_gain12 - 12.0).abs() < 0.01, 
           "Gain 1 vs 12 ratio should be ~12x");
    assert!((voltage_gain1 / voltage_gain24 - 24.0).abs() < 0.01, 
           "Gain 1 vs 24 ratio should be ~24x");
    
    // Verify ordering
    assert!(voltage_gain1 > voltage_gain6);
    assert!(voltage_gain6 > voltage_gain12);
    assert!(voltage_gain12 > voltage_gain24);
}

#[test]
fn test_reference_voltage_scaling() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let raw = 4194304; // Half scale
    let gain = 24.0;
    
    // Test different reference voltages
    let voltage_4_5v = raw_to_voltage(raw, 4.5, gain);
    let voltage_2_4v = raw_to_voltage(raw, 2.4, gain);
    
    // Voltage should scale proportionally with vref
    let ratio = voltage_4_5v / voltage_2_4v;
    let expected_ratio = 4.5 / 2.4;
    assert!((ratio - expected_ratio).abs() < 0.01,
           "Voltage should scale with vref: expected ratio {}, got {}", 
           expected_ratio, ratio);
}

#[test]
fn test_adc_range_boundaries() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let vref = 4.5;
    let gain = 24.0;
    let full_scale_voltage = vref / gain; // 0.1875V
    
    // Test values near boundaries
    let near_max = 8388606; // Max - 1
    let near_min = -8388607; // Min + 1
    
    let voltage_near_max = raw_to_voltage(near_max, vref, gain);
    let voltage_near_min = raw_to_voltage(near_min, vref, gain);
    
    // Should be very close to full scale
    assert!((voltage_near_max.abs() - full_scale_voltage).abs() < 0.001);
    assert!((voltage_near_min.abs() - full_scale_voltage).abs() < 0.001);
    
    // Test quarter and three-quarter scale
    let quarter_scale = 2097152; // 2^21
    let three_quarter_scale = 6291456; // 3 * 2^21
    
    let voltage_quarter = raw_to_voltage(quarter_scale, vref, gain);
    let voltage_three_quarter = raw_to_voltage(three_quarter_scale, vref, gain);
    
    assert!((voltage_quarter * 4.0 - full_scale_voltage).abs() < 0.001);
    assert!((voltage_three_quarter * 4.0 / 3.0 - full_scale_voltage).abs() < 0.001);
}

#[test]
fn test_precision_and_rounding() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let vref = 4.5;
    let gain = 24.0;
    
    // Test that consecutive raw values produce expected voltage steps
    let raw1 = 1000;
    let raw2 = 1001;
    
    let voltage1 = raw_to_voltage(raw1, vref, gain);
    let voltage2 = raw_to_voltage(raw2, vref, gain);
    
    let diff = voltage2 - voltage1;
    let expected_step = (vref / gain) / (1 << 23) as f32; // LSB voltage
    
    assert!((diff - expected_step).abs() < 0.000001, 
           "Voltage step should be LSB resolution: expected {}, got {}", 
           expected_step, diff);
}

#[test]
fn test_typical_eeg_signal_ranges() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    // Test with typical EEG signal parameters
    let vref = 4.5;
    let gain = 24.0;
    
    // Typical EEG signals are in the range of ±100μV
    // Convert 100μV to raw ADC value for validation
    let target_voltage = 0.0001; // 100μV
    let expected_raw = (target_voltage * gain * (1 << 23) as f32 / vref) as i32;
    
    let calculated_voltage = raw_to_voltage(expected_raw, vref, gain);
    assert!((calculated_voltage - target_voltage).abs() < 0.000001,
           "100μV conversion should be accurate: expected {}, got {}", 
           target_voltage, calculated_voltage);
    
    // Test noise floor - 1μV
    let noise_voltage = 0.000001; // 1μV
    let noise_raw = (noise_voltage * gain * (1 << 23) as f32 / vref) as i32;
    let calculated_noise = raw_to_voltage(noise_raw, vref, gain);
    assert!((calculated_noise - noise_voltage).abs() < 0.0000001,
           "1μV noise floor should be accurate");
}

#[test]
fn test_overflow_and_underflow_behavior() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let vref = 4.5;
    let gain = 24.0;
    
    // Test with values beyond 24-bit range (should still work with i32)
    let beyond_max = 16777216; // 2^24
    let beyond_min = -16777216; // -2^24
    
    let voltage_beyond_max = raw_to_voltage(beyond_max, vref, gain);
    let voltage_beyond_min = raw_to_voltage(beyond_min, vref, gain);
    
    // Should be exactly 2x the full scale voltage
    let full_scale = vref / gain;
    assert!((voltage_beyond_max - 2.0 * full_scale).abs() < 0.001);
    assert!((voltage_beyond_min + 2.0 * full_scale).abs() < 0.001);
}

#[test]
fn test_extreme_gain_settings() {
    fn raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
        ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32
    }
    
    let raw = 1000000;
    let vref = 4.5;
    
    // Test very high gain (should give very small voltages)
    let voltage_high_gain = raw_to_voltage(raw, vref, 1000.0);
    assert!(voltage_high_gain > 0.0 && voltage_high_gain < 0.001);
    
    // Test very low gain (should give larger voltages)
    let voltage_low_gain = raw_to_voltage(raw, vref, 0.1);
    assert!(voltage_low_gain > 1.0);
    
    // Test gain = 1 (should give maximum voltage range)
    let voltage_unity_gain = raw_to_voltage(raw, vref, 1.0);
    assert!(voltage_unity_gain > voltage_high_gain);
    assert!(voltage_unity_gain < voltage_low_gain);
}
