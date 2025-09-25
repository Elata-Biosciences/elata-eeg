#!/usr/bin/env python3
# main.py — 8‑channel live FFT + filtered + raw (single‑channel view optional)
import asyncio
import json
import threading
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Dict, Iterable, Tuple, Optional, List

import numpy as np
import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation
import argparse
from scipy.signal import butter, sosfiltfilt  # zero-phase band-pass for clean plotting

from ws_client import start_data_ws

# ----------------------------
# Shared state & helpers
# ----------------------------

@dataclass
class SharedState:
    fs: float = 250.0             # default until meta/header says otherwise
    num_channels: int = 1
    chan_names: List[str] = field(default_factory=list)
    window_secs: float = 15.0       # rolling window length for time/FFT
    buffers: List[deque] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)
    last_meta: Optional[Dict[str, Any]] = None

    def ensure_buffers(self, num_channels: int, fs: float):
        """Initialize or re-size buffers when fs or channel count changes."""
        with self.lock:
            maxlen = int(max(8, round(self.window_secs * fs)))
            if not self.buffers or len(self.buffers) != num_channels:
                self.buffers = [deque(maxlen=maxlen) for _ in range(num_channels)]
            else:
                # Resize deques if fs changed
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
    names = None
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

# ----------------------------
# Live plotter (FFT + filtered + raw)
# ----------------------------

