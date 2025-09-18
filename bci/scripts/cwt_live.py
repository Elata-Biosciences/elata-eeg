#!/usr/bin/env python3
# cwt_live.py — Real-time CWT with COI masking + log power + robust scaling
import argparse
import asyncio
import threading
from collections import deque
from typing import Any, Dict, Iterable, Tuple, Optional, List

import numpy as np
import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation
import pywt  # pip install pywavelets

from ws_client import start_data_ws

class SharedState:
    def __init__(self, fs: float, window_secs: float, chan_index: int):
        self.fs = fs
        self.window_secs = window_secs
        self.chan_index = chan_index
        self.lock = threading.Lock()
        self.buf = deque(maxlen=int(window_secs * fs))
        self.chan_names: List[str] = []
        self.last_meta: Optional[Dict[str, Any]] = None

    def maybe_update_from_meta(self, meta: Optional[Dict[str, Any]]):
        if not meta:
            return
        self.last_meta = meta
        fs = meta.get("fs") or meta.get("sample_rate") or self.fs
        if fs != self.fs and fs > 0:
            with self.lock:
                self.fs = float(fs)
                old = np.array(self.buf, dtype=np.float32)
                self.buf = deque(old, maxlen=int(self.window_secs * self.fs))
        if "chan_names" in meta and isinstance(meta["chan_names"], list):
            self.chan_names = meta["chan_names"]

def make_packet_handler(state: SharedState):
    def on_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
        state.maybe_update_from_meta(meta)
        ch = state.chan_index
        if samples.ndim == 2 and samples.shape[1] > ch:
            x = samples[:, ch].astype(np.float32, copy=False)
        else:
            x = samples.reshape(-1).astype(np.float32, copy=False)
        with state.lock:
            state.buf.extend(x)
    return on_packet

