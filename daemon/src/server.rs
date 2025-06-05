use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, Mutex, mpsc};
use eeg_driver::AdcConfig;

use crate::driver_handler::{EegBatchData, CsvRecorder, FilteredEegData}; // Added FilteredEegData


/// Command message for WebSocket control
#[derive(Deserialize, Debug)]
#[serde(tag = "command")] // Use the "command" field as the tag
enum DaemonCommand {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "set_powerline_filter")]
    SetPowerlineFilter {
        // If 'value' is missing or null in JSON, this will be None.
        // If 'value' is a number, it will be Some(number).
        value: Option<u32>,
    },
}

/// Configuration message for WebSocket control

// Helper functions for warp filters
fn with_broadcast_rx<T: Clone + Send + 'static>(
    rx: broadcast::Sender<T>,
) -> impl Filter<Extract = (broadcast::Receiver<T>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rx.subscribe())
}

fn with_shared_state<T: Clone + Send + Sync + 'static>(
    state: Arc<Mutex<T>>,
) -> impl Filter<Extract = (Arc<Mutex<T>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

fn with_mpsc_tx<T: Send + 'static>(
    tx: mpsc::Sender<T>,
) -> impl Filter<Extract = (mpsc::Sender<T>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || tx.clone())
}

fn with_connection_manager(
    connection_manager: Arc<crate::connection_manager::ConnectionManager>,
) -> impl Filter<Extract = (Arc<crate::connection_manager::ConnectionManager>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || connection_manager.clone())
}

fn with_atomic_bool(
    atomic: Arc<AtomicBool>,
) -> impl Filter<Extract = (Arc<AtomicBool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || atomic.clone())
}

fn with_shared_recorder(
    recorder: Arc<Mutex<CsvRecorder>>,
) -> impl Filter<Extract = (Arc<Mutex<CsvRecorder>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || recorder.clone())
}

#[derive(Deserialize, Debug)]
pub struct ConfigMessage {
    pub channels: Option<Vec<u32>>,
    pub sample_rate: Option<u32>,
    // powerline_filter_hz field removed as part of DSP refactor
}

impl ConfigMessage {
    // Helper method for debugging (powerline filter functionality removed)
    pub fn debug_config(&self) {
        println!("[CONFIG_DEBUG] ConfigMessage channels: {:?}, sample_rate: {:?}", self.channels, self.sample_rate);
    }
}

/// Response message for WebSocket commands
#[derive(Serialize)]
pub struct CommandResponse {
    pub status: String,
    pub message: String,
}

// FFT data structures moved to elata_dsp_brain_waves_fft crate

