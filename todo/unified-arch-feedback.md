> **ARCHITECTURAL NOTE (2025-07-09):** This document contains the critical feedback that led to the project's pivot to a direct, ABI-first plugin architecture. The "bridge stage" and "coexistence" concepts discussed below are now **obsolete**.
>
> The suggestions in this document, particularly the `StageDescriptor` pattern, have been integrated into the primary architecture documents. Please refer to the following for the current, authoritative plan:
>
> *   **Canonical Architecture:** [`todo/unified-pipeline-architecture.md`](todo/unified-pipeline-architecture.md)
>   *   **Implementation Plan:** [`IMPLEMENTATION_CHECKLIST.md`](IMPLEMENTATION_CHECKLIST.md)

---

# Unified EEG Pipeline — Holistic Integration Roadmap

## 1  Current State Snapshot

* **Two worlds in one repo:** the *legacy* `PipelineRuntime` / `PipelineStage` graph that passes `PipelineData (Arc<[T]>)`, and the *new* **Hybrid Data‑Plane** built around `Packet<T>`, `MemoryPool`, and lock‑free queues.
* **Stage duplication:** key stages (`filter`, `gain`, `csv_sink`, etc.) exist both under `crates/pipeline/src/stages/` (new) and in `plugins/` (old), with different trait bounds.
* **Plugin loader & UI hooks** are designed but *not wired* (no `PluginManager`, dynamic import, or ABI enforcement yet).
* **Acquisition chain** still feeds the old graph; `ads1299` and `elata_emu_v1` have no `DataPlane` bridge.

## 2  High‑Level Integration Strategy

1. **Co‑existence first, cut over later.** Add *bridge* stages (`ToDataPlane`, `FromDataPlane`) so both runtimes run side‑by‑side. This lets you migrate a slice at a time and benchmark in prod.
2. **Isolate unsafe code.** Finalise `MemoryPool`+`Packet` in a standalone module with loom/miri tests before other work: the whole architecture leans on this.
3. **Hard‑gate plugins through a stable ABI.** Land the `pipeline‑abi` crate *early* so every plugin and host binary compiles against the same struct layouts.
4. **Single source of config truth.** Move all magic numbers (queue sizes, pool counts, stage params) to the new JSON schema so both graphs read from one file.

## 3  Phase Roadmap (60‑90 days)

| Phase               | Goal                     | Key Deliverables                                                                                                                           |
| ------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **P0 (Week 0‑2)**   | Safety floor & bridges   | • `MemoryPool` + tests<br>• `ToDataPlane` & `FromDataPlane`<br>• `HybridRuntime` skeleton with one thread                                  |
| **P1 (Week 3‑4)**   | Sensor ingress           | • `AcquisitionStage` wrapping `ads1299` driver<br>• `ToVoltageStage` producing `Packet<VoltageEeg>`                                        |
| **P2 (Week 5‑6)**   | Minimal vertical slice   | • Port `GainStage`, `WebSocketSink`, `CsvSink` to new trait<br>• Frontend bridge: existing UI reads Data‑Plane WS stream                   |
| **P3 (Week 7‑10)**  | Plugin system online     | • `pipeline‑abi` 1.0<br>• `PluginManager` with `libloading` + version check + `catch_unwind`<br>• `cargo xtask build‑plugins` and template |
| **P4 (Week 11‑13)** | Full migration & cleanup | • Refactor remaining legacy stages<br>• Remove bridge stages, old traits, dead code                                                        |

## 4  Immediate Next‑Actions

1. **Lock the API surface.** Draft the `AbiVersion`, `StageRegistrar`, `DataPlaneStage` traits and land them behind `#[cfg(feature="unstable-abi")]`; iterate until stable.
2. **Finish `MemoryPool` prototype** (crossbeam `ArrayQueue` + drop safety) and benchmark with synthetic 4 kSps×32‑ch load.
3. **Build `AcquisitionStage`** that:

   * Pulls 400‑sample frames from ADS1299 ISR.
   * Writes into `Packet<RawEeg>`, sets `batch_size` header, pushes to queue.
4. **Write bridge stages** to funnel existing `PipelineData::EegPacket` into/out of `Packet<*>`.
5. **Add tracing spans** (`acquire`, `send`, `recv`) so you can eyeball latency via `tokio‑console`.

## 5  Plugin & UI Hardening Checklist

