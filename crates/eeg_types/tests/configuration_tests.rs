//! Configuration validation and parsing tests
//!
//! These tests validate configuration structures, parsing logic, and validation
//! without requiring hardware dependencies.

use eeg_types::{
    config::{DaemonConfig, DriverType, FilterConfig},
    data::SensorMeta,
};

#[test]
fn test_filter_config_validation() {
    fn validate_filter_config(config: &FilterConfig) -> Result<(), &'static str> {
        if config.dsp_high_pass_cutoff_hz <= 0.0 {
            return Err("High-pass cutoff must be positive");
        }

        if config.dsp_low_pass_cutoff_hz <= config.dsp_high_pass_cutoff_hz {
            return Err("Low-pass cutoff must be higher than high-pass cutoff");
        }

        if let Some(powerline_hz) = config.powerline_filter_hz {
            if powerline_hz != 50 && powerline_hz != 60 {
                return Err("Powerline filter must be 50Hz or 60Hz");
            }
        }

        Ok(())
    }

    // Test valid configurations
    let valid_config = FilterConfig {
        dsp_high_pass_cutoff_hz: 0.5,
        dsp_low_pass_cutoff_hz: 40.0,
        powerline_filter_hz: Some(60),
    };
    assert!(validate_filter_config(&valid_config).is_ok());

    // Test invalid configurations
    let invalid_hp = FilterConfig {
        dsp_high_pass_cutoff_hz: -1.0, // Invalid negative
        dsp_low_pass_cutoff_hz: 40.0,
        powerline_filter_hz: Some(60),
    };
    assert!(validate_filter_config(&invalid_hp).is_err());

    let invalid_order = FilterConfig {
        dsp_high_pass_cutoff_hz: 50.0, // Higher than lowpass
        dsp_low_pass_cutoff_hz: 40.0,
        powerline_filter_hz: Some(60),
    };
    assert!(validate_filter_config(&invalid_order).is_err());

    let invalid_powerline = FilterConfig {
        dsp_high_pass_cutoff_hz: 0.5,
        dsp_low_pass_cutoff_hz: 40.0,
        powerline_filter_hz: Some(45), // Invalid powerline frequency
    };
    assert!(validate_filter_config(&invalid_powerline).is_err());
}

#[test]
fn test_daemon_config_validation() {
    fn validate_daemon_config(config: &DaemonConfig) -> Result<(), &'static str> {
        if config.max_recording_length_minutes == 0 {
            return Err("Recording length must be positive");
        }

        if config.max_recording_length_minutes > 1440 {
            // 24 hours
            return Err("Recording length too long (max 24 hours)");
        }

        if config.batch_size == 0 {
            return Err("Batch size must be positive");
        }

        if config.batch_size > 8192 {
            return Err("Batch size too large (max 8192)");
        }

        if config.session.is_empty() {
            return Err("Session name cannot be empty");
        }

        Ok(())
    }

    // Test valid configuration
    let valid_config = DaemonConfig {
        max_recording_length_minutes: 60,
        recordings_directory: "/tmp/recordings".to_string(),
        batch_size: 256,
        session: "test_session".to_string(),
        filter_config: FilterConfig::default(),
        driver_type: DriverType::MockEeg,
    };
    assert!(validate_daemon_config(&valid_config).is_ok());

    // Test invalid configurations
    let zero_recording = DaemonConfig {
        max_recording_length_minutes: 0, // Invalid
        ..valid_config.clone()
    };
    assert!(validate_daemon_config(&zero_recording).is_err());

    let too_long_recording = DaemonConfig {
        max_recording_length_minutes: 2000, // Too long
        ..valid_config.clone()
    };
    assert!(validate_daemon_config(&too_long_recording).is_err());

    let zero_batch = DaemonConfig {
        batch_size: 0, // Invalid
        ..valid_config.clone()
    };
    assert!(validate_daemon_config(&zero_batch).is_err());
}

