# Baseline Design 2.1 — Implementation Blueprint

This is for an EEG system that gathers data at 4000sps on a Pi5, performs DSP, then exposes it to a websocket.

> **Scope**  A practical guide for standing up the “Baseline” architecture: `Arc<RtPacket>` buffers, YAML/TOML topology, thread‑per‑group scheduler, and minimal‑boilerplate stage plugins.

Copy‑free path; Pi 5 tests hit <10 µs/packet easily.

---

## 1 · Big‑Picture Axes

| Axis What you’re optimizing Typical tension |                                              |                                            |
| ------------------------------------------- | -------------------------------------------- | ------------------------------------------ |
| **Data‑path mechanics**                     | copy‑free, multi‑core, low latency           | adds allocator / ref‑count plumbing        |
| **Topology description**                    | human‑readable configs, reproducible runs    | can ossify if baked into code              |
| **Plugin ergonomics**                       | easy to add new stages without touching core | risk of leaking internals + perf foot‑guns |

> **Design principle**  Keep these three concerns in separate layers so a change in one doesn’t ripple through the others.

###  Design Approach

| Metric Rating Notes      |       |                                                                   |
| ------------------------ | ----- | ----------------------------------------------------------------- |
| Performance              | ★★★★☆ | lock‑free `Arc`, copy‑free; good up to 20 kS/s × 16 ch on Pi 5    |
| Simplicity               | ★★★★☆ | minimal unsafe; uses standard libs (`flume`, `core_affinity`)     |
| Stage Boilerplate        | ★★★★★ | stages just take `Arc<RtPacket>` & return `Option<Arc<RtPacket>>` |

###  Constraints

#### Stage Memory Allocation

* *Safety belt:* `StageResult` is `#[must_use]`; Clippy lint forbids returning a mutably-aliased `Arc`.

| Intent Helper call Cost |                          |            |
| ----------------------- | ------------------------ | ---------- |
| Pass‑through            | `Ok(Some(pkt))`          | 0 alloc    |
| New buffer              | `pkt.map_samples(...)`   | stack      |
| Offload / log           | `PacketOwned::from(pkt)` | heap alloc |

#### Pipeline Config

* Different pipeline configs, but one pipeline at a time.
* If changing pipeline parameters, shutdown pipeline first?

#### Plugin‑friendly APIs

* The `plugin_api` crate + `simple_stage!` macro mean a plugin author writes: 1) a `StageDesc` variant, 2) a \~15‑line `process` block. They never see allocator types or scheduler details, so future core refactors don’t break their code.

#### Fan‑In & Fan‑In Timing

* Any N→1 stage (e.g., `AlignAndZip`) is just another plugin. The scheduler already supports multiple inbound receivers per stage; you supply the rendezvous logic inside the stage.
* The fan‑in stage owns the policy: e.g., “emit when both inputs share a timestamp within ±1 sample; drop older one.” That logic is isolated to the stage—no scheduler changes needed. If one sensor is much faster, you can down‑sample or fuse at a slower master rate.

#### Fan‑Out

