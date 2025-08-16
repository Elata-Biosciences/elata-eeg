//! Pipeline graph construction and management.

use crate::allocator::{PacketAllocator, SharedPacketAllocator};
use crate::config::SystemConfig;
use crate::control::ControlCommand;
use crate::data::RtPacket;
use crate::error::{PipelineError, StageError};
use crate::registry::StageRegistry;
use crate::stage::{DefaultPolicy, Stage, StageContext, StagePolicy, StageState};
use eeg_types::comms::BrokerMessage;
use flume::Receiver;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::broadcast;

use sensors::types::AdcDriver;

pub type StageId = String;

/// Defines the operational mode of a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMode {
    /// A standard stage that processes packets from an input channel.
    Pull,
    /// A source stage that produces packets asynchronously.
    Producer,
}

/// Represents a node in the pipeline graph.
pub struct PipelineNode {
    pub name: StageId,
    pub stage: Arc<Mutex<Box<dyn Stage>>>,
    pub input_source: Option<StageId>,
    pub state: StageState,
    pub policy: Arc<Box<dyn StagePolicy>>,
    pub mode: StageMode,
    pub producer_rx: Option<Receiver<Arc<RtPacket>>>,
}

impl Clone for PipelineNode {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            stage: self.stage.clone(),
            input_source: self.input_source.clone(),
            state: self.state,
            policy: self.policy.clone(),
            mode: self.mode,
            producer_rx: self.producer_rx.clone(),
        }
    }
}

/// Represents the entire pipeline as a graph of connected stages.
#[derive(Clone)]
pub struct PipelineGraph {
    pub nodes: HashMap<StageId, PipelineNode>,
    pub context: StageContext,
    pub allocator: SharedPacketAllocator,
    pub config: SystemConfig,
    pub topo_dirty: bool,
}

impl PipelineGraph {
    /// Builds a new `PipelineGraph` from a configuration and a stage registry.
    pub fn build(
        config: &SystemConfig,
        registry: &StageRegistry,
        event_tx: flume::Sender<crate::control::PipelineEvent>,
        allocator: Option<SharedPacketAllocator>,
        driver: &Option<Arc<Mutex<Box<dyn AdcDriver + Send>>>>,
        websocket_sender: Option<broadcast::Sender<Arc<BrokerMessage>>>,
    ) -> Result<Self, StageError> {
        let mut nodes = HashMap::new();
        let allocator = allocator
            .unwrap_or_else(|| Arc::new(PacketAllocator::with_capacity(16, 16, 16, 1024)));

        for stage_config in &config.stages {
            if nodes.contains_key(&stage_config.name) {
                return Err(StageError::BadConfig(format!(
                    "Duplicate stage name found: {}",
                    stage_config.name
                )));
            }

            let sample_rate = config
                .stages
                .iter()
                .find(|s| s.stage_type == "eeg_source")
                .and_then(|s| s.params.get("driver"))
                .and_then(|d| d.get("sample_rate"))
                .and_then(|sr| sr.as_f64())
                .unwrap_or(250.0);


            let init_ctx = crate::stage::StageInitCtx {
                event_tx: &event_tx,
                allocator: &allocator,
                driver,
                sample_rate,
                websocket_sender: websocket_sender.clone(),
            };

            let (stage, producer_rx) = registry.create_stage(stage_config, &init_ctx)?;
            let stage = Arc::new(Mutex::new(stage));
            let mode = if producer_rx.is_some() {
                StageMode::Producer
            } else {
                StageMode::Pull
            };

            if stage_config.inputs.len() > 1 {
                // This is a temporary check. The new executor will support multiple inputs.
                // log::warn!("Stage '{}' has more than one input. This is not fully supported yet.", stage_config.name);
            }
            let input_source = stage_config
                .inputs
                .first()
                .map(|s| s.split('.').next().unwrap_or(s).to_string());

            let node = PipelineNode {
                name: stage_config.name.clone(),
                stage,
                input_source,
                state: StageState::Running,
                policy: Arc::new(Box::new(DefaultPolicy)),
                mode,
                producer_rx,
            };
            nodes.insert(stage_config.name.clone(), node);
        }

        // Create a map of all available outputs for validation.
        let mut available_outputs = HashMap::new();
        for stage_config in &config.stages {
            for output_name in &stage_config.outputs {
                available_outputs.insert(
                    format!("{}.{}", stage_config.name, output_name),
                    stage_config.name.clone(),
                );
            }
            // For stages without explicit outputs, assume a default output stream named "out".
            if stage_config.outputs.is_empty() {
                available_outputs.insert(format!("{}.out", stage_config.name), stage_config.name.clone());
            }
        }

        // Validate that all input sources exist.
        for stage_config in &config.stages {
            for input_name in &stage_config.inputs {
                if !available_outputs.contains_key(input_name) {
                    return Err(StageError::BadConfig(format!(
                        "Stage '{}' references an unknown input '{}'",
                        stage_config.name, input_name
                    )));
                }
            }
        }

        let context = StageContext::new(event_tx, allocator.clone());

        Ok(Self {
            nodes,
            context,
            allocator,
            config: config.clone(),
            topo_dirty: false,
        })
    }


