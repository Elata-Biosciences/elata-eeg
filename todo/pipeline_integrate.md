# EEG Pipeline Refactor — Hand‑Off Packet

## 0 · Quick Start

* **Goal:** isolate Tokio/networking in `device/`, make `sensors/` sync‑only, add a runtime control plane to `pipeline/`.
* **Status:** sensors drivers still async; control plane stub empty; device crate mixes concerns.
* **You pick up at:** **Phase 1, task 1.1** below.

---

## 1 · Repo Overview (top‑level crates)

| Crate       | Role                                | Tokio?                  | Notes                                   |
| ----------- | ----------------------------------- | ----------------------- | --------------------------------------- |
| `sensors/`  | Low‑level ADC drivers, IRQ handling | **No** (after refactor) | Must compile on bare‑metal later        |
| `device/`   | Daemon / control plane / WebSocket  | **Yes**                 | Owns sockets, logging, restart logic    |
| `pipeline/` | Pure DSP DAG, control API           | **No**                  | Receives `Packet<i32>` → stages → sinks |

```
flow: ADC → sensors::ToVoltage → device (sync→async) → pipeline::run()
control: WS JSON → device → pipeline::ControlCommand
```

---

## 2 · Design Principles

1. **Single entry point**: all external connections terminate in `device/`.
2. **Pure core**: `pipeline/` compiles with `default-features = []` (no Tokio).
3. **Sync drivers**: `sensors/` exposes blocking `fn acquire(&self, tx: Sender<Packet<i32>>)`.
4. **Bridge task** in `device/` calls `spawn_blocking` to run the driver and forwards data over a `tokio::mpsc`.
5. **Control plane**: `pipeline` consumes `ControlCommand` messages via a dedicated channel.

---

## 3 · Roadmap / Task List

### Phase 1 – Tokio‑Free `sensors`

*

### Phase 2 – Pipeline Control Plane

*

### Phase 3 – Device Integration

*

### Phase 4 – Validation & Docs

*

---

## 4 · Code Snippets

### 4.1 Sensor Driver Template (sync)

```rust
pub fn acquire(&mut self, tx: Sender<Packet<i32>>) -> Result<(), DriverError> {
    loop {
        let frame = self.read_frame()?; // blocking SPI + DRDY IRQ
        tx.send(Packet { data: frame, meta: self.meta.clone() })?;
    }
}
```

### 4.2 Device Bridge Task

```rust
let (raw_tx, mut raw_rx) = tokio::sync::mpsc::channel(1024);
// Fan‑in std→tokio
std::thread::spawn(move || {
    while let Ok(pkt) = std_rx.recv() {
        raw_tx.blocking_send(pkt).unwrap();
    }
});
```

### 4.3 WebSocket → ControlCommand

```rust
match msg_json["cmd"].as_str() {
    Some("reconfig") => ControlCommand::ReconfigureStage {
        stage: msg_json["stage"].as_str().unwrap().into(),
        cfg: msg_json["cfg"].clone(),
    },
    Some("pause") => ControlCommand::Pause,
    _ => return Err(Error::UnknownCmd),
}
```

---

## 5 · Getting Started

```bash
# Toolchain
rustup default stable
cargo install cargo-nextest

# Run unit tests
cargo nextest run

# Launch daemon (device crate)
cargo run -p device --features "ws" --release
```

* Connect to `ws://localhost:9000/ctrl` and send: `{ "cmd":"pause" }`.

---

## 6 · Open Questions / TODO

* Define serde schema for stage configs (typed vs raw JSON).
* Decide on persistence of runtime configs (disk or RAM only?).
* Evaluate moving to `axum` + `serde_with` for typed WS payloads.

---

Good luck & happy hacking!


........................

**Short answer:** keep external connection management (WebSocket listeners, TCP sockets, BLE, etc.) in the *device* crate and expose *internal* control/data channels in the *pipeline*. Moving the listeners down into the pipeline sounds tidy at first, but it breaks the layering you just spent time cleaning up and re-introduces Tokio into the core DSP code. Below is the trade-off matrix and a practical compromise.