* Outbound tee is automatic: every stage owns a `broadcast::Sender`; each downstream edge calls `subscribe()`. Zero extra code or allocs.
* Sinks must validate incoming *\`*`RtPacket`*\`*\* variants; mismatch ⇒ \`\`.\*

#### Multi‑Core

* The thread‑per‑group executor pins three logical groups (acquire, DSP, sinks) onto separate A76 cores via `core_affinity`. Back‑pressure is enforced with bounded `flume` channels so one slow group can’t stall the others.

#### Pipeline Configuration & Maintaining Config Between Replacements

* ADC parameters live in `SensorMeta { sensor_id, meta_rev, … }`—these structs are `Arc`‑shared. If you stop the runtime and start a new graph that re‑uses the same `SensorMeta` (or reloads it from a persisted file), the settings are identical. The YAML change touches only the stage wiring, not the metadata inside packets.

#### Back‑pressure Sizing (ideally none? or depth of 2 at the most?)

* Start with `cap = 4×batch_size`. Error out if it happens.
* Executor must drain channels and call \`\` on sinks before thread join.

#### Error Propagation

* Fatal, DrainThenStop, SkipPacket ... example, csv recorder stage raises Err(e), then Executor asks the policy for that stage. Action is **DrainThenStop** – mark stage “dead”, finish current batch, close its sender; downstream stages see `None` and exit cleanly; other branches keep running (e.g., the visualisation branch)... needs to be thought thru but that's the jist
* Clicking *Restart* just calls `POST /pipeline/start` with the same YAML; clicking *Start New* passes fresh YAML.
* policies live in `plugin_api::policy`, executor knows only `ErrorAction`, and stages know nothing at all.
* If new graph can't instnatinate, then fail it with an error. Nothing running or go to default

#### Entropy Control

* Clear crate boundaries + typed descriptors keep drift visible.

---

## 2 · Module Layout (suggested)

```
workspace/
├─ crates/
│  ├─ pipeline_core/      # allocators, graph runtime
│  ├─ plugin_api/         # StageDesc, StageImpl, helper macros
│  ├─ stages_builtin/     # default DSP + sink stages
│  ├─ daemon/             # loads YAML, starts runtime, REST control
│  └─ alloc_pools/        # optional; feature‑flag slot‑pool impl. empty for now
└─ pipelines/             # .yaml configs checked into git

```

## Runtime Control API

1. `POST /pipeline/shutdown`

   * executor sets a “quiesce” flag → stages finish in-flight work
   * bounded channels drain; threads exit; daemon joins them
2. `POST /pipeline/start` with new YAML

   * daemon builds new graph; existing `Arc<SensorMeta>` values are passed in so ADC/gain settings persist
   * executor threads spawn; runtime begins

* *Live-Config WebSocket (/ws/control)*

  * Sends merged JSON view of YAML + overrides
  * Accepts JSON-Patch messages to tweak stage params in real time
* *State model* — Base YAML → In-memory overrides → `pipeline.yaml` on clean shutdown; “Restart Current” reuses overrides, “Load Preset” discards them

## 3 · Dynamic Configuration and Locking

**Status:** `Planned`

To prevent data corruption during sensitive operations (e.g., changing ADC gain while a `csv_sink` is recording), the control plane must query the pipeline state before applying any configuration changes. This is managed through a `Lockable` trait.

### The `Lockable` Trait

Any stage that has a state where its configuration must not be changed (e.g., it is actively writing to a file) can implement the `Lockable` trait.

```rust
// In pipeline_core/src/stage.rs
pub trait Lockable {
    /// Returns true if the stage's configuration is currently locked.
    fn is_locked(&self) -> bool;
}

// with a default implementation on the main Stage trait
pub trait Stage: Send + Lockable {
    // ...
}

// and a default implementation for all stages
impl<T: Stage> Lockable for T {
    fn is_locked(&self) -> bool {
        false // Default to unlocked
    }
}
```

This allows stages like `csv_sink` to opt-in to this safety mechanism by overriding the `is_locked` method, while other stages remain unaffected.

### Query-First Control Flow

The `daemon` acts as the central coordinator and guarantees data integrity by following this sequence:

1.  **Command Received:** The `daemon` receives a command to change a parameter from the control WebSocket.
2.  **Query Pipeline:** Before applying the change, the `daemon` sends a query to the pipeline executor: "Is any stage that would be affected by this change currently locked?"
3.  **Check Lock State:** The executor traverses the pipeline graph downstream from the target of the parameter change. It calls `is_locked()` on each stage.
4.  **Reject or Apply:** If any stage returns `true`, the executor reports "locked" to the `daemon`, which rejects the command. Otherwise, the change is applied.

This ensures that runtime state changes are serialized and validated against the pipeline's collective ability to accept a change, making the system robust and extensible.

## 4 · Key Types

* Downsample stage MUST set `to`; downstream stages rely exclusively on packet header for rate.

```
// pipeline_core/src/packet.rs
pub enum RtPacket {
    RawI32(PacketData<RecycledI32Vec>),
    Voltage(PacketData<RecycledF32Vec>),
    RawAndVoltage(PacketData<RecycledI32F32TupleVec>),
}

