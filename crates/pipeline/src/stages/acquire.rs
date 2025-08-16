//! Data acquisition stage for EEG sensors.

use crate::config::StageConfig;
use crate::data::RtPacket;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext, StageInitCtx};
use flume::Receiver;
use std::sync::Arc;
use tracing::debug;

/// A source stage that generates mock raw EEG data.
#[derive(Default)]
pub struct AcquireFactory;

impl StageFactory for AcquireFactory {
    fn create(
        &self,
        config: &StageConfig,
        _: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        Ok((Box::new(Acquire::new(config.name.clone())), None))
    }
}

pub struct Acquire {
    id: String,
}

impl Acquire {
    pub fn new(id: String) -> Self {
        Self { id }
    }
}

impl Stage for Acquire {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        // In this test setup, the Acquire stage is just a pass-through.
        // The test itself is the source of the data.
        debug!("Acquire stage passing packet through.");
        Ok(vec![("out".to_string(), packet)])
    }
}