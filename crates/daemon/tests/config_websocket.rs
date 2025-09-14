use adc_daemon::{
    api::AppState,
    config::ConfigBroker,
    protocol::{ConfigProposal, ServerMessage},
};
use axum::{extract::ws::WebSocket, routing::get, Router};
use futures_util::{SinkExt, StreamExt};
use sensors::{
    mock_eeg::driver::MockDriver,
    types::{AdcConfig, AdcDriver},
};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

struct TestHarness {
    _server_handle: tokio::task::JoinHandle<()>,
    server_addr: SocketAddr,
    driver: Arc<tokio::sync::Mutex<Box<dyn AdcDriver + Send>>>,
}

impl TestHarness {
    async fn new() -> Self {
        let driver = Arc::new(tokio::sync::Mutex::new(Box::new(
            MockDriver::new(AdcConfig::default()).unwrap(),
        ) as Box<dyn AdcDriver + Send>));
 
        let (config_broker, _) = ConfigBroker::new();
        // Preload initial config so new clients receive an Applied message on connect.
        config_broker
            .broadcast_and_update(Arc::new(AdcConfig::default()))
            .await;
 
        // Set up a minimal data-plane broker and channel required by AppState.
        let (websocket_sender, _) =
            tokio::sync::broadcast::channel::<Arc<eeg_types::comms::pipeline::BrokerMessage>>(1);
        let broker = Arc::new(adc_daemon::websocket_broker::WebSocketBroker::new(
            websocket_sender.subscribe(),
        ));
 
        let app_state = AppState {
            driver: Some(driver.clone()),
            config_broker: Arc::new(config_broker),
            // Add other AppState fields with default/mock values if needed
            pipelines: Default::default(),
            sse_tx: tokio::sync::broadcast::channel(1).0,
            event_tx: flume::unbounded().0,
            pipeline_handle: Default::default(),
            source_meta_cache: Default::default(),
            broker: broker.clone(),
            broker_shutdown_tx: Default::default(),
            websocket_sender,
        };

        let app = Router::new()
            .route("/ws/config", get(adc_daemon::config::config_websocket_handler))
            .with_state(app_state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            _server_handle: server_handle,
            server_addr,
            driver,
        }
    }

    fn ws_url(&self) -> String {
        format!("ws://{}/ws/config", self.server_addr)
    }
}

#[tokio::test]
async fn test_config_happy_path() {
    let harness = TestHarness::new().await;
    let (ws_stream, _) = connect_async(harness.ws_url()).await.unwrap();
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // 1. Expect initial config on connect
    let msg = ws_rx.next().await.unwrap().unwrap();
    let initial_msg: ServerMessage = serde_json::from_str(msg.to_text().unwrap()).unwrap();
    let initial_config = match initial_msg {
        ServerMessage::Applied { config, .. } => config,
        _ => panic!("Expected Applied message"),
    };
    assert_eq!(initial_config, AdcConfig::default());

    // 2. Propose a new config
    let mut new_config = AdcConfig::default();
    new_config.sample_rate = 1000;
    let proposal = ConfigProposal {
        config: new_config.clone(),
        request_id: None,
    };
    let proposal_msg = Message::Text(serde_json::to_string(&proposal).unwrap());
    ws_tx.send(proposal_msg).await.unwrap();

    // 3. Expect a broadcasted Applied message with the new config
    let msg = ws_rx.next().await.unwrap().unwrap();
    let broadcast_msg: ServerMessage = serde_json::from_str(msg.to_text().unwrap()).unwrap();
    let broadcast_config = match broadcast_msg {
        ServerMessage::Applied { config, .. } => config,
        _ => panic!("Expected Applied message"),
    };
    assert_eq!(broadcast_config, new_config);

    // 4. Verify the driver's config was actually updated
    let driver_config = harness.driver.lock().await.get_config().unwrap();
    assert_eq!(driver_config, new_config);
}