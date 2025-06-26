use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, Mutex, mpsc};
use eeg_sensor::AdcConfig;
use eeg_types::{SensorEvent, EegPacket, FilteredEegPacket};

#[derive(Clone, Debug)]
pub enum ClientType {
    EegMonitor,
}

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


fn with_atomic_bool(
    atomic: Arc<AtomicBool>,
) -> impl Filter<Extract = (Arc<AtomicBool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || atomic.clone())
}


#[derive(Deserialize, Debug)]
pub struct ConfigMessage {
    pub channels: Option<Vec<u32>>,
    pub sample_rate: Option<u32>,
}

impl ConfigMessage {
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

/// Handle WebSocket connection for configuration data
pub async fn handle_config_websocket(
    ws: WebSocket,
    config: Arc<Mutex<AdcConfig>>,
    config_update_tx: mpsc::Sender<AdcConfig>, // For sending proposed updates to main
    mut config_applied_rx: broadcast::Receiver<AdcConfig>, // For receiving applied updates from main
    is_recording: Arc<AtomicBool>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    println!("Configuration WebSocket client connected");

    let (tx, mut rx) = mpsc::channel::<Message>(32);

    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if ws_tx.send(message).await.is_err() {
                println!("Config WebSocket: client disconnected.");
                break;
            }
        }
    });

    let initial_config = {
        let config_guard = config.lock().await;
        config_guard.clone()
    };
    if let Ok(config_json) = serde_json::to_string(&initial_config) {
        if tx.send(Message::text(config_json)).await.is_err() {
            return;
        }
    }

    let broadcast_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match config_applied_rx.recv().await {
                Ok(applied_config) => {
                    if let Ok(config_json) = serde_json::to_string(&applied_config) {
                        if broadcast_tx.send(Message::text(config_json)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    println!("Config WebSocket: Lagged behind applied config broadcast by {} messages.", n);
                }
            }
        }
    });

    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                println!("Config WebSocket: error receiving message: {}", e);
                break;
            }
        };

        if msg.is_close() {
            break;
        }

        if let Ok(text_from_client) = msg.to_str() {
            match serde_json::from_str::<ConfigMessage>(text_from_client) {
                Ok(config_msg) => {
                    if is_recording.load(Ordering::Relaxed) {
                        let response = CommandResponse {
                            status: "error".to_string(),
                            message: "Cannot change configuration during recording".to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if tx.send(Message::text(json)).await.is_err() { break; }
                        }
                        continue;
                    }

                    let config_guard = config.lock().await;
                    let mut updated_config = config_guard.clone();
                    let mut config_changed = false;

                    if let Some(new_channels) = config_msg.channels {
                        if new_channels.is_empty() {
                            let response = CommandResponse { status: "error".to_string(), message: "Channel list cannot be empty".to_string() };
                            if let Ok(json) = serde_json::to_string(&response) {
                                if tx.send(Message::text(json)).await.is_err() { break; }
                            }
                            continue;
                        }
                        let new_channels_u8: Vec<u8> = new_channels.iter().map(|&x| x as u8).collect();
                        if updated_config.channels != new_channels_u8 {
                            updated_config.channels = new_channels_u8;
                            config_changed = true;
                        }
                    }

                    if let Some(new_sample_rate) = config_msg.sample_rate {
                        if updated_config.sample_rate != new_sample_rate {
                            updated_config.sample_rate = new_sample_rate;
                            config_changed = true;
                        }
                    }

                    if config_changed {
                        drop(config_guard);
                        if config_update_tx.send(updated_config).await.is_err() {
                            let response = CommandResponse { status: "error".to_string(), message: "Failed to submit update".to_string() };
                            if let Ok(json) = serde_json::to_string(&response) {
                                if tx.send(Message::text(json)).await.is_err() { break; }
                            }
                        } else {
                            let response = CommandResponse { status: "ok".to_string(), message: "Config update submitted".to_string() };
                            if let Ok(json) = serde_json::to_string(&response) {
                                if tx.send(Message::text(json)).await.is_err() { break; }
                            }
                        }
                    } else {
                        let response = CommandResponse { status: "ok".to_string(), message: "Configuration unchanged".to_string() };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if tx.send(Message::text(json)).await.is_err() { break; }
                        }
                    }
                }
                Err(e) => {
                    let response = CommandResponse { status: "error".to_string(), message: format!("Invalid config format: {}", e) };
                    if let Ok(json) = serde_json::to_string(&response) {
                        if tx.send(Message::text(json)).await.is_err() { break; }
                    }
                }
            }
        }
    }
    println!("Config WebSocket: Connection handler finished for a client.");
}

