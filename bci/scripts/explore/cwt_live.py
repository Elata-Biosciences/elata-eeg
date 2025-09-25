#!/usr/bin/env python3
# cwt_live.py — Real-time multi-channel CWT with COI masking + log power + robust scaling
import argparse
import asyncio
import io
import sys
import time
import threading
from collections import deque
from typing import Any, Dict, Iterable, Tuple, Optional, List

import numpy as np
import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation
import pywt  # pip install pywavelets

from ws_client import start_data_ws


class MultiState:
    """Rolling buffers + metadata for a selectable subset of channels."""
    def __init__(self, fs: float, window_secs: float, initial_indices: List[int]):
        self.fs = float(fs)
        self.window_secs = float(window_secs)
        self.lock = threading.Lock()

        # selection bookkeeping
        self.chan_indices: List[int] = list(initial_indices)
        self.total_chans: Optional[int] = None  # learned from meta OR first packet
        self.chan_names: List[str] = []
        self.last_meta: Optional[Dict[str, Any]] = None

        # rolling buffers for selected channels (will be resized when selection changes)
        self.buf_per_ch: List[deque] = [deque(maxlen=int(self.window_secs * self.fs)) for _ in self.chan_indices]

        # flags for layout building after we learn channel count
        self.need_relayout: bool = True

    def _resize_deques(self):
        maxlen = int(self.window_secs * self.fs)
        self.buf_per_ch = [deque(list(d), maxlen=maxlen) for d in self.buf_per_ch]

    def maybe_update_from_meta(self, meta: Optional[Dict[str, Any]]):
        if not meta:
            return
        self.last_meta = meta
        fs = meta.get("fs") or meta.get("sample_rate") or self.fs
        if fs != self.fs and fs > 0:
            with self.lock:
                self.fs = float(fs)
                self._resize_deques()

        # channel names / count
        chnames = None
        if isinstance(meta.get("chan_names"), list):
            chnames = meta["chan_names"]
        elif isinstance(meta.get("channels"), list):
            chnames = meta["channels"]
        if chnames is not None:
            with self.lock:
                self.chan_names = chnames
                if self.total_chans != len(chnames):
                    self.total_chans = len(chnames)
                    self.need_relayout = True

    def learn_channels_from_samples(self, samples: np.ndarray):
        """Infer total channel count from the sample array shape if meta didn't say."""
        if samples.ndim == 2:
            n_ch = samples.shape[1]
        else:
            n_ch = 1
        with self.lock:
            if self.total_chans != n_ch:
                self.total_chans = n_ch
                self.need_relayout = True

    def set_selection(self, indices: List[int]):
        """Set selected indices; rebuild buffers for new selection."""
        with self.lock:
            self.chan_indices = list(indices)
            self.buf_per_ch = [deque(maxlen=int(self.window_secs * self.fs)) for _ in self.chan_indices]
            self.need_relayout = True

    def ensure_subset_in_range(self):
        if self.total_chans is None:
            return
        ok = [i for i in self.chan_indices if 0 <= i < self.total_chans]
        if ok != self.chan_indices:
            self.set_selection(ok)

    def ingest(self, samples: np.ndarray):
        """Append new samples to each selected channel buffer."""
        if samples.ndim == 1:
            xs = [samples.astype(np.float32, copy=False)]
        else:
            xs = []
            for ch in self.chan_indices:
                if ch < samples.shape[1]:
                    xs.append(samples[:, ch].astype(np.float32, copy=False))

        with self.lock:
            if len(self.buf_per_ch) != len(xs):
                self.buf_per_ch = [deque(maxlen=int(self.window_secs * self.fs)) for _ in xs]
            for d, x in zip(self.buf_per_ch, xs):
                d.extend(x)


