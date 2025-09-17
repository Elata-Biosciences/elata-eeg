use backtrace::Backtrace;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use adc_daemon::plugin_supervisor::PluginSupervisor;
use adc_daemon::{
    api::{AppState, PipelineHandle},
    config::ConfigBroker,
    websocket_broker::WebSocketBroker,
};
use boards::elata_v2::driver::ElataV2Driver;
use clap::{Arg, Command};
use eeg_types::comms::pipeline::BrokerMessage;
use pipeline::config::SystemConfig;
use pipeline::control::PipelineEvent;
use pipeline::executor::Executor;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use sensors::{
    mock_eeg::driver::MockDriver,
    types::{AdcConfig, AdcDriver, DriverError},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn resolve_config_path(input: &str) -> Result<PathBuf, DriverError> {
    // 1) As provided (absolute or relative to CWD)
    let p1 = PathBuf::from(input);
    if p1.exists() {
        return Ok(p1);
    }
    // 2) Relative to the binary directory
    if let Ok(exe) = env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            let p2 = bin_dir.join(input);
            if p2.exists() {
                return Ok(p2);
            }
        }
    }
    // 3) Relative to the workspace root (two levels up from crate dir)
    let ws_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent());
    if let Some(root) = ws_root {
        let p3 = root.join(input);
        if p3.exists() {
            return Ok(p3);
        }
    }
    Err(DriverError::IoError(format!(
        "Config file not found. Tried: '{0}', '<bin_dir>/{0}', '<workspace_root>/{0}'",
        input
    )))
}

