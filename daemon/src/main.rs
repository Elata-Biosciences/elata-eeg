mod config;
mod driver_handler;
mod server;

use eeg_driver::{AdcConfig, EegSystem, DriverType};
use tokio::sync::{broadcast, Mutex};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load daemon configuration
    let daemon_config = config::load_config();
    println!("Daemon configuration:");
    println!("  Max recording length: {} minutes", daemon_config.max_recording_length_minutes);
    println!("  Recordings directory: {}", daemon_config.recordings_directory);
    println!("  High-pass filter cutoff: {} Hz", daemon_config.dsp_high_pass_cutoff_hz);
    println!("  Low-pass filter cutoff: {} Hz", daemon_config.dsp_low_pass_cutoff_hz);
    
    // Debug: Print current working directory
    match std::env::current_dir() {
        Ok(path) => println!("Current working directory: {:?}", path),
        Err(e) => println!("Failed to get current working directory: {}", e),
    }

    // Increase channel capacity but not too much to avoid excessive buffering
    let (tx, _) = broadcast::channel::<driver_handler::EegBatchData>(32);  // Reduced from 1024
    let tx_ws = tx.clone();

    // Create the ADC configuration
    let config = AdcConfig {
        sample_rate: 250,
        // channels: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
        channels: vec![0, 1],
        gain: 24.0,
        board_driver: DriverType::Ads1299,
        batch_size: 16,
        Vref: 4.5,
        dsp_high_pass_cutoff_hz: daemon_config.dsp_high_pass_cutoff_hz,
        dsp_low_pass_cutoff_hz: daemon_config.dsp_low_pass_cutoff_hz,
    };

    println!("Starting EEG system...");
    
    // Create and start the EEG system
    let (mut eeg_system, data_rx) = EegSystem::new(config.clone()).await?;
    eeg_system.start(config.clone()).await?;

    println!("EEG system started. Waiting for data...");

    // Create CSV recorder with daemon config and ADC config
    let csv_recorder = Arc::new(Mutex::new(driver_handler::CsvRecorder::new(
        config.sample_rate,
        daemon_config.clone(),
        config.clone()
    )));

    // Set up WebSocket routes
    let ws_routes = server::setup_websocket_routes(tx_ws, config.clone(), csv_recorder.clone());

    println!("WebSocket server starting on:");
    println!("- ws://localhost:8080/eeg (EEG data)");
    println!("- ws://localhost:8080/config (Configuration)");
    println!("- ws://localhost:8080/command (Recording control)");

    // Spawn WebSocket server
    let server_handle = tokio::spawn(warp::serve(ws_routes).run(([127, 0, 0, 1], 8080)));

    // Process EEG data
    let processing_handle = tokio::spawn(driver_handler::process_eeg_data(
        data_rx,
        tx,
        csv_recorder.clone()
    ));

    // Wait for tasks to complete
    tokio::select! {
        _ = processing_handle => println!("Processing task completed"),
        _ = server_handle => println!("Server task completed"),
    }

    // Cleanup
    eeg_system.stop().await?;
    
    Ok(())
}
