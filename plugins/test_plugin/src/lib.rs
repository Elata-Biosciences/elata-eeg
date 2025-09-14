use anyhow::Result;
use pipeline::stage::{Stage, StageContext};
use pipeline::data::{RtPacket, PacketData};
use pipeline::error::StageError;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct TestStage {
    id: String,
}

impl TestStage {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
        }
    }
}

impl Stage for TestStage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        Ok(vec![("out".into(), packet)])
    }
}