    /// Forwards a control command to all stages in the graph.
    pub fn handle_control_command(&mut self, cmd: &ControlCommand) -> Result<(), PipelineError> {
        match cmd {
            ControlCommand::Reconfigure(new_config) => {
                self.topo_dirty = true;
                self.config = new_config.clone();

                for stage_config in &new_config.stages {
                    if let Some(node) = self.nodes.get_mut(&stage_config.name) {
                        // This is a simplified diff. A real implementation would be more robust.
                        let params_value = serde_json::to_value(&stage_config.params)?;
                        node.stage
                            .lock()
                            .unwrap()
                            .reconfigure(&params_value, &mut self.context)?;
                    }
                }
                
                // Emit a ConfigUpdated event
                if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::ConfigUpdated {
                    config: new_config.clone(),
                }) {
                    tracing::error!("Failed to send ConfigUpdated event: {}", e);
                }
            }
            ControlCommand::SetParameter { target_stage, parameters } => {
                // Forward the command to the target stage
                if let Some(node) = self.nodes.get_mut(target_stage) {
                    node.stage.lock().unwrap().control(cmd, &mut self.context)?;
                    
                    // Emit a ParameterChanged event
                    for (param_id, value) in parameters.as_object().unwrap_or(&serde_json::Map::new()) {
                        if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::ParameterChanged {
                            stage_id: target_stage.clone(),
                            parameter_id: param_id.clone(),
                            value: value.clone(),
                        }) {
                            tracing::error!("Failed to send ParameterChanged event: {}", e);
                        }
                    }
                }
            }
            ControlCommand::Start => {
                for node in self.nodes.values_mut() {
                    node.stage.lock().unwrap().control(cmd, &mut self.context)?;
                    
                    // Emit a StageStarted event
                    if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::StageStarted {
                        stage_id: node.name.clone(),
                    }) {
                        tracing::error!("Failed to send StageStarted event: {}", e);
                    }
                }
            }
            ControlCommand::Shutdown => {
                for node in self.nodes.values_mut() {
                    node.stage.lock().unwrap().control(cmd, &mut self.context)?;
                    
                    // Emit a StageStopped event
                    if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::StageStopped {
                        stage_id: node.name.clone(),
                    }) {
                        tracing::error!("Failed to send StageStopped event: {}", e);
                    }
                }
            }
            _ => {
                for node in self.nodes.values_mut() {
                    node.stage.lock().unwrap().control(cmd, &mut self.context)?;
                }
            }
        }
        Ok(())
    }

    /// Checks if the pipeline is idle. In a synchronous model, it's always idle
    /// between `push` calls.
    pub fn is_idle(&self) -> bool {
        true
    }

    /// Flushes all sink stages that implement the `Drains` trait.
    /// TODO: This requires a mechanism to downcast `Stage` to `Drains`.
    pub fn flush(&mut self) -> Result<(), PipelineError> {
        for node in self.nodes.values_mut() {
            if let Some(drains) = node.stage.lock().unwrap().as_drains() {
                drains.flush().map_err(|e| PipelineError::RuntimeError {
                    stage_name: node.name.clone(),
                    message: format!("IO error during flush: {}", e),
                })?;
            }
        }
        Ok(())
    }

    pub fn get_current_config(&self) -> SystemConfig {
        self.config.clone()
    }

}