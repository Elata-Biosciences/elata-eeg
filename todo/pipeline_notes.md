we're looking at doing a task

so the data flow? what is that. it aquires a sample of data from the ads1299, then works it's way thru an event bus all the way to a exposed websocket socket. check plugins/README.md

ok so the task we want to do is remove the event bus for now. so undo all the work and prune the dead code after

so rather than an event bus, we transition to a pipeline. so a bunch of questions then:
1) the pipeline is in rust code, right?
2) how can we make it simple so it's visible in rust code
3) let's leave the plugins/ folder for now. we may re-integrate the stuff later
4) like what's a really clean way of doing this? the data goes from A to B. how can we make this clean as possible, if it's a static single pipeline?

what are your thoughts here? ty


The user is asking me to review the code and understand the current data flow, then help plan a transition from an event bus architecture to a pipeline architecture. Let me start by reading the ai_prompt.md file as instructed, then examine the plugins/README.md to understand the current data flow.

Key questions to address:

Current data flow from ADS1299 → event bus → websocket
Remove event bus and transition to pipeline
Pipeline should be in Rust code
Make it simple and visible in Rust code
Leave plugins/ folder for now
Design a clean static single pipeline from A to B
I need to gather information about the current architecture first.

I'll help you review the code and plan the transition from an event bus to a pipeline architecture. Let me start by reading the guidance and understanding the current data flow.

>>>>>>>>>>>>>>>>>>
I understand the current data flow: ADS1299 → data_acquisition_loop → EventBus → plugins → WebSocket. For the pipeline transition, I need to clarify a few key design decisions:

1. **Pipeline Scope**: Should the pipeline replace just the plugin processing (keeping EventBus for WebSocket communication), or should it replace the entire EventBus system including WebSocket delivery?

2. **Pipeline Stages**: What specific processing stages do you want in the pipeline? For example:
   - Raw EEG → Voltage Conversion → Filtering → FFT → WebSocket
   - Or a different sequence?

3. **Plugin Integration**: You mentioned leaving plugins/ folder for later re-integration. Should the pipeline initially just handle the core processing (voltage filtering, FFT) without the plugin system, or should it provide a way to integrate existing plugin logic directly?

4. **WebSocket Output**: Should the pipeline end by directly sending to WebSocket connections, or should it output to a simple channel that the WebSocket server consumes?

>>>>>>>>>>>>>>>>>>>>>>>
...........Pipeline Stages: let's just do aquistion -> filter -> websocket -> graph it
for now


...........Plugin Integration: the core for now is just hte signal and graph. no fft. let's just leave that folder code as is

.........WebSocket Output: im not sure. what do you think? 

...........ok some questions:
- what would the types look? like if we think this thru? if it's one pipeline, then consumer/produce depends on the data type? then also can we do data points so it doens't recopy? like make it peformant smart


...........Pipeline Scope: defeintly remove evnet bus. the probelm is we need data buffering, ... it got really complex it feels like. here's some notes from another AI session on this stuff... what's your take on this?

..............AI NOTES 1
Key Analysis:

Listed all relevant points from the conversation for keeping current architecture and improving it
Identified that the async architecture isn't fundamentally broken - it just lacks proper bounds and error propagation
Recognized you already transitioned from single-core → async, so going back would be thrashing
Recommended Approach: Bounded Architecture Improvement
Created a detailed plan in todo/bounded-architecture-improvement-plan.md that:

Keeps your current async architecture but adds critical constraints
Implements bounded buffers (2-6x batch size) with hard caps and panic-on-overflow
Consolidates complex logic into a single bounded_core.rs file
Adds timeout wrappers around all plugin operations
Provides fail-fast error handling with context-rich panic messages
Eliminates silent failures through immediate, loud error reporting
Why This Approach:

Maintains parallelism benefits of async plugins
Prevents memory leaks through hard buffer caps
Makes failures obvious and debuggable
Doesn't require architectural upheaval
Creates a "touch once, never again" robust foundation
The plan transforms your fragile async system into a robust one through constraints rather than redesign. This should eliminate the 10-minute "no data" failures while keeping the system extensible for open-source contributors.

### 🔧 Core System Structure

* [ ] **Collapse to single `.rs` file for core loop**
  Reduces mental load; all memory and event bus in one place.

* [ ] **Bounded ring buffer per stage (e.g. `BATCH × 6`)**
  Prevents memory growth and enforces known latency ceilings.

* [ ] **Use fixed-size `RingBuf` or `arraydeque`**
  Overflow = explicit error or panic — never silent.

* [ ] **Print each buffer's size at startup**
  Ensures devs don’t forget to size buffers appropriately.

* [ ] **Push to WebSocket queue via `try_send()`**
  Drops frames if queue full; guarantees core thread never blocks.

---

### 🧠 Plugin System

* [ ] **Use a `Plugin` trait with `MAX_MICROS` and `process()`**
  Enforces per-plugin time budget and synchronous execution.

* [ ] **Plugins register via `inventory` macro**
  No core code needs to change to add/remove a plugin.

* [ ] **Track per-plugin runtime and crash if over budget**
  e.g. `if elapsed > MAX_MICROS { panic!(...) }`.

* [ ] **Plugin failure logs include name + sequence number**
  Makes failure location unambiguous.

* [ ] **Run each plugin synchronously inside loop**
  No async tasks — keeps processing sequential and easy to debug.

* [ ] **Isolate I/O-heavy plugins as supervised async side-processes**
  Keep core loop clean; route large/delayed tasks elsewhere.

---

### 🧱 Safety & Observability

* [ ] **Crash on buffer overflow in critical stages (e.g. acquisition)**
  Protects against silent data loss.

