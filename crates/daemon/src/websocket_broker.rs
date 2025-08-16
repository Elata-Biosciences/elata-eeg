use axum::extract::ws::{Message, WebSocket};
use dashmap::DashMap;
use eeg_types::comms::{
	client::{ClientMessage, ServerMessage, SubscribedAck},
	pipeline::{BrokerMessage, BrokerPayload},
};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
	select,
	sync::{broadcast, Mutex},
	time::interval,
};
use tracing::{debug, info, warn};

const PING_INTERVAL_S: u64 = 20;

/// Manages WebSocket connections and routes data from the pipeline to subscribed clients.
type TopicRx = broadcast::Receiver<Arc<BrokerMessage>>;

struct TopicState {
	sender: broadcast::Sender<Arc<BrokerMessage>>,
	last_meta: Option<Arc<BrokerMessage>>,
	meta_rev: u32,
}

pub struct WebSocketBroker {
    /// Receives messages from the pipeline.
    pipeline_rx: Mutex<broadcast::Receiver<Arc<BrokerMessage>>>,
    /// Manages client subscriptions to different topics.
    topics: DashMap<String, Arc<Mutex<TopicState>>>,
}

impl WebSocketBroker {
    pub fn new(pipeline_rx: broadcast::Receiver<Arc<BrokerMessage>>) -> Self {
        Self {
            pipeline_rx: Mutex::new(pipeline_rx),
            topics: DashMap::new(),
        }
    }