/// Creates a binary EEG packet.
/// Format: [timestamp_u64_le] [error_flag_u8] [payload]
/// error_flag_u8: 0 = no error, 1 = error
/// Payload:
///   If error_flag = 1: UTF-8 error message
///   If error_flag = 0: f32_le raw samples for each channel
pub fn create_eeg_binary_packet(eeg_batch_data: &EegBatchData) -> Vec<u8> {
    let mut buffer = Vec::new();

    // Write timestamp (8 bytes)
    buffer.extend_from_slice(&eeg_batch_data.timestamp.to_le_bytes());

    // Handle error packet
    if let Some(error_msg) = &eeg_batch_data.error {
        buffer.push(1); // error_flag = 1
        // No fft_flag needed here as error packets don't contain FFT data.
        buffer.extend_from_slice(error_msg.as_bytes());
        return buffer;
    }

    // No error, proceed with data
    buffer.push(0); // error_flag = 0

    // FFT data is no longer part of this binary packet.
    // Applets will receive FFT data via their dedicated JSON WebSocket.

    // Append raw channel data
    let num_raw_channels = eeg_batch_data.channels.len();
    if num_raw_channels > 0 {
        // It's implied that if channels is not empty, channels[0] exists.
        // If channels can be empty but power_spectrums is not, this needs adjustment.
        // For now, assuming if there's data, there are raw channels.
        for channel_data in &eeg_batch_data.channels {
            for &sample in channel_data {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
        }
    }
    // If num_raw_channels is 0 and no FFT data and no error, the packet will be:
    // timestamp (8) + error_flag (1, 0) + fft_flag (1, 0) = 10 bytes.
    // This case should be handled by the client if it's possible.

    buffer
}

/// Handle WebSocket connection for EEG data streaming
pub async fn handle_websocket(
    ws: WebSocket,
    mut rx: broadcast::Receiver<EegBatchData>,
    connection_manager: Arc<crate::connection_manager::ConnectionManager>
) {
    use crate::connection_manager::ClientType;
    
    let (mut tx, _) = ws.split();
    
    // Generate unique client ID
    let client_id = format!("eeg_raw_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos());
    
    println!("WebSocket client connected (ID: {}) - sending binary EEG data", client_id);
    println!("Binary format: [timestamp (8 bytes)] [channel_samples...] for each channel");
    
    // Register client with connection manager using new pipeline-aware method
    if let Err(e) = connection_manager.register_client_pipeline(client_id.clone(), ClientType::RawRecording).await {
        eprintln!("Failed to register client {}: {}", client_id, e);
    }
    
    let mut packet_count = 0;
    let start_time = std::time::Instant::now();
    
    while let Ok(eeg_batch_data) = rx.recv().await {
        // Create binary packet
        if eeg_batch_data.channels.is_empty() {
            println!("Warning: Received EegBatchData with no channels, skipping packet.");
            continue; // Skip to the next message in the loop
        }
        // It's now safe to assume channels[0] exists
        let binary_data = create_eeg_binary_packet(&eeg_batch_data);
        let packet_size = binary_data.len();
        let samples_count = eeg_batch_data.channels[0].len();
        
        // Send binary message
        if let Err(_) = tx.send(Message::binary(binary_data)).await {
            println!("WebSocket client disconnected");
            break;
        }
        
        packet_count += 1;
        
        // Log stats every 100 packets
        if packet_count % 100 == 0 {
            let elapsed = start_time.elapsed().as_secs_f32();
            let rate = packet_count as f32 / elapsed;
            println!("Sent {} binary packets at {:.2} Hz", packet_count, rate);
            println!("Last packet size: {} bytes", packet_size);
            println!("Samples per channel: {}", samples_count);
        }
    }
    
    // Unregister client when connection closes using new pipeline-aware method
    if let Err(e) = connection_manager.unregister_client_pipeline(&client_id).await {
        eprintln!("Failed to unregister client {}: {}", client_id, e);
    }
    println!("WebSocket client disconnected (ID: {})", client_id);
}

/// Handle WebSocket connection for FILTERED EEG data streaming
pub async fn handle_filtered_eeg_data_websocket(
    ws: WebSocket,
    mut rx: broadcast::Receiver<FilteredEegData>,
    connection_manager: Arc<crate::connection_manager::ConnectionManager>
) {
    use crate::connection_manager::ClientType;
    
    let (mut tx, _) = ws.split();
    
    // Generate unique client ID
    let client_id = format!("eeg_filtered_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos());
    
    println!("Filtered EEG Data WebSocket client connected (ID: {}) - sending JSON data", client_id);
    
    // Register client with connection manager using new pipeline-aware method
    if let Err(e) = connection_manager.register_client_pipeline(client_id.clone(), ClientType::EegMonitor).await {
        eprintln!("Failed to register client {}: {}", client_id, e);
    }
    
    let mut packet_count = 0;
    let start_time = std::time::Instant::now();
    
    while let Ok(filtered_data) = rx.recv().await {
        match serde_json::to_string(&filtered_data) {
            Ok(json_data) => {
                if let Err(_) = tx.send(Message::text(json_data)).await {
                    println!("Filtered EEG Data WebSocket client disconnected (send error)");
                    break;
                }
            }
            Err(e) => {
                println!("Error serializing FilteredEegData to JSON: {}", e);
                // Optionally send an error message to the client or skip
                let error_response = CommandResponse {
                    status: "error".to_string(),
                    message: format!("Error serializing data: {}", e),
                };
                if let Ok(json_error) = serde_json::to_string(&error_response) {
                    if tx.send(Message::text(json_error)).await.is_err() {
                        println!("Filtered EEG Data WebSocket client disconnected (send error on error serialization)");
                        break;
                    }
                }
                continue;
            }
        }
        
        packet_count += 1;
        
        // Log stats every 100 packets
        if packet_count % 100 == 0 {
            let elapsed = start_time.elapsed().as_secs_f32();
            if elapsed > 0.0 {
                let rate = packet_count as f32 / elapsed;
                println!("Sent {} filtered JSON packets at {:.2} Hz", packet_count, rate);
            }
        }
    }
    
    // Unregister client when connection closes using new pipeline-aware method
    if let Err(e) = connection_manager.unregister_client_pipeline(&client_id).await {
        eprintln!("Failed to unregister client {}: {}", client_id, e);
    }
    println!("Filtered EEG Data WebSocket connection handler finished (ID: {})", client_id);
}


/// Handle WebSocket connection for configuration data
pub async fn handle_config_websocket(
    ws: WebSocket,
    config: Arc<Mutex<AdcConfig>>,
    config_update_tx: mpsc::Sender<AdcConfig>, // For sending proposed updates to main
    mut config_applied_rx: broadcast::Receiver<AdcConfig>, // For receiving applied updates from main
    is_recording: Arc<AtomicBool>
) {
    let (mut ws_tx, mut ws_rx) = ws.split(); // WebSocket sender and receiver
    
    println!("Configuration WebSocket client connected");
    
    // Send initial configuration to the client
    let initial_config = {
        let config_guard = config.lock().await;
        config_guard.clone()
    };
    
    // Convert the configuration to JSON and send it
    if let Ok(config_json) = serde_json::to_string(&initial_config) {
        if let Err(e) = ws_tx.send(Message::text(config_json)).await {
            println!("Error sending initial configuration: {}", e);
        } else {
            println!("Initial configuration sent successfully");
            println!("Sample rate: {}", initial_config.sample_rate);
            println!("Channels: {:?}", initial_config.channels);
            println!("Gain: {}", initial_config.gain);
            println!("Board driver: {:?}", initial_config.board_driver);
            println!("Batch size: {}", initial_config.batch_size);
            println!("Vref: {}", initial_config.Vref);
        }
    } else {
        println!("Error serializing configuration");
    }

    // Create an MPSC channel to forward messages from the spawned task to ws_tx
    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Message>(32); // Channel for warp::ws::Message

    // **NEW: Send current config immediately on connection**
    let initial_config_for_client = {
        let config_guard = config.lock().await; // 'config' is Arc<Mutex<AdcConfig>> passed to handle_config_websocket
        config_guard.clone()
    };
    if let Ok(config_json) = serde_json::to_string(&initial_config_for_client) {
        println!("Config WebSocket: Queuing initial config for client: {}", config_json);
        if let Err(e) = mpsc_tx.send(Message::text(config_json)).await {
             println!("Config WebSocket: Error queueing initial config for client: {}", e);
             // Optionally, could send an error back to client or close if this fails.
        }
    }
    // **END NEW**

    // Task to listen for applied config updates from main.rs and send them to the client via the MPSC channel
    let ws_tx_forwarder = mpsc_tx.clone();
    tokio::spawn(async move {
        loop {
            match config_applied_rx.recv().await {
                Ok(applied_config) => {
                    println!("Config WebSocket: Received applied config from main: {:?}", applied_config.channels);
                    // powerline_filter_hz field removed as part of DSP refactor
                    if let Ok(config_json) = serde_json::to_string(&applied_config) {
                        println!("Config WebSocket: Sending JSON to client: {}", config_json);
                        if let Err(e) = ws_tx_forwarder.send(Message::text(config_json)).await {
                            println!("Config WebSocket: Error queueing applied config for client: {}", e);
                            break; // Stop if queueing fails (channel might be closed)
                        }
                    } else {
                        println!("Config WebSocket: Error serializing applied config");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    println!("Config WebSocket: Lagged behind applied config broadcast by {} messages. Consider increasing channel capacity.", n);
                    // Potentially resend current config or signal client to refresh
                }
                Err(broadcast::error::RecvError::Closed) => {
                    println!("Config WebSocket: Applied config broadcast channel closed.");
                    break;
                }
            }
        }
        println!("Config WebSocket: Applied config listener task finished.");
    });
    
    // Process incoming configuration messages from this specific client
    loop {
        tokio::select! {
            // Arm to receive messages from the MPSC channel (either from spawned task or client message handler)
            // and forward them to the WebSocket client.
            Some(message_to_send) = mpsc_rx.recv() => {
                if let Err(e) = ws_tx.send(message_to_send).await {
                    println!("Config WebSocket: Error sending message from mpsc_rx to client: {}", e);
                    break; // Error sending to client, close connection
                }
            }

            // Arm to process incoming messages from the WebSocket client
            result = ws_rx.next() => {
                match result {
                    Some(Ok(msg)) => {
                        if msg.is_close() {
                            println!("Config WebSocket: Received close frame from client.");
                            break; // Exit loop on close frame
                        }
                        
                        if msg.is_text() {
                            let text_from_client = msg.to_str().unwrap_or_default();
                            println!("Config WebSocket: Received text message: {}", text_from_client);
                            
                            // ADD THIS LOG
                            println!("[SERVER_DEBUG] Attempting to parse JSON: >>>{}<<<", text_from_client);
                            
                            // Try to parse as ConfigMessage
                            match serde_json::from_str::<ConfigMessage>(text_from_client) {
                                Ok(mut config_msg) => {
                                    // Use our new debug method
                                    config_msg.debug_config();
                                    
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Parsed config_msg - channels: {:?}, sample_rate: {:?}", config_msg.channels, config_msg.sample_rate);

                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Checking is_recording status: {}", is_recording.load(Ordering::Relaxed));

                                    if is_recording.load(Ordering::Relaxed) {
                                        let response = CommandResponse {
                                            status: "error".to_string(),
                                            message: "Cannot change configuration during recording".to_string(),
                                        };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            // Send response via mpsc_tx
                                            if let Err(e) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'recording active' response: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                    
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] About to acquire config lock.");
                                    let config_guard = config.lock().await;
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Config lock acquired. Current shared config: channels={:?}, sample_rate={}", config_guard.channels, config_guard.sample_rate);
                                    let mut updated_config = config_guard.clone();
                                    let mut config_changed = false;
                                    let mut update_message = String::new();
                                    let no_params_provided = config_msg.channels.is_none() &&
                                                            config_msg.sample_rate.is_none() &&
                                                            false; // powerline filter removed
 
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] About to process channels. config_msg.channels.is_some(): {}", config_msg.channels.is_some());
                                    if let Some(new_channels) = config_msg.channels.take() {
                                        if new_channels.is_empty() {
                                            let response = CommandResponse { status: "error".to_string(), message: "Channel list cannot be empty".to_string() };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                if let Err(e) = mpsc_tx.send(Message::text(json)).await {
                                                     println!("Config WebSocket: Error queueing 'channel empty' response: {}", e);
                                                }
                                            }
                                            continue;
                                        }
                                        // Channel validation is now handled by the driver
                                        let new_channels_usize: Vec<usize> = new_channels.iter().map(|&x| x as usize).collect();
                                        if updated_config.channels != new_channels_usize {
                                            updated_config.channels = new_channels_usize;
                                            config_changed = true;
                                            update_message = format!("channels: {:?}", updated_config.channels);
                                        }
                                    }

                                    if let Some(new_sample_rate) = config_msg.sample_rate {
                                        // Sample rate validation is now handled by the driver
                                        if updated_config.sample_rate != new_sample_rate {
                                            updated_config.sample_rate = new_sample_rate;
                                            config_changed = true;
                                            if !update_message.is_empty() { update_message.push_str(", "); }
                                            update_message.push_str(&format!("sample rate: {}", new_sample_rate));
                                        }
                                    }

                                    // Powerline filter handling removed as part of DSP refactor
                                    
                                    if !config_changed {
                                        let msg_text_response = if no_params_provided { "No channels or sample rate provided" } else { "Configuration unchanged" };
                                        let status_str = if no_params_provided { "error" } else { "ok" };
                                        let response = CommandResponse { status: status_str.to_string(), message: msg_text_response.to_string() };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            if let Err(e) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'config unchanged' response: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                    
                                    drop(config_guard);
                                    
                                    if let Err(e) = config_update_tx.send(updated_config.clone()).await {
                                        println!("Error sending config update to main: {}", e);
                                        let response = CommandResponse { status: "error".to_string(), message: format!("Failed to submit update: {}", e) };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            if let Err(e_send) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'update submission failed' response: {}", e_send);
                                            }
                                        }
                                    } else {
                                        let response = CommandResponse {
                                            status: "ok".to_string(),
                                            message: format!("Config update for {} submitted for processing.", update_message),
                                        };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            if let Err(e) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'update submitted' response: {}", e);
                                            }
                                        }
                                        // DO NOT send updated_config here. Client will get it via broadcast from the other task.
                                    }
                                },
                                Err(e) => {
                                    println!("Error parsing config message: {}", e);
                                    let response = CommandResponse { status: "error".to_string(), message: format!("Invalid config format: {}", e) };
                                    if let Ok(response_json) = serde_json::to_string(&response) {
                                        if let Err(e_send) = mpsc_tx.send(Message::text(response_json)).await {
                                            println!("Config WebSocket: Error queueing 'invalid format' response: {}", e_send);
                                        }
                                    }
                                }
                            }
                        } else {
                            println!("Config WebSocket: Received non-text message: {:?}", msg);
                        }
                    },
                    Some(Err(e)) => {
                        println!("Config WebSocket: Error receiving message from client: {}", e);
                        break;
                    }
                    None => {
                        println!("Config WebSocket: Client disconnected (stream ended).");
                        break;
                    }
                }
            },
        }
    }
  
    println!("Config WebSocket: Connection handler finished for a client.");
}

