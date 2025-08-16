//! Basic pipeline example demonstrating the new, simplified architecture.

use std::sync::Arc;
use pipeline::allocator::{PacketAllocator, RecycledF32Vec};
use eeg_types::SensorMeta;
use pipeline::data::{RtPacket, PacketData, PacketHeader};
use pipeline::config::StageConfig;
use pipeline::control::PipelineEvent;
use pipeline::stage::{Stage, StageContext, StageInitCtx};
use pipeline::stages::to_voltage::ToVoltageFactory;
use pipeline::error::StageError;
use flume as mpsc;
use pipeline::registry::StageFactory;

// A simple stage that doubles each sample.
struct DoublerStage;

impl Stage for DoublerStage {
    fn id(&self) -> &str {
        "DoublerStage"
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        if let RtPacket::Voltage(packet_data) = &*packet {
            let mut new_samples = RecycledF32Vec::new(ctx.allocator.clone());
            new_samples.extend(packet_data.samples.iter().map(|s| s * 2.0));
            let new_packet_data = PacketData {
                header: packet_data.header.clone(),
                samples: new_samples,
            };
            Ok(vec![("out".to_string(), Arc::new(RtPacket::Voltage(new_packet_data)))])
        } else {
            Err(StageError::BadConfig(
                "Expected RtPacket::Voltage".to_string(),
            ))
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Basic Pipeline Example");
    println!("======================");

    // 1. Create a test packet of raw ADC counts
    let sensor_meta = Arc::new(SensorMeta {
        sensor_id: 0,
        meta_rev: 1,
        schema_ver: 2,
        source_type: "mock".to_string(),
        v_ref: 2.5,
        adc_bits: 24,
        gain: 1.0,
        sample_rate: 250,
        offset_code: 0,
        is_twos_complement: true,
        channel_names: vec!["ch0".to_string(), "ch1".to_string(), "ch2".to_string(), "ch3".to_string()],
        #[cfg(feature = "meta-tags")]
        tags: Default::default(),
    });

    let allocator = Arc::new(PacketAllocator::with_capacity(1, 1, 1, 4));
    let samples = vec![1000i32, 2000, -1000, -2000];
    let input_packet = Arc::new(RtPacket::RawI32(PacketData {
        header: PacketHeader {
            source_id: "test_source".to_string(),
            packet_type: "RawI32".to_string(),
            frame_id: 0,
            ts_ns: 0,
            batch_size: 4,
            num_channels: 4,
            meta: sensor_meta.clone(),
        },
        samples,
    }));
    if let RtPacket::RawI32(d) = &*input_packet {
        println!("Input Samples: {:?}", d.samples);
    }


    // 2. Instantiate the stages
    let (event_tx, _) = mpsc::unbounded::<PipelineEvent>();
    let init_ctx = StageInitCtx {
        event_tx: &event_tx,
        allocator: &allocator,
        driver: &None,
        sample_rate: 250.0,
        websocket_sender: None,
    };
    let (mut to_voltage_stage, _) = ToVoltageFactory::default().create(&StageConfig {
        name: "to_voltage".to_string(),
        stage_type: "ToVoltage".to_string(),
        params: Default::default(),
        inputs: Default::default(),
        outputs: vec![],
        channel_capacity: None,
    }, &init_ctx)?;
    let mut doubler_stage = DoublerStage;
    let mut ctx = StageContext::new(event_tx, allocator.clone());

    // 3. Manually process the packet through the pipeline
    // Stage 1: Convert ADC counts to voltage
    let voltage_output = to_voltage_stage.process(input_packet, &mut ctx)?;
    let (_, voltage_packet) = voltage_output.into_iter().next().unwrap();
    if let RtPacket::Voltage(d) = &*voltage_packet {
        println!("After ToVoltage: {:?}", d.samples);
    }

    // Stage 2: Double the voltage values
    let final_output = doubler_stage.process(voltage_packet, &mut ctx)?;
    let (_, final_packet) = final_output.into_iter().next().unwrap();
    if let RtPacket::Voltage(d) = &*final_packet {
        println!("After Doubler:   {:?}", d.samples);
    }

    println!("\nExample completed successfully!");

    Ok(())
}