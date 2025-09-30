#!/usr/bin/env python3
# stream_fft.py — Live FFT/PSD from DataHub WebSocket (log10(µV^2/Hz))
# Python 3.9+
#
# Requirements:
#   pip install websockets numpy matplotlib
#
# Example:
#   python3 stream_fft.py --host raspberrypi.local --topic eeg_voltage --epoch 1 --fs 250 --fmax 60 --ch 1

import argparse
import asyncio
import json
import struct
import threading
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple, Union

import numpy as np
import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation

# ----------------------------
# Helpers
# ----------------------------

def periodogram_log10_uv2_per_hz(x_v: np.ndarray, fs: float, window: str = "hann"):
    """
    One-sided periodogram PSD with log10(µV^2/Hz).
    - mean removal
    - window: hann|hamming|rect
    - proper window-power normalization
    - doubles non-DC/non-Nyquist bins
    """
    if fs <= 0 or x_v.size < 8:
        return np.array([0.0, 1.0]), np.array([np.nan, np.nan])

    x = (x_v - np.mean(x_v)) * 1e6  # V -> µV
    n = x.size

    wname = (window or "hann").lower()
    if wname in ("hann", "hanning"):
        w = np.hanning(n)
    elif wname == "hamming":
        w = np.hamming(n)
    else:
        w = np.ones(n)

    xw = x * w
    Y = np.fft.rfft(xw)
    freqs = np.fft.rfftfreq(n, d=1.0 / fs)

    U = float(np.sum(w**2)) or float(n)
    Pxx = (np.abs(Y)**2) / (fs * U)

    if n % 2 == 0:
        if Pxx.size > 2:
            Pxx[1:-1] *= 2.0
    else:
        if Pxx.size > 1:
            Pxx[1:] *= 2.0

    with np.errstate(divide="ignore"):
        P_log = np.log10(Pxx + 1e-20)

    return freqs, P_log