/// Handle WebSocket connection for recording control commands
pub async fn handle_command_websocket(
    ws: WebSocket,
    recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<AtomicBool>,
    config: Arc<Mutex<AdcConfig>>,         // Added
    config_update_tx: mpsc::Sender<AdcConfig> // Added
) {
    let (mut tx, mut rx) = ws.split();
    
    println!("Command WebSocket client connected");
    
    // Send initial status
    let initial_status = {
        let recorder_guard = recorder.lock().await;
        CommandResponse {
            status: "ok".to_string(),
            message: if is_recording.load(Ordering::Relaxed) {
                format!("Currently recording to {}", recorder_guard.file_path.clone().unwrap_or_default())
            } else {
                "Not recording".to_string()
            },
        }
    };
    
    if let Ok(status_json) = serde_json::to_string(&initial_status) {
        if let Err(e) = tx.send(Message::text(status_json)).await {
            println!("Error sending initial status: {}", e);
            return;
        }
    }
    
    // Set up periodic status updates (every 5 seconds)
    let recorder_clone = recorder.clone();
    
    // Use a channel to send messages to the status update task
    let (status_tx, mut status_rx) = tokio::sync::mpsc::channel::<String>(32);
    let status_tx_clone = status_tx.clone();
    
    // Clone is_recording before moving it into the task
    let is_recording_clone = is_recording.clone();
    
    // Spawn a task to handle periodic status updates
    let status_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            
            let status_update = {
                let recorder_guard = recorder_clone.lock().await;
                CommandResponse {
                    status: "ok".to_string(),
                    message: if is_recording_clone.load(Ordering::Relaxed) {
                        format!("Currently recording to {}", recorder_guard.file_path.clone().unwrap_or_default())
                    } else {
                        "Not recording".to_string()
                    },
                }
            };
            
            if let Ok(status_json) = serde_json::to_string(&status_update) {
                if let Err(_) = status_tx.send(status_json).await {
                    println!("Error sending status update to channel");
                    break;
                }
            }
        }
    });
    
    // Spawn a task to forward messages from the channel to the WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = status_rx.recv().await {
            if let Err(e) = tx.send(Message::text(msg)).await {
                println!("Error sending status update: {}", e);
                break;
            }
        }
    });
    
    // Process incoming commands
    while let Some(result) = rx.next().await {
        match result {
            Ok(msg) => {
                if !msg.is_text() {
                    continue;
                }
                
                let text = msg.to_str().unwrap_or_default();
                // Clone is_recording for use in the match block
                let is_recording_local = is_recording.clone();
                
                let response = match serde_json::from_str::<DaemonCommand>(text) {
                    Ok(daemon_cmd) => {
                        match daemon_cmd {
                            DaemonCommand::Start => {
                                if is_recording_local.load(Ordering::Relaxed) {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Already recording".to_string(),
                                    }
                                } else {
                                    let mut recorder_guard = recorder.lock().await;
                                    match recorder_guard.start_recording().await {
                                        Ok(msg) => {
                                            CommandResponse {
                                                status: "ok".to_string(),
                                                message: msg,
                                            }
                                        },
                                        Err(e) => CommandResponse {
                                            status: "error".to_string(),
                                            message: format!("Failed to start recording: {}", e),
                                        },
                                    }
                                }
                            },
                            DaemonCommand::Stop => {
                                let mut recorder_guard = recorder.lock().await;
                                match recorder_guard.stop_recording().await {
                                    Ok(msg) => {
                                        CommandResponse {
                                            status: "ok".to_string(),
                                            message: msg,
                                        }
                                    },
                                    Err(e) => CommandResponse {
                                        status: "error".to_string(),
                                        message: format!("Failed to stop recording: {}", e),
                                    },
                                }
                            },
                            DaemonCommand::Status => {
                                let recorder_guard = recorder.lock().await;
                                CommandResponse {
                                    status: "ok".to_string(),
                                    message: if is_recording.load(Ordering::SeqCst) {
                                        format!("Currently recording to {}", recorder_guard.file_path.clone().unwrap_or_default())
                                    } else {
                                        "Not recording".to_string()
                                    },
                                }
                            },
                            DaemonCommand::SetPowerlineFilter { value: new_powerline_filter_opt } => {
                                if is_recording_local.load(Ordering::Relaxed) {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Cannot change configuration during recording".to_string(),
                                    }
                                } else {
                                    // Validate the powerline filter value
                                    // new_powerline_filter_opt is Option<u32>
                                    // We need to check if it's Some(value) where value is not 50 or 60
                                    let is_valid_filter_value = match new_powerline_filter_opt {
                                        Some(val) => val == 50 || val == 60,
                                        None => true, // None (off) is valid
                                    };

                                    if !is_valid_filter_value {
                                        CommandResponse {
                                            status: "error".to_string(),
                                            message: format!("Invalid powerline filter value: {:?}. Valid: 50, 60, or null (off)", new_powerline_filter_opt)
                                        }
                                    } else {
                                        let config_guard = config.lock().await;
                                        let updated_config = config_guard.clone();
                                        let config_changed = false;

                                        // Powerline filter handling removed as part of DSP refactor
                                        // config_changed remains false since no powerline filter to update

                                        if config_changed {
                                            drop(config_guard); // Release lock before sending to channel
                                            match config_update_tx.send(updated_config.clone()).await {
                                                Ok(_) => CommandResponse {
                                                    status: "ok".to_string(),
                                                    message: format!("Powerline filter update ({:?}) submitted.",
                                                        new_powerline_filter_opt.map_or("Off".to_string(), |f| f.to_string() + "Hz"))
                                                },
                                                Err(e) => CommandResponse {
                                                    status: "error".to_string(),
                                                    message: format!("Failed to submit powerline filter update: {}", e),
                                                },
                                            }
                                        } else {
                                            CommandResponse {
                                                status: "ok".to_string(),
                                                message: "Powerline filter configuration unchanged.".to_string(),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => CommandResponse {
                        status: "error".to_string(),
                        message: format!("Invalid command format: {}", e),
                    },
                };
                
                if let Ok(response_json) = serde_json::to_string(&response) {
                    if let Err(_) = status_tx_clone.send(response_json).await {
                        println!("Error sending command response to channel");
                        break;
                    }
                }
            },
            Err(e) => {
                println!("Error receiving command: {}", e);
                break;
            }
        }
    }
    
    // Cancel both tasks when the WebSocket connection is closed
    status_task.abort();
    forward_task.abort();
    
    println!("Command WebSocket client disconnected");
}

// Brain waves FFT WebSocket handler moved to elata_dsp_brain_waves_fft crate

// Set up WebSocket routes and server
pub fn setup_websocket_routes(
    tx_eeg_batch_data: broadcast::Sender<EegBatchData>, // Renamed from tx, for existing /eeg endpoint
    tx_filtered_eeg_data: broadcast::Sender<FilteredEegData>, // New sender for filtered data
    config: Arc<Mutex<AdcConfig>>, // Shared current config
    csv_recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<AtomicBool>,
    config_applied_tx: broadcast::Sender<AdcConfig>, // Sender for applied configs (from main.rs)
    connection_manager: Arc<crate::connection_manager::ConnectionManager>, // Connection manager for client tracking
) -> (warp::filters::BoxedFilter<(impl warp::Reply,)>, mpsc::Receiver<AdcConfig>) {
    // Channel for clients to send proposed config updates TO main.rs
    let (config_update_to_main_tx, config_update_to_main_rx) = mpsc::channel::<AdcConfig>(32);
    
    // Existing /eeg endpoint for EegBatchData (typically unfiltered or pre-basic_voltage_filter)
    let eeg_ws_route = warp::path("eeg")
        .and(warp::ws())
        .and(with_broadcast_rx(tx_eeg_batch_data.clone())) // Use the renamed sender
        .and(with_connection_manager(connection_manager.clone()))
        .map(|ws: warp::ws::Ws, rx: broadcast::Receiver<EegBatchData>, conn_mgr: Arc<crate::connection_manager::ConnectionManager>| {
            ws.on_upgrade(move |socket| handle_websocket(socket, rx, conn_mgr))
        });

    // New /ws/eeg/data__basic_voltage_filter endpoint for FilteredEegData
    let filtered_eeg_data_route = warp::path("ws")
        .and(warp::path("eeg"))
        .and(warp::path("data__basic_voltage_filter"))
        .and(warp::ws())
        .and(with_broadcast_rx(tx_filtered_eeg_data.clone())) // Use the new sender
        .and(with_connection_manager(connection_manager.clone()))
        .map(|ws: warp::ws::Ws, rx_data: broadcast::Receiver<FilteredEegData>, conn_mgr: Arc<crate::connection_manager::ConnectionManager>| {
            ws.on_upgrade(move |socket| handle_filtered_eeg_data_websocket(socket, rx_data, conn_mgr))
        });
        
    let config_clone = config.clone();
    let config_update_to_main_tx_clone = config_update_to_main_tx.clone();
    let is_recording_clone_config = is_recording.clone();
    let config_applied_tx_clone = config_applied_tx.clone();

    let config_ws_route = warp::path("config")
        .and(warp::ws())
        .and(with_shared_state(config_clone)) // Pass current config Arc
        .and(with_mpsc_tx(config_update_to_main_tx_clone)) // Pass sender for proposed updates
        .and(with_broadcast_rx(config_applied_tx_clone)) // Pass receiver for applied updates
        .and(with_atomic_bool(is_recording_clone_config)) // Pass recording status
        .map(|ws: warp::ws::Ws, cfg_arc: Arc<Mutex<AdcConfig>>, cfg_upd_tx: mpsc::Sender<AdcConfig>, cfg_app_rx: broadcast::Receiver<AdcConfig>, rec_status: Arc<AtomicBool>| {
            println!("[DEBUG] /config route matched, attempting WebSocket upgrade...");
            ws.on_upgrade(move |socket| handle_config_websocket(socket, cfg_arc, cfg_upd_tx, cfg_app_rx, rec_status))
        });
    
    let recorder_clone_cmd = csv_recorder.clone();
    let is_recording_clone_cmd = is_recording.clone();
    let config_clone_cmd = config.clone();
    let config_update_tx_clone_cmd = config_update_to_main_tx.clone();

    let command_ws_route = warp::path("command")
        .and(warp::ws())
        .and(with_shared_recorder(recorder_clone_cmd))
        .and(with_atomic_bool(is_recording_clone_cmd))
        .and(with_shared_state(config_clone_cmd)) // Pass cloned config
        .and(with_mpsc_tx(config_update_tx_clone_cmd)) // Pass cloned sender
        .map(|ws: warp::ws::Ws, recorder: Arc<Mutex<CsvRecorder>>, is_recording: Arc<AtomicBool>, cfg: Arc<Mutex<AdcConfig>>, cfg_upd_tx: mpsc::Sender<AdcConfig>| {
            ws.on_upgrade(move |socket| handle_command_websocket(socket, recorder, is_recording, cfg, cfg_upd_tx))
        });
    
    // Combine base routes including the new filtered data route
    // Return base routes - DSP routes will be combined in main.rs
    let routes = eeg_ws_route
        .or(config_ws_route)
        .or(command_ws_route)
        .or(filtered_eeg_data_route)
        .boxed();

    (routes, config_update_to_main_rx)
}