# Connection Issue Summary

This document summarizes the connection issues encountered between the frontend Kiosk application and the backend EEG daemon, the steps taken to resolve them, and the current hypothesis.

## The Situation

The web application is unable to receive data from the backend. The control plane (HTTP/SSE) is working correctly, and the WebSocket connection for data streaming is established successfully, but it appears no data is ever sent from the backend, and the connection eventually times out or is closed by the browser with a `code: 1006` error.

## What We've Tried

Our investigation has peeled back several layers of the problem, from network configuration to application-level logic and data serialization.

1.  **Corrected WebSocket Binding:** We identified that the `websocket_sink` in `pipelines/default.yaml` was bound to `127.0.0.1:9001`, which prevented the proxy from connecting.
    *   **Action:** Changed the address to `0.0.0.0:9001`.
    *   **Result:** This successfully resolved the `ECONNREFUSED` errors and allowed the initial WebSocket handshake to complete.

2.  **Identified Architectural Mismatch (Proxy):** We discovered the frontend was relying on a Next.js server-side proxy to connect to the WebSocket sink. This contradicted the `API.md` which specifies a direct connection.
    *   **Action:** Refactored `EegDataHandler.tsx` to connect directly to the WebSocket address provided by the `/api/state` endpoint. Removed the proxy logic from `server.js`.
    *   **Result:** The application now follows the correct architecture, but the connection still failed.

3.  **Identified Architectural Mismatch (Backend Read):** We hypothesized the backend `websocket_sink` was crashing because it was trying to read from a write-only socket.
    *   **Action:** Refactored the `handle_connection` loop in `websocket_sink.rs` to be a simple, write-only loop, removing all `websocket.read()` calls.
    *   **Result:** The backend no longer crashed, and the WebSocket connection became stable. However, no data was rendered.

4.  **Fixed Frontend Rendering Crashes:** We identified and fixed several cascading import errors in `EegMonitor.tsx` and `EegRecordingControls.tsx` that were causing the React application to crash silently in the background.
    *   **Action:** Corrected all `useCommandWebSocket` imports to `useCommand`.
    *   **Result:** The frontend application is now stable and no longer crashes.

5.  **Fixed Data Format Mismatch:** We discovered the backend was sending JSON strings while the frontend expected a specific binary format.
    *   **Action:** Implemented binary serialization in `websocket_sink.rs`, added the `byteorder` dependency, and corrected the field names (`ts_ns`, `meta.sample_rate`) to match the `PacketHeader` struct.
    *   **Result:** The backend now sends binary data in the format the frontend expects. The connection is stable, but still no data is rendered.

## Current Hypothesis

We have fixed all known bugs in the connection logic, application stability, and data formatting. The logs are now clean on both the frontend and backend. The WebSocket connection is established and remains open.

This strongly suggests the problem is no longer in the *connection* but in the *data pipeline itself*. The `websocket_sink` is correctly waiting for data to arrive, but it appears no data is ever being pushed to it from the preceding stages.

The potential culprits are:
-   **`eeg_source`:** The mock data generator might not be running or producing data correctly.
-   **`to_voltage`:** This stage could be failing silently or not forwarding the data.
-   **Pipeline Executor:** There could be a fundamental issue in the pipeline executor that prevents data from flowing between stages.

## Next Steps

The next investigation must focus on the internal data flow of the pipeline.
1.  **Add Logging to Pipeline Stages:** We need to add `info!` or `debug!` logs to the `process` method of each stage (`eeg_source`, `to_voltage`, and `websocket_sink`) to confirm that packets are entering and leaving each stage.
2.  **Verify `eeg_source`:** Specifically, we need to confirm that the `eeg_source` stage is generating mock data as expected.

## Relevant Files

-   **`crates/pipeline/src/stages/eeg_source.rs`**: The source of the data. **This is a primary suspect.**
-   **`crates/pipeline/src/stages/to_voltage.rs`**: The intermediate processing stage.
-   **`crates/pipeline/src/executor.rs`**: The engine that runs the pipeline.