* [ ] **Drop oldest + log on overflow in non-critical stages (e.g. UI)**
  Allows system to survive low-priority pressure.

* [ ] **Set a global `panic!` hook**
  On crash, print plugin name, sequence number, and backtrace.

* [ ] **Wrap all errors in `anyhow::context(...)`**
  All panics are traceable to the exact stage + frame.

* [ ] **Log total frames dropped/skipped per stage**
  Helps surface which part is hitting capacity.

* [ ] **Add static assertions on buffer sizes**
  Prevent devs from silently increasing caps to “solve” pressure.

---

### ⛓ Runtime Guarantees
* [ ] **Calculate buffer sizes based on transformed size (not just frame count)**
  FFTs and similar ops fan out — must size buffers accordingly.
* [ ] **Use fixed size queues between *every* stage**
  Enforce contract: push succeeds or errors loudly.
* [ ] **Limit logging on hot path — use non-blocking log queues**
  Prevent logging from stalling frame processing.
* [ ] **Have `core.rs` be the *only* thread touching frames directly**
  Everything else (WebSocket, I/O) uses copy or queue.

---

### 🔄 Optional Future Features (if needed)

* [ ] **Support WASM-based plugin sandboxing**
  Enables plugin loading without native ABI headaches.
* [ ] **Enable `dylib`/`so` plugin loading via C ABI**
  Supports native-speed plugin binaries with hot-reload.
* [ ] **Use side-process plugins over gRPC or Cap’n Proto**
  Allows full isolation and crash-restartable plugins.
* [ ] **Use actor-based model via NATS/Zenoh**
  Plugins can exist on remote machines with pub/sub fanout.
* [ ] **Allow Lua/JS micro-plugins via embedded interpreter**
  Enables super-fast prototyping for non-Rust users.

---

### 🧪 Testing/Hardening (Optional per your call)
* [ ] **CI property tests (QuickCheck/fuzzing) for plugin timing + frame flow**
  Helps catch regressions or plugin violations early.
* [ ] **Dead-man switch watchdog (core loop heartbeat)**
  Detects freezes if frame loop halts unexpectedly.
* [ ] **Prometheus metrics per stage (frames in/out, drops, timings)**
  Optional but very helpful for remote debugging.
............AI NOTES 2
Smell Test Result: 🔴 TOO COMPLEX
Bottom Line: The checklist is architecturally sound but way too complex for an open-source EEG project where plugin developers need simplicity.

Current Architecture Assessment:
✅ What's Already Good:

EventBus with bounded broadcast channels (256 capacity)
Plugin supervisor tracking JoinHandles
Clean plugin trait with clone_box() pattern
Separate crate architecture for plugins
🚨 Core Problems with the Checklist:

Complexity Explosion - 40+ items that turn simple plugin development into rocket science
Over-Engineering - Most EEG plugins are simple filters/transforms, not distributed systems
Developer Friction - Open source contributors will run away from timeout wrappers, supervisor patterns, etc.
Simplified Alternative (Keep It Stupid Simple):
Instead of 40 items, focus on 5 core principles:

One File Rule - All async coordination in bounded_core.rs (✅ good idea)
Fail Fast & Loud - Plugin panics = system restart, no silent failures
Fixed Buffers - Pre-allocated ring buffers, panic on overflow (no dynamic sizing)
Plugin Timeout - 100ms max per plugin operation, then kill it
Observable Logs - Structured logging with plugin name + frame sequence
What to Drop:

Plugin supervisors with restart logic (just crash and restart the whole system)
Per-plugin channels (broadcast is fine for EEG data rates)
Complex back-pressure (just drop frames and log it)
Hot-plugging (compile-time plugin registration is simpler)
Monitoring dashboards (logs are enough)
The Real Issue:
Your current architecture is already pretty good. The broadcast channel with 256 capacity handles typical EEG data rates fine. The main problems are:

No plugin timeouts
No restart on plugin failure
Silent frame drops
Recommendation: Pick 3-5 items from the checklist that directly solve these problems, ignore the rest. Keep it simple enough that a grad student can write a plugin in 30 minutes.

>>>>>>>>>>>>>>>>
Pipeline Structure: one core loop. what keeps it multi core tho?

Data Buffering: why data buffering? config.json specifes a batch_size to keep the processing done in batches... like why should it be bigger than the batch_size? like im seeing added complexity but with out any reasoning. like what are the complexities are we missing any others? the idea is to pair down, but look at each one criticialy if we need it

Error Handling: Error wise, we should propegate the error downstream. It should fail. then restart is context dependent. i would say restart, but the end point should specify what to do when this happens. like does it stpo recording or does it skip the frames an keep recording? that is dependent on the recording plugin/applet thing

WebSocket Integration: can it output to a channel that the connection maanger consumes? what's your take here? is there a big peformance hit?

1. Multi-Core Question: let's keep that part. why can't we do that? what do you htink?

2. Temporary backpressure- that makes sense. maybe comment this in the webscoekt connection thing. ocnnection manager. for now just error it. if there is lost data then like we shouldn't be making up data. yeah for now no data buffering, but just add a comment

3. Error Handling. Sounds good

4. Webscoekt integration: there is a performance hit? there is a channel hop if it's sharing hte data pointer? why is that?


1 no channel capactiy. that's what im saying. just batch_size. like why are we doing channel capcity? the only stage that needs taht is the connection manager b/c it's interfacing with web browsers. what happens if there are 3 web browsers connected to it? ... 

2. how do we know if a stage dropped the data? like it was too slow, then the next stage goes to the data and it sees it wasn't updated? how does it know? a counter? then it propegates the error?

3. try_send? so the producers push the data thru? so if the next stage is busy i'd say propegate an error thru and be done with it