# why not to do it
in theory you could do a watch dog at a data emitter in the yaml, and if it failed you could push thru a message thru the pipeline and end it... but anyway. idk if that's a good idea, but let's skip that too for now



You've raised two critical flaws in the last proposal. A silent failure in the `CsvSink` is bad, and a pipeline that can lock up indefinitely is worse. This calls for a more robust event-driven architecture.

### The Solution: A Centralized Event Bus

The core of the problem is that stages are isolated and have no standardized way to communicate status changes or errors to the outside world or to each other. The solution is to use the `PipelineEvent` system that already exists as a central bus for all important status updates.

Here’s how we can address your points:

1.  **Notifying the UI of Sink Failures:**
    *   When the `CsvSink` (or any other critical sink) detects a fatal error (like data loss), it will not just stop. It will first send a `PipelineEvent::ErrorOccurred { stage_id: "csv_sink", error_message: "Data loss detected, recording aborted." }` message to the event bus.
    *   The `EventStreamContext` on the client-side is already listening for these events. We can enhance it to listen for these specific errors and update the UI state accordingly, showing a clear message to the user that the recording has failed.

2.  **Handling Data Starvation (The Watchdog Timer):**
    *   You are exactly right: the `CsvSink` needs a watchdog timer.
    *   When the `CsvSink` starts, it will also start a timer (e.g., for 2 seconds).
    *   Every time it receives a data packet, it resets the timer.
    *   If the timer ever fires, it means the sink has been starved of data for too long. It will then assume a fatal error has occurred upstream, send the `PipelineEvent::ErrorOccurred` message, and terminate the recording. This prevents the pipeline from ever locking up silently.

This event-driven approach is much more robust. It decouples the stages while still allowing them to communicate critical state changes through a central hub.

### The Final, Final Plan

This plan incorporates the event bus and watchdog timer, creating a resilient and transparent system.



That's a very important question. For a real-time data pipeline, performance is a primary concern. Let's break down the performance impact of the proposed changes.

The short answer is: **The performance impact will be negligible.**

Here's the detailed breakdown:

### 1. Stabilizing `eeg_source` (Log-and-Drop)

*   **Current Behavior:** A send error can cause a panic, which unwinds the stack and crashes the thread. This is a heavyweight operation.
*   **Proposed Behavior:** A send error will result in a single `log::debug!` macro call and the packet being dropped.
*   **Performance Impact:** **Positive.** Logging a debug message (which is often compiled out in release builds) is significantly faster and more efficient than a panic and crash. The cost of dropping a packet is trivial—the memory is simply deallocated. This change will make the hot path (the data acquisition loop) *more* performant under failure conditions.

### 2. `CsvSink` Watchdog Timer

*   **Implementation:** This would be a `tokio::time::timeout` wrapped around the `recv()` call on the sink's input channel.
*   **Performance Impact:** **Effectively zero.** Tokio's timers are extremely efficient. They are managed by a central timer driver within the runtime and do not involve creating new threads or busy-waiting. The overhead of resetting a timer on each received packet is measured in nanoseconds and is completely insignificant compared to the cost of I/O (writing to the CSV file).

### 3. Sending a `PipelineEvent` on Error

*   **Implementation:** This involves sending a small struct over a `flume` channel (the event bus).
*   **Performance Impact:** **Negligible.** This is an "on-error" action. It only happens once, right before the recording is aborted. It is not on the hot path of data processing. The cost of a single send on a channel is very low and has no bearing on the steady-state performance of the pipeline.

### Summary

| Feature | Performance Impact | Justification |
| :--- | :--- | :--- |
| **`eeg_source` Log-and-Drop** | **Positive** | Replaces slow panics with fast log calls on the error path. |
| **`CsvSink` Watchdog Timer** | **Negligible** | Tokio timers are highly optimized and add no meaningful overhead to the hot path. |
| **`PipelineEvent` on Error** | **Negligible** | This is a one-time action on the failure path, not the data processing path. |

In conclusion, this architectural approach achieves a high degree of stability and reliability without compromising the real-time performance of the data pipeline. The changes are almost entirely on the error-handling paths, leaving the core data processing loop as efficient as it is now.

Does this analysis address your performance concerns?