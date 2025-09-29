'use strict';
// Bridge: elata-eeg WebSocket broker (Axum) -> Render Socket.IO EEG namespace
// - Subscribes to local broker topic 'eeg_voltage'
// - Emits hello/meta to Render
// - Forwards binary EEG chunks as Float32Array buffers
//
// Env vars:
//   WS_BROKER_URL   ws://localhost:9000/ws/data
//   WS_TOPIC        eeg_voltage
//   RENDER_URL      https://pongo-multiplayer-server.onrender.com
//   SESSION_ID      subject-123
//
const WebSocket = require('ws');
const { io } = require('socket.io-client');

const WS_BROKER_URL = process.env.WS_BROKER_URL || 'ws://localhost:9000/ws/data';
const WS_TOPIC = process.env.WS_TOPIC || 'eeg_voltage';
const RENDER_URL = (process.env.RENDER_URL || '').replace(/\/$/, '');
const SESSION_ID = process.env.SESSION_ID || 'subject-123';
if (!RENDER_URL) {
  console.error('ERROR: Set RENDER_URL env (e.g., https://pongo-multiplayer-server.onrender.com)');
  process.exit(1);
}

console.log('[bridge] starting', { WS_BROKER_URL, WS_TOPIC, RENDER_URL, SESSION_ID });

// 1) Connect to Render Socket.IO namespace /eeg
const eeg = io(RENDER_URL + '/eeg', { transports: ['websocket'] });
let helloSent = false;
let lastMeta = null;

eeg.on('connect', () => {
  console.log('[render] connected', eeg.id);
  maybeSendHello();
});

eeg.on('connect_error', (e) => console.error('[render] error', e.message));

eeg.on('disconnect', (r) => {
  console.log('[render] disconnect', r);
  helloSent = false;
});

function maybeSendHello() {
  if (!eeg.connected || !lastMeta || helloSent) return;
  const channels = Array.isArray(lastMeta.meta?.channel_names) ? lastMeta.meta.channel_names : [];
  const srate = Number(lastMeta.meta?.sample_rate) || undefined;
  const deviceId = String(lastMeta.meta?.source_type || 'elata-eeg');
  if (!channels.length || !srate) return;
  eeg.emit('hello', { sessionId: SESSION_ID, channels, srate, units: 'uV', deviceId }, (ack) => {
    console.log('[render] hello ack', ack);
    if (ack && ack.ok) helloSent = true;
  });
}

// 2) Connect to local broker and subscribe
const ws = new WebSocket(WS_BROKER_URL);
ws.on('open', () => {
  console.log('[broker] connected');
  const sub = { type: 'subscribe', topic: WS_TOPIC, epoch: 0 };
  ws.send(JSON.stringify(sub));
});

ws.on('message', (data, isBinary) => {
  if (!isBinary) {
    try {
      const msg = JSON.parse(data.toString());
      if (msg.message_type === 'meta_update' && msg.topic === WS_TOPIC) {
        lastMeta = msg;
        console.log('[broker] meta_update', { channels: msg.meta?.channel_names?.length, srate: msg.meta?.sample_rate });
        maybeSendHello();
      }
    } catch (_) {}
    return;
  }

  // Binary frame: [u32_be header_len][header_json][samples]
  const buf = Buffer.from(data); // Node Buffer
  if (buf.length < 4) return;
  const headerLen = buf.readUInt32BE(0);
  if (buf.length < 4 + headerLen) return;
  const headerJson = buf.slice(4, 4 + headerLen).toString('utf8');
  let header; try { header = JSON.parse(headerJson); } catch { return; }
  const samplesBuf = buf.slice(4 + headerLen);

  // Interpret samples as Float32 for Voltage/VoltageF32
  if (header.packet_type === 'Voltage' || header.packet_type === 'VoltageF32') {
    // Avoid copying: send binary buffer directly to Render as chunk
    eeg.emit('eeg:chunk', samplesBuf);
  } else {
    // Fallback: forward as JSON numbers (may be large)
    const arr = Array.from(new Float32Array(samplesBuf.buffer, samplesBuf.byteOffset, samplesBuf.byteLength / 4));
    eeg.emit('eeg:sample', { t: Date.now(), samples: arr });
  }
});

ws.on('close', () => console.log('[broker] closed'));
ws.on('error', (e) => console.error('[broker] error', e.message));

