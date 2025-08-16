use axum::{
    extract::{
        FromRef, Path, State,
    },
    http::StatusCode,
    response::{sse::Event, IntoResponse, Json, Sse},
    routing::{get, post},
    Router,
};
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::broadcast;
use eeg_types::{comms::pipeline::BrokerMessage, data::SensorMeta};
use flume::Sender;
use pipeline::{
    control::ControlCommand,
    data::RtPacket,
    executor::{ControlBus, Executor},
};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
// Shared application state
use crate::websocket_broker::WebSocketBroker;

use sensors::types::AdcDriver;

#[derive(Clone)]
pub struct AppState {
    pub pipelines: Arc<Mutex<HashMap<String, PathBuf>>>,
    pub sse_tx: broadcast::Sender<String>,
    pub event_tx: Sender<PipelineEvent>,
    pub pipeline_handle: Arc<Mutex<Option<PipelineHandle>>>,
    pub source_meta_cache: Arc<Mutex<Option<SensorMeta>>>,
    pub broker: Arc<WebSocketBroker>,
    pub broker_shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub websocket_sender: broadcast::Sender<Arc<BrokerMessage>>,
    pub driver: Option<Arc<std::sync::Mutex<Box<dyn AdcDriver + Send>>>>,
}

impl FromRef<AppState> for Arc<WebSocketBroker> {
    fn from_ref(state: &AppState) -> Self {
        state.broker.clone()
    }
}

// Will hold the running pipeline's sender channel
pub struct PipelineHandle {
    pub id: String,
    pub executor: Option<Executor>,
    pub input_tx: Option<Sender<Arc<RtPacket>>>,
    pub control_bus: ControlBus,
}

#[derive(Serialize, Deserialize)]
pub struct PipelineInfo {
    id: String,
    name: String,
    description: String,
}

/// Scans for pipeline YAML files and returns a list of them.
pub async fn list_pipelines_handler(State(state): State<AppState>) -> Json<Value> {
    let mut pipelines = vec![];
    let paths = [PathBuf::from("pipelines"), PathBuf::from("crates/daemon")];

    for path in paths.iter() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                    if let Some(id) = file_path.file_stem().and_then(|s| s.to_str()) {
                        let p_info = PipelineInfo {
                            id: id.to_string(),
                            name: id.replace('_', " ").to_string(),
                            description: format!("Source: {}", file_path.display()),
                        };
                        pipelines.push(p_info);
                        state
                            .pipelines
                            .lock()
                            .await
                            .insert(id.to_string(), file_path);
                    }
                }
            }
        }
    }
    Json(json!(pipelines))
}

use pipeline::{
    config::SystemConfig, control::PipelineEvent, graph::PipelineGraph, registry::StageRegistry,
};
pub async fn start_pipeline_handler(
    Path(pipeline_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("Request to start/restart pipeline: {}", pipeline_id);

    let mut handle_guard = state.pipeline_handle.lock().await;

    // --- Check if a pipeline is already running ---
    if handle_guard.is_some() {
        tracing::info!("Request to start pipeline '{}', but a pipeline is already running.", pipeline_id);
        return (StatusCode::OK, "Pipeline is already running.").into_response();
    }

    // --- Start the new pipeline ---
    tracing::info!("Starting new pipeline '{}'", pipeline_id);

    let config_path = {
        let pipelines = state.pipelines.lock().await;
        match pipelines.get(&pipeline_id) {
            Some(path) => path.clone(),
            None => {
                return (StatusCode::NOT_FOUND, "Pipeline configuration not found").into_response();
            }
        }
    };

    let config_str = match fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read config file: {}", e),
            )
                .into_response();
        }
    };

    let config: SystemConfig = match serde_yaml::from_str(&config_str) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to parse YAML config: {}", e),
            )
                .into_response();
        }
    };

    let mut registry = StageRegistry::new();
    pipeline::stages::register_builtin_stages(&mut registry);

    // NOTE: This assumes the driver is managed globally and not pipeline-specific for now.
    // A more advanced implementation might manage drivers per pipeline.
    let graph = match PipelineGraph::build(
        &config,
        &registry,
        state.event_tx.clone(),
        None,
        &state.driver,
        Some(state.websocket_sender.clone()),
    ) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build pipeline graph: {}", e),
            )
                .into_response();
        }
    };

    let (executor, _fatal_error_rx, control_bus, mut producer_txs) = Executor::new(graph);


    // Store the handle to the new pipeline
    *handle_guard = Some(PipelineHandle {
        id: pipeline_id.clone(),
        executor: Some(executor),
        input_tx: producer_txs.remove("eeg_source"),
        control_bus,
    });

    (StatusCode::OK, "Pipeline started successfully").into_response()
}

