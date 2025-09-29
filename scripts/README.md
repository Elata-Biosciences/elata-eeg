# Stream EEG from elata-eeg to the Render Socket.IO server (Pong)

This guide shows how to forward real‑time EEG from your local elata‑eeg device to the deployed Render Socket.IO server so a Pong client (or any observer) can consume it.

What you’ll do:
- Run the elata‑eeg daemon and pipeline with a WebSocket sink
- Start a small Node bridge that subscribes to the daemon’s broker and forwards data to Render’s /eeg namespace
- Observe the stream from a browser (or your Pong client)

---

## 0) Requirements
- macOS/Linux or WSL
- Node.js 18+ installed (`node -v`)
- elata‑eeg built and runnable on your device
- Your Render URL for the Pong server, e.g.:
  - https://pongo-multiplayer-server.onrender.com

Optional (for browser testing):
- A modern browser with DevTools

---

## 1) Start the elata‑eeg daemon and broker
The daemon exposes a WebSocket broker at `ws://localhost:9000/ws/data`.

Common ways to start it (pick one):
- `./run_backend.sh` (if provided by your setup)
- `cargo run -p daemon` (from the repo root)

You should see logs indicating the server is listening on port 9000.

---

## 2) Ensure your pipeline publishes EEG over the broker
Use a pipeline that includes a `websocket_sink` with topic `eeg_voltage`. Example excerpt (pipelines/v2.yaml):

```
- name: "websocket_sink"
  type: "websocket_sink"
  params:
    topic: "eeg_voltage"
  inputs:
    - "filter.filtered_data"
```

Start the pipeline however you normally do in your environment. Once the device is streaming, the broker will emit:
- JSON `meta_update` once (or when metadata changes)
- Binary `data_packet` frames containing float32 EEG samples

---

## 3) Install bridge dependencies
The bridge script lives here:
- elata-eeg/scripts/forward_to_render.js

Install dependencies once:

```
cd elata-eeg/scripts
npm init -y
npm install ws socket.io-client
```

---

## 4) Run the bridge to Render
Set these environment variables and run the script:
- RENDER_URL: Your Render service base URL (no trailing slash)
- SESSION_ID: A label you pick for this stream (e.g., subject-123)
- WS_BROKER_URL: (optional) defaults to ws://localhost:9000/ws/data
- WS_TOPIC: (optional) defaults to eeg_voltage

Example:

```
RENDER_URL=https://pongo-multiplayer-server.onrender.com \
SESSION_ID=subject-123 \
node forward_to_render.js
```

What to expect in logs:
- [broker] connected → subscription sent
- [broker] meta_update … → bridge sends a hello to Render with channels+srate
- [render] hello ack { ok: true, sessionId: … }
- Continuous forwarding of binary EEG chunks to Render

---

## 5) Observe the stream (smoke test)
Option A: Use the provided observer test page
- Open `Pongo/test_socket_observer.html` in a browser
- Paste your Render URL and the same SESSION_ID you used in the bridge
- Click Connect → Observe. You should see meta and incoming sample/chunk logs

Option B: Quick DevTools snippet on any page
1) Open DevTools → Console
2) Paste to load Socket.IO client:

```
var s=document.createElement('script');s.src='https://cdn.socket.io/4.7.5/socket.io.min.js';document.head.appendChild(s);
```

3) Connect and observe:

```
const eeg=io('https://pongo-multiplayer-server.onrender.com/eeg',{transports:['websocket']});
eeg.on('connect',()=>eeg.emit('observe','subject-123',(ack)=>console.log('observe ack',ack)));
eeg.on('meta',(m)=>console.log('meta',m));
eeg.on('eeg:chunk',(buf)=>{const f32=new Float32Array(buf.buffer,buf.byteOffset||0,(buf.byteLength||buf.length)/4);console.log('chunk f32 len',f32.length)});
```

You should see an observe ack, then meta, and chunks arriving.

---

## 6) Using the stream in your Pong client
- Connect to the same namespace/session as in the smoke test
- Listen for `meta` once
- Use `eeg:chunk` (binary Float32) for efficiency. Convert:
  - `new Float32Array(buf.buffer, buf.byteOffset||0, (buf.byteLength||buf.length)/4)`
- Compute a control value (e.g., bandpower, amplitude) and map to paddle Y with smoothing

---

## 7) Troubleshooting
- Health check: open `https://pongo-multiplayer-server.onrender.com/health` → `ok`
- No hello ack on Render:
  - Ensure the bridge printed a `[broker] meta_update` before sending hello (the bridge needs channel names + sample_rate)
  - Confirm your pipeline emits `websocket_sink` with `topic: eeg_voltage`
- No data arriving at observer:
  - Verify the bridge logs no errors and stays connected to both broker and Render
  - Confirm the observer uses the exact same SESSION_ID as the bridge
  - Check firewall/VPN blocking websockets
- CORS:
  - If you load the observer test from `file://`, your origin is `null` and may be blocked if CORS is strict. Prefer serving over `http://localhost` or temporarily allowing `*` while testing.

---

## 8) Advanced
- Override defaults:
  - `WS_BROKER_URL=ws://localhost:9000/ws/data`
  - `WS_TOPIC=eeg_voltage`
- The bridge will automatically resend hello after reconnects, once it sees a new meta update.

---

## 9) Stop everything
- Stop the bridge: Ctrl+C
- Stop the pipeline/daemon with your usual command or Ctrl+C

---

## Appendix: Bridge script location
- File: `elata-eeg/scripts/forward_to_render.js`
- Key behavior:
  - Subscribes to broker, waits for `meta_update`, sends `hello` to Render with `{sessionId, channels, srate}`
  - Forwards binary Float32 EEG chunks as `eeg:chunk` to `/eeg`