/// Handle WebSocket connection for recording control commands
pub async fn handle_command_websocket(
    ws: WebSocket,
    is_recording: Arc<AtomicBool>,
    config: Arc<Mutex<AdcConfig>>,
    config_update_tx: mpsc::Sender<AdcConfig>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    println!("Command WebSocket client connected");

    let (tx, mut rx) = mpsc::channel::<Message>(32);

    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if ws_tx.send(message).await.is_err() {
                println!("Command WebSocket: client disconnected.");
                break;
            }
        }
    });

    let initial_status = CommandResponse {
        status: "ok".to_string(),
        message: if is_recording.load(Ordering::Relaxed) {
            "Currently recording".to_string()
        } else {
            "Not recording".to_string()
        },
    };
    if let Ok(status_json) = serde_json::to_string(&initial_status) {
        if tx.send(Message::text(status_json)).await.is_err() {
            return;
        }
    }

    let periodic_tx = tx.clone();
    let is_recording_clone = is_recording.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let status_update = CommandResponse {
                status: "ok".to_string(),
                message: if is_recording_clone.load(Ordering::Relaxed) {
                    "Currently recording".to_string()
                } else {
                    "Not recording".to_string()
                },
            };
            if let Ok(status_json) = serde_json::to_string(&status_update) {
                if periodic_tx.send(Message::text(status_json)).await.is_err() {
                    break;
                }
            }
        }
    });

    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                println!("Command WebSocket: error receiving message: {}", e);
                break;
            }
        };

        if msg.is_close() {
            break;
        }

        if let Ok(text) = msg.to_str() {
            let response = match serde_json::from_str::<DaemonCommand>(text) {
                Ok(daemon_cmd) => {
                    match daemon_cmd {
                        DaemonCommand::Start => {
                            is_recording.store(true, Ordering::Relaxed);
                            CommandResponse {
                                status: "ok".to_string(),
                                message: "Recording started (placeholder)".to_string(),
                            }
                        }
                        DaemonCommand::Stop => {
                            is_recording.store(false, Ordering::Relaxed);
                            CommandResponse {
                                status: "ok".to_string(),
                                message: "Recording stopped (placeholder)".to_string(),
                            }
                        }
                        DaemonCommand::Status => CommandResponse {
                            status: "ok".to_string(),
                            message: if is_recording.load(Ordering::SeqCst) {
                                "Currently recording".to_string()
                            } else {
                                "Not recording".to_string()
                            },
                        },
                        DaemonCommand::SetPowerlineFilter { value: new_powerline_filter_opt } => {
                            if is_recording.load(Ordering::Relaxed) {
                                CommandResponse {
                                    status: "error".to_string(),
                                    message: "Cannot change configuration during recording".to_string(),
                                }
                            } else {
                                let is_valid = match new_powerline_filter_opt {
                                    Some(val) => val == 50 || val == 60,
                                    None => true,
                                };
                                if !is_valid {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Invalid powerline filter value".to_string(),
                                    }
                                } else {
                                    let _config_guard = config.lock().await;
                                    CommandResponse {
                                        status: "ok".to_string(),
                                        message: "Powerline filter configuration unchanged.".to_string(),
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => CommandResponse {
                    status: "error".to_string(),
                    message: format!("Invalid command format: {}", e),
                },
            };

            if let Ok(response_json) = serde_json::to_string(&response) {
                if tx.send(Message::text(response_json)).await.is_err() {
                    break;
                }
            }
        }
    }
    println!("Command WebSocket client disconnected");
}

// Set up WebSocket routes and server
pub fn setup_websocket_routes(
    config: Arc<Mutex<AdcConfig>>,
    config_applied_tx: broadcast::Sender<AdcConfig>,
    connection_tx: mpsc::Sender<WebSocket>, // Sender to pass new connections to the ConnectionManager
    is_recording: Arc<AtomicBool>,
) -> (
    impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone,
    mpsc::Receiver<AdcConfig>,
) {
    let (config_update_tx, config_update_rx) = mpsc::channel::<AdcConfig>(32);

    let config_route = warp::path("config")
        .and(warp::ws())
        .and(with_shared_state(config.clone()))
        .and(with_mpsc_tx(config_update_tx.clone()))
        .and(with_broadcast_rx(config_applied_tx.clone()))
        .and(with_atomic_bool(is_recording.clone()))
        .map(|ws: warp::ws::Ws, conf, tx, rx, is_rec| {
            ws.on_upgrade(move |socket| {
                handle_config_websocket(socket, conf, tx, rx, is_rec)
            })
        });

    let command_route = warp::path("command")
        .and(warp::ws())
        .and(with_atomic_bool(is_recording.clone()))
        .and(with_shared_state(config.clone()))
        .and(with_mpsc_tx(config_update_tx.clone()))
        .map(|ws: warp::ws::Ws, is_rec, conf, tx| {
            ws.on_upgrade(move |socket| {
                handle_command_websocket(socket, is_rec, conf, tx)
            })
        });

    // This is the single data endpoint for all UI clients.
    // It accepts a WebSocket connection and hands it off to the ConnectionManager.
    let data_route = warp::path!("ws" / "data")
        .and(warp::ws())
        .and(with_mpsc_tx(connection_tx))
        .map(|ws: warp::ws::Ws, tx: mpsc::Sender<WebSocket>| {
            ws.on_upgrade(move |socket| async move {
                if tx.send(socket).await.is_err() {
                    eprintln!("Failed to send new WebSocket connection to ConnectionManager");
                }
            })
        });

    let routes = config_route
        .or(command_route)
        .or(data_route);

    (routes, config_update_rx)
}