pub async fn stop_pipeline_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Stopping pipeline");

    if let Some(mut handle) = state.pipeline_handle.lock().await.take() {
        if let Some(executor) = handle.executor.take() {
            executor.stop();
        }
        (StatusCode::OK, "Pipeline stopped").into_response()
    } else {
        (StatusCode::NOT_FOUND, "No pipeline running").into_response()
    }
}

#[derive(Deserialize)]
pub struct UpdatePipelinePayload {
    pub config: Value,
}

pub async fn update_pipeline_handler(
    Path(pipeline_id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<UpdatePipelinePayload>,
) -> impl IntoResponse {
    tracing::info!("Updating pipeline: {}", pipeline_id);

    let path = {
        let pipelines = state.pipelines.lock().await;
        match pipelines.get(&pipeline_id) {
            Some(path) => path.clone(),
            None => return (StatusCode::NOT_FOUND, "Pipeline not found").into_response(),
        }
    };

    let yaml_str = match serde_yaml::to_string(&payload.config) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize config: {}", e),
            )
                .into_response()
        }
    };

    match fs::write(path, yaml_str) {
        Ok(_) => (StatusCode::OK, "Pipeline updated").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write config: {}", e),
        )
            .into_response(),
    }
}


pub async fn control_handler(
    State(state): State<AppState>,
    Json(payload): Json<ControlCommand>,
) -> impl IntoResponse {
    tracing::info!("Received control command: {:?}", payload);

    if let Some(ref mut handle) = *state.pipeline_handle.lock().await {
        handle.control_bus.send_all(payload);
        (StatusCode::ACCEPTED, "Command received").into_response()
    } else {
        (StatusCode::NOT_FOUND, "No pipeline running").into_response()
    }
}

pub async fn state_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Fetching current state");
    if let Some(handle) = &*state.pipeline_handle.lock().await {
        if let Some(executor) = &handle.executor {
            let config = executor.get_current_config();
            Json(json!(config)).into_response()
        } else {
            (StatusCode::NOT_FOUND, "No pipeline running").into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "No pipeline running").into_response()
    }
}

// --- New minimal runtime-config endpoints ---

/// Alias for current runtime config. Equivalent to `/api/state` but semantically clearer.
pub async fn get_config_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("GET /api/config");
    if let Some(handle) = &*state.pipeline_handle.lock().await {
        if let Some(executor) = &handle.executor {
            let config = executor.get_current_config();
            return Json(json!(config)).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "No pipeline running").into_response()
}

#[derive(Deserialize)]
pub struct SetDriverConfigPayload {
    /// Parameters to forward to the eeg_source stage as `{ driver: ... }`.
    pub driver: Value,
    /// Optional target stage name; defaults to "eeg_source".
    #[serde(default = "default_eeg_source_name")]
    pub target_stage: String,
}

fn default_eeg_source_name() -> String { "eeg_source".to_string() }

