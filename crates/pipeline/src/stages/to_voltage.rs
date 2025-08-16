//! Converts raw i32 ADC samples into f32 voltage values.

use crate::allocator::RecycledF32Vec;
use crate::config::StageConfig;
use crate::data::{PacketData, PacketView, RtPacket};
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext, StageInitCtx};
use flume::Receiver;
use sensors::ads1299::helpers::ch_raw_to_voltage;
use std::sync::Arc;

/// A factory for creating `ToVoltage` stages.
#[derive(Default)]
pub struct ToVoltageFactory;

impl StageFactory for ToVoltageFactory {
    fn create(
        &self,
        config: &StageConfig,
        _: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        Ok((
            Box::new(ToVoltage::new(config.name.clone(), config.outputs.clone())),
            None,
        ))
    }
}

pub struct ToVoltage {
    id: String,
    output_name: String,
}

impl ToVoltage {
    pub fn new(id: String, outputs: Vec<String>) -> Self {
        let output_name = outputs.get(0).cloned().unwrap_or_else(|| "out".to_string());
        Self { id, output_name }
    }
}

impl Stage for ToVoltage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        pkt: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        let result_packet = match PacketView::from(&*pkt) {
            PacketView::RawI32 { header, data } => {
                // tracing::debug!(
                //     stage_id = self.id(),
                //     packet_type = "RawI32",
                //     "Converting RawI32 packet to Voltage"
                // );
                let mut voltage_samples =
                    RecycledF32Vec::with_capacity(ctx.allocator.clone(), data.len());

                for &raw_sample in data.iter() {
                    let voltage =
                        ch_raw_to_voltage(raw_sample, header.meta.v_ref, header.meta.gain);
                    voltage_samples.push(voltage);
                }

                let mut output_packet = PacketData {
                    header: header.clone(),
                    samples: voltage_samples,
                };
                output_packet.header.source_id = format!("{}.{}", self.id, self.output_name);
                output_packet.header.packet_type = "Voltage".to_string();

                Arc::new(RtPacket::Voltage(output_packet))
            }
            // If the packet is not RawI32, pass it through.
            _ => pkt,
        };
        Ok(vec![(self.output_name.clone(), result_packet)])
    }
}