def run_plot(host: str, topic: str, epoch: int, fs: float, window: float, chan: int,
             fmin: float, fmax: float, n_freqs: int, use_zscore: bool, hp_demean: bool):
    state = SharedState(fs=fs, window_secs=window, chan_index=chan)

    freqs = np.linspace(fmin, fmax, n_freqs).astype(np.float64)
    wavelet = 'morl'
    def compute_scales(dt: float) -> np.ndarray:
        cf = pywt.central_frequency(wavelet)
        return (cf / (freqs * dt))

    fig, ax = plt.subplots(figsize=(10, 5))
    ax.set_title("Real-time CWT (channel 0)")
    ax.set_xlabel("Time (s)")
    ax.set_ylabel("Frequency (Hz)")
    # pre-allocate image
    img = ax.imshow(
        np.zeros((n_freqs, max(2, int(fs*window))), dtype=np.float32),
        origin="lower", aspect="auto",
        extent=[-window, 0, freqs[0], freqs[-1]],
        interpolation="nearest", animated=True,
    )
    cbar = plt.colorbar(img, ax=ax, pad=0.01)
    cbar.set_label("Log power (|coeff|^2, dB-like)")

    # start WS thread
    def ws_thread():
        asyncio.run(start_data_ws(host=host, subscriptions=[(topic, epoch)],
                                  on_packet=make_packet_handler(state)))
    threading.Thread(target=ws_thread, daemon=True).start()

    def update(_frame):
        with state.lock:
            buf = np.array(state.buf, dtype=np.float32)
            cur_fs = state.fs
            ch_label = state.chan_names[state.chan_index] if (state.chan_names and 0 <= state.chan_index < len(state.chan_names)) else None

        if buf.size < int(0.5 * cur_fs):
            return img,

        dt = 1.0 / cur_fs

        # light DC / slow drift removal if requested
        x = buf
        if hp_demean:
            x = x - np.median(x)

        # compute CWT
        scales = compute_scales(dt)
        coeffs, _ = pywt.cwt(x, scales, wavelet, sampling_period=dt)  # (n_scales, n_samples)

        # power -> log power
        power = (np.abs(coeffs) ** 2).astype(np.float32)
        power = np.log10(power + 1e-12)

        # ---- Cone of Influence (mask edges that are inside ~sqrt(2)*scale) ----
        n = power.shape[1]
        t = np.arange(n, dtype=np.float64) * dt
        # distance (seconds) to nearest edge for each time column
        dist = np.minimum(t, (n - 1) * dt - t)
        # COI for each scale (seconds)
        coi = np.sqrt(2.0) * (scales * dt)  # scales already in samples/dt; multiply by dt -> seconds
        # build mask: True where inside COI (to drop)
        # shape broadcast: (n_scales, 1) vs (1, n_time)
        mask = (coi[:, None] > dist[None, :])
        power_masked = power.copy()
        power_masked[mask] = np.nan  # ignore edges in scaling

        # keep only last "window" secs
        max_cols = int(window * cur_fs)
        if power_masked.shape[1] > max_cols:
            power_masked = power_masked[:, -max_cols:]

        # optional per-frequency normalization (robust z-score)
        if use_zscore:
            med = np.nanmedian(power_masked, axis=1, keepdims=True)
            mad = np.nanmedian(np.abs(power_masked - med), axis=1, keepdims=True)
            mad[mad < 1e-9] = 1e-9
            power_norm = (power_masked - med) / (1.4826 * mad)
        else:
            power_norm = power_masked

        # robust color scale (ignore NaNs/edges and heavy spikes)
        vmin = np.nanpercentile(power_norm, 5.0)
        vmax = np.nanpercentile(power_norm, 99.0)
        if not np.isfinite(vmin): vmin = -3.0
        if not np.isfinite(vmax): vmax = 3.0
        if vmax <= vmin: vmax = vmin + 1e-6

        img.set_data(power_norm)
        img.set_extent([-power_norm.shape[1] / cur_fs, 0.0, freqs[0], freqs[-1]])
        img.set_clim(float(vmin), float(vmax))

        title = f"Real-time CWT — ch{state.chan_index}"
        if ch_label: title += f" ({ch_label})"
        title += f"  fs={cur_fs:.1f} Hz  window={window:.1f}s"
        ax.set_title(title)

        return img,

    ani = FuncAnimation(fig, update, interval=100, blit=True)
    plt.tight_layout()
    plt.show()

if __name__ == "__main__":
    p = argparse.ArgumentParser(description="Real-time CWT viewer with COI masking.")
    p.add_argument("--host", default="raspberrypi.local")
    p.add_argument("--topic", default="eeg_voltage")
    p.add_argument("--epoch", type=int, default=1)
    p.add_argument("--fs", type=float, default=250.0, help="Fallback fs if not in meta.")
    p.add_argument("--window", type=float, default=12.0, help="Rolling window (s).")
    p.add_argument("--chan", type=int, default=0, help="0-based channel index.")
    p.add_argument("--fmin", type=float, default=1.0)
    p.add_argument("--fmax", type=float, default=45.0)
    p.add_argument("--n-freqs", type=int, default=80)
    p.add_argument("--zscore", dest="zscore", action="store_true",
                   help="Per-frequency robust z-score (helps when a spike dominates).")
    p.add_argument("--no-zscore", dest="zscore", action="store_false")
    p.set_defaults(zscore=False)
    p.add_argument("--hp", dest="hp", action="store_true",
                   help="Simple median-demean to reduce DC drift.")
    p.add_argument("--no-hp", dest="hp", action="store_false")
    p.set_defaults(hp=True)
    args = p.parse_args()

    run_plot(
        host=args.host, topic=args.topic, epoch=args.epoch,
        fs=args.fs, window=args.window, chan=args.chan,
        fmin=args.fmin, fmax=args.fmax, n_freqs=args.n_freqs,
        use_zscore=args.zscore, hp_demean=args.hp
    )
