//! Integration tests for the pipeline.

use crate::config::SystemConfig;
use crate::stages::register_builtin_stages;
use crate::registry::StageRegistry;
use std::sync::Arc;

use crate::control::PipelineEvent;
use crate::executor::Executor;
use flume as crossbeam_channel;

#[test]
fn test_full_static_pipeline() {
    // 1. Set up the stage registry
    let mut registry = StageRegistry::new();
    register_builtin_stages(&mut registry);
    let registry = Arc::new(registry);

    // 2. Define a pipeline configuration
    let config_json = r#"
    {
        "version": "1.0",
        "stages": [
            {
                "name": "acquire1",
                "type": "acquire",
                "params": { "sps": 500 }
            },
            {
                "name": "to_voltage1",
                "type": "to_voltage",
                "inputs": ["acquire1"]
            },
            {
                "name": "filter1",
                "type": "filter",
                "params": { "lowpass": 40.0 },
                "inputs": ["to_voltage1"]
            }
        ]
    }
    "#;
    let config: SystemConfig = serde_json::from_str(config_json).unwrap();

    // 3. Create channels for events
    let (event_tx, _event_rx) = crossbeam_channel::unbounded::<PipelineEvent>();

    // 4. Build the graph
    let graph =
        crate::graph::PipelineGraph::build(&config, &registry, event_tx.clone(), None, &None, None).unwrap();

    // 5. Run the pipeline in a separate task
    let (executor, _, _, _) = Executor::new(graph);

    // 6. Send a shutdown command
    executor.stop();

    // 7. We can't easily check for the shutdown ack without more complex event handling,
    // but we can at least ensure the executor thread joins.
}
