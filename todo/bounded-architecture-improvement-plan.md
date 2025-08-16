# Bounded Architecture Improvement Plan

## Analysis of External AI Conversation

Based on the conversation with the other AI, here are all relevant points for keeping the current architecture and improving it:

### Memory Management & Buffers
- **Fixed capacity buffers**: 2-6x batch size per stage to prevent memory leaks
- **Print all buffer allocations at startup** for visibility
- **Hard caps on all channels/buffers** with panic on overflow
- **Ring buffers with compile-time capacity assertions**
- **Drop oldest frames when buffers full** (non-critical stages)
- **Static sizing generated at runtime** based on batch size and pipeline stage output

### Error Handling & Visibility
- **Panic with context on buffer overflow** showing exact location
- **Add timeout guards around plugin execution** with clear error messages
- **Crash-with-context using anyhow** for full backtraces
- **Log frames_dropped counters** before panicking
- **Non-blocking telemetry** to avoid stalling hot path

### Plugin Safety
- **Add tokio::time::timeout wrapper** around plugin tasks
- **Bounded mpsc channels instead of broadcast** to prevent lag
- **Plugin budget enforcement** (MAX_MICROS per plugin)
- **Supervisor pattern** for plugin task lifecycle management
- **Static assertions on buffer sizes** to prevent "just bump the size" fixes

### Architecture Constraints
- **One file containing all memory buffers and event bus logic**
- **Bounded everything** - channels, buffers, processing times
- **Immediate feedback on failures** rather than silent drops
- **Unit tests with property-based testing** (QuickCheck) for buffer behavior

## Recommended Approach: Bounded Current Architecture

### Why Keep Current Architecture + Add Bounds

1. **Already transitioned from single-core → async** - going back feels like thrashing
2. **Async architecture isn't fundamentally broken** - just lacks proper bounds and error propagation
3. **Adding hard caps + timeouts + panic-on-overflow** addresses core issues without architectural upheaval
4. **"One file for buffers/eventbus" approach** gives containment benefits without losing parallelism

### Core Implementation Strategy

```rust
// bounded_core.rs - Single file containing all buffer/eventbus logic
const RING_CAP: usize = BATCH_SIZE * 6;  // 6x headroom
static_assertions::const_assert!(RING_CAP <= 10_000);

struct BoundedEventBus {
    // All channels are bounded with hard caps
    raw_data_tx: mpsc::Sender<EegFrame>,
    plugin_channels: HashMap<PluginId, BoundedSender<Event>>,
    websocket_tx: mpsc::Sender<WebSocketEvent>,
}

impl BoundedEventBus {
    fn broadcast_with_timeout(&self, event: Event, timeout: Duration) -> Result<(), BusError> {
        // Timeout + context on failure
        // Panic with exact plugin/buffer info on overflow
    }
}
```

### Key Principles

1. **Fail-Fast**: Panic immediately on buffer overflow with exact context
2. **Bounded Everything**: Every channel/buffer has compile-time or runtime capacity limits
3. **Visible Failures**: All failures log exactly which component and why
4. **Simple Debugging**: One file contains all the complex async coordination

### Implementation Steps

1. **Create bounded_core.rs** - Move all EventBus and buffer logic here
2. **Add hard caps to all channels** - Replace unbounded with bounded variants
3. **Add timeout wrappers** - Wrap all plugin async operations
4. **Add panic hooks** - Context-rich error messages on failures
5. **Print allocations at startup** - Log all buffer sizes and capacities

### Expected Outcome

- **Silent failures eliminated** - All failures are loud and obvious
- **Memory leaks impossible** - Hard caps prevent unbounded growth
- **Easy debugging** - Single file contains all coordination logic
- **Plugin safety** - Bad plugins can't silently break the system
- **Maintainable** - Future contributors can't accidentally introduce fragility

## Conclusion

The system doesn't need redesign - it needs **constraints**. Bounded channels, timeouts, and loud failures turn a fragile async system into a robust one. This approach maintains the benefits of the current architecture while eliminating the silent failure modes that cause the "no data" issue.