// Never implement Clone; share via Arc<RtPacket>

```

```
// plugin_api/src/lib.rs
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StageDesc { /* … */ }

pub trait StageImpl: Send {
    fn new(desc: &StageDesc, ctx: &StageInitCtx) -> anyhow::Result<Box<dyn StageImpl>>;
    fn process(&mut self, pkt: Arc<RtPacket>, ctx: &mut StageCtx) -> StageResult;
    fn descriptor(&self) -> StageDesc;
}

...
pub trait StagePolicy {
    fn on_error(&self) -> ErrorAction;
}

pub enum ErrorAction {
    Fatal,          // tear down whole pipeline
    DrainThenStop,  // finish in-flight packets, then halt this stage
    SkipPacket,     // drop the offending packet, keep running
}


```

---

## 4 · Macros to Kill Boilerplate

```
// plugin_api/src/macros.rs
#[macro_export]
macro_rules! simple_stage {
    (
        $name:ident,
        $desc_variant:ident,
        $process:block
    ) => {
        pub struct $name;
        impl $crate::StageImpl for $name {
            fn new(_: &$crate::StageDesc, _: &$crate::StageInitCtx) -> anyhow::Result<Box<dyn $crate::StageImpl>> {
                Ok(Box::new(Self))
            }
            fn process(&mut self, pkt: std::sync::Arc<pipeline_core::RtPacket>, _ctx: &mut $crate::StageCtx) -> $crate::StageResult { $process }
            fn descriptor(&self) -> $crate::StageDesc { $crate::StageDesc::$desc_variant }
        }
    };
}

```

With that macro a **ToVoltage** stage collapses to \~15 lines.

```
simple_stage!(ToVoltage, ToVoltage, {
    use pipeline_core::{PacketView, RtPacket};
    if let PacketView::RawI32 { header, data } = PacketView::from(&*pkt) {
        // … convert & emit …
    }
    Ok(Some(pkt))
});

```

---

## 5 · Thread‑Per‑Group Scheduler

*TODO: make the group‑count configurable*

* `Sensor → Acquire` group pinned to core 0
* DSP group (filter / zip / …) cores 1‑2
* Sinks group core 3
* Back‑pressure via `flume::bounded(cap)`

See `pipeline_core/src/executor.rs` for the simple thread loop.

---

## 6 · YAML Example (fan‑out + fan‑in)

Fail on typos via `#[serde(deny_unknown_fields)]`.

```
- acquire: { board: 0, sps: 500, gain: 24 }
- to_voltage: {}
- notch: { hz: 60 }
- align_and_zip:
    inputs: [to_voltage, ext_sensor]
- websocket_sink: { topic: "eeg_filtered" }
- csv_sink: { path: filtered.csv }

```

---

## 7 · Boilerplate‑Reduction Checklist

| Pain Point Fix                   |                                                                                  |
| -------------------------------- | -------------------------------------------------------------------------------- |
| `StageFactory` impl per stage    | Replace with `simple_stage!` macro (or `#[derive(Stage)]` in a proc‑macro crate) |
| Cached meta logic everywhere     | Provide `meta_helpers.rs` with `ScaleCache` struct reusable by stages            |
| Recycled\* generics in signature | Hide behind `RtPacket`; stages work on `PacketView<'_>` slices                   |
| Repeated YAML `inputs:` arrays   | Add optional default: if omitted, wire to previous stage in list                 |

---

## 8 · Sinks

### `websocket_sink`

The `websocket_sink` stage's role is to forward data from the pipeline to the central `WebSocketBroker` located in the `daemon`. It does not host a server or manage any network connections directly.

Its responsibilities are strictly limited to:
1.  **Connecting to the Broker:** During initialization, the `StageInitCtx` provides it with an in-memory channel sender (`tokio::sync::broadcast::Sender`). This sender connects the stage to the `WebSocketBroker`.
2.  **Forwarding Data:** In its `process` loop, the stage takes the incoming `Arc<RtPacket>`, wraps it in a `BrokerMessage` containing the destination `topic`, and sends it to the broker over the channel.

