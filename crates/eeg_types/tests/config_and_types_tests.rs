//! Configuration and Data Type Tests
//!
//! Tests for configuration structures, data types, and serialization
//! functionality in the eeg_types crate.

use eeg_types::{
    config::{DaemonConfig, DriverType},
    data::SensorMeta,
};

#[test]
fn test_daemon_config_creation_and_defaults() {
    // Test the actual DaemonConfig struct
    let config = DaemonConfig::default();

    assert_eq!(config.max_recording_length_minutes, 60);
    assert_eq!(config.recordings_directory, "recordings");
    assert_eq!(config.batch_size, 128);
    assert_eq!(config.session, "session1");
    assert_eq!(config.driver_type, DriverType::MockEeg);

    // Test that we can modify values
    let custom_config = DaemonConfig {
        max_recording_length_minutes: 120,
        recordings_directory: "/custom/path".to_string(),
        batch_size: 256,
        session: "custom_session".to_string(),
        driver_type: DriverType::ElataV2,
        ..Default::default()
    };

    assert_eq!(custom_config.max_recording_length_minutes, 120);
    assert_eq!(custom_config.recordings_directory, "/custom/path");
    assert_eq!(custom_config.driver_type, DriverType::ElataV2);
}

#[test]
fn test_driver_type_variants() {
    // Test all DriverType variants exist and can be compared
    assert_eq!(DriverType::ElataV1, DriverType::ElataV1);
    assert_eq!(DriverType::ElataV2, DriverType::ElataV2);
    assert_eq!(DriverType::Ads1299, DriverType::Ads1299);
    assert_eq!(DriverType::MockEeg, DriverType::MockEeg);

    // Test they're different from each other
    assert_ne!(DriverType::ElataV1, DriverType::ElataV2);
    assert_ne!(DriverType::MockEeg, DriverType::Ads1299);
}

#[test]
fn test_sensor_meta_structure() {
    // Test the actual SensorMeta struct
    let meta = SensorMeta::default();

    // Test default values
    assert_eq!(meta.sensor_id, 0);
    assert_eq!(meta.meta_rev, 0);
    assert_eq!(meta.schema_ver, 1);
    assert_eq!(meta.source_type, "default");
    assert_eq!(meta.v_ref, 4.5);
    assert_eq!(meta.adc_bits, 24);
    assert_eq!(meta.gain, 1.0);
    assert_eq!(meta.sample_rate, 1000);
    assert_eq!(meta.offset_code, 0);
    assert!(meta.is_twos_complement);
    assert!(meta.channel_names.is_empty());
    assert!(meta.filter.is_none());

    // Test that we can create custom metadata
    let custom_meta = SensorMeta {
        sensor_id: 42,
        source_type: "ADS1299".to_string(),
        sample_rate: 500,
        gain: 24.0,
        channel_names: vec!["Fp1".to_string(), "Fp2".to_string()],
        ..Default::default()
    };

    assert_eq!(custom_meta.sensor_id, 42);
    assert_eq!(custom_meta.source_type, "ADS1299");
    assert_eq!(custom_meta.sample_rate, 500);
    assert_eq!(custom_meta.gain, 24.0);
    assert_eq!(custom_meta.channel_names.len(), 2);
}

#[test]
fn test_serialization_works() {
    // Test that our types can be serialized (important for config files)
    let config = DaemonConfig::default();

    // This should not panic
    let json_result = serde_json::to_string(&config);
    assert!(json_result.is_ok(), "DaemonConfig should be serializable");

    let json_str = json_result.unwrap();
    assert!(json_str.contains("recordings"));
    assert!(json_str.contains("session1"));

    // Test deserialization
    let deserialized_result: Result<DaemonConfig, _> = serde_json::from_str(&json_str);
    assert!(
        deserialized_result.is_ok(),
        "DaemonConfig should be deserializable"
    );

    let deserialized = deserialized_result.unwrap();
    assert_eq!(deserialized.session, config.session);
    assert_eq!(deserialized.driver_type, config.driver_type);
}

#[test]
fn test_driver_type_serialization() {
    // Test DriverType serialization (important for config files)
    let driver_types = vec![
        DriverType::ElataV1,
        DriverType::ElataV2,
        DriverType::Ads1299,
        DriverType::MockEeg,
    ];

    for driver_type in driver_types {
        let json_result = serde_json::to_string(&driver_type);
        assert!(json_result.is_ok(), "DriverType should be serializable");

        let json_str = json_result.unwrap();
        let deserialized_result: Result<DriverType, _> = serde_json::from_str(&json_str);
        assert!(
            deserialized_result.is_ok(),
            "DriverType should be deserializable"
        );

        let deserialized = deserialized_result.unwrap();
        assert_eq!(deserialized, driver_type);
    }
}
