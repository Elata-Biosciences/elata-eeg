# WebSocket Connection Debugging Chronicle

This document chronicles the extensive debugging process for a persistent WebSocket connection failure between the Kiosk frontend and the Rust backend daemon. It serves as a record to prevent repeating failed attempts.

## 1. Initial State & Problem

*   **Goal:** The Kiosk frontend should connect to the backend daemon via WebSocket and graph streaming EEG data.
*   **Observed Behavior:** The frontend displayed a "Disconnected" status.
*   **Initial Error:** Browser console showed a **404 Not Found** for the WebSocket endpoint.

## 2. The Debugging Journey: A Process of Elimination

The debugging process involved systematically testing and eliminating hypotheses.

### Attempt 1: Fixing the Backend Panic (500 Error)

*   **Hypothesis:** After fixing the initial 404, a **500 Internal Server Error** appeared. We believed the Axum server was panicking due to an issue with its `AppState` or a middleware conflict.
*   **Actions:**
    1.  Systematically commented out fields in `AppState` to isolate a problematic component.
    2.  This led to the discovery that the `tower-http` `TraceLayer` was incompatible with WebSocket upgrades, causing the server to panic.
*   **Result:** The `TraceLayer` was removed from `server.rs` and the `trace` feature was disabled in `Cargo.toml`. The 500 error was resolved, but a new error appeared.

### Attempt 2: Fixing the Frontend Proxy (Connection Refused)

*   **Hypothesis:** With the backend no longer panicking, the browser now showed a **NS_ERROR_WEBSOCKET_CONNECTION_REFUSED** error. This suggested the Next.js frontend was failing to proxy the WebSocket request to the backend daemon.
*   **Actions:**
    1.  A custom server script (`kiosk/server.js`) was created to handle WebSocket proxying, as the standard Next.js `rewrites` are insufficient for WebSockets.
    2.  This custom server script went through multiple rounds of debugging to fix race conditions and logical errors.
*   **Result:** Despite multiple fixes, the "Connection Refused" error persisted, proving the proxy was a red herring. The architecture was simplified by removing the custom server and having the frontend connect directly to the backend's WebSocket port (9000).

## 3. The Breakthrough: `git grep` and Path Inconsistency

After exhausting all other options, a simple `git grep "/ws"` command provided the crucial insight.

*   **Discovery:** The repository contained conflicting WebSocket paths.
    *   The backend Axum server in `crates/daemon/src/server.rs` was listening on the path `/ws`.
    *   The frontend `EventStreamContext.tsx` was attempting to connect to `/ws`.
    *   However, multiple other files, including `API.md` and other frontend components, referenced the path `/ws/data`.

## 4. Final Hypothesis & Next Steps

*   **Current Hypothesis:** The root cause of the entire issue is a simple path mismatch. The backend server is listening on `/ws`, while the frontend application logic is ultimately trying to subscribe to and use data from a `/ws/data` endpoint that does not exist on the router.
*   **Next Steps:** When the session resumes, the plan is to:
    1.  **Align Paths:** Modify the backend route in `crates/daemon/src/server.rs` from `/ws` to `/ws/data`.
    2.  **Align Frontend Connection:** Modify the connection URL in `kiosk/src/context/EventStreamContext.tsx` from `/ws` to `/ws/data`.
    3.  This should finally resolve the connection issue and allow the application to function as intended.

## 5. The Saga Continues: Post-Path-Alignment Debugging

Even after aligning the backend and frontend paths to `/ws/data`, the `NS_ERROR_WEBSOCKET_CONNECTION_REFUSED` error persisted. This prompted a deeper, more systematic investigation to rule out all possible server-side issues.

### Attempt 3: Removing the Ghost of the Proxy Server

*   **Hypothesis:** Although the original plan was to remove the custom Node.js proxy server (`kiosk/server.js`), it was still being referenced in the production `start` script in `kiosk/package.json`, causing a conflict.
*   **Actions:**
    1.  Inspected `kiosk/package.json` and confirmed the `start` script was `"NODE_ENV=production node server.js"`.
    2.  Modified the script to be `"start": "next start"`, aligning the production environment with the intended architecture of connecting directly to the daemon.
*   **Result:** The error persisted. While fixing the `start` script was a necessary correction, it was not the root cause of the connection refusal.

### Attempt 4: Verifying the Server Binding Address

*   **Hypothesis:** The daemon was only listening on `127.0.0.1` (localhost), making it inaccessible from other machines on the network.
*   **Actions:**
    1.  Manually inspected the server startup code in `crates/daemon/src/server.rs`.
*   **Result:** The code explicitly binds to `let addr = SocketAddr::from(([0, 0, 0, 0], 9000));`. The server was already correctly configured to listen on all network interfaces. This hypothesis was incorrect.

### Attempt 5: Investigating the Firewall

*   **Hypothesis:** A firewall on the Raspberry Pi host was blocking incoming connections on port 9000.
*   **Actions:**
    1.  Attempted to check firewall status using `sudo ufw status` and `sudo iptables -L`.
