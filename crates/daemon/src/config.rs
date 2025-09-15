use crate::{
    api::AppState,
    protocol::{ConfigProposal, ServerMessage},
};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::{debug, error, info, warn};
use sensors::types::AdcConfig;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

// A single client connected to the config WebSocket.
struct ConfigClient {
    sender: SplitSink<WebSocket, Message>,
}

// Manages all connected config clients and broadcasts config updates.
pub struct ConfigBroker {
    clients: Mutex<Vec<SplitSink<WebSocket, Message>>>,
    tx: broadcast::Sender<Arc<AdcConfig>>,
    /// Holds the latest known config for new clients.
    latest_config: Arc<Mutex<Option<Arc<AdcConfig>>>>,
}

impl Default for ConfigBroker {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            clients: Mutex::new(Vec::new()),
            tx,
            latest_config: Arc::new(Mutex::new(None)),
        }
    }
}

impl ConfigBroker {
    pub fn new() -> (Self, broadcast::Receiver<Arc<AdcConfig>>) {
        let (tx, rx) = broadcast::channel(16);
        let broker = Self {
            clients: Mutex::new(Vec::new()),
            tx,
            latest_config: Arc::new(Mutex::new(None)),
        };
        (broker, rx)
    }

    // Add a new client to the broker.
    async fn add_client(&self, mut sender: SplitSink<WebSocket, Message>) {
        let mut clients = self.clients.lock().await;
        clients.push(sender);
    }

    // Broadcast a new config to all connected clients.
    // Broadcast a new config to all connected clients and update the latest snapshot.
    pub async fn broadcast_and_update(&self, config: Arc<AdcConfig>) {
        // Update the latest config snapshot.
        {
            let mut latest = self.latest_config.lock().await;
            *latest = Some(config.clone());
        }

        // Broadcast to existing clients.
        let msg = ServerMessage::Applied {
            config: (*config).clone(),
            revision: None,
        };
        let serialized = match serde_json::to_string(&msg) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to serialize config for broadcast: {}", e);
                return;
            }
        };

        let mut clients = self.clients.lock().await;
        let mut keep = Vec::with_capacity(clients.len());
        for mut client in clients.drain(..) {
            if client.send(Message::Text(serialized.clone())).await.is_ok() {
                keep.push(client);
            } else {
                info!("Removing disconnected config client during broadcast.");
            }
        }
        *clients = keep;
    }
}

pub async fn config_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, receiver) = socket.split();

    // Send the initial configuration state on connect, if we have it.
    let latest_config_guard = state.config_broker.latest_config.lock().await;
    if let Some(config) = &*latest_config_guard {
        let msg = ServerMessage::Applied {
            config: (**config).clone(),
            revision: None,
        };
        if let Ok(serialized) = serde_json::to_string(&msg) {
            if sender.send(Message::Text(serialized)).await.is_err() {
                error!("Failed to send initial config to new WebSocket client. It will receive the next update.");
            }
        }
    } else {
        info!("No initial config available for new client. It will receive the next update.");
    }
    drop(latest_config_guard);

    state.config_broker.add_client(sender).await;

    // This task will handle messages from the client
    let receive_handle = tokio::spawn(async move {
        process_incoming_messages(receiver, state).await;
    });

    // Wait for the client to disconnect
    receive_handle.await.unwrap();
    info!("Config WebSocket client disconnected.");
}

async fn process_incoming_messages(mut receiver: SplitStream<WebSocket>, state: AppState) {
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<ConfigProposal>(&text) {
                Ok(proposal) => {
                    info!("Received config proposal: {:?}", proposal);
                    // Here you would handle the proposal:
                    // 1. Check if recording is active (and reject if so).
                    // 2. Validate the proposed config.
                    // 3. If valid, apply it to the driver.
                    // 4. On success, broadcast the new config to all clients.
                    // For now, we just log it.
                    if let Some(driver_arc) = &state.driver {
                        let mut driver_guard = driver_arc.lock().await;
                        let new_config = proposal.config.clone();
                        if let Err(e) = driver_guard.reconfigure(&new_config) {
                            error!("Failed to apply new config: {}", e);
                            // TODO: Send a `Rejected` message back to the proposing client specifically.
                        } else {
                            info!("Successfully applied new config.");
                            state
                                .config_broker
                                .broadcast_and_update(Arc::new(new_config))
                                .await;
                        }
                    } else {
                        warn!("Config proposal received, but no driver is available. Rejecting.");
                        // TODO: Send a `Rejected` message back to the proposing client.
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to deserialize config proposal from client: {}. Raw: {}",
                        e, text
                    );
                }
            }
        }
    }
}
