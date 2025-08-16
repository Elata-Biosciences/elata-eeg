# Pin 7 Debugging Summary

This document summarizes the debugging process to resolve the `PinUsed(7)` error. The root cause was a panic in a pipeline stage that prevented proper resource cleanup, leaving GPIO pins in a used state.

## Problem

- On pipeline failure (panic), the daemon would crash.
- Resources, specifically GPIO pins, were not being released.
- Restarting the daemon would result in a `PinUsed(7)` error because the pin was still held by the crashed process.
- The frontend would enter a frustrating auto-reconnect loop.

## Architectural Solution

The solution involved making the pipeline robust against panics and ensuring graceful shutdown and clear error reporting.

1.  **Fatal Error Channel:** A `flume` channel was added to the `Executor` to communicate fatal errors from stage threads to the main daemon.
2.  **Panic Handling:** The `stage.process()` call in [`crates/pipeline/src/executor.rs`](crates/pipeline/src/executor.rs) was wrapped in `std::panic::catch_unwind` to catch panics within stages.
3.  **Error Signaling:** On a panic, a detailed error message is sent over the fatal error channel.
4.  **Graceful Shutdown:** The main daemon loop listens for fatal errors. Upon receiving one, it calls the pipeline's `shutdown()` method to release resources and then broadcasts a `PipelineFailed` event to the UI via the SSE stream.
5.  **Frontend Error Handling:** The web UI was updated to handle the `PipelineFailed` event, display a persistent error message, and stop trying to reconnect.

## Implementation Steps

### Backend Changes

- Fixed compiler errors in [`crates/daemon/tests/full_stack.rs`](crates/daemon/tests/full_stack.rs) related to the `Executor::new` call.
- Fixed compiler errors in [`crates/pipeline/examples/full_pipeline_test.rs`](crates/pipeline/examples/full_pipeline_test.rs).
- Implemented the panic handling and graceful shutdown logic in the pipeline executor and daemon.

### Frontend Changes

1.  **Event Handling:**
    -   File: [`kiosk/src/context/EventStreamContext.tsx`](kiosk/src/context/EventStreamContext.tsx)
    -   Action: Added logic to the `EventStreamProvider` to listen for the `PipelineFailed` SSE event. On receipt, it sets a `fatalError` state variable.

2.  **Error Display:**
    -   File: [`kiosk/src/components/EegMonitor.tsx`](kiosk/src/components/EegMonitor.tsx)
    -   Action: Added UI elements to display the `fatalError` message prominently to the user, preventing the reconnect loop and providing clear feedback.

## Outcome

The system is now robust against panics. It guarantees resource cleanup, preventing `PinUsed(7)` errors on restart, and provides clear error feedback to the user on the frontend.