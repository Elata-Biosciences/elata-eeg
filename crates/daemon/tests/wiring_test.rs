use pipeline::config::SystemConfig;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[test]
fn test_default_pipeline_wiring() {
    let config_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("pipelines/default.yaml");

    let config_str = fs::read_to_string(config_path).expect("Failed to read default pipeline config");
    let config: SystemConfig =
        serde_yaml::from_str(&config_str).expect("Failed to parse default pipeline config");

    let registry = Arc::new(StageRegistry::new());
    let (event_tx, _) = flume::unbounded();
    let graph = PipelineGraph::build(&config, &registry, event_tx, None, &None, None)
        .expect("Failed to build pipeline graph");

    // Assert that the graph has the correct number of stages
    assert_eq!(graph.nodes.len(), 4, "Expected 4 stages in the graph");

    // Assert that the stages are connected correctly
    let to_voltage_inputs = graph.config.stages.iter().find(|s| s.name == "to_voltage").unwrap().inputs.clone();
    assert_eq!(to_voltage_inputs, vec!["eeg_source.raw_data"]);

    let csv_sink_inputs = graph.config.stages.iter().find(|s| s.name == "csv_sink").unwrap().inputs.clone();
    assert_eq!(csv_sink_inputs, vec!["to_voltage.voltage_data"]);

    let websocket_sink_inputs = graph.config.stages.iter().find(|s| s.name == "websocket_sink").unwrap().inputs.clone();
    assert_eq!(websocket_sink_inputs, vec!["to_voltage.voltage_data"]);
}