class LiveFFTAndSignal:
    def __init__(
        self,
        state: SharedState,
        topic: str,
        initial_channel: int = 0,
        fmax: Optional[float] = 45.0,
        use_log10_uv2_per_hz: bool = True,
        bp_lo: float = 16.0,
        bp_hi: float = 40.0,
        bp_order: int = 4,
        show_all: bool = True,
    ):
        self.state = state
        self.topic = topic
        self.channel = max(0, initial_channel)
        self.fmax = fmax
        self.use_log10_uv2_per_hz = use_log10_uv2_per_hz
        self.show_all = show_all

        # Band-pass params (will redesign on fs changes)
        self.bp_lo = bp_lo
        self.bp_hi = bp_hi
        self.bp_order = bp_order
        self._fs_for_sos = None
        self._sos = None  # designed lazily

        if self.show_all:
            # 3 stacked axes; each axis will have N lines (one per channel)
            self.fig, (self.ax_fft, self.ax_sig, self.ax_raw) = plt.subplots(
                3, 1, figsize=(10, 10.5), height_ratios=[1.0, 1.1, 1.1], sharex=False
            )
            self.lines_fft: List[Any] = []
            self.lines_sig: List[Any] = []
            self.lines_raw: List[Any] = []
        else:
            # Single-channel view (like original)
            self.fig, (self.ax_fft, self.ax_sig, self.ax_raw) = plt.subplots(
                3, 1, figsize=(9, 9.5), height_ratios=[1.0, 1.1, 1.1], sharex=False
            )
            (self.line_fft,) = self.ax_fft.plot([], [], lw=1.5)
            (self.line_sig,) = self.ax_sig.plot([], [], lw=1.0)
            (self.line_raw,) = self.ax_raw.plot([], [], lw=0.9)

        # Axis labels/grid
        self.ax_fft.set_xlabel("Frequency (Hz)")
        self.ax_fft.set_ylabel("Power " + ("log10(µV²/Hz)" if self.use_log10_uv2_per_hz else "µV²/Hz"))
        self.ax_fft.grid(True, which="both", linestyle="--", alpha=0.35)

        self.ax_sig.set_xlabel("Time (s)")
        self.ax_sig.set_ylabel("Amplitude (filtered)")
        self.ax_sig.grid(True, which="both", linestyle="--", alpha=0.35)

        self.ax_raw.set_xlabel("Time (s)")
        self.ax_raw.set_ylabel("Amplitude (raw)")
        self.ax_raw.grid(True, which="both", linestyle="--", alpha=0.35)

        # Key controls
        self.fig.canvas.mpl_connect("key_press_event", self.on_key)

        # Animation timer
        self.ani = FuncAnimation(self.fig, self.update, interval=100, blit=False)

    # ---------- UI ----------
    def on_key(self, event):
        if self.show_all:
            return  # channel keys only relevant in single‑channel mode
        if event.key in ("left", "j"):
            with self.state.lock:
                self.channel = (self.channel - 1) % max(1, self.state.num_channels)
        elif event.key in ("right", "l"):
            with self.state.lock:
                self.channel = (self.channel + 1) % max(1, self.state.num_channels)

    # ---------- DSP ----------
    def _ensure_bp(self, fs: float):
        if self._sos is None or self._fs_for_sos is None or abs(self._fs_for_sos - fs) > 1e-9:
            nyq = 0.5 * fs
            low = max(0.001, self.bp_lo) / nyq
            high = min(self.bp_hi, 0.49 * fs) / nyq
            if high <= low:
                # Fallback to a very small valid band if config is odd for given fs
                high = min(0.49, low * 1.5)
            self._sos = butter(self.bp_order, [low, high], btype="bandpass", output="sos")
            self._fs_for_sos = fs

    def _compute_psd_log10_uv2_per_hz(self, x: np.ndarray, fs: float):
        """
        Periodogram with Hann window, scaled to density (units: V²/Hz),
        then converted to µV²/Hz and finally log10(µV²/Hz).
        """
        if len(x) < 8:
            # Minimal return to keep axes happy
            return np.array([0.0, 1.0]), np.array([np.nan, np.nan])

        # Detrend + window
        x = x - np.mean(x)
        N = len(x)
        w = np.hanning(N)
        xw = x * w

        # FFT and frequency bins
        X = np.fft.rfft(xw)
        freqs = np.fft.rfftfreq(N, d=1.0 / fs)

        # Periodogram scaling to density:
        # PSD_V2_per_Hz = |X|^2 / (fs * sum(w^2))
        psd_v2_per_hz = (np.abs(X) ** 2) / (fs * np.sum(w ** 2) + 1e-30)

        # Convert to µV²/Hz
        psd_uv2_per_hz = psd_v2_per_hz * (1e6 ** 2)

        # Return either log10(µV²/Hz) or linear µV²/Hz
        if self.use_log10_uv2_per_hz:
            with np.errstate(divide="ignore", invalid="ignore"):
                y = np.log10(psd_uv2_per_hz + 1e-30)
        else:
            y = psd_uv2_per_hz
        return freqs, y

    def _filter_signal(self, x: np.ndarray, fs: float) -> np.ndarray:
        if len(x) < 16:
            return x  # not enough data; skip filtering
        self._ensure_bp(fs)
        try:
            # zero-phase for clean visualization on the rolling window
            return sosfiltfilt(self._sos, x, padlen=min(3 * (self._sos.shape[0] * 2), len(x) - 1))
        except Exception:
            return x

    # ---------- helpers for all‑channels mode ----------
    def _ensure_lines_for_all(self, nch: int):
        # Create line objects to match channel count
        def ensure_list(ax, storage: List[Any], lw: float):
            if len(storage) == nch:
                return
            # remove old
            for ln in storage:
                try:
                    ln.remove()
                except Exception:
                    pass
            storage.clear()
            # create new
            for _ in range(nch):
                (ln,) = ax.plot([], [], lw=lw)
                storage.append(ln)
        self._ensure_axis_limits_initialized()
        ensure_list(self.ax_fft, self.lines_fft, 1.0)
        ensure_list(self.ax_sig, self.lines_sig, 0.9)
        ensure_list(self.ax_raw, self.lines_raw, 0.8)

    def _ensure_axis_limits_initialized(self):
        # Prevent autoscale weirdness on first frames
        for ax in (self.ax_fft, self.ax_sig, self.ax_raw):
            ax.set_xlim(0, 1)
            ax.set_ylim(-1, 1)

    # ---------- Animation frame ----------
    def update(self, _frame):
        with self.state.lock:
            fs = float(self.state.fs)
            nch = self.state.num_channels
            names = self.state.chan_names or [f"ch{i+1}" for i in range(nch)]
            bufs = [np.array(self.state.buffers[i], dtype=np.float64) if self.state.buffers else np.array([])
                    for i in range(nch)]

        if self.show_all:
            self._ensure_lines_for_all(nch)

            # FFT per channel
            ymins, ymaxs = [], []
            freqs_ref = None
            for i, buf in enumerate(bufs):
                freqs, Y = self._compute_psd_log10_uv2_per_hz(buf, fs)
                if freqs_ref is None:
                    freqs_ref = freqs
                mask = (freqs <= self.fmax) if self.fmax is not None else np.ones_like(freqs, dtype=bool)
                self.lines_fft[i].set_data(freqs[mask], Y[mask])
                finite = np.isfinite(Y[mask])
                if finite.any():
                    ymins.append(np.nanmin(Y[mask][finite]))
                    ymaxs.append(np.nanmax(Y[mask][finite]))
            if freqs_ref is not None and freqs_ref.size > 1:
                xmax = (self.fmax if self.fmax is not None else freqs_ref[-1])
                self.ax_fft.set_xlim(0, max(1e-3, xmax))
                if ymins and ymaxs:
                    ymin, ymax = float(np.nanmin(ymins)), float(np.nanmax(ymaxs))
                    if np.isfinite(ymin) and np.isfinite(ymax) and ymin != ymax:
                        pad = 0.1 * (ymax - ymin)
                        self.ax_fft.set_ylim(ymin - pad, ymax + pad)

            # Filtered + Raw per channel (stacked with offsets so curves don't overlap)
            # Compute a robust per‑axis scale across all channels
            def set_lines(ax, lines, arrs):
                # Determine vertical offsets based on per‑channel RMS
                rms = np.array([np.sqrt(np.mean(a**2)) if a.size else 0.0 for a in arrs])
                base = np.median(rms[rms>0]) if (rms>0).any() else 1.0
                step = 4.0 * base  # spacing between channels
                ymin, ymax = +np.inf, -np.inf
                for i, a in enumerate(arrs):
                    if a.size:
                        t = np.arange(a.size) / fs
                        off = i * step
                        lines[i].set_data(t, a + off)
                        ymin = min(ymin, float(np.min(a + off)))
                        ymax = max(ymax, float(np.max(a + off)))
                    else:
                        lines[i].set_data([], [])
                if not np.isfinite(ymin) or not np.isfinite(ymax):
                    ymin, ymax = -1.0, 1.0
                ax.set_xlim(0, max(1.0, *( (np.arange(arrs[0].size)/fs).tolist() if arrs and arrs[0].size else [1.0] )))
                pad = 0.05 * (ymax - ymin + 1e-12)
                ax.set_ylim(ymin - pad, ymax + pad)

            # Filtered signals
            y_filts = [self._filter_signal(b, fs) for b in bufs]
            set_lines(self.ax_sig, self.lines_sig, y_filts)

            # Raw signals
            set_lines(self.ax_raw, self.lines_raw, bufs)

            unit_str = "log10(µV²/Hz)" if self.use_log10_uv2_per_hz else "µV²/Hz"
            self.ax_fft.set_title(f"Live FFT — {self.topic} | fs={fs:.2f} Hz | {nch} channels | {unit_str}")
            self.ax_sig.set_title(f"Filtered ({self.bp_lo:.1f}–{self.bp_hi:.1f} Hz, order {self.bp_order}) — stacked by channel")
            self.ax_raw.set_title("Raw (unfiltered) — stacked by channel")

            return tuple(self.lines_fft + self.lines_sig + self.lines_raw)

        else:
            # single‑channel path (original behavior)
            if self.channel >= nch:
                self.channel = 0
            buf = bufs[self.channel] if nch else np.array([])

            # FFT (top)
            freqs, Y = self._compute_psd_log10_uv2_per_hz(buf, fs)
            mask = (freqs <= self.fmax) if self.fmax is not None else np.ones_like(freqs, dtype=bool)
            self.line_fft.set_data(freqs[mask], Y[mask])

            if freqs.size > 1:
                xmax = (self.fmax if self.fmax is not None else freqs[-1])
                self.ax_fft.set_xlim(0, max(1e-3, xmax))
                finite = np.isfinite(Y[mask])
                if finite.any():
                    ymin = np.nanmin(Y[mask][finite])
                    ymax = np.nanmax(Y[mask][finite])
                    if np.isfinite(ymin) and np.isfinite(ymax) and ymin != ymax:
                        pad = 0.1 * (ymax - ymin)
                        self.ax_fft.set_ylim(ymin - pad, ymax + pad)

            # Filtered signal (middle)
            y_filt = self._filter_signal(buf, fs)
            if y_filt.size > 0:
                t = np.arange(y_filt.size) / fs
                self.line_sig.set_data(t, y_filt)
                self.ax_sig.set_xlim(max(0, t.min()), max(1.0, t.max()))
                yfinite = y_filt[np.isfinite(y_filt)]
                if yfinite.size > 0:
                    ypad = 0.1 * (np.max(yfinite) - np.min(yfinite) + 1e-12)
                    self.ax_sig.set_ylim(np.min(yfinite) - ypad, np.max(yfinite) + ypad)

            # Raw signal (bottom)
            if buf.size > 0:
                t_raw = np.arange(buf.size) / fs
                self.line_raw.set_data(t_raw, buf)
                self.ax_raw.set_xlim(max(0, t_raw.min()), max(1.0, t_raw.max()))
                rfinite = buf[np.isfinite(buf)]
                if rfinite.size > 0:
                    rpad = 0.1 * (np.max(rfinite) - np.min(rfinite) + 1e-12)
                    self.ax_raw.set_ylim(np.min(rfinite) - rpad, np.max(rfinite) + rpad)

            # Titles
            names = self.state.chan_names or [f"ch{i+1}" for i in range(nch)]
            unit_str = "log10(µV²/Hz)" if self.use_log10_uv2_per_hz else "µV²/Hz"
            self.ax_fft.set_title(
                f"Live FFT — {self.topic} | fs={fs:.2f} Hz | "
                f"chan {self.channel+1}/{nch} ({names[self.channel] if names else 'ch'}) | {unit_str}"
            )
            self.ax_sig.set_title(
                f"Filtered signal ({self.bp_lo:.1f}–{self.bp_hi:.1f} Hz, order {self.bp_order})"
            )
            self.ax_raw.set_title("Raw signal (unfiltered)")

            return self.line_fft, self.line_sig, self.line_raw

    def show(self):
        plt.tight_layout()
        plt.show()