def run_plot(
    host: str,
    topic: str,
    epoch: int,
    fs: float,
    window: float,
    chan_indices_cli: List[int],
    use_all: bool,
    first_n: Optional[int],
    grid_cols: int,
    fmin: float,
    fmax: float,
    n_freqs: int,
    use_zscore: bool,
    hp_demean: bool,
    # NEW: offline replay inputs (stdin .npz)
    offline_data: Optional[np.ndarray] = None,   # shape (C, T) expected
    offline_fs: Optional[float] = None,
    offline_chan_names: Optional[List[str]] = None,
    replay_speed: float = 1.0,
    replay_batch: int = 64,
):
    # Start with 1 channel until we learn the real count; selection finalized after first meta/packet.
    initial_indices = chan_indices_cli if (chan_indices_cli or (not use_all and not first_n)) else [0]
    state = MultiState(fs=fs, window_secs=window, initial_indices=initial_indices)

    # Frequency grid + scales
    freqs = np.linspace(fmin, fmax, n_freqs).astype(np.float64)
    wavelet = "morl"

    def compute_scales(dt: float) -> np.ndarray:
        cf = pywt.central_frequency(wavelet)
        return (cf / (freqs * dt))

    # Packet handler shared by live WS and offline replay
    def on_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
        state.maybe_update_from_meta(meta)
        state.learn_channels_from_samples(samples)

        # Finalize selection once we know total_chans
        with state.lock:
            if state.need_relayout and state.total_chans is not None:
                if use_all:
                    desired = list(range(state.total_chans))
                elif first_n is not None:
                    desired = list(range(min(first_n, state.total_chans)))
                elif chan_indices_cli:
                    desired = [i for i in chan_indices_cli if 0 <= i < state.total_chans]
                else:
                    desired = state.chan_indices or [0]
                if desired != state.chan_indices:
                    state.chan_indices = desired
                    state.buf_per_ch = [deque(maxlen=int(state.window_secs * state.fs)) for _ in desired]

        state.ensure_subset_in_range()
        state.ingest(samples)

    # Live or offline feeder
    def ws_thread():
        asyncio.run(start_data_ws(host=host, subscriptions=[(topic, epoch)], on_packet=on_packet))

    def offline_thread():
        assert offline_data is not None
        # offline_data: (C, T) → we emit (B, C)
        C, T = int(offline_data.shape[0]), int(offline_data.shape[1])
        fs0 = float(offline_fs or fs or 250.0)
        ch_names = offline_chan_names or [f"ch{i}" for i in range(C)]
        hdr = {"topic": "offline_stdin", "packet_type": "f32", "num_channels": C}

        # one-time meta
        on_packet(hdr, np.zeros((0, C), dtype=np.float32), {"fs": fs0, "chan_names": ch_names})

        t0 = time.perf_counter()
        emitted = 0
        while emitted < T:
            b = min(replay_batch, T - emitted)
            # slice from (C, T) and transpose to (B, C)
            batch = offline_data[:, emitted:emitted + b].T.astype(np.float32, copy=False)
            on_packet(hdr, batch, {"fs": fs0, "chan_names": ch_names})
            emitted += b

            if replay_speed <= 0:
                continue  # as fast as possible
            # sleep to approximate real time
            # batch duration at fs0 is (b / fs0); scale by 1/replay_speed
            dt = (b / fs0) / max(1e-9, replay_speed)
            # try to keep cumulative timing tight
            target = (emitted / fs0) / max(1e-9, replay_speed)
            now = time.perf_counter() - t0
            to_sleep = target - now
            if to_sleep > 0:
                time.sleep(to_sleep)

    # Start the appropriate feeder thread
    if offline_data is None:
        threading.Thread(target=ws_thread, daemon=True).start()
    else:
        threading.Thread(target=offline_thread, daemon=True).start()

    # Matplotlib setup
    fig = plt.figure(figsize=(8, 6))
    axes: List[plt.Axes] = []
    images: List[Any] = []
    cbars: List[Any] = []

    def rebuild_layout_if_needed():
        nonlocal axes, images, cbars
        with state.lock:
            if not state.need_relayout:
                return
            n = len(state.chan_indices)
            if n <= 0:
                return
            state.need_relayout = False

        # Clear and rebuild
        fig.clf()
        axes, images, cbars = [], [], []
        rows = int(np.ceil(n / max(1, grid_cols)))
        cols = min(grid_cols, n)
        gs = fig.add_gridspec(rows, cols)
        max_w = max(2, int(state.fs * state.window_secs))
        blank = np.zeros((n_freqs, max_w), dtype=np.float32)

        for i in range(n):
            r, c = divmod(i, cols)
            ax = fig.add_subplot(gs[r, c])
            im = ax.imshow(
                blank,
                origin="lower", aspect="auto",
                extent=[-state.window_secs, 0, freqs[0], freqs[-1]],
                interpolation="nearest", animated=False,
            )
            axes.append(ax)
            images.append(im)
            cb = fig.colorbar(im, ax=ax, pad=0.01)
            cb.set_label("Log power (|coeff|^2, dB-like)")
            cbars.append(cb)

        fig.tight_layout()

    def panel_title(idx: int, fs_now: float) -> str:
        with state.lock:
            ch = state.chan_indices[idx]
            label = None
            if state.chan_names and 0 <= ch < len(state.chan_names):
                label = state.chan_names[ch]
        t = f"ch{ch}" + (f" ({label})" if label else "")
        t += f"  fs={fs_now:.1f} Hz  window={state.window_secs:.1f}s"
        return t

    def update(_frame):
        rebuild_layout_if_needed()

        with state.lock:
            bufs = [np.array(d, dtype=np.float32) for d in state.buf_per_ch]
            cur_fs = state.fs

        if not bufs:
            return []

        dt = 1.0 / cur_fs
        scales = compute_scales(dt)

        artists = []
        for i, (ax, img, x) in enumerate(zip(axes, images, bufs)):
            if x.size < int(0.5 * cur_fs):
                continue

            # simple DC drift removal
            if hp_demean and x.size:
                x = x - np.median(x)

            coeffs, _ = pywt.cwt(x, scales, "morl", sampling_period=dt)
            power = np.log10((np.abs(coeffs) ** 2).astype(np.float32) + 1e-12)

            # COI mask
            ncols = power.shape[1]
            t = np.arange(ncols, dtype=np.float64) * dt
            dist = np.minimum(t, (ncols - 1) * dt - t)
            coi = np.sqrt(2.0) * (scales * dt)
            mask = (coi[:, None] > dist[None, :])
            power[mask] = np.nan

            # crop
            max_cols = int(state.window_secs * cur_fs)
            if power.shape[1] > max_cols:
                power = power[:, -max_cols:]

            # optional robust z-score per frequency
            if use_zscore:
                med = np.nanmedian(power, axis=1, keepdims=True)
                mad = np.nanmedian(np.abs(power - med), axis=1, keepdims=True)
                mad[mad < 1e-9] = 1e-9
                disp = (power - med) / (1.4826 * mad)
            else:
                disp = power

            # scale
            vmin = np.nanpercentile(disp, 5.0)
            vmax = np.nanpercentile(disp, 99.0)
            if not np.isfinite(vmin): vmin = -3.0
            if not np.isfinite(vmax): vmax = 3.0
            if vmax <= vmin: vmax = vmin + 1e-6

            img.set_data(disp)
            img.set_extent([-disp.shape[1] / cur_fs, 0.0, freqs[0], freqs[-1]])
            img.set_clim(float(vmin), float(vmax))
            ax.set_ylabel("Hz")
            ax.set_title(panel_title(i, cur_fs))
            artists.append(img)

        return artists

    # Keep a reference to avoid GC; disable cache to silence warning.
    ani = FuncAnimation(
        fig,
        update,
        interval=120,
        blit=False,
        cache_frame_data=False,
        save_count=300,
    )
    fig._ani = ani  # keep it alive
    plt.show()
    return ani