* `plugin.toml` schema v1 (name, version, \[library], \[ui]).
* Static asset layout: `plugins/foo/bin/<triple>/libfoo.so`, `plugins/foo/ui/bundle.js`.
* Kiosk route `/static/plugins/:id/*` and discovery endpoint `/api/plugins`.
* React dynamic `import()`; each bundle exports `default` component.

## 6  Risk & Mitigation

| Risk                     | Impact              | Mitigation                                                |
| ------------------------ | ------------------- | --------------------------------------------------------- |
| UB in `unsafe` pool code | Data corruption     | Loom tests + strict invariants comments                   |
| Plugin panic             | Pipeline crash      | Wrap every call in `catch_unwind`, disable plugin on fail |
| ABI drift                | Loader segfault     | Major/minor check at `register_factories()`               |
| Queue sizing wrong       | Latency/buffer drop | Config‑driven + runtime `tracing` metrics                 |

## 7  Open Decision Points

* **Queue backend:** `rtrb` vs `crossbeam`. Prototype both, pick based on perf + API ergonomics.
* **Python / Wasm guest stages:** embed CPython now or defer until Data‑Plane stabilises?
* **Hot‑reload:** file‑watcher in dev only or prod too? Balance convenience vs. stability.

## 8  Ownership & Next Review

* **Responsible:** @core‑pipeline‑team
* **Dependencies:** ADS1299 driver stabilization, kiosk UI refactor.
* **Checkpoint:** Re‑evaluate after Phase P1 demo (Week 4).

## 9  Gaps & Show‑Stoppers to Watch

| Area                             | Concern                                                                                                  | Suggested Mitigation                                                                                                                  |
| -------------------------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| **ISR jitter → queue underrun**  | High‑rate ADS1299 IRQ bursts could starve the Data‑Plane thread if it shares a core with Tokio reactors. | Pin the Data‑Plane thread to an isolated core (`taskset`/`pthread_attr_setaffinity_np`) and benchmark worst‑case latency with `perf`. |
| **MemoryPool exhaustion**        | Pool sizing guess wrong → dropped frames or hard block.                                                  | Add runtime metrics (`pool.free`, `queue.len`) and a guard that logs + expands pool size when free < 5 %.                             |
| **Plugin binary trust**          | Loading un‑signed `.so` from internet could inject malware.                                              | Accept only plugins signed with a project key or run user plugins in Wasmtime sandbox.                                                |
| **Cross‑platform builds**        | Contributors on macOS/Windows may fail to cross‑compile CDylib for Pi (aarch64‑linux‑gnu).               | Provide pre‑configured `cross.toml` + GitHub Action that outputs artefacts for all targets.                                           |
| **Schema drift in config JSON**  | Stage params renamed → legacy configs fail silently.                                                     | Version the JSON schema; validate on load and emit explicit error list.                                                               |
| **CI coverage**                  | Loom/Miri tests can be slow and flaky → skipped.                                                         | Separate “fast” unit tests vs nightly “deep” concurrency suite; gate merges on both.                                                  |
| **Frontend dynamic import race** | UI may render before plugin bundle loads, showing blank panels.                                          | Add suspense fallback + retry logic; cache manifest on SW.                                                                            |

## 10  Nice‑to‑Haves (Not Blocking MVP)

* **Live re‑layout editor** for pipelines with drag‑and‑drop (could be GrapesJS in kiosk).
* **Time‑travel debugger**: record Packet headers + hash to replay a segment offline.
* **Auto‑tuner** that sweeps queue capacities & pool counts to hit latency target.
* **Pipeline presets**: YAML fragments for common tasks (e.g., "alpha‑band filter → FFT → heatmap").

---

*Last updated: 2025-07-09*

......
Excellent questions. These get to the heart of the practical implementation and operational aspects of the architecture. Let's address them one by one.

### 1. Plugin Workflow: `git clone` and `cargo build`

Yes, the "clone, build, run" workflow is a primary design goal and is absolutely possible with this architecture. The system is explicitly designed to avoid requiring a recompile of the main host application when adding a new plugin.

Here's how it works:
1.  **Stable ABI:** The `pipeline-abi` crate provides a stable contract between the host and plugins. As long as a plugin is compiled with a compatible version of this ABI, the host can load it.
2.  **Dynamic Loading:** The `PluginManager` component in the host is responsible for scanning the `plugins/` directory, finding the compiled dynamic libraries (`.so`, `.dll`, etc.), and loading them using `libloading`.
3.  **Registration:** Once loaded, the `PluginManager` calls the plugin's `register_factories` function, which the plugin uses to tell the host about the new stages it provides.

