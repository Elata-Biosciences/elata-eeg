// Re-export modules for testing
pub mod config;
pub mod driver_handler;
pub mod server;
pub mod pid_manager;
pub mod connection_manager;

// Test modules
#[cfg(test)]
mod config_test;
#[cfg(test)]
mod driver_handler_test;
#[cfg(test)]
mod server_test;

#[cfg(test)]
mod integration_tests {
    use crate::config::DaemonConfig;
    use crate::driver_handler::{CsvRecorder, EegBatchData};
    use crate::server::create_eeg_binary_packet;
    use eeg_driver::{AdcConfig, DriverType};
    use std::sync::Arc;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_end_to_end_data_flow() {
        // Create configuration
        let daemon_config = Arc::new(DaemonConfig {
            max_recording_length_minutes: 10,
            recordings_directory: "./test_recordings".to_string(),
        });

        // Create ADC configuration
        let adc_config = AdcConfig {
            sample_rate: 250,
            channels: vec![0, 1, 2, 3],
            gain: 4.0,
            board_driver: DriverType::Mock,
            batch_size: 32,
            Vref: 4.5,
        };

        // Create broadcast channel
        let (tx, mut rx) = broadcast::channel::<EegBatchData>(32);

        // Create CSV recorder (not used in this test, but kept for future expansion)
        let _recorder = CsvRecorder::new(
            adc_config.sample_rate,
            daemon_config.clone(),
            adc_config.clone(),
        );

        // Create test data
        let test_data = EegBatchData {
            channels: vec![
                vec![1.0, 2.0, 3.0],
                vec![4.0, 5.0, 6.0],
                vec![7.0, 8.0, 9.0],
                vec![10.0, 11.0, 12.0],
            ],
            timestamp: 1000000,
        };

        // Send test data to channel
        tx.send(test_data.clone()).expect("Failed to send test data");

        // Receive data from channel
        let received_data = rx.recv().await.expect("Failed to receive test data");

        // Verify received data
        assert_eq!(received_data.timestamp, test_data.timestamp);
        assert_eq!(received_data.channels.len(), test_data.channels.len());
        
        for (i, channel) in received_data.channels.iter().enumerate() {
            assert_eq!(channel, &test_data.channels[i]);
        }

        // Create binary packet from received data
        let binary_packet = create_eeg_binary_packet(&received_data);

        // Verify binary packet size
        let expected_size = 8 + (4 * 3 * 4); // 8 bytes timestamp + 4 channels * 3 samples * 4 bytes per float
        assert_eq!(binary_packet.len(), expected_size);

        // Verify timestamp in binary packet
        let timestamp_bytes = &binary_packet[0..8];
        let timestamp = u64::from_le_bytes([
            timestamp_bytes[0], timestamp_bytes[1], timestamp_bytes[2], timestamp_bytes[3],
            timestamp_bytes[4], timestamp_bytes[5], timestamp_bytes[6], timestamp_bytes[7],
        ]);
        assert_eq!(timestamp, 1000000);
    }
}