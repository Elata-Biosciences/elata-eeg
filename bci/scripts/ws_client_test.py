import os
import json
import struct
import argparse
import websocket  # pip install websocket-client

try:
    import numpy as np  # Optional for parsing samples
except Exception:
    np = None


def parse_binary_packet(data: bytes):
    if len(data) < 8:
        return None, None
    json_len = struct.unpack(">I", data[:4])[0]
    header_bytes = data[4:4 + json_len]
    header = json.loads(header_bytes.decode("utf-8"))
    payload = data[4 + json_len:]
    return header, payload


def on_message(ws, message):
    # Text messages: subscribed ack or meta updates
    if isinstance(message, str):
        try:
            obj = json.loads(message)
            print(f"TEXT: {obj}")
        except Exception:
            print(f"TEXT: {message}")
        return

    # Binary messages: [u32_be json_len][json][samples]
    if isinstance(message, (bytes, bytearray)):
        header, payload = parse_binary_packet(message)
        if header is None:
            print(f"BIN: too short ({len(message)} bytes)")
            return
        topic = header.get("topic")
        pkt_type = header.get("packet_type")
        num_channels = header.get("num_channels")
        batch_size = header.get("batch_size")
        ts_ns = header.get("ts_ns")
        meta_rev = header.get("meta_rev")
        print(f"BIN: topic={topic} type={pkt_type} ch={num_channels} batch={batch_size} ts={ts_ns} meta_rev={meta_rev} bytes={len(payload)}")

        # Best-effort sample peek without spamming
        if np is not None and len(payload) >= 4:
            if pkt_type in ("RawI32", "Raw"):
                arr = np.frombuffer(payload, dtype="<i4", count=min(16, len(payload)//4))
            else:  # Voltage or VoltageF32
                arr = np.frombuffer(payload, dtype="<f4", count=min(16, len(payload)//4))
            print(f"  samples[0:{arr.size}]:", arr.tolist())
        return

    print("Unknown message type", type(message))


def on_error(ws, error):
    print(f"Error: {error}")


def on_close(ws, close_status_code, close_msg):
    print("### closed ###")


def on_open_factory(topic: str, epoch: int):
    def on_open(ws):
        print("Opened connection; subscribing...")
        sub = {"type": "subscribe", "topic": topic, "epoch": epoch}
        ws.send(json.dumps(sub))
        print(f"Sent subscribe: {sub}")
    return on_open


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="EEG WebSocket client test")
    parser.add_argument("--url", default=os.environ.get("WS_URL", "ws://127.0.0.1:9000/ws/data"))
    parser.add_argument("--topic", default=os.environ.get("TOPIC", "eeg_voltage"))
    parser.add_argument("--epoch", type=int, default=int(os.environ.get("EPOCH", 1)))
    args = parser.parse_args()

    ws_app = websocket.WebSocketApp(
        args.url,
        on_open=on_open_factory(args.topic, args.epoch),
        on_message=on_message,
        on_error=on_error,
        on_close=on_close,
    )

    # For Pi over LAN, you might need ping_interval to keep NAT alive
    ws_app.run_forever(ping_interval=20, ping_timeout=10)
