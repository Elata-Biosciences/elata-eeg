TL;DR: This..

## Simple Description
This..

## Behavior
- Reconfiguration: No dynamic reconfiguration. Reconfigure by shutdown then destruct obj
- Sample time: Based on the ADC clock. First sample comes from the Pi 5
## Sample Code

## Basic Usage
```rust
use eeg_driver::{AdcConfig, EegSystem, DriverType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = AdcConfig {
        sample_rate: 250,
        channels: vec![0, 1, 2, 3],
        gain: 4.0,
        board_driver: DriverType::Mock,
        batch_size: 32,
        Vref: 4.5,
    };

    // Initialize the EEG system
    let (mut eeg_system, mut data_rx) = EegSystem::new(config.clone()).await?;
    
    // Start acquisition
    eeg_system.start(config).await?;

    // Process data
    while let Some(processed_data_batch) = data_rx.recv().await {
        println!("Received data with timestamp: {}", processed_data_batch.timestamp);
        // Process your data here
    }

    // Always shut down properly when done
    eeg_system.shutdown().await?;
    
    Ok(())
}
```