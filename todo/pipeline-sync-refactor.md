# Pipeline Crate Synchronous Refactor Plan

## 1. Purpose

The primary goal of this refactor is to make the `crates/pipeline` a pure, synchronous library, completely removing the `tokio` dependency. This aligns with the core design principle of isolating all asynchronous logic into the `crates/device` crate.

This change will:
-   **Improve Testability**: A synchronous pipeline is easier to test deterministically.
-   **Enforce Architectural Boundaries**: Ensures that the core DSP logic remains independent of the async runtime and networking concerns.
-   **Prepare for Bare-Metal**: A pure `pipeline` crate is a step towards compiling it for embedded, `no-std` environments in the future.

## 2. Target Architecture

The refactor will center around a new synchronous execution loop in `pipeline/src/runtime.rs`. This loop will be driven by a single, unified message channel and a pre-computed execution order for the static pipeline graph.

### Key Architectural Components

**A. Unified Runtime Message:**
To avoid race conditions between control and data channels, we will use a single enum to wrap all incoming messages for the pipeline.

```rust
pub enum RuntimeMsg {
    Data(Packet<T>),
    Ctrl(ControlCommand),
}
```

**B. Draining Trait for Sinks:**
To ensure stateful sinks (like `CsvSink`) flush their data before shutdown, we will introduce a `Drains` trait.

```rust
pub trait Drains {
    fn flush(&mut self) -> std::io::Result<()>;
}
```

**C. Synchronous `run` Loop:**
The main execution loop will be synchronous, using a non-blocking `try_recv` pattern. It will handle data, control commands, and a graceful, draining shutdown.

```rust
pub fn run(
    rx: Receiver<RuntimeMsg>,
    event_tx: Sender<PipelineEvent>,
    mut graph: PipelineGraph,
) -> Result<(), RuntimeError> {
    let topo = graph.topology(); // Vec<StageId>
    let mut draining = false;

    loop {
        match rx.try_recv() {
            Ok(RuntimeMsg::Ctrl(ControlCommand::Shutdown)) => draining = true,
            Ok(RuntimeMsg::Ctrl(cmd)) => graph.handle_ctrl(cmd)?,
            Ok(RuntimeMsg::Data(pkt)) => graph.push(pkt, &topo)?,
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => break,
        }

        if draining && graph.is_idle() && rx.is_empty() {
            graph.flush()?;          // Drains trait
            event_tx.send(PipelineEvent::ShutdownAck)?;
            break;
        }
        
        // Optional: prevent high CPU spin on an idle pipeline
        // std::thread::sleep(Duration::from_micros(50));
    }
    Ok(())
}
```

## 3. Implementation Checklist

- [ ] **Phase 2.1**: Define `RuntimeMsg` enum and `Drains` trait.
- [ ] **Phase 2.2**: Add a method to `PipelineGraph` to compute a topological sort of the stages.
- [ ] **Phase 2.3**: Refactor `graph.rs` and `runtime.rs` to use `std::sync::mpsc` and remove all `async`/`tokio` code.
- [ ] **Phase 2.4**: Implement the new synchronous `run` loop in `runtime.rs` using the `try_recv` pattern.
- [ ] **Phase 2.5**: Implement the data-pushing logic in `PipelineGraph` using the topological sort.
- [ ] **Phase 2.6**: Implement the draining shutdown logic in the `run` loop.
- [ ] **Phase 2.7**: Implement the `Drains` trait for `CsvSink` and any other stateful sinks.
- [ ] **Phase 2.8**: Remove `tokio` from `pipeline/Cargo.toml` and clean up dependencies.

## 4. Core Architectural Decisions

To ensure the long-term health and scalability of the pipeline, the following architectural principles are to be considered "frozen" for this phase of development.

