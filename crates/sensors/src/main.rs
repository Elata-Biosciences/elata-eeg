use tokio;
use std::error::Error;
use clap::Parser;
use eeg_sensor::{AdcConfig, DriverType};

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
        board_driver: if args.mock { DriverType::MockEeg } else { DriverType::Ads1299 },
        batch_size: 4,
        vref: 4.5,
    };

    // Note: EegSystem has been moved to the device crate (elata_emu_v1 module)
    // This main.rs is now just for testing individual sensor drivers
    println!("Sensors crate main - use device crate for full EEG system");
    println!("Config: {:?}", config);

    Ok(())
}
