#!/usr/bin/env python3
# main.py (Python 3.9 compatible, log10(uV^2/Hz) scale)

import asyncio
import json
import struct
import threading
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Dict, Iterable, Tuple, Optional, List, Union

import numpy as np
import websockets
import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation
import argparse
import time

# ----------------------------
# WebSocket helpers (from your IO)
# ----------------------------
MetaMap = Dict[str, Dict[str, Any]]
PacketHandler = Callable[
    [Dict[str, Any], np.ndarray, Optional[Dict[str, Any]]],
    Optional[Awaitable[None]]
]

async def start_data_ws(
    host: str,
    subscriptions: Iterable[Tuple[str, int]],
    on_packet: PacketHandler,
) -> None:
    url = f"ws://{host}:9000/ws/data"
    metadata: MetaMap = {}

    async with websockets.connect(url, max_size=None) as ws:
        for topic, epoch in subscriptions:
            await ws.send(json.dumps({"type": "subscribe", "topic": topic, "epoch": epoch}))

        async for message in ws:
            kind, payload = ws_data_received(message, metadata)
            if kind == "samples":
                header, samples, meta = payload
                result = on_packet(header, samples, meta)
                if asyncio.iscoroutine(result):
                    await result

def ws_data_received(
    message: Union[str, bytes],
    metadata: MetaMap,
) -> Tuple[str, Any]:
    if isinstance(message, str):
        obj = json.loads(message)
        if obj.get("message_type") == "meta_update":
            metadata[obj["topic"]] = obj["meta"]
            return "meta", obj
        return "control", obj

    view = memoryview(message)
    (header_len,) = struct.unpack(">I", view[:4])
    header = json.loads(view[4:4 + header_len].tobytes())
    payload = view[4 + header_len :].tobytes()
    samples = ws_data_to_np_array(header, payload)
    meta = metadata.get(header["topic"])
    return "samples", (header, samples, meta)

def ws_data_to_np_array(header: Dict[str, Any], payload: bytes) -> np.ndarray:
    dtype = "<i4" if header.get("packet_type") == "RawI32" else "<f4"
    data = np.frombuffer(payload, dtype=dtype)
    channels = header.get("num_channels", 0) or 1
    batch = header.get("batch_size", 0) or max(1, len(data) // channels)
    return data.reshape(batch, channels, order="C")

# ----------------------------
# Live FFT plotter
# ----------------------------

@dataclass
class SharedState:
    fs: float = 250.0
    num_channels: int = 1
    chan_names: List[str] = field(default_factory=list)
    window_secs: float = 5.0
    buffers: List[deque] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)
    last_meta: Optional[Dict[str, Any]] = None

    def ensure_buffers(self, num_channels: int, fs: float):
        with self.lock:
            maxlen = int(max(1, round(self.window_secs * fs)))
            if not self.buffers or len(self.buffers) != num_channels:
                self.buffers = [deque(maxlen=maxlen) for _ in range(num_channels)]
            else:
                for i in range(num_channels):
                    old = list(self.buffers[i])[-maxlen:]
                    self.buffers[i] = deque(old, maxlen=maxlen)

def extract_fs(header: Dict[str, Any], meta: Optional[Dict[str, Any]], default: float) -> float:
    for d in (header, meta or {}):
        for k in ("fs", "sample_rate", "sampling_rate", "sr", "hz"):
            if k in d:
                try:
                    return float(d[k])
                except Exception:
                    pass
    return default

def extract_names(header: Dict[str, Any], meta: Optional[Dict[str, Any]], num_channels: int) -> List[str]:
    names: Optional[List[str]] = None
    for d in (meta or {}, header):
        for k in ("channels", "channel_names", "labels", "ch_names"):
            if k in d and isinstance(d[k], (list, tuple)) and len(d[k]) > 0:
                names = [str(x) for x in d[k]]
                break
        if names:
            break
    if not names or len(names) != num_channels:
        names = [f"ch{i+1}" for i in range(num_channels)]
    return names

