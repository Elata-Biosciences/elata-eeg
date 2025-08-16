# EEG Pipeline Refactor: Implementation Plan

This document outlines the detailed plan to refactor the EEG data pipeline, based on the principles in `pipeline_integrate.md` and our subsequent architectural discussions.

## 1. Overview & Goals

The primary goal is to decouple the core components of the application for improved testability, robustness, and maintainability.

- **`sensors` crate:** Will become a fully synchronous library responsible only for low-level hardware I/O. It will have no knowledge of `tokio`.
- **`pipeline` crate:** Will be a synchronous data processing engine, consuming data and control commands and running stages. It will not have any networking code.
- **`device` crate:** Will be the sole asynchronous orchestrator, managing network connections (WebSockets), owning the system configuration, and bridging the gap between the async network world and the sync sensor/pipeline world.

## 2. Target Architecture

```mermaid
graph TD
    subgraph "Client / Network"
        WS[WebSocket Client]
    end

    subgraph "Device Crate (Tokio Runtime)"
        direction LR
        A[WebSocket Server] -- JSON --> B(Control & Config Handler);
        B -- "ControlCommand" --> F;
        C(Bridge Task) -- "std::thread::spawn" --> D["Sensor Thread (blocking)"];
        D -- "BridgeMsg" --> E[tokio::mpsc::channel];
    end

    subgraph "Pipeline Crate (Sync Context)"
        direction LR
        F(Control Channel rx);
        G(Data Channel rx);
        H[pipeline::run()];
        I(DSP & Sink Stages);

        H -- reads --> F;
        H -- reads --> G;
        G -- data --> I;
    end

    subgraph "Sensor Crate (Sync Context)"
        direction LR
        K[Sensor Driver];
        L[acquire() loop];
        K -- contains --> L;
    end

    WS -- connects to --> A;
    D -- calls --> L;
    E -- sends to --> G;

    style "Device Crate (Tokio Runtime)" fill:#D6EAF8,stroke:#3498DB
    style "Pipeline Crate (Sync Context)" fill:#D5F5E3,stroke:#2ECC71
    style "Sensor Crate (Sync Context)" fill:#FDEDEC,stroke:#E74C3C
```

## 3. Core Type Definitions

These are the key data structures that will enable the decoupled architecture.

```rust
// In `pipeline` crate
pub struct SystemConfig {
    // ... fields for sample rate, channel configs, etc.
    // Uses #[serde(default)] for forward-compatibility
}

pub enum ControlCommand {
    Pause,
    Resume,
    Shutdown,
    Reconfigure(SystemConfig),
}

// In `device` crate (or a shared types crate)
#[derive(Debug)]
pub enum SensorError {
    HardwareFault(String),
    BufferOverrun,
    Disconnected,
}

#[derive(Debug)]
pub enum BridgeMsg {
    Data(Packet<i32>),
    Error(SensorError),
    // Can be extended with other events like `Started`, `Stopped`
}
```

## 4. Key Implementation Patterns

### 4.1. Sensor `acquire()` Loop (in `sensors` crate)

The core of the synchronous driver. It blocks waiting for a hardware interrupt but with a timeout to remain responsive to shutdown requests.

```rust
// In Ads1299Driver::acquire

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

pub fn acquire(
    &mut self,
    tx: Sender<BridgeMsg>,
    stop_flag: &AtomicBool,
) -> Result<(), SensorError> {
    let drdy_pin = self.get_drdy_pin(); // Get the GPIO pin

    while !stop_flag.load(Ordering::Relaxed) {
        // Block this thread waiting for an interrupt, but with a timeout.
        match drdy_pin.poll_interrupt(Some(Duration::from_millis(100))) {
            Ok(Some(_)) => {
                // IRQ fired, read the data
                let frame = self.read_data_frame()?;
                if tx.send(BridgeMsg::Data(frame)).is_err() {
                    // Receiver has hung up, so we stop.
                    break;
                }
            }
            Ok(None) => {
                // Timeout occurred, loop again to check stop_flag
                continue;
            }
            Err(e) => {
                // GPIO error, propagate it and stop.
                tx.send(BridgeMsg::Error(SensorError::HardwareFault(e.to_string()))).ok();
                return Err(SensorError::HardwareFault(e.to_string()));
            }
        }
    }
    Ok(())
}
```