def ws_binary_to_array(header: Dict[str, Any], payload: bytes) -> np.ndarray:
    """Decode DataHub binary frame to (batch, channels) float array."""
    dtype = "<i4" if header.get("packet_type") == "RawI32" else "<f4"
    data = np.frombuffer(payload, dtype=dtype)
    ch = int(header.get("num_channels") or 1)
    bs = int(header.get("batch_size") or max(1, len(data)//max(1, ch)))
    arr = data.reshape(bs, ch, order="C")
    return arr.astype(np.float32, copy=False)

def extract_fs(header: Dict[str, Any], meta: Optional[Dict[str, Any]], default: float) -> float:
    for d in (header, meta or {}):
        for k in ("fs", "sample_rate", "sampling_rate", "sr", "hz"):
            if k in d:
                try:
                    return float(d[k])
                except Exception:
                    pass
    return float(default)

def extract_names(header: Dict[str, Any], meta: Optional[Dict[str, Any]], num_channels: int) -> List[str]:
    for d in (meta or {}, header):
        for k in ("channels", "channel_names", "labels", "ch_names"):
            names = d.get(k)
            if isinstance(names, (list, tuple)) and len(names) == num_channels:
                return [str(x) for x in names]
    return [f"ch{i+1}" for i in range(num_channels)]

# ----------------------------
# Shared streaming state
# ----------------------------

@dataclass
class State:
    fs: float = 250.0
    num_channels: int = 1
    ch_names: List[str] = field(default_factory=lambda: ["ch1"])
    window_secs: float = 2.0
    buf_per_ch: List[deque] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def set_layout(self, num_channels: int, fs: float):
        maxlen = int(max(1, round(self.window_secs * fs)))
        if not self.buf_per_ch or len(self.buf_per_ch) != num_channels:
            self.buf_per_ch = [deque(maxlen=maxlen) for _ in range(num_channels)]
        else:
            for i in range(num_channels):
                recent = list(self.buf_per_ch[i])[-maxlen:]
                self.buf_per_ch[i] = deque(recent, maxlen=maxlen)

# ----------------------------
# WebSocket consumer (background thread)
# ----------------------------

def start_ws_thread(host: str, topic: str, epoch: int, state: State):
    meta_map: Dict[str, Dict[str, Any]] = {}

    async def ws_main():
        url = f"ws://{host}:9000/ws/data"
        backoff = 1.0
        while True:
            try:
                print(f"[WS] connect {url}")
                async with __import__("websockets").connect(url, max_size=None) as ws:
                    await ws.send(json.dumps({"type": "subscribe", "topic": topic, "epoch": epoch}))
                    backoff = 1.0
                    async for msg in ws:
                        if isinstance(msg, str):
                            obj = json.loads(msg)
                            if obj.get("message_type") == "meta_update":
                                meta_map[obj["topic"]] = obj["meta"]
                            continue

                        # Binary frame: [4B big-endian header_len][header JSON][payload bytes]
                        view = memoryview(msg)
                        (hlen,) = struct.unpack(">I", view[:4])
                        header = json.loads(view[4:4+hlen].tobytes())
                        payload = view[4+hlen:].tobytes()

                        samples = ws_binary_to_array(header, payload)  # (batch, ch)
                        meta = meta_map.get(header.get("topic", ""), {})

                        fs_new = extract_fs(header, meta, state.fs)
                        ch = samples.shape[1] if samples.ndim == 2 else 1
                        names = extract_names(header, meta, ch)

                        with state.lock:
                            layout_changed = (abs(fs_new - state.fs) > 1e-9) or (ch != state.num_channels)
                            if layout_changed:
                                state.fs = fs_new
                                state.num_channels = ch
                                state.ch_names = names
                                state.set_layout(ch, fs_new)

                            if samples.ndim == 1:
                                samples = samples[:, None]

                            # Append to deques
                            for i in range(state.num_channels):
                                state.buf_per_ch[i].extend(samples[:, i].tolist())

            except Exception as e:
                print(f"[WS] error: {e} (retrying in {backoff:.1f}s)")
                await asyncio.sleep(backoff)
                backoff = min(10.0, backoff * 1.5)

    def runner():
        try:
            asyncio.run(ws_main())
        except Exception as e:
            print(f"[WS] thread died: {e}")

    t = threading.Thread(target=runner, daemon=True)
    t.start()
    return t

# ----------------------------
# Live plotter
# ----------------------------

class LiveFFT:
    def __init__(self, state: State, ch_index: int, fmax: float, window: str):
        self.state = state
        self.ch_index = max(0, ch_index)
        self.fmax = fmax if (fmax is None or fmax > 0) else None
        self.window = window

        self.fig, self.ax = plt.subplots(figsize=(8, 5))
        (self.line,) = self.ax.plot([], [], linewidth=1.5)
        self.ax.set_xlabel("Frequency (Hz)")
        self.ax.set_ylabel("log10(µV²/Hz)")
        self.ax.grid(True, which="both", linestyle="--", alpha=0.4)

        self.fig.canvas.mpl_connect("key_press_event", self.on_key)
        self.ani = FuncAnimation(self.fig, self._update, interval=100, blit=False)

    def on_key(self, ev):
        if ev.key in ("left", "j"):
            with self.state.lock:
                self.ch_index = (self.ch_index - 1) % max(1, self.state.num_channels)
        elif ev.key in ("right", "l"):
            with self.state.lock:
                self.ch_index = (self.ch_index + 1) % max(1, self.state.num_channels)

    def _update(self, _frame):
        with self.state.lock:
            fs = float(self.state.fs)
            nch = self.state.num_channels
            ch_names = self.state.ch_names
            ch = self.ch_index if nch else 0
            buf = np.array(self.state.buf_per_ch[ch], dtype=np.float64) if (nch and self.state.buf_per_ch) else np.array([])

        freqs, logP = periodogram_log10_uv2_per_hz(buf, fs, window=self.window)

        if self.fmax is not None and freqs.size:
            m = freqs <= self.fmax
            freqs = freqs[m]
            logP = logP[m]

        if freqs.size and logP.size:
            self.line.set_data(freqs, logP)
            xmin, xmax = 0.0, (self.fmax if self.fmax is not None else float(freqs[-1]))
            self.ax.set_xlim(xmin, max(1e-3, xmax))
            finite = np.isfinite(logP)
            if finite.any():
                lo, hi = np.nanmin(logP[finite]), np.nanmax(logP[finite])
                if np.isfinite(lo) and np.isfinite(hi) and lo != hi:
                    pad = 0.1 * (hi - lo)
                    self.ax.set_ylim(lo - pad, hi + pad)
        else:
            self.line.set_data([], [])

        title = f"Live FFT — ch {self.ch_index+1}/{max(1,nch)} ({ch_names[self.ch_index] if self.ch_index < len(ch_names) else 'n/a'}) | fs={fs:.2f} Hz | win={self.window}"
        self.ax.set_title(title)
        return self.line,

    def show(self):
        plt.tight_layout()
        plt.show()

# ----------------------------
# CLI
# ----------------------------

def parse_args():
    ap = argparse.ArgumentParser(description="Live FFT/PSD from DataHub WebSocket (log10(µV^2/Hz)).")
    ap.add_argument("--host", default="raspberrypi.local", help="Data host (ws://HOST:9000/ws/data).")
    ap.add_argument("--topic", default="eeg_voltage", help="Topic to subscribe.")
    ap.add_argument("--epoch", type=int, default=1, help="Epoch id.")
    ap.add_argument("--fs", type=float, default=250.0, help="Fallback sample rate if none in header/meta.")
    ap.add_argument("--window", default="hann", help="Window: hann | hamming | rect")
    ap.add_argument("--fmax", type=float, default=60.0, help="Max frequency to display (Hz). 0 = auto")
    ap.add_argument("--secs", type=float, default=2.0, help="Rolling window length (seconds).")
    ap.add_argument("--ch", type=int, default=1, help="Initial 1-based channel to display (←/→ to change).")
    return ap.parse_args()

def main():
    args = parse_args()
    state = State(fs=args.fs, window_secs=args.secs)
    state.set_layout(1, args.fs)  # bootstrap a single buffer until meta arrives

    start_ws_thread(args.host, args.topic, args.epoch, state)

    plot = LiveFFT(
        state=state,
        ch_index=max(0, args.ch - 1),
        fmax=(None if (args.fmax is not None and args.fmax <= 0) else args.fmax),
        window=args.window,
    )
    print(
        "Controls:\n"
        "  ← / j : previous channel\n"
        "  → / l : next channel\n"
        f"  Window: {args.secs}s | fmax: {args.fmax} Hz | power: log10(µV²/Hz)\n"
    )
    plot.show()

if __name__ == "__main__":
    main()