*   **Result:** Neither `ufw` nor `iptables` were installed or active. There is no active firewall on the Raspberry Pi. This hypothesis was incorrect.

### Attempt 6: The Final Confirmation (Kernel-Level Check)

*   **Hypothesis:** To resolve the contradiction between a running server and a refused connection, we needed to bypass the application layer and ask the OS kernel directly if the port was open.
*   **Actions:**
    1.  Executed `ss -tlpn` on the Raspberry Pi.
*   **Result:** The command output provided **definitive proof** that the `eeg_daemon` process was successfully listening on `0.0.0.0:9000`.

## 6. Final Conclusion & Next Steps (As of this update)

*   **Current State:** All potential server-side and application-level causes have been systematically investigated and ruled out.
*   **Confirmed Facts:**
    1.  The backend Axum server is running.
    2.  It is correctly listening on `0.0.0.0:9000`.
    3.  The WebSocket route is correctly set to `/ws/data`.
    4.  The frontend is correctly configured to connect to `ws://raspberrypi.local:9000/ws/data`.
    5.  There is no firewall on the server blocking the connection.
*   **Final Hypothesis:** The connection refusal is **not** a server-side issue. The problem must exist on the client side or within the network infrastructure between the client and the server (e.g., a firewall on the client machine, a misconfigured router, or a client-side proxy).
*   **Next Steps:** The investigation must now shift entirely to the client's network environment.

## 7. The Great Refactoring & The Stubborn 500 Error

After the previous investigation hit a dead end, a new session was started with a fresh perspective, assuming all prior knowledge could be flawed. This led to a systematic, ground-up investigation that uncovered and fixed multiple, layered issues.

### Attempt 7: Full-Stack Code Audit (The Port Mismatch)

*   **Hypothesis:** The root cause was a simple configuration mismatch between the client and server.
*   **Actions:**
    1.  A full code audit was performed.
    2.  **Discovery 1:** The server route was confirmed to be `/ws/data` in `crates/daemon/src/server.rs`.
    3.  **Discovery 2:** The client was hardcoded to connect to port `9001` in `kiosk/src/components/EegDataHandler.tsx`.
    4.  The `ss -tlpn` command was used on the server to provide definitive proof that `eeg_daemon` was listening on port **`9000`**, not `9001`.
*   **Result:** The client-side port was corrected from `9001` to `9000`. The error persisted.

### Attempt 8: Isolating the Server with `websocat`

*   **Hypothesis:** The issue might be specific to the browser environment.
*   **Actions:**
    1.  `ping raspberrypi.local` was used to confirm network connectivity and DNS resolution, which succeeded.
    2.  `websocat ws://raspberrypi.local:9000/ws/data` was used to bypass the browser.
*   **Result:** This was a **major breakthrough**. `websocat` did not receive a "Connection Refused" error. Instead, it received a **`500 Internal Server Error`**. This proved the issue was a panic on the server, not a network or client-side problem.

### Attempt 9: Fixing the Startup Panics

*   **Hypothesis:** The server was panicking on startup, before logging could even initialize.
*   **Actions:**
    1.  A code review of `crates/daemon/src/main.rs` revealed a call to `.unwrap()` when building the `PipelineGraph`. This was patched with proper error handling.
    2.  With the patch in place, logs were finally generated. The logs immediately confirmed a **startup race condition**: the `WebSocketBroker` was being started *before* the pipeline it depended on was built.
    3.  The startup sequence in `main.rs` was reordered to be correct.
    4.  This reordering introduced a compiler error (`E0382` move error), which was subsequently fixed by correctly cloning the `sse_tx` channel.
*   **Result:** The application now compiled and started cleanly with a correct startup sequence. The `500` error, however, persisted.

### Attempt 10: The Final Fixes (Architectural and Middleware)

*   **Hypothesis:** With startup fixed, the panic had to be happening at runtime, within the connection handler itself.
*   **Actions:**
    1.  A deep-dive code review revealed a major architectural flaw: a `websocket_tx` sender was being passed around in the global `AppState`, violating the project's stated architecture and creating unpredictable state. This was removed.
    2.  The final remaining hypothesis was an incompatibility with the `tower-http` `TraceLayer` middleware, a known cause of panics on WebSocket upgrades. The `tower_http=debug` directive was removed from the logging configuration.
*   **Result:** Despite fixing all identified architectural flaws, race conditions, configuration errors, and middleware conflicts, the `500 Internal Server Error` **still persists**.

## 8. Current State & Next Steps (End of Session)

*   **Current State:** The `eeg_daemon` starts perfectly. The logs are clean. The architecture is sound. All previously identified bugs have been fixed. However, a connection attempt still causes an immediate, un-logged panic, resulting in a `500` error.
*   **Final Hypothesis:** There is a subtle, final bug hiding in the runtime logic of the `websocket_handler` in `server.rs` or the `handle_connection` function in `websocket_broker.rs` that is triggered only upon an actual connection.
*   **Next Steps:** The next session must begin with a line-by-line, exhaustive manual review of these specific functions to find the final logic error.