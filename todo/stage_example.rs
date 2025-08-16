/* crates/pipeline/src/stages/to_voltage.rs (refactored version)
The macro handles all the boilerplate:
- Generates trait impls (DataPlaneStage, Erased*, Factory)
- Sets up lazy I/O, run loop, yielding, back-pressure
- Handles param schema/serde via the `params` struct
- `inventory::submit!` registration
*/
use crate::{
    data::{Packet, RawEegPacket, VoltageEegPacket},
    error::StageError,
    stage::{ControlMsg, StageContext, StageParams},
    stage_def,
};
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::trace;

stage_def! {
    name: ToVoltageStage,
    inputs: RawEegPacket,
    outputs: VoltageEegPacket,

    /// config
    params: {
        vref: f32 = 4.5,
        adc_bits: u8 = 24,
        yield_threshold: u32 = 64,
    },

    /// state
    fields: {
        scale: AtomicU32, // Pre-computed for the hot loop
    },

    /// init
    init: |params| {
        let max_value = (1 << (params.adc_bits - 1)) - 1;
        let scale = params.vref / max_value as f32;
        Self {
            // `scale` is from `fields`, the rest are from `params`
            scale: AtomicU32::new(scale.to_bits()),
        }
    },

    /// process data batch here
    process: |self, pkt: Packet<RawEegPacket>, ctx: &mut StageContext<_, _>| -> Result<Packet<VoltageEegPacket>, StageError> {
        let scale = f32::from_bits(self.scale.load(Ordering::Acquire));

        /// batch process
        let voltage_samples: Vec<f32> = pkt.samples.samples
            .iter()
            .map(|&raw| (raw as f32) * scale)
            .collect();

        // construct and return this
        Ok(Packet::new(
            pkt.header,
            VoltageEegPacket { samples: voltage_samples },
            pkt.pool(), // Re-using the memory pool handle
        ))
    },

    /// Optional custom logic for handling parameter updates from control messages.
    update_param: |self, key: &str, val: Value| -> Result<(), StageError> {
        match key {
            "vref" => {
                let new_vref = val.as_f64().map(|v| v as f32).ok_or_else(|| StageError::BadParam(key.into()))?;
                // Note: `self.params.adc_bits` would be available if the macro stores params.
                let max_value = (1 << (self.params.adc_bits - 1)) - 1;
                let new_scale = new_vref / max_value as f32;
                self.scale.store(new_scale.to_bits(), Ordering::Release);
                trace!("Updated scale to {}", new_scale);
            }
            // The macro could auto-handle updates for other params if they don't need custom logic.
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }
}