# ----------------------------
# Wiring: WS consumer -> buffers
# ----------------------------

def run_ws_in_thread(host: str, topic: str, epoch: int, state: SharedState):
    async def handle_packet(header, samples: np.ndarray, meta):
        # Update fs / channel names / buffer sizes if needed
        fs_new = extract_fs(header, meta, state.fs)
        num_channels = int(header.get("num_channels") or samples.shape[1] if samples.ndim == 2 else 1)
        names = extract_names(header, meta, num_channels)

        changed = (abs(fs_new - state.fs) > 1e-6) or (num_channels != state.num_channels) or (names != state.chan_names)
        if changed:
            state.fs = fs_new
            state.num_channels = num_channels
            state.chan_names = names
            state.ensure_buffers(num_channels, fs_new)

        # Append samples to per-channel buffers
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
    ap = argparse.ArgumentParser(description="Live FFT + filtered + raw plot from streaming EEG WebSocket")
    ap.add_argument("--host", default="raspberrypi.local", help="Data host (default: raspberrypi.local)")
    ap.add_argument("--topic", default="eeg_voltage", help="Topic name to subscribe to")
    ap.add_argument("--epoch", type=int, default=1, help="Epoch to subscribe with")
    ap.add_argument("--window", type=float, default=4.0, help="Rolling window length in seconds")
    ap.add_argument("--channel", type=int, default=0, help="Initial channel index (0-based; single‑channel mode)")
    ap.add_argument("--fmax", type=float, default=45.0, help="Max frequency to display (Hz). Use large value for full band.")
    ap.add_argument("--lin", action="store_true", help="Use linear µV²/Hz scale instead of log10(µV²/Hz)")
    ap.add_argument("--bp_lo", type=float, default=16.0, help="Band-pass low cut (Hz)")
    ap.add_argument("--bp_hi", type=float, default=40.0, help="Band-pass high cut (Hz)")
    ap.add_argument("--bp_order", type=int, default=4, help="Band-pass Butterworth order")
    ap.add_argument("--single", action="store_true", help="Single‑channel view (use ←/→ to switch)")
    return ap.parse_args()