### 4.2. Device Bridge Task (in `device` crate)

This is the glue between the sync and async worlds.

```rust
// In `device` crate main logic

let (bridge_tx, mut pipeline_rx) = tokio::sync::mpsc::channel::<BridgeMsg>(1024);
let (std_tx, std_rx) = std::sync::mpsc::channel::<BridgeMsg>();

// 1. The dedicated, blocking sensor thread
let stop_flag = Arc::new(AtomicBool::new(false));
let sensor_thread_stop_flag = stop_flag.clone();
let mut sensor_driver = create_driver(); // Your driver instance
let sensor_thread = std::thread::spawn(move || {
    sensor_driver.acquire(std_tx, &sensor_thread_stop_flag)
});

// 2. The async bridge task that forwards messages
tokio::spawn(async move {
    while let Ok(msg) = std_rx.recv() {
        if bridge_tx.send(msg).await.is_err() {
            // Pipeline receiver has been dropped, stop forwarding.
            break;
        }
    }
});

// 3. The pipeline now consumes from `pipeline_rx`
// ...
```

## 5. Detailed Task List

### Phase 1: Synchronize `sensors` Crate
-   **1.1: Shim:** In the `AdcDriver` trait, add the new synchronous `acquire()` method signature, keeping the old async methods for now.
-   **1.2: Implement:** Implement the new `acquire()` method in `MockDriver` and `Ads1299Driver`. The implementation must include a **timeout on the blocking IRQ wait** and check an `AtomicBool` stop-flag on each loop.
-   **1.3: Bridge:** In the `device` crate, spawn a `std::thread` to run the driver. This bridge will use a channel to send a `enum BridgeMsg { Data(Packet), Err(SensorError) }` back to the Tokio runtime.
-   **1.4: Cleanup:** Once the `device` crate is using the new bridge, delete the old async methods from the `AdcDriver` trait and remove the `tokio` dependency from `sensors/Cargo.toml`.

### Phase 2: Solidify `pipeline` Crate
-   **2.1: Control Plane:** Define a versioned, strongly-typed `SystemConfig` struct. Define the `ControlCommand` enum for runtime instructions.
-   **2.2: Execution Loop:** Implement the main `pipeline::run()` function using `crossbeam_channel::select!` to react to both incoming `BridgeMsg` and `ControlCommand` messages.
-   **2.3: Graceful Shutdown:** Implement the bidirectional shutdown handshake: `device` sends `Shutdown`, `pipeline` drains and replies `Ack`, then `device` joins the sensor thread.

### Phase 3: Integrate & Refine `device` Crate
-   **3.1: Configuration:** The `device` WebSocket handler will own the configuration lifecycle, parsing incoming JSON into the typed `SystemConfig` to hot-reload the pipeline.
-   **3.2: Error Handling:** The `device` crate will handle the `BridgeMsg::Err` variant by logging the error and forwarding a structured error message to the UI.
-   **3.3: Wiring:** Connect the bridge and control channels to the `pipeline` instance upon application startup.

### Phase 4: Validation & Documentation
-   **4.1: Integration Test:** Write a full-stack test that sends a WebSocket command and asserts a state change in a pipeline stage.
-   **4.2: Shutdown Test:** Write an integration test that verifies the shutdown handshake completes, the sensor thread joins cleanly, and hardware resources are released (mocked).
-   **4.3: Documentation:** Update the `ai_prompt.md` files in the `device`, `sensors`, and `pipeline` crates to reflect the final architecture.