    /// A long-running task that listens for incoming messages and broadcasts them to clients.
    pub fn start(self: Arc<Self>, mut shutdown_rx: tokio::sync::oneshot::Receiver<()>) {
        tokio::spawn(async move {
            loop {
                let msg = tokio::select! {
                    biased;
                    _ = &mut shutdown_rx => {
                        info!("[Broker] Shutdown signal received. Terminating broker loop.");
                        break;
                    },
                    res = async { self.pipeline_rx.lock().await.recv().await } => {
                        match res {
                            Ok(msg) => msg,
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("[Broker] Main pipeline channel closed. Shutting down message broadcast task.");
                                break;
                            },
                            Err(e) => {
                                warn!("[Broker] Error receiving message from pipeline: {}. Ignoring.", e);
                                continue;
                            }
                        }
                    }
                };

                match &*msg {
                    BrokerMessage::Data { topic, payload } => {
                        if let Some(topic_state_entry) = self.topics.get(topic) {
                            let mut topic_state = topic_state_entry.lock().await;
                            match payload {
                                BrokerPayload::Meta { meta_rev, .. } => {
                                    topic_state.last_meta = Some(msg.clone());
                                    topic_state.meta_rev = *meta_rev;
                                }
                                BrokerPayload::Data(_) => {
                                    // Forward data directly without caching
                                }
                            }
                            if topic_state.sender.send(msg.clone()).is_err() {
                            	debug!("[Broker] No subscribers for topic '{}', message not sent.", topic);
                            }
                        }
                        // If the topic doesn't exist, we simply drop the message.
                        // It will be created on the first subscription.
                    }
                    BrokerMessage::RegisterTopic { .. } => {
                        // This is now deprecated and will be ignored.
                        // Topics are created on-demand by subscribers.
                    }
                }
            }
        });
    }

    /// Adds a new client to the broker, creating a dedicated task to manage its lifecycle.
    pub fn add_client(self: Arc<Self>, ws: WebSocket) {
        let client_id = uuid::Uuid::new_v4().to_string();
        info!("[Client {}] New WebSocket connection established.", client_id);
        tokio::spawn(async move {
            self.handle_client(ws, client_id).await;
        });
    }

    /// Manages the entire lifecycle of a single client connection.
    async fn handle_client(self: Arc<Self>, ws: WebSocket, client_id: String) {
        let (ws_tx, mut ws_rx) = ws.split();
  let ws_tx = Arc::new(Mutex::new(ws_tx));
        let mut subs: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_S));

        loop {
            select! {
                biased; // Prioritize client messages

                // 1. Handle messages from the client (subscribe, unsubscribe)
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(txt))) => {
                            if let Ok(msg) = serde_json::from_str::<ClientMessage>(&txt) {
                                match msg {
                                    ClientMessage::Subscribe { topic, .. } => {
                                        if subs.contains_key(&topic) {
                                            warn!("[Client {}] Already subscribed to topic '{}'. Ignoring.", client_id, topic);
                                            continue;
                                        }
                                        info!("[Client {}] Subscribing to topic: {}", client_id, topic);

                                        let topic_state_entry = self.topics.entry(topic.clone()).or_insert_with(|| {
                                            let (sender, _) = broadcast::channel(1024);
                                            Arc::new(Mutex::new(TopicState { sender, last_meta: None, meta_rev: 0 }))
                                        });

                                        let topic_state = topic_state_entry.lock().await;
                                        let mut main_rx = topic_state.sender.subscribe();

                                        // Send ACK and initial meta message
                                        let ack = ServerMessage::Subscribed(SubscribedAck {
                                            topic: topic.clone(),
                                            meta_rev: topic_state.last_meta.as_ref().map(|_| topic_state.meta_rev as u64),
                                        });
                                        if ws_tx.lock().await.send(Message::Text(serde_json::to_string(&ack).unwrap())).await.is_err() {
                                            break; // Client disconnected
                                        }
                                        if let Some(meta_msg) = &topic_state.last_meta {
                                            if let BrokerMessage::Data { payload: BrokerPayload::Meta { json, .. }, .. } = &**meta_msg {
                                                if ws_tx.lock().await.send(Message::Text(json.clone())).await.is_err() {
                                                    break; // Client disconnected
                                                }
                                            }
                                        }

                                        // Spawn a dedicated task to bridge the broadcast channel to this client's WebSocket.
                                        // This implements the "latest-wins" strategy by its nature. If the sender is slow,
                                        // the receiver will only get the latest message when it's ready.
                                        let client_ws_tx = Arc::clone(&ws_tx);
                                        let topic_clone = topic.clone();
                                        let client_id_clone = client_id.clone();
                                        let bridge_task = tokio::spawn(async move {
                                            loop {
                                                match main_rx.recv().await {
                                                    Ok(msg) => {
                                                        let frame = match &*msg {
                                                            BrokerMessage::Data { payload: BrokerPayload::Data(data), .. } => Message::Binary(data.to_vec()),
                                                            BrokerMessage::Data { payload: BrokerPayload::Meta { json, .. }, .. } => Message::Text(json.clone()),
                                                            _ => continue,
                                                        };
                                                        if client_ws_tx.lock().await.send(frame).await.is_err() {
                                                            break; // Client disconnected
                                                        }
                                                    },
                                                    Err(broadcast::error::RecvError::Lagged(n)) => {
                                                        warn!("[Client {}] Main broadcast for topic '{}' lagged by {} messages.", client_id_clone, &topic_clone, n);
                                                    },
                                                    Err(broadcast::error::RecvError::Closed) => {
                                                        break; // Topic was closed
                                                    }
                                                }
                                            }
                                        });
                                        subs.insert(topic, bridge_task);
                                    },
                                    ClientMessage::Unsubscribe { topic } => {
                                        info!("[Client {}] Unsubscribed from topic: {}", client_id, topic);
                                        if let Some(task) = subs.remove(&topic) {
                                            task.abort();
                                        }
                                    }
                                }
                            } else {
                                warn!("[Client {}] Received malformed control message. Closing.", client_id);
                                break;
                            }
                        },
                        Some(Ok(Message::Close(_))) | None => {
                            info!("[Client {}] Connection closed by client.", client_id);
                            break;
                        },
                        Some(Err(e)) => {
                            warn!("[Client {}] WebSocket error: {}", client_id, e);
                            break;
                        },
                        _ => {} // Ignore other message types
                    }
                },

                // 2. Handle ping timer
                _ = ping_interval.tick() => {
                    if ws_tx.lock().await.send(Message::Ping(vec![])).await.is_err() {
                        info!("[Client {}] Ping failed, client disconnected.", client_id);
                        break;
                    }
                },
            }
        }

        // Cleanup: Abort all subscription tasks for this client
        for (_, task) in subs {
            task.abort();
        }
        info!("[Client {}] Cleaning up connection.", client_id);
    }
}