| Surface                      | What to freeze now                                                                                                                      | Why it saves pain later                                                                              |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| **Stage API**                | `fn process(&mut self, pkt: &Packet) -> StageResult` must be **pure** (no hidden global state) and each stage must be `Send + 'static`. | Any thread-pool can call that safely. No interior redesign needed.                                   |
| **Runtime ↔ Stage boundary** | Always pass ownership of `Packet` (or an `Arc<Packet>` when you need fan-out).                                                          | Allows zero-copy cloning when several workers need the same frame.                                   |
| **Message enum**             | `RuntimeMsg::{Data(Packet), Ctrl(ControlCommand)}`                                                                                      | Scheduler can sit behind the same enum whether it’s a loop or a work-stealing deque.                 |
| **No Tokio primitives**      | Stay on `std` / `crossbeam` now.                                                                                                        | Avoids type-footguns if you switch to Rayon later (Tokio’s `Send + 'static` bound causes conflicts). |

## 5. Session Notes & Feedback

*This section contains a log of feedback from the architectural planning session to provide context for the decisions made.*

Your assessment lines up with what I saw in `runtime.rs`, and the sub-goal list is exactly what Phase 2 needs. A few nits and guard-rails so the refactor doesn’t stall:

| Topic                     | 👍 What’s covered                                                                     | ⚠️ Extra thing to nail down                                                                                                                                                                                                                                                                                                                                                 |
| ------------------------- | ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Async → sync cut-over** | Replace `tokio::sync::mpsc` / `select!` with `std::sync::mpsc` + manual polling loop. | Use **one** enum for both channels:<br/>`rust<br/>enum RuntimeMsg { Data(Packet), Ctrl(ControlCommand) }<br/>`<br/>Then a single `Receiver<RuntimeMsg>` avoids the classic “which channel woke first?” race.                                                                                                                                                                |
| **Full-graph data flow**  | You’ve noted the need to push beyond entry points.                                    | Implement a simple topological scheduler:<br/>1. Pre-compute `Vec<StageId>` in topo order.<br/>2. On each `Data(pkt)` pop, iterate that list and call `stage.process(&pkt)`.<br/>It’s cache-friendly and avoids per-edge queues until you *really* need concurrency.                                                                                                        |
| **Stage draining**        | Call out that `CsvSink` must flush.                                                   | Add a trait: <br/>`rust<br/>pub trait Drains { fn flush(&mut self) -> io::Result<()>; }<br/>`<br/>`Stage` objects that implement `Drains` get invoked in the shutdown path.                                                                                                                                                                                                 |
| **Shutdown handshake**    | Break loop, send `ShutdownAck`.                                                       | Instead of breaking immediately:<br/>1. Set `state = Draining`.<br/>2. Keep looping until every input channel is closed **and** internal queues are empty.<br/>3. Flush sinks ⇒ send `Ack` ⇒ return.                                                                                                                                                                        |
| **Select! replacement**   | You’ll poll two receivers.                                                            | Pattern:<br/>`rust<br/>loop {<br/>    match rx.try_recv() {<br/>        Ok(msg) => handle(msg),<br/>        Err(TryRecvError::Empty) => (),<br/>        Err(TryRecvError::Disconnected) => break,<br/>    }<br/>    // optional: std::thread::yield_now();<br/>}<br/>`<br/>Add a tiny sleep (`std::thread::sleep(Duration::from_micros(50))`) if CPU spin becomes an issue. |
| **Error bubbling**        | Still via `Result<Packet, SensorError>`.                                              | Convert to the unified enum above so control and data share one path. UI can log `SensorError` without burying it in nested `Result<Result<…>>`.                                                                                                                                                                                                                            |

### Suggested task ordering

1. **Introduce `RuntimeMsg` enum + topo list** – compile fails everywhere it should.
2. Replace `tokio::mpsc` with `std::sync::mpsc` (same compile unit).
3. Drop async keywords and `select!`.
4. Add `Drains` trait and implement for sinks.
5. Re-work shutdown logic to drain + flush + ack.
6. Run unit tests; CPU-spin tweak if needed.
