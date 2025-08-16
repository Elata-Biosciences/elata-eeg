//! A test demonstrating a multi-stage synchronous pipeline.

use pipeline::allocator::{PacketAllocator, RecycledF32Vec};
use pipeline::config::{StageConfig, SystemConfig};
use eeg_types::SensorMeta;
use pipeline::data::{PacketData, PacketHeader, RtPacket};
use pipeline::control::{PipelineEvent};
use pipeline::error::StageError;
use pipeline::executor::Executor;
use pipeline::graph::PipelineGraph;
use pipeline::registry::{StageFactory, StageRegistry};
use pipeline::stage::{Stage, StageContext, StageInitCtx};
use serde_json::json;
use flume::{self as mpsc, Receiver, Sender};
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

// A simple stage that multiplies every sample by a given factor.
struct MultiplierStage {
    id: String,
    factor: f32,
}

#[derive(Default)]
struct MultiplierStageFactory;

impl StageFactory for MultiplierStageFactory {
    fn create(
        &self,
        config: &StageConfig,
        _init_ctx: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let factor = config
            .params
            .get("factor")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .ok_or_else(|| StageError::BadConfig("Missing 'factor' param".to_string()))?;

        Ok((
            Box::new(MultiplierStage {
                id: config.name.clone(),
                factor,
            }),
            None,
        ))
    }
}

impl Stage for MultiplierStage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        info!("MultiplierStage processing packet");
        if let RtPacket::Voltage(packet_data) = &*packet {
            let mut new_samples = RecycledF32Vec::new(ctx.allocator.clone());
            new_samples.extend(packet_data.samples.iter().map(|s| s * self.factor));

            let new_packet_data = PacketData {
                header: packet_data.header.clone(),
                samples: new_samples,
            };
            Ok(vec![(
                "out".to_string(),
                Arc::new(RtPacket::Voltage(new_packet_data)),
            )])
        } else {
            Err(StageError::BadConfig(
                "Expected RtPacket::Voltage".to_string(),
            ))
        }
    }
}

// A sink stage to capture the final output for verification.
struct TestSink {
    id: String,
    output_tx: Sender<usize>, // Send back the number of samples received
}

#[derive(Default)]
struct TestSinkFactory {
    // The factory will be configured with the sender at runtime.
    output_tx: Arc<std::sync::Mutex<Option<Sender<usize>>>>,
}

impl StageFactory for TestSinkFactory {
    fn create(
        &self,
        config: &StageConfig,
        _init_ctx: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let tx = self
            .output_tx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| StageError::BadConfig("output_tx already taken".to_string()))?;

        Ok((
            Box::new(TestSink {
                id: config.name.clone(),
                output_tx: tx,
            }),
            None,
        ))
    }
}

impl Stage for TestSink {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        if let RtPacket::Voltage(packet_data) = &*packet {
            self.output_tx.send(packet_data.samples.len()).unwrap();
        }
        Ok(vec![]) // Sinks consume the packet
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Full Synchronous Pipeline Test");
    info!("==============================");

    // 1. Set up the stage registry
    let mut registry = StageRegistry::new();
    registry.register(
        "Acquire",
        Box::new(pipeline::stages::acquire::AcquireFactory::default()),
    );
    registry.register(
        "ToVoltage",
        Box::new(pipeline::stages::to_voltage::ToVoltageFactory::default()),
    );
    registry.register("Multiplier", Box::new(MultiplierStageFactory::default()));

    // The TestSink needs to communicate the result count back to the main thread.
    let (output_tx, output_rx) = mpsc::unbounded::<usize>();
    let sink_factory = TestSinkFactory {
        output_tx: Arc::new(std::sync::Mutex::new(Some(output_tx))),
    };
    registry.register("TestSink", Box::new(sink_factory));

    // 2. Define the pipeline configuration
    let config: SystemConfig = serde_json::from_value(json!({
        "version": "1.0",
        "stages": [
            {
                "name": "acquire",
                "type": "Acquire",
                "params": { "samples_per_packet": 4 }
            },
            {
                "name": "to_voltage",
                "type": "ToVoltage",
                "inputs": ["acquire"]
            },
            {
                "name": "multiplier",
                "type": "Multiplier",
                "inputs": ["to_voltage"],
                "params": { "factor": 10.0 }
            },
            {
                "name": "test_sink",
                "type": "TestSink",
                "inputs": ["multiplier"]
            }
        ]
    }))?;

    // 3. Build the pipeline graph
    let (event_tx, _event_rx) = mpsc::unbounded::<PipelineEvent>();
    let test_allocator = Arc::new(PacketAllocator::with_capacity(16, 16, 16, 1024));
    let graph =
        PipelineGraph::build(&config, &registry, event_tx, Some(test_allocator.clone()), &None, None)?;

    // 4. Create and start the executor
    let (executor, _, _control_bus, mut producer_txs) = Executor::new(graph);
    let input_tx = producer_txs.remove("eeg_source").unwrap();

    // 5. Send a large number of packets into the pipeline
    let num_packets = 1_000;
    let samples_per_packet = 4;
    let total_samples_sent = num_packets * samples_per_packet;

    info!(
        "Sending {} packets with {} samples each (total {} samples)...",
        num_packets, samples_per_packet, total_samples_sent
    );

    for i in 0..num_packets {
        let samples = vec![1000i32, 2000, -1000, -2000];
        let packet = RtPacket::RawI32(PacketData {
            header: PacketHeader {
                source_id: "acquire".to_string(),
                packet_type: "RawI32".to_string(),
                frame_id: i as u64,
                ts_ns: i as u64,
                batch_size: samples_per_packet as u32,
                num_channels: 4,
                meta: Arc::new(SensorMeta::default()),
            },
            samples,
        });
        input_tx.send(Arc::new(packet))?;
    }

    // 6. Wait for the final output from the TestSink
    let mut total_samples_received = 0;
    for _ in 0..num_packets {
        total_samples_received += output_rx.recv()?;
    }

    info!("\nTotal samples received: {}", total_samples_received);
    assert_eq!(total_samples_sent, total_samples_received);

    // 7. Shut down the pipeline
    info!("Shutting down pipeline...");
    executor.stop();

    info!("\nTest completed successfully!");
    Ok(())
}