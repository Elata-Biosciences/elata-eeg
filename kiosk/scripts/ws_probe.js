// Quick probe to read one header from the EEG WebSocket and print num_channels
const WebSocket = require('ws');

const url = process.env.WS_URL || 'ws://127.0.0.1:9000/ws/data';
const topic = process.env.TOPIC || 'eeg_voltage';
const epoch = Number(process.env.EPOCH || 1);

const ws = new WebSocket(url);

let done = false;

function exitOnce(code) {
  if (!done) {
    done = true;
    setTimeout(() => process.exit(code), 0);
  }
}

ws.on('open', () => {
  ws.send(JSON.stringify({ type: 'subscribe', topic, epoch }));
});

ws.on('message', (data) => {
  try {
    if (typeof data === 'string') {
      // Meta or FFT JSON; ignore
      return;
    }
    const buf = Buffer.isBuffer(data) ? data : Buffer.from(data);
    if (buf.length < 8) return;
    const jsonLen = buf.readUInt32BE(0);
    const jsonStr = buf.slice(4, 4 + jsonLen).toString('utf8');
    const header = JSON.parse(jsonStr);
    const out = {
      topic: header.topic,
      num_channels: header.num_channels,
      packet_type: header.packet_type,
      batch_size: header.batch_size,
    };
    console.log(JSON.stringify(out));
    ws.close();
    exitOnce(0);
  } catch (e) {
    console.error('parse_error', String(e));
    exitOnce(2);
  }
});

ws.on('error', (e) => {
  console.error('ws_error', e.message || String(e));
  exitOnce(2);
});

setTimeout(() => {
  console.error('timeout');
  try { ws.close(); } catch {}
  exitOnce(3);
}, 6000);

