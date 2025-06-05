mod config;
mod driver_handler;
mod server;
mod pid_manager;
mod connection_manager;

use eeg_driver::{AdcConfig, EegSystem};
use eeg_driver::dsp::coordinator::DspCoordinator;
use connection_manager::ConnectionManager;
use tokio::sync::{broadcast, Mutex};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;
use std::fmt;
use warp::Filter;

// Define a custom error type that implements Send + Sync
#[derive(Debug)]
struct DaemonError(String);

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DaemonError {}

// Helper function to convert any error to our custom error type
fn to_daemon_error<E: std::fmt::Display>(e: E) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(DaemonError(e.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logger - Reads RUST_LOG environment variable
    env_logger::init();

    // Initialize PID manager to ensure single daemon instance
    let pid_file_path = "/tmp/eeg_daemon.pid";
    let pid_manager = pid_manager::PidManager::new(pid_file_path);
    
    // Check if another instance is already running
    if let Err(e) = pid_manager.acquire_lock() {
        eprintln!("Failed to start daemon: {}", e);
        eprintln!("If you're sure no other instance is running, try removing the PID file: {}", pid_file_path);
        std::process::exit(1);
    }
    
    println!("EEG Daemon starting (PID: {})...", std::process::id());

    // Load daemon configuration
    let daemon_config = config::load_config();
    println!("Daemon configuration:");
    println!("  Max recording length: {} minutes", daemon_config.max_recording_length_minutes);
    println!("  Recordings directory: {}", daemon_config.recordings_directory);
    println!("  High-pass filter cutoff: {} Hz", daemon_config.filter_config.dsp_high_pass_cutoff_hz);
    println!("  Low-pass filter cutoff: {} Hz", daemon_config.filter_config.dsp_low_pass_cutoff_hz);
    println!("  Batch size: {}", daemon_config.batch_size);
    println!("  Driver type: {:?}", daemon_config.driver_type);
    // Powerline filter removed as part of DSP refactor
    
    // Debug: Print current working directory
    match std::env::current_dir() {
        Ok(path) => println!("Current working directory: {:?}", path),
        Err(e) => println!("Failed to get current working directory: {}", e),
    }

    // Increase channel capacity but not too much to avoid excessive buffering
    // Channel for existing /eeg endpoint (unfiltered EegBatchData)
    let (tx_eeg_batch_data, _) = broadcast::channel::<driver_handler::EegBatchData>(32);
    let tx_eeg_batch_data_ws = tx_eeg_batch_data.clone();

    // Channel for new /ws/eeg/data__basic_voltage_filter endpoint (FilteredEegData)
    let (tx_filtered_eeg_data, _) = broadcast::channel::<driver_handler::FilteredEegData>(32);
    let tx_filtered_eeg_data_ws = tx_filtered_eeg_data.clone();

    // Create the ADC configuration
    let initial_config = AdcConfig {
        sample_rate: 500, // Example, should ideally come from a more specific hardware config or AdcConfig defaults
        channels: vec![0, 1, 2], // Example
        gain: 24.0, // Example
        board_driver: daemon_config.driver_type,
        batch_size: daemon_config.batch_size, // This batch_size is for the driver
        Vref: 4.5, // Example
        // DSP fields are removed from AdcConfig as per Phase 1
        // dsp_high_pass_cutoff_hz, dsp_low_pass_cutoff_hz, powerline_filter_hz
        // are now managed by the daemon via daemon_config.filter_config for its own SignalProcessor
    };
    
    // Create shared state
    let config = Arc::new(Mutex::new(initial_config.clone()));
    let is_recording = Arc::new(AtomicBool::new(false));

    println!("Starting EEG system...");
    
    // Create and start the EEG system
    let (mut eeg_system, data_rx) = EegSystem::new(initial_config.clone()).await
        .map_err(to_daemon_error)?;
    
    eeg_system.start(initial_config.clone()).await
        .map_err(to_daemon_error)?;

    println!("EEG system started. Waiting for data...");

    // Initialize DSP coordinator for centralized processing
    println!("Initializing DSP coordinator...");
    let dsp_coordinator = Arc::new(Mutex::new(DspCoordinator::new(config.clone()).await));
    
    // Initialize connection manager for tracking WebSocket clients
    let default_channels: Vec<usize> = (0..initial_config.channels.len()).collect();
    let connection_manager = Arc::new(ConnectionManager::new(dsp_coordinator.clone(), default_channels));
    
    println!("DSP coordinator and connection manager initialized");

    // Create a broadcast channel for applied config updates
    let (config_applied_tx, _) = broadcast::channel::<AdcConfig>(16); // Channel for broadcasting applied configs

    // Broadcast the initial configuration
    if let Err(e) = config_applied_tx.send(initial_config.clone()) {
        println!("Error broadcasting initial config: {}", e);
    }

    // Create CSV recorder with daemon config and ADC config
    let csv_recorder = Arc::new(Mutex::new(driver_handler::CsvRecorder::new(
        initial_config.sample_rate,
        daemon_config.clone(),
        initial_config.clone(),
        is_recording.clone()
    )));

    // Set up WebSocket routes and get config update channel
    let (base_ws_routes, mut config_update_rx) = server::setup_websocket_routes(
        tx_eeg_batch_data_ws, // For existing /eeg endpoint
        tx_filtered_eeg_data_ws, // For new filtered data endpoint
        config.clone(),
        csv_recorder.clone(),
        is_recording.clone(),
        config_applied_tx.clone(), // Pass the sender for applied configs
        connection_manager.clone(), // Pass connection manager for client tracking
    );

    // Initialize DSP modules and combine routes
    let ws_routes = {
        #[cfg(feature = "brain_waves_fft_feature")]
        {
            use elata_dsp_brain_waves_fft::{setup_fft_websocket_endpoint, DspSharedConfig};
            
            let dsp_config = DspSharedConfig {};
            let eeg_data_tx_for_dsp = tx_eeg_batch_data.clone();
            let adc_config_tx_for_dsp = config_applied_tx.clone(); // This is the broadcast sender for updates
            let shared_adc_config_for_dsp = config.clone(); // This is the Arc<Mutex<AdcConfig>> for initial state
            
            let fft_route = setup_fft_websocket_endpoint(
                &dsp_config,
                eeg_data_tx_for_dsp,
                adc_config_tx_for_dsp,
                shared_adc_config_for_dsp, // Pass the Arc<Mutex<AdcConfig>>
            );
            
            // Combine FFT route with main server routes on port 8080
            base_ws_routes.or(fft_route)
        }
        #[cfg(not(feature = "brain_waves_fft_feature"))]
        {
            base_ws_routes
        }
    };
    
    println!("WebSocket server starting on:");
    println!("- ws://0.0.0.0:8080/eeg (EEG data) - accessible via this machine's IP address");
    println!("- ws://0.0.0.0:8080/config (Configuration) - accessible via this machine's IP address");
    println!("- ws://0.0.0.0:8080/command (Recording control) - accessible via this machine's IP address");
    println!("- ws://0.0.0.0:8080/ws/eeg/data__basic_voltage_filter (Filtered EEG data) - accessible via this machine's IP address");
    #[cfg(feature = "brain_waves_fft_feature")]
    println!("- ws://0.0.0.0:8080/applet/brain_waves/data (Brain Waves FFT) - accessible via this machine's IP address");
    

    // Spawn WebSocket server
    let server_handle = tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080)));

    // Create a cancellation token for the processing task
    let processing_token = CancellationToken::new();
    let processing_token_clone = processing_token.clone();

    // Process EEG data
    let processing_handle = tokio::spawn(driver_handler::process_eeg_data(
        data_rx,
        tx_eeg_batch_data.clone(), // For existing /eeg endpoint
        tx_filtered_eeg_data.clone(), // For new filtered data endpoint
        csv_recorder.clone(),
        is_recording.clone(),
        connection_manager.clone(), // For demand-based processing
        processing_token
    ));

    // Create a loop to handle configuration updates
    let mut current_processing_handle = processing_handle;
    let mut current_eeg_system = eeg_system;
    let mut current_token = processing_token_clone;
    
    // Create a oneshot channel to signal when the server is done
    let (server_tx, mut server_rx) = tokio::sync::oneshot::channel();
    
    // Spawn a task to wait for the server to complete and send a signal
    tokio::spawn(async move {
        let _ = server_handle.await;
        let _ = server_tx.send(());
    });
    
    let mut server_done = false;
    let mut processing_done = false;
    
    while !server_done && !processing_done {
        tokio::select! {
            result = &mut current_processing_handle => {
                println!("Processing task completed: {:?}", result);
                processing_done = true;
            },
            _ = &mut server_rx => {
                println!("Server task completed");
                server_done = true;
            },
            config_update = config_update_rx.recv() => {
                if let Some(new_config_from_channel) = config_update {
                    println!("[MAIN] Received proposed config update. Channels: {:?}, Sample rate: {}", new_config_from_channel.channels, new_config_from_channel.sample_rate);
                    
                    // Check if recording is in progress
                    let recording_in_progress = is_recording.load(Ordering::Relaxed);
                    
                    if recording_in_progress {
                        println!("Warning: Cannot update configuration during recording");
                    } else {
                        // IMPORTANT: First check if the new config is actually different from the current one
                        // This must happen BEFORE stopping the system and BEFORE updating the shared config
                        let current_shared_config = {
                            let config_guard = config.lock().await;
                            config_guard.clone()
                        };
                        println!("[MAIN] Current shared config before comparison. Channels: {:?}, Sample rate: {}", current_shared_config.channels, current_shared_config.sample_rate);
                        
                        // Check if powerline filter is being turned off (set to None)
                        // Powerline filter logic removed as part of DSP refactor
                        let powerline_filter_turning_off = false;
                            
                        if powerline_filter_turning_off {
                            println!("[MAIN] CRITICAL: Detected powerline filter being turned OFF. Forcing update regardless of equality check.");
                            // Continue to the update code below
                        }
                        // If the config hasn't changed and we're not turning off powerline filter, skip the restart
                        else if new_config_from_channel == current_shared_config {
                            println!("[MAIN] Proposed configuration is THE SAME as current shared_config. Skipping restart. Proposed channels: {:?}, Current channels: {:?}",
                                new_config_from_channel.channels, current_shared_config.channels);
                            
                            // Even if we skip, let's ensure the shared config is what we think it is and broadcast it,
                            // as the server.rs might have sent "unchanged" based on a different view if there was a race.
                            // However, server.rs makes its "unchanged" decision *before* sending to main.
                            // The key is that if main skips, it doesn't broadcast an "applied" config.
                            // The client is waiting for an applied config.
                            // If main skips, the client gets nothing new after "unchanged" or "submitted".
                            // Let's send the current_shared_config if we skip, so the client gets *something*.
                            if let Err(e) = config_applied_tx.send(current_shared_config.clone()) {
                                println!("[MAIN] Error broadcasting current_shared_config after skip: {}", e);
                            }
                            continue;
                        }
                        println!("[MAIN] Proposed configuration IS DIFFERENT. Proceeding with EegSystem restart. Proposed channels: {:?}, Current channels: {:?}", new_config_from_channel.channels, current_shared_config.channels);
                        
                        println!("[MAIN] Stopping current EEG system...");
                        // Stop current EEG system
                        if let Err(e) = current_eeg_system.stop().await {
                            println!("Error stopping EEG system: {}", e);
                        }
                        
                        // Signal cancellation to the processing task
                        current_token.cancel();
                        
                        // Wait for the task to complete gracefully with a longer timeout
                        // This allows time for CSV flushing and other cleanup operations
                        if let Err(e) = tokio::time::timeout(
                            tokio::time::Duration::from_secs(10), // Increased from 2s to 10s
                            &mut current_processing_handle
                        ).await {
                            println!("Warning: Processing task did not complete in time, forcing abort: {}", e);
                            current_processing_handle.abort();
                        }
                        
                        println!("[MAIN] Starting new EEG system with proposed configuration: Channels: {:?}, Sample rate: {}", new_config_from_channel.channels, new_config_from_channel.sample_rate);
                        // Create and start new EEG system with updated configuration
                        let (mut new_eeg_system, new_data_rx) = match EegSystem::new(new_config_from_channel.clone()).await {
                            Ok(result) => result,
                            Err(e) => {
                                println!("[MAIN] Error creating new EEG system: {}", e);
                                
                                // Check if this is a configuration error from the driver
                                let error_msg = e.to_string();
                                if error_msg.contains("Configuration error") || error_msg.contains("ConfigurationError") {
                                    println!("[MAIN] Configuration validation failed in driver: {}", error_msg);
                                    // Send error response back to the client via broadcast
                                    // Note: We can't directly send to the WebSocket client from here,
                                    // but we could implement a separate error broadcast channel if needed
                                    // For now, we'll continue with the current approach and let the client timeout
                                }
                                
                                return Err(to_daemon_error(e));
                            }
                        };
                        
                        if let Err(e) = new_eeg_system.start(new_config_from_channel.clone()).await {
                            println!("[MAIN] Error starting new EEG system: {}", e);
                            
                            // Check if this is a configuration error from the driver
                            let error_msg = e.to_string();
                            if error_msg.contains("Configuration error") || error_msg.contains("ConfigurationError") {
                                println!("[MAIN] Configuration validation failed during start: {}", error_msg);
                            }
                            
                            return Err(to_daemon_error(e));
                        }
                        
                        // Update the shared config with the new configuration from the channel
                        let applied_config_for_broadcast = new_config_from_channel.clone();
                        {
                            let mut config_guard = config.lock().await;
                            *config_guard = applied_config_for_broadcast.clone();
                        }
                        println!("[MAIN] Shared configuration updated. Channels: {:?}, Sample rate: {}", applied_config_for_broadcast.channels, applied_config_for_broadcast.sample_rate);

                        // Broadcast the newly applied configuration
                        println!("[MAIN] Broadcasting applied config. Channels: {:?}, Sample rate: {}", applied_config_for_broadcast.channels, applied_config_for_broadcast.sample_rate);
                        if let Err(e) = config_applied_tx.send(applied_config_for_broadcast.clone()) {
                            println!("[MAIN] Error broadcasting updated config: {}", e);
                        }
                        
                        // Update CSV recorder with new config
                        {
                            let mut recorder_guard = csv_recorder.lock().await;
                            recorder_guard.update_config(new_config_from_channel.clone());
                        }
                        
                        // Create a new cancellation token
                        let new_token = CancellationToken::new();
                        
                        // Start new processing task
                        let new_processing_handle = tokio::spawn(driver_handler::process_eeg_data(
                            new_data_rx,
                            tx_eeg_batch_data.clone(), // For existing /eeg endpoint
                            tx_filtered_eeg_data.clone(), // For new filtered data endpoint
                            csv_recorder.clone(),
                            is_recording.clone(),
                            connection_manager.clone(), // For demand-based processing
                            new_token.clone()
                        ));
                        
                        // Update variables for next iteration
                        current_eeg_system = new_eeg_system;
                        current_processing_handle = new_processing_handle;
                        current_token = new_token;
                        
                        println!("EEG system restarted with new configuration");
                    }
                }
            }
        }
    }

    // Cleanup
    if let Err(e) = current_eeg_system.stop().await {
        println!("Error stopping EEG system during cleanup: {}", e);
    }
    
    // Release PID lock
    if let Err(e) = pid_manager.release_lock() {
        eprintln!("Warning: Failed to release PID lock during cleanup: {}", e);
    }
    
    println!("EEG Daemon shutdown complete");
    Ok(())
}
