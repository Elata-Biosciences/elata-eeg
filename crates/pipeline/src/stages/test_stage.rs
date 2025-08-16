// crates/pipeline/src/stages/test_stage.rs

//! A simple pipeline stage for testing state changes via control commands.

use crate::control::{ControlCommand, PipelineEvent};
use crate::data::RtPacket;
use crate::error::StageError;
use crate::stage::{Stage, StageContext};
use std::sync::Arc;

/// A pipeline stage with an internal, modifiable state for testing.
#[derive(Debug)]
pub struct StatefulTestStage {
    id: String,
    state: u32,
}

impl StatefulTestStage {
    // The `new` function now accepts an `id` to align with the test code.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            state: 0,
        }
    }
}

// This stage is a simple pass-through. Its main purpose is to test control-plane functionality.
// We use `serde_json::Value` as a generic data container.
impl Stage for StatefulTestStage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        // Pass data through unmodified
        Ok(vec![("out".to_string(), packet)])
    }

    fn control(
        &mut self,
        cmd: &ControlCommand,
        ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        if let ControlCommand::SetTestState(new_state) = cmd {
            self.state = *new_state;
            ctx.emit_event(PipelineEvent::TestStateChanged(self.state))?;
        }
        Ok(())
    }
}