//! Manages WebSocket connections for streaming data to UI clients.
use anyhow::Result;
use bytes::Bytes;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;
use warp::ws::{Message, WebSocket};

use eeg_types::event::{SensorEvent, PROTOCOL_VERSION};

type ClientId = String;
type ClientTx = SplitSink<WebSocket, Message>;

/// Manages all WebSocket client connections and broadcasts events.
pub struct ConnectionManager {
    connection_rx: mpsc::Receiver<WebSocket>,
    event_rx: broadcast::Receiver<SensorEvent>,
    clients: Arc<Mutex<HashMap<ClientId, ClientTx>>>,
}

impl ConnectionManager {
    pub fn new(
        connection_rx: mpsc::Receiver<WebSocket>,
        event_rx: broadcast::Receiver<SensorEvent>,
    ) -> Self {
        Self {
            connection_rx,
            event_rx,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(&mut self, shutdown_token: CancellationToken) {
        info!("ConnectionManager is running.");
        loop {
            tokio::select! {
                biased;
                _ = shutdown_token.cancelled() => {
                    info!("ConnectionManager received shutdown signal.");
                    break;
                }
                Some(websocket) = self.connection_rx.recv() => {
                    self.add_client(websocket);
                }
                Ok(event) = self.event_rx.recv() => {
                    if let SensorEvent::WebSocketBroadcast { topic, payload } = event {
                        self.dispatch_binary_event(topic as u8, payload).await;
                    }
                }
            }
        }
        info!("ConnectionManager has shut down.");
    }

    /// Adds a new client and spawns a task to handle its lifecycle.
    fn add_client(&self, websocket: WebSocket) {
        let client_id = Uuid::new_v4().to_string();
        let (sender, mut receiver) = websocket.split();
        let clients_arc = self.clients.clone();

        info!(client_id = %client_id, "New WebSocket client connected.");

        // Spawn a task to add the client and then monitor its connection.
        tokio::spawn(async move {
            clients_arc.lock().await.insert(client_id.clone(), sender);

            // We only care about the connection closing.
            while let Some(result) = receiver.next().await {
                if let Err(e) = result {
                    warn!(client_id = %client_id, "Error on client WebSocket: {}", e);
                    break;
                }
                if result.unwrap().is_close() {
                    debug!(client_id = %client_id, "Client sent close frame.");
                    break;
                }
            }
            info!(client_id = %client_id, "Client disconnected. Removing.");
            clients_arc.lock().await.remove(&client_id);
        });
    }

    /// Dispatches a binary event to all connected clients.
    async fn dispatch_binary_event(&self, topic: u8, payload: Bytes) {
        let mut clients_guard = self.clients.lock().await;
        if clients_guard.is_empty() {
            return;
        }

        // Prepare the message once
        let header = [PROTOCOL_VERSION, topic];
        let mut combined = header.to_vec();
        combined.extend_from_slice(&payload);
        let message = Message::binary(Bytes::from(combined));

        let mut disconnected_clients = Vec::new();

        for (id, client_tx) in clients_guard.iter_mut() {
            if let Err(e) = client_tx.send(message.clone()).await {
                warn!(client_id = %id, "Failed to send message, client disconnected: {}", e);
                disconnected_clients.push(id.clone());
            }
        }

        // Clean up disconnected clients
        for id in disconnected_clients {
            clients_guard.remove(&id);
            info!(client_id = %id, "Removed disconnected client.");
        }
    }
}