def parse_channels(s: str) -> List[int]:
    parts = [p.strip() for p in s.split(",") if p.strip() != ""]
    return [int(p) for p in parts]


def load_npz_from_stdin() -> Tuple[Optional[np.ndarray], Optional[float], Optional[List[str]]]:
    """
    If stdin has data, read an .npz (as saved by `record_epochs.py`) and return:
      data (C, T), fs (float), ch_names (list[str])  — any missing returns None.
    If stdin is a TTY or empty, returns (None, None, None).
    """
    try:
        if sys.stdin.isatty():
            return None, None, None
    except Exception:
        pass

    # Peek without blocking too long: read everything available.
    raw = sys.stdin.buffer.read()
    if not raw:
        return None, None, None

    bio = io.BytesIO(raw)
    try:
        npz = np.load(bio, allow_pickle=True)
    except Exception as e:
        print(f"[stdin] Failed to load npz from stdin: {e}")
        return None, None, None

    data = None
    fs = None
    ch_names = None

    # Common recorder keys
    if "data" in npz:
        arr = npz["data"]
        if arr.ndim == 2:
            data = arr  # expected (C, T)
    elif "X" in npz:
        arr = npz["X"]
        if arr.ndim == 2:
            data = arr

    if "fs" in npz:
        try:
            fs = float(np.array(npz["fs"]).astype(np.float64))
        except Exception:
            fs = None

    # ch_names could be saved as object array
    for key in ("ch_names", "channels", "chan_names"):
        if key in npz:
            arr = npz[key]
            try:
                ch_names = [str(x) for x in list(arr.tolist())]
                break
            except Exception:
                pass

    # sanitize orientation
    if data is not None and data.shape[0] < data.shape[1]:
        # assume (C, T) already correct (recorder saves this way)
        pass
    elif data is not None:
        # looks like (T, C) — transpose to (C, T)
        data = data.T

    return data, fs, ch_names


