# Pipeline Stage Code Example (Primary Design Document)

This document is the **primary source of truth** for the pipeline's design, showing concrete code examples. The `unified-pipeline-architecture.md` document provides the higher-level overview.

Here, we illustrate how stages are implemented and how core concepts like memory management, error handling, and pipeline control work in practice.

## New code exmaple
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
use serde_json::Value;

#[derive(thiserror::Error, Debug)]
pub enum StageError {
    #[error("queue closed")]
    QueueClosed,
    #[error("bad param {0}")]
    BadParam(String),
    #[error("fatal hw error: {0}")]
    Fatal(String),
}

pub struct LowPass {
    coeffs: Vec<f32>,
    enabled: AtomicBool, // toggled live
}

#[async_trait]
impl DataPlaneStage for LowPass {
    async fn run(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        // ---- control messages ----
        while let Ok(msg) = ctx.control_rx.try_recv() {
            match msg {
                ControlMsg::Pause => { self.enabled.store(false, Ordering::Relaxed); }
                ControlMsg::Resume => { self.enabled.store(true,  Ordering::Relaxed); }
                ControlMsg::UpdateParam(k,v) => self.update_param(&k, v)?,
            }
        }

        // ---- fast path ----
        let mut pkt = match ctx.inputs["in"].recv().await? {
            Some(p) => p,
            None    => return Ok(()), // idle
        };

        if self.enabled.load(Ordering::Relaxed) {
            // FIR in-place
            for s in &mut pkt.samples {
                *s = *s * self.coeffs[0];            // demo
            }
        }

        ctx.outputs["out"].send(pkt).await?;
        Ok(())
    }
}

