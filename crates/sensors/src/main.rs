use std::error::Error;
use clap::Parser;
use sensors::types::{AdcConfig, ChipConfig};

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
    channels: Vec<u8>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Create a basic ADC configuration using chip-based format
    let chip_config = ChipConfig {
        channels: args.channels,
        spi_bus: 0,
        cs_pin: 0,
    };

    let config = AdcConfig {
        sample_rate: args.sample_rate,
        vref: 4.5,
        gain: 1.0,
        drdy_pin: 25,
        chips: vec![chip_config],
    };

    // Note: EegSystem has been moved to the device crate (elata_emu_v1 module)
    // This main.rs is now just for testing individual sensor drivers
    println!("Sensors crate main - use device crate for full EEG system");
    println!("Config: {:?}", config);

    Ok(())
}
