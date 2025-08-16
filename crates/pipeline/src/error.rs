//! Error types for the pipeline system

use sensors::DriverError;
use thiserror::Error;

use crate::graph::StageId;
use std::any::Any;

/// A struct to wrap fatal errors (panics) with more context.
#[derive(Debug)]
pub struct FatalError {
    pub stage_id: StageId,
    pub error: Box<dyn Any + Send>,
}
/// Pipeline-specific error types
#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("Stage not found: {name}")]
    StageNotFound { name: String },

    #[error("Circular dependency detected in pipeline graph")]
    CircularDependency,

    #[error("Invalid stage configuration: {message}")]
    InvalidConfiguration { message: String },

    #[error("Stage type not registered: {stage_type}")]
    UnknownStageType { stage_type: String },

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Send error: {0}")]
    SendError(String),

    #[error("Runtime error in stage '{stage_name}': {message}")]
    RuntimeError { stage_name: String, message: String },

    #[error("Pipeline is already running")]
    AlreadyRunning,

    #[error("Pipeline is not running")]
    NotRunning,

    #[error("Invalid pipeline state: {0}")]
    InvalidState(String),

    #[error("Stage '{stage_name}' is locked and cannot be modified")]
    StageLocked { stage_name: String },

    #[error("Processing error: {message}")]
    ProcessingError { message: String },

    #[error("Invalid input: {message}")]
    InvalidInput { message: String },

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Generic error: {0}")]
    Other(#[from] anyhow::Error),
}

impl From<StageError> for PipelineError {
    fn from(err: StageError) -> Self {
        PipelineError::RuntimeError {
            stage_name: "unknown".to_string(), // Or find a way to pass the stage name
            message: err.to_string(),
        }
    }
}

/// Result type for pipeline operations
pub type PipelineResult<T> = Result<T, PipelineError>;
/// Error types for a single stage in the data plane.
#[derive(thiserror::Error, Debug, Clone)]
pub enum StageError {
    #[error("queue closed")]
    QueueClosed,
    #[error("backpressure from {0}")]
    Backpressure(&'static str),
    #[error("bad param {0}")]
    BadParam(String),
    #[error("stage type not found: {0}")]
    NotFound(String),
    #[error("invalid configuration: {0}")]
    BadConfig(String),
    #[error("pipeline is not ready: {0}")]
    NotReady(String),
    #[error("fatal hw error: {0}")]
    Fatal(String),
    #[error("stage is busy and cannot perform the requested operation")]
    Busy,
    #[error("send error: {0}")]
    SendError(String),
    #[error("operation would block: {0}")]
    WouldBlock(String),
    #[error("invalid context: {0}")]
    InvalidContext(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("JSON serialization/deserialization error: {0}")]
    JsonError(String),
    #[error("live reconfiguration is not supported by this stage")]
    UnsupportedReconfig,
    #[error("processing error: {0}")]
    Processing(String),
    #[error("stream closed")]
    StreamClosed,
    #[error("driver error: {0}")]
    Driver(#[from] DriverError),
    #[error("driver error: {0}")]
    DriverError(String),
}

impl From<serde_json::Error> for StageError {
    fn from(err: serde_json::Error) -> Self {
        StageError::JsonError(err.to_string())
    }
}

