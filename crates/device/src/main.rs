// Import plugin implementations
use adc_daemon::plugin_supervisor::PluginSupervisor;
use adc_daemon::elata_emu_v1::EegSystem;
use adc_daemon::connection_manager::ConnectionManager;
use adc_daemon::event_bus::EventBus;
use adc_daemon::pid_manager::PidManager;
use adc_daemon::{config, server};

use eeg_sensor::AdcConfig;
use tokio::sync::{broadcast, Mutex, mpsc};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fmt;
use tokio_util::sync::CancellationToken;

// Import event-driven types
use eeg_types::{EegPacket, SensorEvent, EegPlugin, EventFilter, DriverType};
use eeg_sensor::AdcData;

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

/// Data acquisition loop that converts raw ADC data to SensorEvents and broadcasts them
async fn data_acquisition_loop(
    mut adc_rx: tokio::sync::mpsc::Receiver<AdcData>,
    bus: Arc<EventBus>,
    config: Arc<Mutex<AdcConfig>>,
    shutdown_token: CancellationToken,
) -> anyhow::Result<()> {
    use std::collections::HashMap;

    let mut frame_counter = 0u64;
    let mut channel_buffers: HashMap<u8, Vec<(i32, f32, u64)>> = HashMap::new();

    tracing::info!("Starting data acquisition loop");

    loop {
        tokio::select! {
            biased;
            _ = shutdown_token.cancelled() => {
                tracing::info!("Data acquisition loop received shutdown signal");
                break;
            }
            Some(adc_data) = adc_rx.recv() => {
                let (batch_size, vref, num_channels, sample_rate) = {
                    let config_guard = config.lock().await;
                    (
                        config_guard.batch_size as usize,
                        config_guard.vref,
                        config_guard.channels.len(),
                        config_guard.sample_rate as f32,
                    )
                };

                let buffer = channel_buffers.entry(adc_data.channel).or_insert_with(Vec::new);
                buffer.push((adc_data.raw_value, adc_data.voltage, adc_data.timestamp));

                let min_buffer_size = channel_buffers.values().map(|v| v.len()).min().unwrap_or(0);
                if min_buffer_size >= batch_size {
                    let mut flattened_raw_samples = Vec::with_capacity(batch_size * num_channels);
                    let mut flattened_voltage_samples = Vec::with_capacity(batch_size * num_channels);
                    let mut sample_timestamps = Vec::with_capacity(batch_size * num_channels);

                    // Create a structure to hold all the batches
                    let mut channel_batches: Vec<Vec<(i32, f32, u64)>> = Vec::with_capacity(num_channels);
                    for i in 0..num_channels {
                        if let Some(buffer) = channel_buffers.get_mut(&(i as u8)) {
                            channel_batches.push(buffer.drain(0..batch_size).collect());
                        } else {
                            // If a channel is missing data, push an empty vec to maintain index mapping
                            channel_batches.push(Vec::new());
                        }
                    }

                    // Interleave the data
                    for sample_idx in 0..batch_size {
                        for channel_idx in 0..num_channels {
                            if let Some((raw_val, voltage, timestamp)) = channel_batches[channel_idx].get(sample_idx) {
                                flattened_raw_samples.push(*raw_val);
                                flattened_voltage_samples.push(*voltage);
                                sample_timestamps.push(*timestamp);
                            } else {
                                // Handle missing data for a channel gracefully
                                flattened_raw_samples.push(0);
                                flattened_voltage_samples.push(0.0);
                                // Attempt to use a timestamp from another channel for this sample index, or 0
                                let fallback_ts = channel_batches.iter()
                                    .find_map(|b| b.get(sample_idx).map(|(_, _, ts)| *ts))
                                    .unwrap_or(0);
                                sample_timestamps.push(fallback_ts);
                            }
                        }
                    }

                    let eeg_packet = EegPacket::new(
                        sample_timestamps,
                        frame_counter,
                        flattened_raw_samples,
                        flattened_voltage_samples,
                        num_channels,
                        sample_rate.into(),
                    );

                    let event = SensorEvent::RawEeg(Arc::new(eeg_packet));
                    bus.broadcast_event(event).await;
                    frame_counter += 1;

                    if frame_counter % 100 == 0 {
                        tracing::debug!("Processed {} frames", frame_counter);
                    }
                }
            }
            else => {
                tracing::warn!("ADC data receiver channel closed");
                break;
            }
        }
    }

    tracing::info!("Data acquisition loop ended");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();

    let pid_file_path = "/tmp/eeg_daemon.pid";
    let pid_manager = PidManager::new(pid_file_path);
    
    if let Err(e) = pid_manager.acquire_lock() {
        eprintln!("Failed to start daemon: {}", e);
        eprintln!("If you're sure no other instance is running, try removing the PID file: {}", pid_file_path);
        std::process::exit(1);
    }
    
    tracing::info!("EEG Daemon starting (PID: {})...", std::process::id());

    let daemon_config = config::load_config();
    tracing::info!("Daemon configuration loaded.");

    let initial_config = AdcConfig {
        sample_rate: 500,
        channels: vec![0, 1, 2],
        gain: 24.0,
        board_driver: daemon_config.driver_type.clone(),
        batch_size: daemon_config.batch_size,
        vref: 4.5,
    };
    
    let config = Arc::new(Mutex::new(initial_config.clone()));
    let is_recording = Arc::new(AtomicBool::new(false));

    println!("Starting EEG system...");
    
    let (mut eeg_system, adc_data_rx) = EegSystem::new(initial_config.clone()).await
        .map_err(to_daemon_error)?;
    
    eeg_system.start(initial_config.clone()).await
        .map_err(to_daemon_error)?;

    tracing::info!("EEG system started. Waiting for data...");

    // === EVENT-DRIVEN ARCHITECTURE SETUP ===
    
    let event_bus = Arc::new(EventBus::new());
    let shutdown_token = CancellationToken::new();
    
    tracing::info!("EventBus initialized");
    
    let data_acq_bus = event_bus.clone();
    let data_acq_shutdown = shutdown_token.clone();
    let data_acq_config = config.clone();
    let mut data_acquisition_handle = tokio::spawn(async move {
        if let Err(e) = data_acquisition_loop(adc_data_rx, data_acq_bus, data_acq_config, data_acq_shutdown).await {
            tracing::error!("Data acquisition loop failed: {}", e);
        }
    });
    
    // === PLUGIN SUPERVISOR INITIALIZATION ===
    
    let mut plugin_supervisor = PluginSupervisor::new(event_bus.clone());
    // Register plugins
    #[cfg(feature = "brain_waves_fft_feature")]
    plugin_supervisor.add_plugin(Box::new(brain_waves_fft_plugin::BrainWavesFftPlugin::new(
        initial_config.channels.len(),
        initial_config.sample_rate as f32,
    )));
    plugin_supervisor.add_plugin(Box::new(basic_voltage_filter_plugin::BasicVoltageFilterPlugin::new()));
    plugin_supervisor.add_plugin(Box::new(csv_recorder_plugin::CsvRecorderPlugin::new()));

    plugin_supervisor.initialize_plugins().await;
    plugin_supervisor.start_all(shutdown_token.clone());
    
    tracing::info!("Plugin supervisor initialized and all plugins started.");
    
    // Create a broadcast channel for config updates
    let (config_applied_tx, _) = broadcast::channel::<AdcConfig>(16);

    // The WebSocket server will now subscribe directly to the main event bus.
    // This removes the need for a separate forwarding task and an intermediate channel.

    // === CONNECTION MANAGER SETUP ===
    let (connection_tx, connection_rx) = mpsc::channel(32);

    let cm_event_subscriber = event_bus.subscribe();

    let mut connection_manager = ConnectionManager::new(connection_rx, cm_event_subscriber);
    let cm_shutdown = shutdown_token.clone();
    let mut connection_manager_handle = tokio::spawn(async move {
        connection_manager.run(cm_shutdown).await;
    });

    // Set up WebSocket routes
    let (ws_routes, mut config_update_rx) = server::setup_websocket_routes(
        config.clone(),
        config_applied_tx.clone(),
        connection_tx,
        is_recording.clone(),
    );
    
    println!("WebSocket server starting on ws://0.0.0.0:8080");

    let mut server_handle = tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080)));

    // === MAIN SUPERVISOR LOOP ===
    let mut current_eeg_system = eeg_system;
    
    tracing::info!("EEG Daemon fully initialized and running");
    
    loop {
        tokio::select! {
            biased;
            
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl-C received, initiating shutdown");
                shutdown_token.cancel();
                break;
            },
            
            result = &mut data_acquisition_handle => {
                tracing::warn!("Data acquisition loop completed: {:?}", result);
                break;
            },
            
            result = plugin_supervisor.join_all() => {
                tracing::warn!("Plugin supervisor completed: {:?}", result);
                break;
            },
            
            result = &mut server_handle => {
                tracing::warn!("Server task completed: {:?}", result);
                break;
            },

            result = &mut connection_manager_handle => {
                tracing::warn!("ConnectionManager task completed: {:?}", result);
                break;
            },

            config_update = config_update_rx.recv() => {
                if let Some(new_config) = config_update {
                    tracing::info!("Received config update. Channels: {:?}, Sample rate: {}",
                                 new_config.channels, new_config.sample_rate);
                    
                    if is_recording.load(Ordering::Relaxed) {
                        tracing::warn!("Cannot update configuration during recording");
                    } else {
                        {
                            let mut config_guard = config.lock().await;
                            *config_guard = new_config.clone();
                        }
                        
                        if let Err(e) = current_eeg_system.reconfigure(new_config.clone()).await {
                            tracing::error!("Error reconfiguring EEG system: {}", e);
                        } else {
                            tracing::info!("EEG system reconfigured successfully");
                            
                            if let Err(e) = config_applied_tx.send(new_config) {
                                tracing::error!("Error broadcasting applied config: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // Cleanup
    tracing::info!("Initiating graceful shutdown...");
    shutdown_token.cancel();
    
    if let Err(e) = data_acquisition_handle.await {
        tracing::error!("Data acquisition handle join error: {}", e);
    }
    
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    tracing::info!("Shutting down EEG system...");
    if let Err(e) = current_eeg_system.shutdown().await {
        tracing::error!("Error shutting down EEG system: {}", e);
    }

    if let Err(e) = pid_manager.release_lock() {
        tracing::warn!("Failed to release PID lock: {}", e);
    }

    tracing::info!("EEG Daemon stopped gracefully");
    Ok(())
}