class LiveFFT:
    def __init__(self, state: SharedState, topic: str, initial_channel: int = 0, fmax: Optional[float] = None):
        self.state = state
        self.topic = topic
        self.channel = initial_channel
        self.fmax = fmax

        self.fig, self.ax = plt.subplots(figsize=(8, 5))
        (self.line,) = self.ax.plot([], [], lw=1.5)
        self.ax.set_xlabel("Frequency (Hz)")
        self.ax.set_ylabel("log10(µV²/Hz)")
        self.ax.set_title("Live FFT — connecting...")
        self.ax.grid(True, which="both", linestyle="--", alpha=0.4)

        self.fig.canvas.mpl_connect("key_press_event", self.on_key)
        self.ani = FuncAnimation(self.fig, self.update, interval=100, blit=False)

    def on_key(self, event):
        if event.key in ("left", "j"):
            self.channel = (self.channel - 1) % max(1, self.state.num_channels)
        elif event.key in ("right", "l"):
            self.channel = (self.channel + 1) % max(1, self.state.num_channels)

    def compute_fft(self, x: np.ndarray, fs: float):
        if len(x) < 8:
            return np.array([0.0, 1.0]), np.array([np.nan, np.nan])

        x = x - np.mean(x)
        w = np.hanning(len(x))
        xw = x * w

        Y = np.fft.rfft(xw)
        freqs = np.fft.rfftfreq(len(xw), d=1.0 / fs)

        # Power spectral density
        P = (np.abs(Y) ** 2) / max(1, len(xw))
        with np.errstate(divide="ignore"):
            P = np.log10(P + 1e-20)

        return freqs, P

    def update(self, _frame):
        with self.state.lock:
            fs = float(self.state.fs)
            nch = self.state.num_channels
            ch_names = self.state.chan_names or [f"ch{i+1}" for i in range(nch)]
            if self.channel >= nch:
                self.channel = 0
            buf = np.array(self.state.buffers[self.channel], dtype=np.float64) if self.state.buffers else np.array([])

        freqs, P = self.compute_fft(buf, fs)

        xmask = np.ones_like(freqs, dtype=bool)
        if self.fmax is not None:
            xmask = freqs <= self.fmax

        self.line.set_data(freqs[xmask], P[xmask])

        if freqs.size > 1:
            xmax = (self.fmax if self.fmax is not None else freqs[-1])
            self.ax.set_xlim(0, max(1e-3, xmax))
            if np.isfinite(P[xmask]).any():
                ymin = np.nanmin(P[xmask])
                ymax = np.nanmax(P[xmask])
                if np.isfinite(ymin) and np.isfinite(ymax) and ymin != ymax:
                    pad = 0.1 * (ymax - ymin)
                    self.ax.set_ylim(ymin - pad, ymax + pad)

        title = f"Live FFT — {self.topic} | fs={fs:.2f} Hz | chan {self.channel+1}/{nch} ({ch_names[self.channel]})"
        self.ax.set_title(title)
        return self.line,

    def show(self):
        plt.tight_layout()
        plt.show()

# ----------------------------
# WS consumer -> buffers
# ----------------------------

def run_ws_in_thread(host: str, topic: str, epoch: int, state: SharedState):
    async def handle_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
        fs_new = extract_fs(header, meta, state.fs)
        num_channels = int(header.get("num_channels") or (samples.shape[1] if samples.ndim == 2 else 1))
        names = extract_names(header, meta, num_channels)

        changed = (abs(fs_new - state.fs) > 1e-6) or (num_channels != state.num_channels) or (names != state.chan_names)
        if changed:
            state.fs = fs_new
            state.num_channels = num_channels
            state.chan_names = names
            state.ensure_buffers(num_channels, fs_new)

        if samples.ndim == 1:
            samples = samples[:, None]
        with state.lock:
            for ch in range(min(state.num_channels, samples.shape[1])):
                state.buffers[ch].extend(samples[:, ch].tolist())

    async def ws_main():
        await start_data_ws(host, [(topic, epoch)], handle_packet)

    def runner():
        try:
            asyncio.run(ws_main())
        except Exception as e:
            print(f"[WS thread] error: {e}")

    t = threading.Thread(target=runner, daemon=True)
    t.start()
    return t

# ----------------------------
# CLI / Main
# ----------------------------

def parse_args():
    ap = argparse.ArgumentParser(description="Live FFT plot from streaming EEG WebSocket")
    ap.add_argument("--host", default="raspberrypi.local", help="Data host")
    ap.add_argument("--topic", default="eeg_voltage", help="Topic name")
    ap.add_argument("--epoch", type=int, default=1, help="Epoch number")
    ap.add_argument("--window", type=float, default=2.0, help="Rolling window length (s)")
    ap.add_argument("--channel", type=int, default=0, help="Initial channel index (0-based)")
    ap.add_argument("--fmax", type=float, default=60.0, help="Max frequency to display (Hz)")
    return ap.parse_args()

def main():
    args = parse_args()
    state = SharedState(window_secs=args.window)
    state.ensure_buffers(num_channels=1, fs=state.fs)

    run_ws_in_thread(args.host, args.topic, args.epoch, state)

    plot = LiveFFT(
        state=state,
        topic=args.topic,
        initial_channel=max(0, args.channel),
        fmax=args.fmax,
    )
    print(
        "Controls:\n"
        "  ← / j : previous channel\n"
        "  → / l : next channel\n"
        f"  Window: {args.window}s | fmax: {args.fmax} Hz | power: log10(µV²/Hz)\n"
    )
    plot.show()

if __name__ == "__main__":
    main()