impl LowPass {
    fn update_param(&mut self, key: &str, val: Value) -> Result<(), StageError> {
        match key {
            "enabled" => {
                self.enabled.store(val.as_bool().unwrap_or(true), Ordering::Relaxed);
            }
            "coeffs" => {
                let v: Vec<f32> = serde_json::from_value(val)
                    .map_err(|_| StageError::BadParam(key.into()))?;
                if v.is_empty() {                 // example fatal error
                    return Err(StageError::Fatal("empty coeffs".into()));
                }
                self.coeffs = v;
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }
}
```


## 1. Core Types

First, let's assume we have these core types defined, as discussed in the implementation plan:

```rust
// A smart pointer that contains a header and a buffer, and automatically returns
// its buffer to the MemoryPool on Drop.
pub struct Packet<T> {
    pub header: PacketHeader,
    pub payload: T,
}

// The header contains metadata about the packet.
pub struct PacketHeader {
    pub batch_size: usize,
    pub timestamp: u64,
    // etc.
}

// ### A Note on Lifetimes and Safety
// While the *buffer* managed by the pool has a `'static` lifetime, the `Packet<T>`
// smart pointer itself does not. It acts as a temporary lease. This design is what
// allows the packet to be `Send` and `Sync`, as the lease can be safely transferred
// between threads, but the underlying buffer is guaranteed by the pool's logic
// to only have one active user at a time.

// The trait for receiving a packet from an upstream stage.
#[async_trait]
pub trait Input<T> {
    // Distinguishes between an empty queue and a closed one.
    async fn recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
}

// The trait for sending a packet to a downstream stage.
#[async_trait]
pub trait Output<T> {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), StageError>;
}

// The main trait for a data plane stage.
#[async_trait]
pub trait DataPlaneStage {
    async fn run(&mut self, context: &mut StageContext) -> Result<(), StageError>;
}

// A message sent from the Control Plane to a specific stage.
// Note the use of a lifetime to allow for zero-copy parameter updates.
pub enum ControlMsg<'a> {
    Pause,
    Resume,
    UpdateParam(String, serde_json::Value),
    UpdateCoefficients(&'a [f32]),
}

// Contains everything a stage needs to run.
pub struct StageContext {
    // Access to named MemoryPools, configured in the pipeline's JSON file.
    // This allows stages to request packets of specific, pre-configured sizes.
    pub memory_pools: HashMap<String, Arc<MemoryPool>>,
    // The stage's inputs and outputs.
    pub inputs: HashMap<String, Box<dyn Input<...>>>,
    pub outputs: HashMap<String, Box<dyn Output<...>>>,
    // A channel to receive control messages from the main runtime.
    pub control_rx: mpsc::Receiver<ControlMsg<'static>>,
}

// Example data packets.
pub struct RawEegPacket { pub samples: Vec<i32> }
pub struct VoltageEegPacket { pub samples: Vec<f32> }
```

## 2. Example Stage 1: `Acquisition` (The "Interrupt" Source)

This stage is the entry point for data. It's "data-driven" by an external source (the hardware) and shows how the **pipeline lock/pause** mechanism works.

```rust
struct Acquisition {
    // This stage's state. Is it currently paused by the control plane?
    is_paused: bool,
    // A handle to the physical hardware driver.
    driver: EegDriver,
}

#[async_trait]
impl DataPlaneStage for Acquisition {
    async fn run(&mut self, context: &mut StageContext) -> Result<(), StageError> {
        // --- Step 1: Handle Control Messages ---
        // First, check for non-blocking control messages (e.g., Pause, Resume).
        // This allows the control plane to "lock" the pipeline at its source.
        match context.control_rx.try_recv() {
            Ok(ControlMsg::Pause) => self.is_paused = true,
            Ok(ControlMsg::Resume) => self.is_paused = false,
            _ => {} // No message or channel empty, just continue.
        }

        if self.is_paused {
            // If paused, do nothing. The pipeline is now "locked".
            return Ok(());
        }

        // --- Step 2: Wait for Data ---
        // This is the "interrupt wait". .await here yields control until the
        // hardware driver has a full batch of data ready.
        let raw_data = self.driver.wait_for_data().await;

        // --- Step 3: Process Data ---
        // Acquire a packet from the pool named "raw_packets", which is
        // defined in the pipeline's JSON configuration.
        // Acquire a packet using the non-blocking `try_acquire`.
        // This allows the stage to handle pool exhaustion gracefully.
        let mut output_packet = match context.memory_pools
            .get("raw_packets").expect("Memory pool 'raw_packets' not found")
            .try_acquire::<RawEegPacket>()
        {
            Some(packet) => packet,
            // If the pool is empty, we can choose to drop the data or wait.
            // Here, we'll just skip this cycle.
            None => return Ok(()),
        };
        
        // Populate the payload and the header.
        output_packet.payload.samples = raw_data;
        output_packet.header.batch_size = output_packet.payload.samples.len();
        output_packet.header.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        // --- Step 4: Send Data Downstream ---
        // This push is what "triggers" the rest of the pipeline.
        context.outputs.get_mut("output_raw")
            .expect("Output 'output_raw' not connected")
            .send(output_packet).await?;

        Ok(())
    }
}
```

## 3. Example Stage 2: `RawToVoltage`

This stage consumes a `RawEegPacket` and produces a `VoltageEegPacket`. Since it's creating a new *type* of packet, it needs to interact with a different `MemoryPool`.

```rust
struct RawToVoltageConverter {
    scale_factor: f32,
}

#[async_trait]
impl DataPlaneStage for RawToVoltageConverter {
    async fn run(&mut self, context: &mut StageContext) -> Result<(), StageError> {
        // --- Step 1: Receive an input packet ---
        // The .run() method is called repeatedly by the data plane's run loop.
        // .recv().await will efficiently wait if the input queue is empty,
        // yielding control so other stages can run. It doesn't burn CPU.
        // If the queue is empty right now, we just return and let the loop call us again later.
        let input_packet = match context.inputs.get_mut("input_raw")
            .expect("Input 'input_raw' not connected")
            .recv().await? {
            Some(packet) => packet,
            // This is the normal, expected case when the pipeline is idle.
            None => return Ok(()), 
        };

        // --- Step 2: Acquire a NEW, empty packet for our output ---
        // This stage transforms data, so it needs a new packet from a different pool.
        // The name "voltage_packets" is defined in the pipeline config, allowing
        // for different memory configurations without changing this code.
        let mut output_packet = context.memory_pools
            .get("voltage_packets").expect("Memory pool 'voltage_packets' not found")
            .acquire::<VoltageEegPacket>().await;

        // --- Step 3: Process the data ---
        // Copy the header from the input packet, as we are not changing its shape.
        output_packet.header = input_packet.header;

        // Process the data
        output_packet.payload.samples = input_packet.payload.samples
            .iter()
            .map(|&s| s as f32 * self.scale_factor)
            .collect();
        
        // If we were downsampling, we would update the header's batch_size here.
        // output_packet.header.batch_size = output_packet.payload.samples.len();

        // --- Step 4: Send the new packet downstream ---
        context.outputs.get_mut("output_voltage")
            .expect("Output 'output_voltage' not connected")
            .send(output_packet).await?;

        // --- Step 5: Automatic Memory Management ---
        // `input_packet` goes out of scope here. Its `Drop` implementation automatically
        // returns its memory buffer to its original pool. No manual free/release needed.

        Ok(())
    }
}
```

## 4. Example Stage 3: `InPlaceFirFilter`

This stage consumes a `VoltageEegPacket` and filters it *in-place*. It is the most efficient type of stage as it doesn't need to interact with the memory pool at all.

```rust
struct InPlaceFirFilter {
    coefficients: Vec<f32>,
    state: Vec<f32>, // Internal filter state
}

#[async_trait]
impl DataPlaneStage for InPlaceFirFilter {
    async fn run(&mut self, context: &mut StageContext) -> Result<(), StageError> {
        // --- Step 1: Receive an input packet ---
        // As before, we wait for a packet to arrive.
        let mut packet = match context.inputs.get_mut("input_voltage")
            .expect("Input 'input_voltage' not connected")
            .recv().await? {
            Some(packet) => packet,
            None => return Ok(()), // Queue is empty.
        };

        // --- Step 2: Process the data IN-PLACE ---
        // This is the most efficient operation. No new allocation is needed.
        // We are modifying the packet's data buffer directly.
        for sample in &mut packet.payload.samples {
            // (Simplified FIR filter logic)
            *sample = (*sample * self.coefficients[0]) + self.state[0];
            // ... update state ...
        }

        // --- Step 3: Send the MODIFIED packet downstream ---
        // The *exact same* packet we received is passed on. Ownership is transferred
        // to the next stage. This is the essence of the zero-copy design.
        context.outputs.get_mut("output_filtered")
            .expect("Output 'output_filtered' not connected")
            .send(packet).await?;

        // `packet` is "moved" into the send function. We no longer own it.
        // The next stage is now responsible for it. This prevents double-frees.

        Ok(())
    }
}
```

### Summary of Key Concepts

| Concept             | How it's handled                                                                                                                                                           |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Memory Allocation** | **Implicit.** You only call `memory_pools.get("pool_name").acquire()` if you are a "source" stage or need to create a new packet type. Otherwise, you just receive and pass on packets. |
| **Memory Release**    | **Automatic.** When a `Packet<T>` goes out of scope (like `input_packet` in the first example), its `Drop` implementation returns it to the pool. No manual `free()` calls. |
| **Error Handling**    | **Explicit `Result`s.** The `run` and `recv`/`send` methods all return `Result`. This forces the runtime to handle cases where a queue is disconnected or a stage fails.   |
| **Backpressure**      | **Implicit.** If a downstream queue is full, the `send()` call will block (or timeout), preventing the current stage from processing more data and naturally throttling the pipeline. |

This approach contains the complexity within the `Packet`, `MemoryPool`, and `StageQueue` implementations, allowing the stage developer to focus on their specific logic while still getting the benefits of a high-performance, zero-copy system.