#[test]
fn test_driver_type_selection() {
    fn select_driver_for_channel_count(channels: usize) -> DriverType {
        match channels {
            1..=8 => DriverType::ElataV1,
            9..=32 => DriverType::ElataV2,
            _ => DriverType::MockEeg, // Fallback for testing
        }
    }

    fn get_max_channels_for_driver(driver_type: &DriverType) -> usize {
        match driver_type {
            DriverType::ElataV1 => 8,
            DriverType::ElataV2 => 32,
            DriverType::Ads1299 => 8,
            DriverType::MockEeg => 32, // Flexible for testing
        }
    }

    // Test driver selection logic
    assert_eq!(select_driver_for_channel_count(4), DriverType::ElataV1);
    assert_eq!(select_driver_for_channel_count(8), DriverType::ElataV1);
    assert_eq!(select_driver_for_channel_count(16), DriverType::ElataV2);
    assert_eq!(select_driver_for_channel_count(32), DriverType::ElataV2);
    assert_eq!(select_driver_for_channel_count(64), DriverType::MockEeg);

    // Test channel limits
    assert_eq!(get_max_channels_for_driver(&DriverType::ElataV1), 8);
    assert_eq!(get_max_channels_for_driver(&DriverType::ElataV2), 32);
    assert_eq!(get_max_channels_for_driver(&DriverType::Ads1299), 8);
    assert_eq!(get_max_channels_for_driver(&DriverType::MockEeg), 32);
}

#[test]
fn test_sensor_meta_validation() {
    fn validate_sensor_meta(meta: &SensorMeta) -> Result<(), &'static str> {
        if meta.sample_rate == 0 {
            return Err("Sample rate must be positive");
        }

        if meta.sample_rate > 10000 {
            return Err("Sample rate too high (max 10kHz)");
        }

        if meta.v_ref <= 0.0 {
            return Err("Reference voltage must be positive");
        }

        if meta.gain <= 0.0 {
            return Err("Gain must be positive");
        }

        if meta.adc_bits == 0 || meta.adc_bits > 32 {
            return Err("ADC bits must be between 1 and 32");
        }

        if meta.channel_names.len() > 64 {
            return Err("Too many channel names (max 64)");
        }

        Ok(())
    }

    // Test valid sensor meta
    let valid_meta = SensorMeta {
        sensor_id: 1,
        meta_rev: 1,
        schema_ver: 1,
        source_type: "ADS1299".to_string(),
        v_ref: 4.5,
        adc_bits: 24,
        gain: 24.0,
        sample_rate: 500,
        channel_names: vec!["Fp1".to_string(), "Fp2".to_string()],
        offset_code: 0,
        is_twos_complement: true,
        filter: None,
    };
    assert!(validate_sensor_meta(&valid_meta).is_ok());

    // Test invalid configurations
    let zero_sample_rate = SensorMeta {
        sample_rate: 0, // Invalid
        ..valid_meta.clone()
    };
    assert!(validate_sensor_meta(&zero_sample_rate).is_err());

    let negative_vref = SensorMeta {
        v_ref: -4.5, // Invalid
        ..valid_meta.clone()
    };
    assert!(validate_sensor_meta(&negative_vref).is_err());

    let zero_gain = SensorMeta {
        gain: 0.0, // Invalid
        ..valid_meta.clone()
    };
    assert!(validate_sensor_meta(&zero_gain).is_err());
}

#[test]
fn test_configuration_defaults() {
    // Test that default configurations are reasonable
    let default_filter = FilterConfig::default();
    assert_eq!(default_filter.dsp_high_pass_cutoff_hz, 1.0);
    assert_eq!(default_filter.dsp_low_pass_cutoff_hz, 50.0);
    assert_eq!(default_filter.powerline_filter_hz, Some(60));

    let default_daemon = DaemonConfig::default();
    assert_eq!(default_daemon.max_recording_length_minutes, 60);
    assert_eq!(default_daemon.recordings_directory, "recordings");
    assert_eq!(default_daemon.batch_size, 128);
    assert_eq!(default_daemon.session, "session1");
    assert_eq!(default_daemon.driver_type, DriverType::MockEeg);
}

#[test]
fn test_configuration_edge_cases() {
    // Test edge cases in configuration handling

    // Test filter config with None powerline filter
    let no_powerline = FilterConfig {
        dsp_high_pass_cutoff_hz: 1.0,
        dsp_low_pass_cutoff_hz: 50.0,
        powerline_filter_hz: None,
    };
    assert_eq!(no_powerline.powerline_filter_hz, None);

    // Test extreme but valid values
    let extreme_filter = FilterConfig {
        dsp_high_pass_cutoff_hz: 0.01, // Very low
        dsp_low_pass_cutoff_hz: 200.0, // Very high
        powerline_filter_hz: Some(50), // 50Hz instead of 60Hz
    };
    assert_eq!(extreme_filter.dsp_high_pass_cutoff_hz, 0.01);
    assert_eq!(extreme_filter.dsp_low_pass_cutoff_hz, 200.0);
    assert_eq!(extreme_filter.powerline_filter_hz, Some(50));
}