def main():
    args = parse_args()

    state = SharedState(window_secs=args.window)
    state.ensure_buffers(num_channels=8, fs=state.fs)  # pre‑allocate 8; will resize if meta says otherwise

    # Start WS consumer
    run_ws_in_thread(args.host, args.topic, args.epoch, state)

    # Plotter
    plot = LiveFFTAndSignal(
        state=state,
        topic=args.topic,
        initial_channel=max(0, args.channel),
        fmax=args.fmax,
        use_log10_uv2_per_hz=not args.lin,
        bp_lo=args.bp_lo,
        bp_hi=args.bp_hi,
        bp_order=args.bp_order,
        show_all=not args.single,
    )

    if args.single:
        print(
            "Controls (single‑channel):\n"
            "  ← / j : previous channel\n"
            "  → / l : next channel\n"
        )
    print(
        f"Window: {args.window}s | FFT fmax: {args.fmax} Hz | scale: "
        f"{'linear µV²/Hz' if args.lin else 'log10(µV²/Hz)'}\n"
        f"Band‑pass (filtered trace): {args.bp_lo}-{args.bp_hi} Hz (order {args.bp_order})\n"
        f"Mode: {'ALL channels (stacked)' if not args.single else 'Single‑channel'}\n"
        "Bottom trace shows raw, unfiltered samples.\n"
    )

    plot.show()


if __name__ == "__main__":
    main()
