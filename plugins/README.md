# EEG Daemon Plugin Architecture

This document outlines the standardized architecture for creating and integrating plugins into the Elata EEG Daemon. Adhering to this architecture ensures that the system remains modular, performant, and easy to maintain.

## 1. Core Philosophy: Event-Driven & Parallel

The daemon's plugin system is built on a high-performance, event-driven architecture. This design is crucial for handling real-time EEG data efficiently, especially on multi-core embedded systems like the Raspberry Pi 5.

-   **Decoupled Components:** Plugins do not call each other directly. Instead, they communicate by broadcasting and subscribing to events on a central `EventBus`.
-   **Parallel Execution:** Each plugin runs in its own dedicated asynchronous task. This allows the system to leverage all available CPU cores, ensuring that a slow or CPU-intensive plugin does not block the real-time data acquisition loop.
-   **Zero-Copy Data Handling:** The `EventBus` passes around pointers to data (`Arc<T>`) rather than copying the data itself. This is extremely efficient, minimizing memory usage and CPU cycles.

### Architecture Diagram

```mermaid
graph TD
    subgraph "Device Crate (Running on Pi 5)"
        subgraph "Core 1"
            A[Acquisition Task] -- RawEeg Event --> B((Event Bus));
        end
        subgraph "Core 2"
            B -- RawEeg Event --> C[Voltage Filter Plugin Task];
            C -- FilteredEeg Event --> B;
        end
        subgraph "Core 3"
            B -- FilteredEeg Event --> D[Brain Wave Plugin Task];
            D -- FftEvent --> B;
        end
        subgraph "Core 4"
            B -- RawEeg & FilteredEeg & FftEvent --> E[WebSocket & Recorder Tasks];
        end
        F[Plugin Supervisor] -- Manages Lifecycle --> C;
        F -- Manages Lifecycle --> D;
    end

    subgraph "External"
        E -- Data Stream --> G(Kiosk GUI);
    end

    style `Device Crate (Running on Pi 5)` fill:#f9f,stroke:#333,stroke-width:2px
    style External fill:#ccf,stroke:#333,stroke-width:2px
```

## 2. The Standard: External Crate Dependencies

All plugins are developed as **separate, external crates** located within this `plugins/` directory. They are then integrated into the main `device` crate at compile time.

This approach provides a clean separation of concerns, allowing plugins to be developed and tested independently.

## 3. How to Create a New Plugin

To create a new plugin (e.g., `my_awesome_plugin`), follow these steps:

1.  **Create the Crate:**
    *   Use `cargo new --lib plugins/my_awesome_plugin` to create a new library crate inside the `plugins` directory.

2.  **Define the Plugin Struct:**
    *   In your new plugin's `src/lib.rs`, define your main plugin struct. It must derive `Clone`.
    *   Implement a parameterless `new()` function that sets up any default state.

    ```rust
    use eeg_types::plugin::EegPlugin;
    
    #[derive(Clone)]
    pub struct MyAwesomePlugin {
        // ... your plugin's state ...
    }

    impl MyAwesomePlugin {
        pub fn new() -> Self {
            Self { /* ... default state ... */ }
        }
    }
    ```

3.  **Implement the `EegPlugin` Trait:**
    *   This is the core contract for all plugins.
    *   You must implement `name`, `clone_box`, and `run`.
    *   The `run` method is where your plugin's main logic lives. It receives events from the bus and can broadcast new events back out.

    ```rust
    use async_trait::async_trait;
    use eeg_types::plugin::{EegPlugin, EventBus};
    use eeg_types::event::SensorEvent;
    use anyhow::Result;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[async_trait]
    impl EegPlugin for MyAwesomePlugin {
        fn name(&self) -> &'static str {
            "my_awesome_plugin"
        }

        fn clone_box(&self) -> Box<dyn EegPlugin> {
            Box::new(self.clone())
        }

        async fn run(
            &mut self,
            bus: Arc<dyn EventBus>,
            mut receiver: broadcast::Receiver<SensorEvent>,
            shutdown_token: CancellationToken,
        ) -> Result<()> {
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_token.cancelled() => break,
                    Ok(event) = receiver.recv() => {
                        // --- YOUR LOGIC HERE ---
                        // 1. Process the incoming event
                        // 2. Create a new event with your results
                        // 3. Broadcast it back to the bus
                        // bus.broadcast(your_new_event).await;
                    }
                }
            }
            Ok(())
        }
    }
    ```

4.  **Specify Event Subscriptions (Optional but Recommended):**
    *   Implement the `event_filter` method to tell the `EventBus` which events your plugin is interested in. This is a performance optimization that prevents your plugin's task from waking up unnecessarily.
    *   The `EegPacket` now contains both `raw_samples: Vec<i32>` and `voltage_samples: Vec<f32>`. Your plugin should subscribe only to the data it needs.

    ```rust
    use eeg_types::event::EventFilter;

    fn event_filter(&self) -> Vec<EventFilter> {
        // This plugin only wants to see raw EEG data.
        // It will not be woken up for FilteredEeg, FftEvent, etc.
        vec![EventFilter::RawEegOnly]
    }
    ```

## 4. How to Integrate the New Plugin

1.  **Add to `device/Cargo.toml`:**
    *   Open `crates/device/Cargo.toml` and add your new plugin as a dependency:

    ```toml
    [dependencies]
    # ... other dependencies
    my_awesome_plugin = { path = "../../plugins/my_awesome_plugin" }
    ```

2.  **Register in `device/src/main.rs`:**
    *   Add your plugin to the plugin supervisor initialization section.

    ```rust
    // In main.rs, in the plugin supervisor initialization section
    plugin_supervisor.add_plugin(Box::new(my_awesome_plugin::MyAwesomePlugin::new()));
    ```

After these steps, a recompile of the `device` crate will include your new plugin, and the `PluginSupervisor` will automatically start and manage it.

# Considerations
1 Metrics plugin
 - Publish BusStats { queue_len, drops, max_latency } every second. Surface it in the GUI.