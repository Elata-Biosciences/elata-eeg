mod board_drivers;

use tokio;
use std::error::Error;
use clap::Parser;
use eeg_driver::{AdcConfig, EegSystem, DriverType};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run in mock mode with random data
    #[arg(long)]
    mock: bool,

    /// Sample rate in Hz
    #[arg(long, default_value_t = 250)]
    sample_rate: u32,

    /// Channels to read (comma-separated)
    #[arg(long, value_delimiter = ',', default_values_t = vec![0, 1, 2, 3])]
    channels: Vec<usize>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Create a basic ADC configuration
    let config = AdcConfig {
        sample_rate: 250,
        channels: vec![0],
        gain: 1.0,
        board_driver: if args.mock { DriverType::Mock } else { DriverType::Ads1299 },
        batch_size: 4,
        Vref: 4.5,
        // Note: DSP filter parameters will be added to AdcConfig in future optimization
    };

    // Create the EEG system
    let (mut eeg_system, mut data_rx) = EegSystem::new(config.clone()).await?;
    
    // Create a copy of the channel count before moving config
    let channel_count = config.channels.len();
    
    // Print driver status before starting
    let status = eeg_system.driver_status().await;
    println!("Driver status before starting: {:?}", status);
    
    // Start the system
    eeg_system.start(config).await?;
    
    // Print driver status after starting
    let status = eeg_system.driver_status().await;
    println!("Driver status after starting: {:?}", status);
    
    // Print driver config
    if let Ok(config) = eeg_system.driver_config().await {
        println!("Driver config: {:?}", config);
    }

    // Example: Process received data for a while
    while let Some(processed_data) = data_rx.recv().await {
        println!("Received data with {} channels", channel_count);
        println!("Data: {:?}", processed_data); 
        // Add your data handling logic here
    }

    // Stop the system when done
    eeg_system.stop().await?;

    Ok(())
}