if __name__ == "__main__":
    p = argparse.ArgumentParser(description="Real-time/Offline multi-channel CWT (one window, separate subplots).")
    p.add_argument("--host", default="raspberrypi.local")
    p.add_argument("--topic", default="eeg_voltage")
    p.add_argument("--epoch", type=int, default=1)
    p.add_argument("--fs", type=float, default=250.0, help="Fallback fs if not in meta or stdin.")
    p.add_argument("--window", type=float, default=12.0, help="Rolling window (s).")

    g = p.add_mutually_exclusive_group()
    g.add_argument("--channels", type=str, default="", help="Comma-separated 0-based indices, e.g. '0,2,5'.")
    g.add_argument("--all", dest="use_all", action="store_true", help="Show all channels.")
    g.add_argument("--nch", type=int, default=None, help="Show first N channels.")

    p.add_argument("--grid-cols", type=int, default=3, help="Columns in subplot grid.")
    p.add_argument("--fmin", type=float, default=1.0)
    p.add_argument("--fmax", type=float, default=45.0)
    p.add_argument("--n-freqs", type=int, default=80)
    p.add_argument("--zscore", dest="zscore", action="store_true", help="Per-frequency robust z-score.")
    p.add_argument("--no-zscore", dest="zscore", action="store_false")
    p.set_defaults(zscore=False)
    p.add_argument("--hp", dest="hp", action="store_true", help="Median-demean to reduce DC drift.")
    p.add_argument("--no-hp", dest="hp", action="store_false")
    p.set_defaults(hp=True)

    # NEW: stdin replay controls
    p.add_argument("--stdin", dest="force_stdin", action="store_true",
                   help="Force reading a .npz from stdin (even if TTY).")
    p.add_argument("--replay-speed", type=float, default=1.0,
                   help="Replay speed multiplier (1.0=real-time, 2.0=2x, 0=as fast as possible).")
    p.add_argument("--replay-batch", type=int, default=64, help="Samples per replay push.")

    args = p.parse_args()
    chan_list = parse_channels(args.channels) if args.channels else []

    # Autodetect stdin .npz unless user forces off (no flag needed) or forces on.
    offline_data = None
    offline_fs = None
    offline_ch_names = None

    if args.force_stdin or (not sys.stdin.isatty()):
        d, f_in, nms = load_npz_from_stdin()
        if d is not None:
            offline_data, offline_fs, offline_ch_names = d, f_in, nms
            print(f"[stdin] Loaded offline .npz: C={offline_data.shape[0]} T={offline_data.shape[1]} "
                  f"fs={offline_fs if offline_fs else args.fs} "
                  f"names={len(offline_ch_names) if offline_ch_names else 'none'}")
        else:
            if args.force_stdin:
                print("[stdin] --stdin set but no valid .npz detected on stdin. Exiting.")
                sys.exit(2)

    _anim = run_plot(
        host=args.host,
        topic=args.topic,
        epoch=args.epoch,
        fs=args.fs,
        window=args.window,
        chan_indices_cli=chan_list,
        use_all=bool(args.use_all),
        first_n=args.nch,
        grid_cols=args.grid_cols,
        fmin=args.fmin,
        fmax=args.fmax,
        n_freqs=args.n_freqs,
        use_zscore=args.zscore,
        hp_demean=args.hp,
        offline_data=offline_data,
        offline_fs=offline_fs,
        offline_chan_names=offline_ch_names,
        replay_speed=float(args.replay_speed),
        replay_batch=int(args.replay_batch),
    )
