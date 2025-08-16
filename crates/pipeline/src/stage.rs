//! Core definitions for pipeline stages.

use crate::allocator::SharedPacketAllocator;
use crate::control::{ControlCommand, PipelineEvent};
use crate::data::RtPacket;
use crate::error::StageError;
use eeg_types::comms::BrokerMessage;
use flume::Sender;
use sensors::types::AdcDriver;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// The possible states of a stage in the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageState {
    /// The stage is running normally.
    Running,
    /// The stage is draining its input queue and will shut down once empty.
    Draining,
    /// The stage has been halted and will no longer process packets.
    Halted,
}

/// Defines the action to take when a stage encounters a non-fatal error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorAction {
    /// Shut down the entire pipeline.
    Fatal,
    /// Stop sending new packets to this stage and let it finish processing its queue.
    DrainThenStop,
    /// Drop the current packet and continue processing.
    SkipPacket,
}

/// A policy that determines how a stage should respond to errors.
pub trait StagePolicy: Send + Sync {
    /// Called when a processing error occurs.
    fn on_error(&self) -> ErrorAction;
}

/// A default policy that treats all errors as fatal.
pub struct DefaultPolicy;
impl StagePolicy for DefaultPolicy {
    fn on_error(&self) -> ErrorAction {
        ErrorAction::Fatal
    }
}

/// A context object passed to each stage's `process` method.
///
/// It provides access to shared resources and control mechanisms, like the
/// ability to emit events back to the main control loop.
#[derive(Clone)]
pub struct StageContext {
    pub event_tx: Sender<PipelineEvent>,
    pub allocator: SharedPacketAllocator,
}

/// A context object passed to each stage during initialization.
///
/// It provides mechanisms for the stage to register itself with the pipeline,
/// for example as a producer.
pub struct StageInitCtx<'a> {
    pub event_tx: &'a Sender<PipelineEvent>,
    pub allocator: &'a SharedPacketAllocator,
    pub driver: &'a Option<Arc<Mutex<Box<dyn AdcDriver + Send>>>>,
    pub sample_rate: f64,
    pub websocket_sender: Option<broadcast::Sender<Arc<BrokerMessage>>>,
}

impl StageContext {
    pub fn new(event_tx: Sender<PipelineEvent>, allocator: SharedPacketAllocator) -> Self {
        Self {
            event_tx,
            allocator,
        }
    }

    /// Emits an event to the main control loop.
    pub fn emit_event(&self, event: PipelineEvent) -> Result<(), StageError> {
        self.event_tx
            .send(event)
            .map_err(|e| StageError::SendError(e.to_string()))
    }
}

/// The core trait for a processing stage in the pipeline.
///
/// A stage is a component that receives packets of one type (`I`), processes
/// them, and outputs packets of another type (`O`). It can also respond to
pub trait Stage: Send + Sync {
    /// A unique identifier for this stage instance.
    fn id(&self) -> &str;

    /// Processes an input packet and returns an optional output packet.
    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError>;

    /// Producer path (actively generates data).
    /// Default: do nothing this tick.
    fn produce(
        &mut self,
        _ctx: &mut StageContext,
    ) -> Result<Option<Vec<(String, Arc<RtPacket>)>>, StageError> {
        Ok(None)
    }

    /// Reconfigures the stage with new parameters.
    ///
    /// The default implementation returns an error, forcing stages to opt-in
    /// to live reconfiguration.
    fn reconfigure(
        &mut self,
        _config: &serde_json::Value,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        Err(StageError::UnsupportedReconfig)
    }

    /// Handles a control command sent to the pipeline.
    /// The default implementation does nothing, allowing stages to opt-in.
    fn control(
        &mut self,
        _cmd: &ControlCommand,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        Ok(())
    }

    /// Called when the stage is being shut down.
    fn shutdown(&mut self, _ctx: &mut StageContext) -> Result<(), StageError> {
        Ok(()) // Default implementation does nothing.
    }

    /// Returns true if the stage's configuration is currently locked, preventing
    /// dynamic reconfiguration. Stages that need to protect their state (e.g.,
    /// while recording to a file) should override this method.
    fn is_locked(&self) -> bool {
        false
    }

    /// Returns this stage as a mutable `Drains` trait object if it implements it.
    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        None
    }
}

/// A trait for stages that need to flush internal buffers before shutdown.
///
/// This is typically implemented by "sink" stages that write to files or network
/// sockets, ensuring that all data is persisted before the pipeline terminates.
pub trait Drains {
    /// Flushes any internal buffers to their destination.
    fn flush(&mut self) -> std::io::Result<()>;
}

// Implement `Stage` for `Box<dyn Stage>` to allow for dynamic dispatch.
impl<T: Stage + ?Sized> Stage for Box<T> {
    fn id(&self) -> &str {
        (**self).id()
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        (**self).process(packet, ctx)
    }

    fn reconfigure(
        &mut self,
        config: &serde_json::Value,
        ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        (**self).reconfigure(config, ctx)
    }

    fn produce(
        &mut self,
        ctx: &mut StageContext,
    ) -> Result<Option<Vec<(String, Arc<RtPacket>)>>, StageError> {
        (**self).produce(ctx)
    }

    fn control(
        &mut self,
        cmd: &ControlCommand,
        ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        (**self).control(cmd, ctx)
    }

    fn shutdown(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        (**self).shutdown(ctx)
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        (**self).as_drains()
    }
}
