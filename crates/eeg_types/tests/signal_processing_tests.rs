//! Signal processing algorithm tests
//!
//! These tests validate core signal processing algorithms used in EEG data processing
//! without requiring hardware dependencies.

use std::f32::consts::PI;

#[test]
fn test_filter_coefficient_calculation() {
    // Test Butterworth lowpass filter coefficient calculation
    fn lowpass_coefficients(sample_rate: f32, cutoff_freq: f32) -> (f32, f32, f32, f32, f32) {
        let q = 0.7071067811865476; // 1/sqrt(2)
        let omega = 2.0 * PI * cutoff_freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();

        let b0 = (1.0 - cos_omega) / 2.0;
        let b1 = 1.0 - cos_omega;
        let b2 = (1.0 - cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

        // Normalize by a0
        (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
    }

    // Test 40Hz lowpass at 500Hz sample rate
    let (b0, b1, b2, a1, a2) = lowpass_coefficients(500.0, 40.0);

    // Basic sanity checks
    assert!(b0 > 0.0 && b0 < 1.0);
    assert!(b1 > 0.0);
    assert!(b2 > 0.0 && b2 < 1.0);
    assert!(a1 < 0.0); // Should be negative for typical lowpass
    assert!(a2.abs() < 1.0); // Should be stable

    // For lowpass filters, DC gain should be close to 1
    let numerator = b0 + b1 + b2;
    let denominator = 1.0 + a1 + a2;
    let dc_gain = numerator / denominator;
    assert!(
        (dc_gain - 1.0).abs() < 0.1,
        "DC gain should be close to 1.0, got {}",
        dc_gain
    );
}

#[test]
fn test_highpass_filter_coefficients() {
    // Test Butterworth highpass filter coefficient calculation
    fn highpass_coefficients(sample_rate: f32, cutoff_freq: f32) -> (f32, f32, f32, f32, f32) {
        let q = 0.7071067811865476; // 1/sqrt(2)
        let omega = 2.0 * PI * cutoff_freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();

        let b0 = (1.0 + cos_omega) / 2.0;
        let b1 = -(1.0 + cos_omega);
        let b2 = (1.0 + cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

        // Normalize by a0
        (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
    }

    // Test 0.5Hz highpass at 500Hz sample rate
    let (b0, b1, b2, a1, a2) = highpass_coefficients(500.0, 0.5);

    // Basic sanity checks for highpass
    assert!(b0 > 0.0);
    assert!(b1 < 0.0); // Should be negative for highpass
    assert!(b2 > 0.0);
    assert!(a2.abs() < 1.0); // Should be stable

    // For highpass, DC gain should be close to 0
    let numerator = b0 + b1 + b2;
    let denominator = 1.0 + a1 + a2;
    let dc_gain = numerator / denominator;
    assert!(
        dc_gain.abs() < 0.1,
        "DC gain should be close to 0.0 for highpass, got {}",
        dc_gain
    );
}

#[test]
fn test_notch_filter_coefficients() {
    // Test notch filter coefficient calculation
    fn notch_coefficients(sample_rate: f32, notch_freq: f32, q: f32) -> (f32, f32, f32, f32, f32) {
        let omega = 2.0 * PI * notch_freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();

        let b0 = 1.0;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

        // Normalize by a0
        (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
    }

    // Test 60Hz notch at 500Hz sample rate
    let (b0, b1, b2, _a1, a2) = notch_coefficients(500.0, 60.0, 30.0);

    // Basic sanity checks for notch
    assert!((b0 - 1.0).abs() < 0.1); // Should be close to 1
    assert!(b1 < 0.0); // Should be negative
    assert!((b2 - 1.0).abs() < 0.1); // Should be close to 1
    assert!(a2.abs() < 1.0); // Should be stable
}

#[test]
fn test_biquad_filter_processing() {
    // Test a complete biquad filter implementation
    struct BiquadFilter {
        b0: f32,
        b1: f32,
        b2: f32,
        a1: f32,
        a2: f32,
        z1: f32,
        z2: f32,
    }

    impl BiquadFilter {
        fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
            Self {
                b0,
                b1,
                b2,
                a1,
                a2,
                z1: 0.0,
                z2: 0.0,
            }
        }

        fn process(&mut self, x: f32) -> f32 {
            let y = self.b0 * x + self.z1;
            self.z1 = self.b1 * x - self.a1 * y + self.z2;
            self.z2 = self.b2 * x - self.a2 * y;
            y
        }

        fn reset(&mut self) {
            self.z1 = 0.0;
            self.z2 = 0.0;
        }
    }

    // Test unity gain filter (should pass signal through)
    let mut filter = BiquadFilter::new(1.0, 0.0, 0.0, 0.0, 0.0);

    assert_eq!(filter.process(1.0), 1.0);
    assert_eq!(filter.process(2.0), 2.0);
    assert_eq!(filter.process(-1.0), -1.0);

    // Test filter reset
    filter.process(100.0); // Put some state in the filter
    filter.reset();
    assert_eq!(filter.z1, 0.0);
    assert_eq!(filter.z2, 0.0);
}

#[test]
fn test_eeg_signal_validation() {
    // Test typical EEG signal range validation
    fn is_valid_eeg_voltage(voltage_uv: f32) -> bool {
        // Typical EEG signals are ±200μV, extreme artifacts might go to ±1000μV
        voltage_uv.abs() <= 1000.0
    }

    fn classify_eeg_signal(voltage_uv: f32) -> &'static str {
        let abs_voltage = voltage_uv.abs();
        if abs_voltage <= 50.0 {
            "normal"
        } else if abs_voltage <= 200.0 {
            "elevated"
        } else if abs_voltage <= 500.0 {
            "artifact"
        } else if abs_voltage <= 1000.0 {
            "severe_artifact"
        } else {
            "invalid"
        }
    }

    // Test normal EEG ranges
    assert!(is_valid_eeg_voltage(50.0));
    assert_eq!(classify_eeg_signal(25.0), "normal");
    assert_eq!(classify_eeg_signal(150.0), "elevated");
    assert_eq!(classify_eeg_signal(300.0), "artifact");
    assert_eq!(classify_eeg_signal(800.0), "severe_artifact");
    assert_eq!(classify_eeg_signal(2000.0), "invalid");
}

#[test]
fn test_sample_rate_validation() {
    // Test comprehensive sample rate validation
    fn is_valid_sample_rate(rate: u32) -> bool {
        matches!(rate, 250 | 500 | 1000 | 2000 | 4000 | 8000)
    }

    fn get_nyquist_frequency(sample_rate: u32) -> f32 {
        sample_rate as f32 / 2.0
    }

    fn is_frequency_alias_safe(signal_freq: f32, sample_rate: u32) -> bool {
        signal_freq < get_nyquist_frequency(sample_rate)
    }

    // Test valid rates
    assert!(is_valid_sample_rate(250));
    assert!(is_valid_sample_rate(500));
    assert!(is_valid_sample_rate(1000));
    assert!(is_valid_sample_rate(8000));

    // Test invalid rates
    assert!(!is_valid_sample_rate(100));
    assert!(!is_valid_sample_rate(300));
    assert!(!is_valid_sample_rate(16000));

    // Test Nyquist frequency calculations
    assert_eq!(get_nyquist_frequency(500), 250.0);
    assert_eq!(get_nyquist_frequency(1000), 500.0);

    // Test aliasing protection
    assert!(is_frequency_alias_safe(40.0, 500)); // 40Hz at 500Hz sampling - OK
    assert!(is_frequency_alias_safe(100.0, 500)); // 100Hz at 500Hz sampling - OK
    assert!(!is_frequency_alias_safe(300.0, 500)); // 300Hz at 500Hz sampling - ALIASING!
}

#[test]
fn test_channel_mapping_logic() {
    // Test channel index validation and mapping
    fn map_physical_to_logical_channel(physical_ch: u8, active_channels: &[u8]) -> Option<usize> {
        active_channels.iter().position(|&ch| ch == physical_ch)
    }

    fn validate_channel_configuration(channels: &[u8]) -> Result<(), &'static str> {
        if channels.is_empty() {
            return Err("No channels configured");
        }

        if channels.len() > 32 {
            return Err("Too many channels (max 32)");
        }

        // Check for duplicates
        for (i, &ch1) in channels.iter().enumerate() {
            for &ch2 in channels.iter().skip(i + 1) {
                if ch1 == ch2 {
                    return Err("Duplicate channel detected");
                }
            }
        }

        // Check for invalid channel numbers
        for &ch in channels {
            if ch >= 32 {
                return Err("Invalid channel number (max 31)");
            }
        }

        Ok(())
    }

    let active_channels = vec![0, 2, 4, 6]; // Every other channel active

    // Test valid mappings
    assert_eq!(
        map_physical_to_logical_channel(0, &active_channels),
        Some(0)
    );
    assert_eq!(
        map_physical_to_logical_channel(2, &active_channels),
        Some(1)
    );
    assert_eq!(
        map_physical_to_logical_channel(4, &active_channels),
        Some(2)
    );
    assert_eq!(
        map_physical_to_logical_channel(6, &active_channels),
        Some(3)
    );

    // Test invalid channels
    assert_eq!(map_physical_to_logical_channel(1, &active_channels), None);
    assert_eq!(map_physical_to_logical_channel(8, &active_channels), None);

    // Test configuration validation
    assert!(validate_channel_configuration(&[0, 1, 2, 3]).is_ok());
    assert!(validate_channel_configuration(&[]).is_err());
    assert!(validate_channel_configuration(&[0, 1, 1, 2]).is_err()); // Duplicate
    assert!(validate_channel_configuration(&[0, 32]).is_err()); // Invalid channel
}

#[test]
fn test_timestamp_calculations() {
    // Test timestamp calculation logic
    fn calculate_sample_timestamp(
        base_timestamp_us: u64,
        sample_index: u64,
        sample_rate: f32,
    ) -> u64 {
        let sample_period_us = 1_000_000.0 / sample_rate;
        base_timestamp_us + (sample_index as f32 * sample_period_us) as u64
    }

    fn calculate_sample_period_us(sample_rate: f32) -> f32 {
        1_000_000.0 / sample_rate
    }

    fn samples_to_duration_us(num_samples: u64, sample_rate: f32) -> u64 {
        (num_samples as f32 * calculate_sample_period_us(sample_rate)) as u64
    }

    let base_time = 1_000_000; // 1 second
    let sample_rate = 500.0; // 500 Hz

    // Test first sample
    assert_eq!(
        calculate_sample_timestamp(base_time, 0, sample_rate),
        base_time
    );

    // Test second sample (should be 2ms later at 500Hz)
    let expected = base_time + 2000; // 2000μs = 2ms
    assert_eq!(
        calculate_sample_timestamp(base_time, 1, sample_rate),
        expected
    );

    // Test 500th sample (should be 1 second later)
    let expected = base_time + 1_000_000; // 1 second
    assert_eq!(
        calculate_sample_timestamp(base_time, 500, sample_rate),
        expected
    );

    // Test sample period calculations
    assert_eq!(calculate_sample_period_us(500.0), 2000.0); // 2ms
    assert_eq!(calculate_sample_period_us(1000.0), 1000.0); // 1ms

    // Test duration calculations
    assert_eq!(samples_to_duration_us(500, 500.0), 1_000_000); // 1 second
    assert_eq!(samples_to_duration_us(1000, 1000.0), 1_000_000); // 1 second
}

#[test]
fn test_data_buffer_management() {
    // Test circular buffer logic for data management
    struct CircularBuffer<T: Copy + Default> {
        data: Vec<T>,
        capacity: usize,
        write_pos: usize,
        size: usize,
    }

    impl<T: Copy + Default> CircularBuffer<T> {
        fn new(capacity: usize) -> Self {
            Self {
                data: vec![T::default(); capacity],
                capacity,
                write_pos: 0,
                size: 0,
            }
        }

        fn push(&mut self, value: T) {
            self.data[self.write_pos] = value;
            self.write_pos = (self.write_pos + 1) % self.capacity;
            if self.size < self.capacity {
                self.size += 1;
            }
        }

        fn len(&self) -> usize {
            self.size
        }

        fn is_full(&self) -> bool {
            self.size == self.capacity
        }

        fn get(&self, index: usize) -> Option<T> {
            if index < self.size {
                let actual_pos = if self.write_pos >= self.size {
                    index
                } else {
                    (self.write_pos + self.capacity - self.size + index) % self.capacity
                };
                Some(self.data[actual_pos])
            } else {
                None
            }
        }
    }

    let mut buffer: CircularBuffer<f32> = CircularBuffer::new(3);

    // Test filling buffer
    assert_eq!(buffer.len(), 0);
    assert!(!buffer.is_full());

    buffer.push(1.0);
    assert_eq!(buffer.len(), 1);
    assert_eq!(buffer.get(0), Some(1.0));

    buffer.push(2.0);
    buffer.push(3.0);
    assert_eq!(buffer.len(), 3);
    assert!(buffer.is_full());

    // Test overflow behavior
    buffer.push(4.0);
    assert_eq!(buffer.len(), 3); // Should still be 3
    assert!(buffer.is_full());
}

#[test]
fn test_gain_to_lsb_voltage() {
    // Test LSB voltage calculation for different gains
    fn calculate_lsb_voltage(vref: f32, gain: f32, adc_bits: u8) -> f32 {
        let full_scale_range = 2.0_f32.powi(adc_bits as i32 - 1); // 2^(n-1) for signed
        (vref / gain) / full_scale_range
    }

    fn voltage_to_adc_counts(voltage: f32, vref: f32, gain: f32, adc_bits: u8) -> i32 {
        let lsb = calculate_lsb_voltage(vref, gain, adc_bits);
        (voltage / lsb) as i32
    }

    // Test for 24-bit ADC with 4.5V reference
    let vref = 4.5;
    let adc_bits = 24;

    // Test different gain settings
    let lsb_gain1 = calculate_lsb_voltage(vref, 1.0, adc_bits);
    let lsb_gain24 = calculate_lsb_voltage(vref, 24.0, adc_bits);

    // Higher gain should give smaller LSB voltage
    assert!(lsb_gain24 < lsb_gain1);

    // Test specific expected values
    let expected_lsb_gain24 = 4.5 / 24.0 / 8388608.0; // About 22.35 nanovolts
    assert!((lsb_gain24 - expected_lsb_gain24).abs() < 0.000000001);

    // Test round-trip conversion
    let test_voltage = 0.0001; // 100μV
    let adc_counts = voltage_to_adc_counts(test_voltage, vref, 24.0, adc_bits);
    let recovered_voltage = adc_counts as f32 * lsb_gain24;
    assert!((recovered_voltage - test_voltage).abs() < lsb_gain24 * 2.0); // Within 2 LSBs
}

#[test]
fn test_frequency_domain_calculations() {
    // Test frequency domain calculations for EEG analysis
    fn calculate_frequency_bin(bin_index: usize, sample_rate: f32, fft_size: usize) -> f32 {
        (bin_index as f32 * sample_rate) / fft_size as f32
    }

    fn find_frequency_bin(target_freq: f32, sample_rate: f32, fft_size: usize) -> usize {
        ((target_freq * fft_size as f32) / sample_rate).round() as usize
    }

    fn is_eeg_frequency_band(freq: f32) -> &'static str {
        match freq {
            f if f >= 0.5 && f < 4.0 => "delta",
            f if f >= 4.0 && f < 8.0 => "theta",
            f if f >= 8.0 && f < 12.0 => "alpha",
            f if f >= 12.0 && f < 30.0 => "beta",
            f if f >= 30.0 && f < 100.0 => "gamma",
            _ => "other",
        }
    }

    let sample_rate = 500.0;
    let fft_size = 1024;

    // Test frequency bin calculations
    assert_eq!(calculate_frequency_bin(0, sample_rate, fft_size), 0.0);
    assert_eq!(
        calculate_frequency_bin(1, sample_rate, fft_size),
        sample_rate / fft_size as f32
    );

    // Test finding specific frequencies
    let alpha_bin = find_frequency_bin(10.0, sample_rate, fft_size); // 10Hz alpha wave
    let calculated_freq = calculate_frequency_bin(alpha_bin, sample_rate, fft_size);
    assert!((calculated_freq - 10.0).abs() < 1.0); // Should be close to 10Hz

    // Test EEG band classification
    assert_eq!(is_eeg_frequency_band(2.0), "delta");
    assert_eq!(is_eeg_frequency_band(6.0), "theta");
    assert_eq!(is_eeg_frequency_band(10.0), "alpha");
    assert_eq!(is_eeg_frequency_band(20.0), "beta");
    assert_eq!(is_eeg_frequency_band(50.0), "gamma");
    assert_eq!(is_eeg_frequency_band(150.0), "other");
}

#[test]
fn test_impedance_calculations() {
    // Test electrode impedance calculation logic
    fn calculate_impedance_ohms(test_current_ua: f32, measured_voltage_mv: f32) -> f32 {
        // Ohm's law: R = V / I
        (measured_voltage_mv / 1000.0) / (test_current_ua / 1_000_000.0)
    }

    fn classify_impedance(impedance_ohms: f32) -> &'static str {
        match impedance_ohms {
            z if z <= 5_000.0 => "excellent",
            z if z <= 25_000.0 => "good",
            z if z <= 50_000.0 => "acceptable",
            z if z <= 100_000.0 => "poor",
            _ => "unacceptable",
        }
    }

    // Test impedance calculations
    let impedance1 = calculate_impedance_ohms(10.0, 50.0); // 10μA, 50mV -> 5kΩ
    assert!((impedance1 - 5000.0).abs() < 1.0);
    assert_eq!(classify_impedance(impedance1), "excellent");

    let impedance2 = calculate_impedance_ohms(10.0, 200.0); // 10μA, 200mV -> 20kΩ
    assert!((impedance2 - 20000.0).abs() < 1.0);
    assert_eq!(classify_impedance(impedance2), "good");

    let impedance3 = calculate_impedance_ohms(10.0, 1000.0); // 10μA, 1V -> 100kΩ
    assert!((impedance3 - 100000.0).abs() < 1.0);
    assert_eq!(classify_impedance(impedance3), "poor");
}