---

### 1 · What “connection management” really means

| Concern                                              | What it touches                             | Best home                   |
| ---------------------------------------------------- | ------------------------------------------- | --------------------------- |
| Hardware IRQ/GPIO, SPI, I²C                          | Low-level driver loops                      | **sensors**                 |
| Translating hardware frames → `Packet<i32>`          | Thin driver + `ToVoltage`                   | **sensors → pipeline edge** |
| Routing `Packet<f32>` through DAG                    | SPSC/MPSC channels, per-stage buffers       | **pipeline**                |
| Accepting WebSocket/TCP clients, auth, TLS, JSON RPC | OS sockets, Tokio runtime, logging, metrics | **device**                  |
| Turning JSON `{stage:"filter",…}` → `ControlCommand` | Control channel to running `Pipeline`       | **device → pipeline**       |

Trying to push the bold row *up* (into `pipeline`) forces that crate to depend on network, TLS, HTTP upgrades, logging, etc. The core DSP code becomes un-testable on bare-metal and impossible to embed later (e.g., if you ever want the pipeline in a WASM or no-std context).

---

### 2 · Why “device owns the sockets” scales better

1. **Single security boundary** – Auth, rate-limiting, and logging sit in one place. Pipeline code never sees raw networks.
2. **Tokio containment** – You just made `crates/sensors` sync-only. Keeping Tokio out of `pipeline` lets you compile it for `--no-default-features` (or even `#![no_std]` later).
3. **Deployment flexibility** – Today the device crate is a daemon. Tomorrow it might be a desktop GUI or a cloud micro-service. All of them can import the same `pipeline` library and wire it up to *their* preferred runtime without modifying DSP code.
4. **Fan-out** – A single device instance can spin up *multiple* pipelines (think: EEG + fNIRS) and multiplex them over one port. Putting sockets inside each pipeline would mean N separate listeners and duplicated TLS handshakes.

---

### 3 · But you still need introspection & hot-reload

The missing piece you found — an empty `control.rs` — is exactly where the *internal* API lives:

```rust
enum ControlCommand {
    ReconfigureStage { stage: String, cfg: serde_json::Value },
    Pause,
    Resume,
    Shutdown,
}
```

* Device owns `tokio::mpsc::Sender<ControlCommand>` for each running pipeline.
* WebSocket handler parses JSON → sends command → awaits `Result`.
* Pipeline’s `run()` does `select! { data = stage_rx => …, cmd = ctrl_rx => … }`.

This keeps the public surface *only* in the device crate while giving you full runtime mutability.

---

### 4 · Compromise pattern if you really want “pipeline-first”

If you still feel uneasy, expose an *optional* helper in `pipeline::net`:

```rust
pub fn websocket_endpoint(
    pipeline: Arc<Pipeline>,
    addr: impl ToSocketAddrs,
) -> impl Future<Output=Result<()>> { … }
```

It’s **feature-gated** (`#[cfg(feature="ws")]`) and lives behind a Cargo flag that pulls in `tokio` and `warp`. The default build stays pure-DSP; the device crate just enables the `ws` feature. That gives you the “pipeline knows how to speak WebSocket” convenience without forcing every downstream consumer to haul in Tokio.

---

### 5 · Practical next steps

1. Finish `ControlCommand` enum + dispatch loop in `Pipeline`.
2. Add a tiny `JsonRpcServer` in `device`, translating JSON → command.
3. Write an integration test that spins up the whole stack, connects via WebSocket, sends `ReconfigureStage`, and asserts the stage’s param changed.

Once that’s green, your layering is clean:

```
sensors ─▶ device (Tokio, sockets) ─▶ pipeline (pure DSP) ─▶ stages
```

That separation will save you pain when you refactor again or port the processing core to another platform.