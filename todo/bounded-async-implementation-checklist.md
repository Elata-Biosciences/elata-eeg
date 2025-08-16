# Bounded Async Architecture Implementation Checklist

## Focus: Keep Current Async Architecture + Add Robust Bounds

### 🔧 Core System Structure

* [ ] **Create `bounded_core.rs` - single file for all EventBus/buffer logic**
  Consolidates complex async coordination in one maintainable place.

* [ ] **Replace all unbounded channels with bounded variants**
  Use `mpsc::channel(capacity)` instead of `broadcast::channel()` to prevent lag.

* [ ] **Implement bounded ring buffers per stage (e.g. `BATCH × 6`)**
  Prevents memory growth and enforces known latency ceilings.

* [ ] **Print each buffer's capacity at startup**
  Log all buffer sizes so devs can see allocation decisions.

* [ ] **Add `try_send()` for WebSocket queue with frame dropping**
  Guarantees async tasks never block the data acquisition loop.

---

### 🧠 Plugin System (Async with Bounds)

* [ ] **Wrap all plugin async operations with `tokio::time::timeout`**
  Prevents plugins from hanging indefinitely - fail fast with clear errors.

* [ ] **Add per-plugin bounded channels (not broadcast)**
  Each plugin gets its own `mpsc::channel(capacity)` to prevent cross-plugin lag.

* [ ] **Implement plugin supervisor pattern**
  Track plugin task `JoinHandle`s and restart failed/panicked plugins.

* [ ] **Add plugin runtime budgets with monitoring**
  Track per-plugin processing time and log warnings when approaching limits.

* [ ] **Plugin failure logs include name + sequence number + timeout info**
  Makes failure location and cause unambiguous.

* [ ] **Keep plugins as async tasks but with strict resource limits**
  Maintain parallelism while preventing resource exhaustion.

---

### 🧱 Safety & Observability

* [ ] **Panic on buffer overflow in critical data acquisition stages**
  Protects against silent data loss in the main pipeline.

* [ ] **Drop oldest + log warnings on overflow in non-critical stages**
  UI/visualization can tolerate drops - log but continue.

* [ ] **Set global `panic!` hook with rich context**
  Print plugin name, buffer stage, sequence number, and full backtrace.

* [ ] **Wrap all async operations in `anyhow::context(...)`**
  All errors are traceable to exact async task + operation.

* [ ] **Add per-stage frame drop counters**
  Track which async stages are hitting capacity limits.

* [ ] **Use static assertions on buffer capacities**
  `const_assert!(BUFFER_CAP <= MAX_SAFE_SIZE)` prevents silent cap increases.

---

### ⛓ Runtime Guarantees (Async-Safe)

* [ ] **Calculate buffer sizes based on output size (FFT fan-out, etc.)**
  Account for data transformation when sizing downstream buffers.

* [ ] **Use bounded queues between every async stage**
  Enforce back-pressure: send succeeds or errors loudly.

* [ ] **Implement non-blocking logging for hot async paths**
  Use `tracing` with async-safe appenders to prevent log-induced stalls.

* [ ] **Add async task health monitoring**
  Detect when async tasks stop processing (heartbeat counters).

* [ ] **Implement graceful async task shutdown**
  Use `CancellationToken` properly to avoid orphaned tasks.

---

### 🔄 EventBus Improvements (Async-Focused)

* [ ] **Replace broadcast channels with targeted mpsc channels**
  Eliminates broadcast lag issues while maintaining async benefits.

* [ ] **Add EventBus metrics and health checks**
  Monitor channel utilization and async task responsiveness.

* [ ] **Implement async back-pressure handling**
  When buffers fill, apply back-pressure to data acquisition rather than dropping.

* [ ] **Add async plugin registration/deregistration**
  Support hot-plugging async plugins without restarting the system.

* [ ] **Create async-safe error propagation**
  Ensure plugin errors bubble up to supervisors properly.

---

### 🧪 Testing/Validation

* [ ] **Stress test with bounded channels under load**
  Verify system behavior when buffers approach capacity limits.

* [ ] **Test async plugin timeout scenarios**
  Ensure timeouts work correctly and don't leave orphaned tasks.

* [ ] **Validate back-pressure propagation**
  Confirm that downstream pressure properly slows upstream data flow.

* [ ] **Test plugin supervisor restart behavior**
  Verify failed plugins restart cleanly without affecting others.

---

### 📊 Monitoring & Debugging

* [ ] **Add async task monitoring dashboard**
  Track which async tasks are running, their resource usage, and health.

* [ ] **Implement structured logging for async operations**
  Use correlation IDs to trace data flow across async boundaries.

* [ ] **Add buffer utilization metrics**
  Monitor how close each bounded buffer gets to capacity.

* [ ] **Create async deadlock detection**
  Monitor for tasks that stop making progress.

---

## Implementation Priority

1. **Phase 1**: Replace unbounded channels with bounded variants
2. **Phase 2**: Add timeout wrappers around all plugin async operations  
3. **Phase 3**: Implement plugin supervisor pattern
4. **Phase 4**: Add comprehensive logging and monitoring
5. **Phase 5**: Stress test and tune buffer capacities

🔍 Comments by Section
🧠 Plugin System (Async with Bounds)

    ✅ tokio::time::timeout + per-plugin channels = essential for async robustness.

    ✅ Supervisor pattern is great—especially for catching panics and ensuring no orphaned tasks.

    🔶 Suggestion: log plugin restart count and last crash cause. If someone makes a plugin that fails every 5 seconds, you'll catch it.

🧱 Safety & Observability

    ✅ Very strong. Panic on critical overflows + drop+log on non-critical = right tradeoff.

    🔶 Add: a logging macro like log_plugin!(...) to standardize log output from plugin tasks.

⛓ Runtime Guarantees

    ✅ Buffer sizing based on transformed size (e.g., FFT output count) is excellent and often forgotten.

    🔶 Consider: throttle plugin warnings if a plugin goes over budget repeatedly — log once per X seconds to avoid flooding logs.

🔄 EventBus Improvements

    ✅ Replacing broadcast is smart; this is one of the top fragility points in Rust async pipelines.

    🔶 Async plugin hot-plugging is ambitious — defer this unless you really need it.

🧪 Testing/Validation

    ✅ Good list. Covers the real failure modes.

    🔶 Optional: include manual test scripts in the repo for open-source contributors (e.g., stress test 1, timeout test 2).

📊 Monitoring & Debugging

    ✅ Well thought-out for a production-level system.

    🔶 If you skip building a full dashboard, at least log summary stats every N seconds (task alive, buffer fill %).

🧠 Final Take

This plan:

    ✔ Trades some simplicity for async scalability.

    ✔ Preserves plugin parallelism.

    ✔ Converts nearly all "silent fail" modes into either observable logs or loud crashes.

    ❗ The biggest remaining complexity is still task lifetime management and cross-task coordination, but this checklist makes it as safe as async gets.

🟢 Suggest Minor Edits

Add these:

Log plugin restart count and crash reason
Helps detect flapping or unstable plugins.

Throttle repeated warnings per plugin
Avoid log spam if a plugin keeps going over budget.

Standardize plugin logging format
Use a macro like log_plugin!(name, seq, "msg").

Periodic system summary logs
Every 5–10 seconds, log buffer fill %, dropped frames, and plugin status.