The edge cases you're rightly concerned about (like ABI mismatches or panics inside a plugin) are handled:
*   **ABI Mismatch:** The host will check the plugin's ABI version at load time and will refuse to load an incompatible plugin, preventing a crash.
*   **Plugin Panics:** The host will wrap calls into the plugin with `std::panic::catch_unwind`. If the plugin crashes, it will be safely disabled without taking down the entire pipeline.

### 2. Master Implementation Checklist

This is a great idea. A master checklist will be invaluable for tracking progress. I will create a new file, [`IMPLEMENTATION_CHECKLIST.md`](IMPLEMENTATION_CHECKLIST.md), based on the roadmap outlined in the transition plan.

### 3. Where Pipeline Stages are Defined

Pipeline stages are defined in two distinct locations, reflecting a separation of concerns:

1.  **Core Stages (`crates/pipeline/src/stages/`):** This directory is for fundamental, built-in stages that are considered part of the main application. Examples would be an `AcquisitionStage` that knows how to talk to a specific sensor driver, a `ToVoltageStage` for converting raw ADC values, or common sinks like `CsvSink` and `WebSocketSink`.
2.  **Plugin Stages (`plugins/`):** This directory is for optional, dynamically-loaded stages provided by plugins. These are for specialized processing (e.g., a specific type of filter, an advanced artifact detection algorithm) that are not part of the core offering.

`crates/sensors` and `crates/device` do **not** define pipeline stages.
*   `crates/sensors`: Contains the low-level hardware **drivers** (e.g., the code that communicates over SPI with the ADS1299). An `AcquisitionStage` (defined in `crates/pipeline/src/stages/`) would *use* a driver from this crate to get data.
*   `crates/device`: Contains the main application logic (the "daemon" or "server") that **hosts and runs** the pipeline. It uses the `PipelineRuntime` to manage the pipeline's lifecycle.

### 4. Handling Multiple ADS1299 Sensors

The architecture handles multiple sensors through configuration. If you were to add 3 more ADS1299 sensors, the process would be:

1.  **Hardware:** Connect the new sensors to your host system (e.g., via separate SPI buses or chip-select lines).
2.  **Configuration (`pipeline.json`):** You would define multiple, independent `AcquisitionStage` instances in your pipeline configuration file. Each instance would be configured with the unique identifier for one of the physical sensors (e.g., `/dev/spi0.0`, `/dev/spi0.1`, etc.).
3.  **Pipeline Graph:** These acquisition stages would become the starting points for parallel data flows. You could have three entirely separate pipelines, or you could design a pipeline graph that merges their data streams if needed. The system is flexible enough to support either topology.

### 5. WebSocket Exposure and Pipeline Control

The interaction with the UI via WebSockets is split into two distinct functions:

1.  **Data Streaming:** To get data from a pipeline to the UI, you add a `WebSocketSink` stage to the end of your pipeline graph. This stage receives `Packet<T>` data, serializes it, and streams it out over a dedicated WebSocket connection. A UI component would connect to that specific WebSocket endpoint to visualize the data.
2.  **Pipeline Control:** The main daemon (`crates/device/src/server.rs`) exposes a separate, central **command API** (likely over another WebSocket or a REST endpoint). To create a new pipeline, the UI would send a command to this API, including the full JSON definition of the pipeline graph. The server receives this command, uses the `GraphBuilder` to construct the pipeline, and tells the `PipelineRuntime` to start it. The same API is used for `pause`, `resume`, and `stop` commands.

### 6. Dynamic Batch Sizes

Yes, the architecture is explicitly designed to handle dynamic batch sizes. This is a critical feature for efficient processing. As you described:

*   An `AcquisitionStage` can produce packets with `batch_size: 16`.
*   A `FilterStage` would process that data in-place, passing on the packet with `batch_size: 16`.
*   A `DownsampleStage` would consume the packet with 16 samples. It would then acquire a **new, smaller packet** from a `MemoryPool` configured for single samples, populate it with the one downsampled value, and send this new packet (with `batch_size: 1`) downstream.

The `batch_size` is carried in each `Packet`'s header, not as a global constant, which makes this flexibility possible.