This architecture cleanly separates the data processing logic of the pipeline from the network and client management logic of the `daemon`. The pipeline remains unaware of how many clients are connected or the specifics of the WebSocket protocol.

---

## 9 · Milestones

1. **Load YAML → build graph → pass compile‑time test**
2. Implement `simple_stage!` macro & port `ToVoltage` stage
3. Set up core‑affinity executor
4. Run latency benchmark & record baseline numbers

---

## 9 · Misc

**Documentation diagram** – a single SVG showing acquire → DSP → sink threads with bounded channels and back-pressure arrows will help new contributors grok the flow instantly.

```
impl Clone for RtPacket {
    fn clone(&self) -> Self {
        panic!("Use Arc<RtPacket>; deep copy is explicit.");
    }
}

```

## 9 · Possible Future

* Realtime hygiene

  * Even on Pi 5 you’ll see random 100 µs+ scheduler hiccups unless you lock memory, set thread priorities and isolate IRQs.
  * • Add mlockall on startup • Use SCHED\_FIFO/real-time priorities for acquire & DSP groups • Pin non-RT threads (loggers, REST) to the E-cores
* Metrics & tracing&#x20;

  * Latency/buffer depth drift is invisible today.&#x20;
  * Wire Prometheus/Influx counters: pkt latency histogram, channel depth, per-core load
* How is data timestamped? should be on the first data ready. Then increment by 1/SPS
* **Plugin sandboxing** – if third-party DSP stages are expected, consider running them in a `wasmtime` guest or at least in an isolated thread group with capped memory.
* **Cross-compile story** – state which `rustup target` and linker flags are known-good for a Pi 5 (aarch64-unknown-linux-gnu + `-C target-cpu=cortex-a72` still works fine).
* **OTA / field updates** – if these rigs end up in multiple labs, add a short section about updating the daemon and stages atomically (systemd-service + `execReload=` works).
* Conduct Study - Inside this folder, the daemon saves two separate files:      session\_meta.json: Contains the study details ({ "participant": "P001", "notes": "..." }).      pipeline.yaml: A snapshot of the exact pipeline configuration used for this recording. This is vital for reproducibility.
* **Stage hot-swap:** Implemntation and API, POST /pipeline/stage/fft { action: "add", after: "notch" }
Extensibility / Hot‑swap 
* YAML graph reload via `ArcSwap`; no restart needed

*Last updated 2025‑07‑16*


---

## 10 · Architectural Lessons: Channeling and Runtimes

An important lesson was learned during the implementation of the `WebSocketBroker` regarding the choice of in-memory channels.

Initially, the high-performance, multi-producer, multi-consumer (MPMC) channel library `flume` was used to connect the `websocket_sink` to the broker. However, this led to a difficult-to-diagnose `panic` that occurred specifically when a WebSocket client disconnected.

The root cause was a subtle incompatibility between `flume` and the `tokio` runtime's task lifecycle. When `axum` (the web framework) handled a client disconnection, it would drop the associated `tokio` task. This task held a `flume` receiver. The way `flume`'s internals interact with thread-local storage and task destruction was not perfectly aligned with `tokio`'s expectations, leading to an intermittent panic.

The resolution was to replace `flume` with `tokio::sync::broadcast`, the `tokio` runtime's native MPMC broadcast channel. Because `tokio::sync::broadcast` is designed as a core part of the `tokio` ecosystem, it is guaranteed to be compatible with `tokio`'s task management and lifecycle. The switch immediately resolved the panic and stabilized the system.

This experience serves as a key architectural reminder: **when building on an async runtime like `tokio`, prefer using the runtime's native synchronization primitives (`tokio::sync`) over external libraries unless there is a compelling and well-understood reason to do otherwise.** The tight integration ensures compatibility and avoids subtle, hard-to-debug runtime issues.
