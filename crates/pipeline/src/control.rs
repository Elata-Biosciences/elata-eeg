//! Defines the control commands for the pipeline.

use crate::config::SystemConfig;
use std::any::Any;
use std::fmt::Debug;

/// A trait for custom commands that can be cloned.
pub trait CustomCommand: Any + Send + Debug {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn CustomCommand>;
}

impl<T: Any + Send + Clone + Debug> CustomCommand for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn CustomCommand> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn CustomCommand> {
    fn clone(&self) -> Box<dyn CustomCommand> {
        self.clone_box()
    }
}

/// Commands that can be sent to a running pipeline to alter its state.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
pub enum ControlCommand {
    /// Start the pipeline.
    Start,
    /// Pause data processing.
    Pause,
    /// Resume data processing.
    Resume,
    /// Initiate a graceful shutdown of the pipeline.
    Shutdown,
    /// Signal to all stages that they should finish processing remaining data and then halt.
    Drain,
    /// Tell a sink stage to start recording.
    StartRecording,
    /// Tell a sink stage to stop recording.
    StopRecording,
    /// Replace the current system configuration with a new one.
    Reconfigure(SystemConfig),
    /// Set a parameter on a specific stage
    SetParameter {
        target_stage: String,
        parameters: Value,
    },
    /// (For testing) Set the state of a test stage.
    SetTestState(u32),
    /// A custom command for a specific stage.
    #[serde(skip)]
    Custom(Box<dyn CustomCommand>),
}

impl Debug for ControlCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlCommand::Start => write!(f, "Start"),
            ControlCommand::Pause => write!(f, "Pause"),
            ControlCommand::Resume => write!(f, "Resume"),
            ControlCommand::Shutdown => write!(f, "Shutdown"),
            ControlCommand::Drain => write!(f, "Drain"),
            ControlCommand::StartRecording => write!(f, "StartRecording"),
            ControlCommand::StopRecording => write!(f, "StopRecording"),
            ControlCommand::Reconfigure(config) => f.debug_tuple("Reconfigure").field(config).finish(),
            ControlCommand::SetParameter { target_stage, parameters } => f.debug_struct("SetParameter").field("target_stage", target_stage).field("parameters", parameters).finish(),
            ControlCommand::SetTestState(state) => f.debug_tuple("SetTestState").field(state).finish(),
            ControlCommand::Custom(cmd) => f.debug_tuple("Custom").field(cmd).finish(),
        }
    }
}

/// Events sent from the pipeline back to the control plane (e.g., the `device` crate).
use eeg_types::data::SensorMeta;

#[derive(Debug, Serialize)]
pub enum PipelineEvent {
    /// Indicates that a pipeline has started and includes its configuration.
    PipelineStarted {
        id: String,
        config: SystemConfig,
    },
    /// Acknowledges that the pipeline has completed its shutdown sequence.
    ShutdownAck,
    /// (For testing) Confirms a test stage's state has changed.
    TestStateChanged(u32),
    /// Indicates that a stage has started processing.
    StageStarted { stage_id: String },
    /// Indicates that a stage has stopped processing.
    StageStopped { stage_id: String },
    /// Indicates that a parameter has been changed on a stage.
    ParameterChanged {
        stage_id: String,
        parameter_id: String,
        value: serde_json::Value,
    },
    /// Indicates that an error has occurred in a stage.
    ErrorOccurred {
        stage_id: String,
        error_message: String,
    },
    /// Indicates that data is flowing through the pipeline.
    DataFlowing { packet_count: u64 },
    /// Indicates that the pipeline configuration has been updated.
    ConfigUpdated {
        config: crate::config::SystemConfig,
    },
    /// Indicates that a source stage is ready and provides its metadata.
    SourceReady { meta: SensorMeta },
    /// Indicates that the entire pipeline has failed due to a panic.
    PipelineFailed { error: String },
}

impl PartialEq for PipelineEvent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::PipelineStarted { id: lid, config: lc }, Self::PipelineStarted { id: rid, config: rc }) => {
                lid == rid && lc == rc
            }
            (Self::ShutdownAck, Self::ShutdownAck) => true,
            (Self::TestStateChanged(l), Self::TestStateChanged(r)) => l == r,
            (Self::StageStarted { stage_id: l }, Self::StageStarted { stage_id: r }) => l == r,
            (Self::StageStopped { stage_id: l }, Self::StageStopped { stage_id: r }) => l == r,
            (Self::ParameterChanged { stage_id: ls, parameter_id: lp, value: lv }, Self::ParameterChanged { stage_id: rs, parameter_id: rp, value: rv }) => {
                ls == rs && lp == rp && lv == rv
            }
            (Self::ErrorOccurred { stage_id: ls, error_message: le }, Self::ErrorOccurred { stage_id: rs, error_message: re }) => {
                ls == rs && le == re
            }
            (Self::DataFlowing { packet_count: l }, Self::DataFlowing { packet_count: r }) => l == r,
            (Self::ConfigUpdated { config: l }, Self::ConfigUpdated { config: r }) => l == r,
            (Self::SourceReady { meta: l }, Self::SourceReady { meta: r }) => l == r,
            (Self::PipelineFailed { error: l }, Self::PipelineFailed { error: r }) => l == r,
            _ => false,
        }
    }
}