/// Applies a driver configuration to the running pipeline by forwarding a SetParameter
/// to the eeg_source stage. Returns 202 on success dispatch.
pub async fn set_config_handler(
    State(state): State<AppState>,
    Json(payload): Json<SetDriverConfigPayload>,
) -> impl IntoResponse {
    tracing::info!("POST /api/set-config -> forwarding SetParameter to '{}'", payload.target_stage);

    if let Some(ref mut handle) = *state.pipeline_handle.lock().await {
        let params = json!({ "driver": payload.driver });
        let cmd = pipeline::control::ControlCommand::SetParameter {
            target_stage: payload.target_stage.clone(),
            parameters: params,
        };
        handle.control_bus.send_all(cmd);

        // Return the current runtime snapshot for convenience
        if let Some(executor) = &handle.executor {
            let config = executor.get_current_config();
            return (StatusCode::ACCEPTED, Json(json!(config))).into_response();
        }
        return (StatusCode::ACCEPTED, Json(json!({}))).into_response();
    }

    (StatusCode::NOT_FOUND, "No pipeline running").into_response()
}

/// Persists the current runtime config to the YAML file backing the running pipeline ID.
pub async fn save_config_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("POST /api/save-config");

    // Resolve current pipeline and its config
    let (pipeline_id, config) = {
        let handle_guard = state.pipeline_handle.lock().await;
        if let Some(handle) = &*handle_guard {
            if let Some(executor) = &handle.executor {
                (handle.id.clone(), executor.get_current_config())
            } else {
                return (StatusCode::NOT_FOUND, "No pipeline running").into_response();
            }
        } else {
            return (StatusCode::NOT_FOUND, "No pipeline running").into_response();
        }
    };

    // Find YAML path for this pipeline
    let path = {
        let map = state.pipelines.lock().await;
        match map.get(&pipeline_id) {
            Some(p) => p.clone(),
            None => return (StatusCode::NOT_FOUND, "Pipeline configuration not found").into_response(),
        }
    };

    // Serialize and write
    match serde_yaml::to_string(&config) {
        Ok(yaml) => match std::fs::write(&path, yaml) {
            Ok(_) => (StatusCode::OK, format!("Saved runtime config to {}", path.display())).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write YAML: {}", e)).into_response(),
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to serialize runtime config: {}", e)).into_response(),
    }
}



use tokio_stream::wrappers::BroadcastStream;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut initial_events = Vec::new();

    // Immediately send the current pipeline state if a pipeline is running
    if let Some(handle) = &*state.pipeline_handle.lock().await {
        if let Some(executor) = &handle.executor {
            let config = executor.get_current_config();
            let event = PipelineEvent::PipelineStarted {
                id: handle.id.clone(),
                config,
            };
            if let Ok(event_json) = serde_json::to_string(&event) {
                initial_events.push(Event::default().data(event_json));
            }
        }
    }

    // Also send the cached SourceReady event if it exists
    if let Some(meta) = &*state.source_meta_cache.lock().await {
        let event = PipelineEvent::SourceReady { meta: meta.clone() };
        if let Ok(event_json) = serde_json::to_string(&event) {
            initial_events.push(Event::default().data(event_json));
        }
    }

    let initial_stream = stream::iter(initial_events.into_iter().map(Ok));
    let live_stream = BroadcastStream::new(state.sse_tx.subscribe())
        .filter(|res| {
            if let Err(e) = res {
                tracing::error!("SSE broadcast stream error: {}", e);
            }
            // Use the correct error enum from tokio-stream
            futures::future::ready(!matches!(res, Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_))))
        })
        .map(|res| Ok(Event::default().data(res.expect("Lagged errors are filtered out"))));

    let stream = initial_stream.chain(live_stream);

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}

pub fn create_router() -> Router<AppState> {
    Router::new()
        .route("/api/pipelines", get(list_pipelines_handler))
        .route("/api/pipelines/:id/start", post(start_pipeline_handler))
        .route("/api/pipelines/stop", post(stop_pipeline_handler))
        .route("/api/pipelines/:id", post(update_pipeline_handler))
        .route("/api/pipelines/:id/control", post(control_handler)) // Added control route
        .route("/api/state", get(state_handler))
        .route("/api/config", get(get_config_handler))
        .route("/api/set-config", post(set_config_handler))
        .route("/api/save-config", post(save_config_handler))
        .route("/api/events", get(sse_handler))
}