#[tokio::main]
async fn main() -> Result<(), DriverError> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "adc_daemon=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("EEG Daemon starting...");

    // --- Argument Parsing ---
    let matches = Command::new("eeg_daemon")
        .about("EEG data acquisition daemon")
        .arg(
            Arg::new("mock")
                .long("mock")
                .action(clap::ArgAction::SetTrue)
                .help("Use mock EEG data instead of real hardware"),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .short('c')
                .num_args(1)
                .value_name("FILE")
                .help("Path to pipeline config YAML (default: pipelines/default.yaml)"),
        )
        .get_matches();

    // --- Centralized State ---
    let (sse_tx, _) = tokio::sync::broadcast::channel(1024);
    let (event_tx, event_rx) = flume::bounded(100);
    let (ws_tx, ws_rx) = tokio::sync::broadcast::channel::<Arc<BrokerMessage>>(1024);

    // --- Plugin Supervisor ---
    let _supervisor = PluginSupervisor::new();

    // --- Default Pipeline Startup ---
    let config_input = matches
        .get_one::<String>("config")
        .map(|s| s.as_str())
        .unwrap_or("pipelines/default.yaml");
    let resolved_config_path = resolve_config_path(config_input)?;
    tracing::info!("Loading config from: {}", resolved_config_path.display());
    let config_str = fs::read_to_string(&resolved_config_path)
        .map_err(|e| DriverError::IoError(e.to_string()))?;
    let initial_config: SystemConfig = serde_yaml::from_str(&config_str)
        .map_err(|e| DriverError::ConfigurationError(e.to_string()))?;
    tracing::info!("Loaded initial config: {:?}", initial_config);

    let mut registry = StageRegistry::new();
    pipeline::stages::register_builtin_stages(&mut registry);

    // --- Driver Initialization ---
    let use_mock = matches.get_flag("mock");
    let eeg_source_config = initial_config
        .stages
        .iter()
        .find(|s| s.stage_type == "eeg_source")
        .expect("No eeg_source stage found in config");

    let driver_config_value = eeg_source_config
        .params
        .get("driver")
        .expect("No driver configuration found in eeg_source stage");

    let driver_type = driver_config_value
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("Mock");

    let driver: Option<Arc<tokio::sync::Mutex<Box<dyn AdcDriver + Send>>>> =
        if use_mock || driver_type == "Mock" {
            tracing::info!("Using mock EEG driver");
            // Parse the driver configuration from the pipeline
            let adc_config: AdcConfig = serde_json::from_value(driver_config_value.clone())
                .map_err(|e| DriverError::ConfigurationError(e.to_string()))?;
            Some(Arc::new(tokio::sync::Mutex::new(Box::new(
                MockDriver::new(adc_config)?,
            ))))
        } else {
            tracing::info!("Using ElataV2 hardware driver");
            // Parse the driver configuration from the pipeline
            let adc_config: AdcConfig = serde_json::from_value(driver_config_value.clone())
                .map_err(|e| DriverError::ConfigurationError(e.to_string()))?;
            let driver_instance = ElataV2Driver::with_default_board(adc_config)?;
            Some(Arc::new(tokio::sync::Mutex::new(Box::new(driver_instance))))
        };

    tracing::info!("Building pipeline graph...");
    let graph = match PipelineGraph::build(
        &initial_config,
        &registry,
        event_tx.clone(),
        None,
        &driver,
        Some(ws_tx.clone()),
    ) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("Failed to build pipeline graph: {}", e);
            // Exit gracefully
            return Ok(());
        }
    };
    tracing::info!("Pipeline graph built.");

    let (executor, fatal_error_rx, control_bus, mut producer_txs) = Executor::new(graph);
    tracing::info!("Default pipeline executor started.");

    let pipeline_handle = Arc::new(tokio::sync::Mutex::new(Some(PipelineHandle {
        id: "default".to_string(),
        executor: Some(executor),
        control_bus,
        input_tx: {
            let tx = producer_txs.remove("eeg_source");
            if tx.is_none() {
                tracing::error!("Could not find 'eeg_source' producer in the pipeline configuration. The default pipeline will not be able to receive input.");
            }
            tx
        },
    })));

    // Spawn a task to listen for fatal errors from the default pipeline
    let pipeline_handle_clone = pipeline_handle.clone();
    let fatal_error_sse_tx = sse_tx.clone();
    let fatal_error_handle = tokio::spawn(async move {
        if let Ok(fatal_error) = fatal_error_rx.recv_async().await {
            tracing::error!(
                "Fatal pipeline error in stage '{}'. Shutting down.",
                fatal_error.stage_id
            );

            let backtrace = Backtrace::new();
            let error_msg = if let Some(s) = fatal_error.error.downcast_ref::<&'static str>() {
                format!("Panic: '{}'\n{:?}", s, backtrace)
            } else if let Some(s) = fatal_error.error.downcast_ref::<String>() {
                format!("Panic: '{}'\n{:?}", s, backtrace)
            } else {
                format!("Unknown panic payload\n{:?}", backtrace)
            };

            if let Some(mut handle) = pipeline_handle_clone.lock().await.take() {
                if let Some(executor) = handle.executor.take() {
                    executor.stop();
                }
            }

            let event = PipelineEvent::PipelineFailed { error: error_msg };
            if let Ok(event_json) = serde_json::to_string(&event) {
                if fatal_error_sse_tx.send(event_json).is_err() {
                    tracing::warn!(
                        "Failed to send pipeline failure SSE event: receiver disconnected."
                    );
                }
            }
        }
    });

    // --- Event Forwarding Task ---
    // Forwards events from the pipeline to the SSE broadcast channel
    let source_meta_cache = Arc::new(tokio::sync::Mutex::new(None));
    let event_forwarding_cache = source_meta_cache.clone();
    let sse_tx_clone_for_forwarding = sse_tx.clone();
    let event_forwarding_handle = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv_async().await {
            // If this is the source ready event, cache its metadata
            if let PipelineEvent::SourceReady { meta } = &event {
                tracing::debug!("Caching SourceReady event metadata");
                let mut cache = event_forwarding_cache.lock().await;
                *cache = Some(meta.clone());
            }

            if let Ok(event_json) = serde_json::to_string(&event) {
                // Send to SSE clients
                if sse_tx_clone_for_forwarding
                    .send(event_json.clone())
                    .is_err()
                {
                    tracing::debug!("No active SSE subscribers to send event to.");
                }
            } else {
                tracing::error!("Failed to serialize pipeline event");
            }
        }
        tracing::info!("Event forwarding task finished.");
    });

    // --- WebSocket Broker ---
    let (broker_shutdown_tx, broker_shutdown_rx) = tokio::sync::oneshot::channel();
    let broker = Arc::new(WebSocketBroker::new(ws_rx));
    broker.clone().start(broker_shutdown_rx);

    // --- Config Broker ---
    let (config_broker, _) = ConfigBroker::new();
    let config_broker = Arc::new(config_broker);

    // --- App State ---
    let app_state = AppState {
        pipelines: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        sse_tx,
        pipeline_handle,
        source_meta_cache,
        broker,
        config_broker,
        broker_shutdown_tx: Arc::new(Mutex::new(Some(broker_shutdown_tx))),
        websocket_sender: ws_tx,
        driver: driver.clone(),
        event_tx: event_tx.clone(),
    };

    // --- Server Thread ---
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_handle = tokio::spawn(adc_daemon::server::run(app_state.clone(), shutdown_rx));

    // --- Graceful Shutdown ---
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| DriverError::IoError(e.to_string()))?;
    tracing::info!("Shutdown signal received. Stopping services...");

    // 1. Signal the WebSocket broker to shut down gracefully.
    if let Some(broker_shutdown) = app_state.broker_shutdown_tx.lock().await.take() {
        tracing::debug!("Sending shutdown signal to WebSocket broker.");
        let _ = broker_shutdown.send(());
    }

    // 2. Stop the pipeline executor.
    if let Some(mut handle) = app_state.pipeline_handle.lock().await.take() {
        if let Some(executor) = handle.executor.take() {
            executor.stop();
        }
    }

    // By dropping the event_tx, we signal the event forwarding task to terminate.
    drop(event_tx);

    // Signal the server to shut down
    let _ = shutdown_tx.send(());

    // The server holds the last clone of the app_state, but we need to drop
    // the main one here to release the WebSocket sender and allow the broker
    // to terminate.
    drop(app_state);

    // Wait for the server to shut down
    match server_handle.await {
        Ok(Ok(_)) => {
            tracing::info!("Server shut down successfully");
        }
        Ok(Err(e)) => {
            tracing::error!("Server error: {}", e);
            return Err(DriverError::Other(format!("Server error: {}", e)));
        }
        Err(e) => {
            tracing::error!("Server task panicked: {:?}", e);
            return Err(DriverError::Other(format!("Server task failed: {:?}", e)));
        }
    }

    // Wait for the background tasks to complete.
    if let Err(e) = event_forwarding_handle.await {
        tracing::error!("Event forwarding task panicked: {:?}", e);
    }
    if let Err(e) = fatal_error_handle.await {
        tracing::error!("Fatal error task panicked: {:?}", e);
    }

    tracing::info!("EEG Daemon stopped gracefully.");

    Ok(())
}
