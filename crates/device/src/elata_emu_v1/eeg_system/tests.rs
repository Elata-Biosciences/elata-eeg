use super::*;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_eeg_system_lifecycle() -> Result<(), Box<dyn Error>> {
    // Create a basic configuration
    let config = AdcConfig {
        sample_rate: 250,
        channels: vec![0, 1], // Two channels
        gain: 1.0,
        ..Default::default()
    };

    // Create system with mock driver
    let (mut system, mut rx) = EegSystem::new(config.clone(), false).await?;
    
    // Check initial state
    assert_eq!(system.driver_status(), DriverStatus::NotInitialized);
    
    // Start the system
    system.start(config.clone()).await?;
    assert_eq!(system.driver_status(), DriverStatus::Running);
    
    // Wait briefly to collect some data
    let timeout = Duration::from_millis(100);
    let start = std::time::Instant::now();
    let mut data_received = false;
    
    while start.elapsed() < timeout {
        match rx.try_recv() {
            Ok(data) => {
                data_received = true;
                // Verify data structure
                assert_eq!(data.channel_count, 2);
                assert!(!data.data.is_empty());
                break;
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                sleep(Duration::from_millis(10)).await;
                continue;
            }
            Err(e) => return Err(Box::new(DriverError::Other(format!("Receive error: {}", e)))),
        }
    }
    
    assert!(data_received, "No data received within timeout");
    
    // Test stopping
    system.stop().await?;
    assert_eq!(system.driver_status(), DriverStatus::Stopped);
    
    // Test shutdown
    system.shutdown().await?;
    assert_eq!(system.driver_status(), DriverStatus::NotInitialized);
    
    Ok(())
}

#[tokio::test]
async fn test_eeg_system_reconfigure() -> Result<(), Box<dyn Error>> {
    let initial_config = AdcConfig {
        sample_rate: 250,
        channels: vec![0],
        gain: 1.0,
        ..Default::default()
    };

    let (mut system, _rx) = EegSystem::new(initial_config.clone(), false).await?;
    system.start(initial_config).await?;
    
    // Test reconfiguration with different settings
    let new_config = AdcConfig {
        sample_rate: 500,
        channels: vec![0, 1],
        gain: 2.0,
        ..Default::default()
    };
    
    system.reconfigure(new_config.clone()).await?;
    
    // Verify new configuration took effect
    let current_config = system.driver_config()?;
    assert_eq!(current_config.sample_rate, new_config.sample_rate);
    assert_eq!(current_config.channels.len(), new_config.channels.len());
    
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<(), Box<dyn Error>> {
    // Test invalid configuration
    let invalid_config = AdcConfig {
        sample_rate: 0, // Invalid sample rate
        channels: vec![],
        gain: 1.0,
        ..Default::default()
    };

    let (mut system, _rx) = EegSystem::new(invalid_config.clone(), false).await?;
    
    // Should fail with appropriate error
    let result = system.start(invalid_config).await;
    assert!(result.is_err());
    
    if let Err(e) = result {
        assert!(e.to_string().contains("Sample rate must be greater than 0"));
    }
    
    Ok(())
}

#[tokio::test]
async fn test_signal_processing() -> Result<(), Box<dyn Error>> {
    let config = AdcConfig {
        sample_rate: 250,
        channels: vec![0],
        gain: 1.0,
        ..Default::default()
    };

    let (mut system, mut rx) = EegSystem::new(config.clone(), false).await?;
    system.start(config).await?;
    
    // Collect some processed data
    let mut samples = Vec::new();
    let timeout = Duration::from_millis(200);
    let start = std::time::Instant::now();
    
    while start.elapsed() < timeout {
        if let Ok(data) = rx.try_recv() {
            samples.extend(data.data[0].clone()); // Get channel 0 data
            if samples.len() >= 50 {
                break;
            }
        }
        sleep(Duration::from_millis(10)).await;
    }
    
    // Verify we got enough samples
    assert!(samples.len() >= 50, "Not enough samples collected");
    
    // Basic signal validation
    for sample in samples {
        // Check if the processed values are within reasonable bounds
        assert!(sample.abs() < 8192.0, "Sample outside expected range");
    }
    
    system.shutdown().await?;
    Ok(())
}

// Add this to src/eeg_system.rs:
#[cfg(test)]
mod tests; 