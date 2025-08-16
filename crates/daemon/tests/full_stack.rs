use adc_daemon::websocket_broker::WebSocketBroker;
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use eeg_types::comms::{
	client::{ClientMessage, ServerMessage, SubscribedAck},
	pipeline::BrokerMessage,
};
use futures_util::{SinkExt, StreamExt};
use pipeline::{
    config::{StageConfig, SystemConfig},
    control::{ControlCommand, PipelineEvent},
    error::StageError,
    executor::Executor,
    graph::PipelineGraph,
    registry::{StageFactory, StageRegistry},
    stage::{Stage, StageInitCtx},
    data::RtPacket,
};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use flume::Receiver;
use lazy_static::lazy_static;

lazy_static! {
    static ref STATEFUL_TEST_STAGE_TX: std::sync::Mutex<Option<flume::Sender<Arc<RtPacket>>>> = std::sync::Mutex::new(None);
}

// A simple pass-through stage for testing control-plane functionality.
struct StatefulTestStage {
    id: String,
    state: u32,
}

impl StatefulTestStage {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            state: 0,
        }
    }
}

impl Stage for StatefulTestStage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut pipeline::stage::StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        Ok(vec![("out".to_string(), packet)])
    }

    fn control(
        &mut self,
        cmd: &ControlCommand,
        ctx: &mut pipeline::stage::StageContext,
    ) -> Result<(), StageError> {
        if let ControlCommand::SetTestState(new_state) = cmd {
            self.state = *new_state;
            ctx.emit_event(PipelineEvent::TestStateChanged(self.state))?;
        }
        Ok(())
    }
}

struct StatefulTestStageFactory;
impl StageFactory for StatefulTestStageFactory {
    fn create(
        &self,
        config: &StageConfig,
        _init_ctx: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let (tx, rx) = flume::unbounded();
        // This is a producer stage for the test, so we send the TX half to the test
        // and the RX half to the executor.
        STATEFUL_TEST_STAGE_TX.lock().unwrap().replace(tx);
        Ok((Box::new(StatefulTestStage::new(&config.name)), Some(rx)))
    }
}


// A simple sink stage that does nothing.
struct TestSink;
impl Stage for TestSink {
    fn id(&self) -> &str {
        "test_sink"
    }
    fn process(
        &mut self,
        _packet: Arc<RtPacket>,
        _ctx: &mut pipeline::stage::StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        Ok(vec![])
    }
}
struct TestSinkFactory;
impl StageFactory for TestSinkFactory {
    fn create(
        &self,
        _config: &StageConfig,
        _init_ctx: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        Ok((Box::new(TestSink), None))
    }
}

/// A test harness to simplify setup and teardown of integrated tests.
struct TestHarness {
    pub broker: Arc<WebSocketBroker>,
    pub server_addr: SocketAddr,
    pub pipeline_tx: broadcast::Sender<Arc<BrokerMessage>>,
    _server_handle: tokio::task::JoinHandle<()>,
}

impl TestHarness {
    async fn new() -> Self {
        let (pipeline_tx, pipeline_rx) = broadcast::channel(128);
        let broker = Arc::new(WebSocketBroker::new(pipeline_rx));
        let (_, shutdown_rx) = tokio::sync::oneshot::channel();
        broker.clone().start(shutdown_rx);

        let app = Router::new()
            .route("/ws", get(ws_handler))
            .with_state(broker.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            broker,
            server_addr,
            pipeline_tx,
            _server_handle: server_handle,
        }
    }

    fn ws_url(&self) -> String {
        format!("ws://{}/ws", self.server_addr)
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(broker): State<Arc<WebSocketBroker>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        broker.add_client(socket);
    })
}

#[tokio::test]
async fn test_full_stack_command_and_shutdown() {
    // 1. Setup
    let (event_tx, event_rx) = flume::unbounded();
    let test_config = SystemConfig {
        version: "1.0".to_string(),
        metadata: Default::default(),
        stages: vec![
            serde_json::from_value(json!({
                "name": "test_stage_1",
                "type": "stateful_test_stage",
                "outputs": ["out"]
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "name": "test_sink_1",
                "type": "test_sink",
                "inputs": ["test_stage_1.out"]
            }))
            .unwrap(),
        ],
    };
    let mut registry = StageRegistry::new();
    registry.register("stateful_test_stage", Box::new(StatefulTestStageFactory));
    registry.register("test_sink", Box::new(TestSinkFactory));
    let graph = PipelineGraph::build(&test_config, &registry, event_tx, None, &None, None).unwrap();
    let (executor, _fatal_error_rx, control_bus, _) = Executor::new(graph);

    // 2. Send command to change state
    let cmd = ControlCommand::SetTestState(42);
    control_bus.send_all(cmd);
   
       // 2a. Send a dummy packet to unblock the producer stage so it can process the control command.
       let dummy_packet = Arc::new(RtPacket::Voltage(eeg_types::data::PacketData {
           header: Default::default(),
           samples: (vec![], Default::default()).into(),
       }));
       STATEFUL_TEST_STAGE_TX.lock().unwrap().as_ref().unwrap().send(dummy_packet).unwrap();
       tokio::time::sleep(Duration::from_millis(100)).await;


    // 3. Verify the pipeline emits the correct event
    let event = event_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(event, PipelineEvent::TestStateChanged(42));

    // 4. Initiate graceful shutdown and wait for it to complete
    executor.stop();
}

#[tokio::test]
async fn test_broker_closes_connection_on_invalid_payload() {
    // 1. Setup
    let harness = TestHarness::new().await;
    let topic = "eeg_raw".to_string();

    // 2. Register the topic before the client connects to avoid race condition
    harness
        .pipeline_tx
        .send(Arc::new(BrokerMessage::RegisterTopic {
            topic: topic.clone(),
            epoch: 1,
        }))
        .unwrap();
    // Give the broker a moment to process the registration
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Connect a client and subscribe
    let (ws_stream, _) = connect_async(harness.ws_url())
        .await
        .expect("Failed to connect");
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let subscribe_msg = ClientMessage::Subscribe { topic: topic.clone(), epoch: 1 };
    ws_tx
        .send(Message::Text(serde_json::to_string(&subscribe_msg).unwrap()))
        .await
        .unwrap();

    // 4. Wait for subscription ACK
    let ack_msg = ws_rx.next().await.expect("Server closed connection before sending ACK").expect("Error receiving ACK");
    assert!(matches!(ack_msg, Message::Text(_)), "Expected Text message for ACK");
    let ack: ServerMessage = serde_json::from_str(ack_msg.to_text().unwrap()).expect("Failed to parse ACK message");
    assert_eq!(
    	ack,
    	ServerMessage::Subscribed(SubscribedAck {
    		topic: topic.clone(),
    		meta_rev: None
    	})
    );

    // 5. Send a malicious binary payload from the client
    ws_tx
        .send(Message::Binary(vec![0, 1, 2, 3]))
        .await
        .unwrap();

    // 6. Verify the connection is closed
    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) => break, // Correctly closed
                    Some(Ok(Message::Ping(_))) => continue, // Ignore pings
                    other => panic!("Expected connection to be closed, but received: {:?}", other),
                }
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("Test timed out waiting for connection to close");
            }
        }
    }
}

#[tokio::test]
async fn test_broker_closes_connection_on_malformed_json() {
    // 1. Setup
    let harness = TestHarness::new().await;
    let topic = "eeg_raw".to_string();

    // 2. Register the topic before the client connects
    harness
        .pipeline_tx
        .send(Arc::new(BrokerMessage::RegisterTopic {
            topic: topic.clone(),
            epoch: 1,
        }))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Connect a client and subscribe
    let (ws_stream, _) = connect_async(harness.ws_url())
        .await
        .expect("Failed to connect");
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let subscribe_msg = ClientMessage::Subscribe { topic: topic.clone(), epoch: 1 };
    ws_tx
        .send(Message::Text(serde_json::to_string(&subscribe_msg).unwrap()))
        .await
        .unwrap();

    // 4. Wait for subscription ACK
    let ack_msg = ws_rx.next().await.expect("Server closed connection before sending ACK").expect("Error receiving ACK");
    assert!(matches!(ack_msg, Message::Text(_)), "Expected Text message for ACK");
    let ack: ServerMessage = serde_json::from_str(ack_msg.to_text().unwrap()).expect("Failed to parse ACK message");
    assert_eq!(
    	ack,
    	ServerMessage::Subscribed(SubscribedAck {
    		topic: topic.clone(),
    		meta_rev: None
    	})
    );

    // 5. Send a malformed JSON payload from the client
    let malformed_json = r#"{"type": "some_unsupported_action"}"#;
    ws_tx
        .send(Message::Text(malformed_json.to_string()))
        .await
        .unwrap();

    // 6. Verify the connection is closed
    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) => break, // Correctly closed
                    Some(Ok(Message::Ping(_))) => continue, // Ignore pings
                    other => panic!("Expected connection to be closed, but received: {:?}", other),
                }
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("Test timed out waiting for connection to close");
            }
        }
    }
}