#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import math
import socket
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Dict, Iterable, List, Optional, Tuple

import numpy as np

# Local WS helper
from ws_client import start_data_ws


@dataclass
class Buffers:
    fs: float = 250.0
    n_ch: int = 0
    names: List[str] = field(default_factory=list)
    bufs: List[deque] = field(default_factory=list)
    dtype: str = "f32"  # informational only

    def ensure(self, n_ch: int, fs: float, max_seconds: float):
        self.n_ch = int(n_ch)
        self.fs = float(fs)
        maxlen = int(max(1, round(max_seconds * self.fs)))
        if not self.bufs or len(self.bufs) != n_ch:
            self.bufs = [deque(maxlen=maxlen) for _ in range(n_ch)]
        else:
            # resize
            for i in range(n_ch):
                old = list(self.bufs[i])[-maxlen:]
                self.bufs[i] = deque(old, maxlen=maxlen)

    def ingest(self, x: np.ndarray):
        if x.ndim == 1:
            x = x[:, None]
        for ch in range(min(self.n_ch, x.shape[1])):
            self.bufs[ch].extend(x[:, ch].tolist())

    def to_array(self) -> np.ndarray:
        if not self.bufs:
            return np.zeros((0, 0), dtype=np.float32)
        arrs = [np.array(d, dtype=np.float32) for d in self.bufs]
        # Pad to equal lengths
        T = max(a.size for a in arrs)
        out = np.zeros((len(arrs), T), dtype=np.float32)
        for i, a in enumerate(arrs):
            out[i, -a.size:] = a
        return out


def detrend(x: np.ndarray) -> np.ndarray:
    t = np.arange(x.size, dtype=np.float32)
    # simple linear detrend via least squares
    t_mean = t.mean()
    x_mean = x.mean()
    denom = ((t - t_mean) ** 2).sum() + 1e-12
    slope = ((t - t_mean) * (x - x_mean)).sum() / denom
    intercept = x_mean - slope * t_mean
    return (x - (slope * t + intercept)).astype(np.float32)


def band_power(x: np.ndarray, fs: float, f_lo: float, f_hi: float) -> float:
    if x.size < 8:
        return float("nan")
    x = x - float(np.mean(x))
    w = np.hanning(x.size)
    X = np.fft.rfft(x * w)
    freqs = np.fft.rfftfreq(x.size, d=1.0 / fs)
    P = (np.abs(X) ** 2) / max(1, x.size)
    mask = (freqs >= f_lo) & (freqs <= f_hi)
    if not np.any(mask):
        return float("nan")
    return float(np.mean(P[mask]))


# Optional helpers: override names via CSV and drop ground/reference channels
GROUND_TOKENS = {"fpz", "ground", "ref", "reference", "a1", "a2"}

def parse_names_csv(csv: str, n_expected: int) -> List[str]:
    names = [s.strip() for s in str(csv).split(",") if s.strip()]
    if not names:
        return [f"ch{i+1}" for i in range(n_expected)]
    if len(names) != n_expected:
        # pad or trim to match
        if len(names) < n_expected:
            names = names + [f"ch{i+1}" for i in range(len(names), n_expected)]
        else:
            names = names[:n_expected]
    return names

def drop_ground_channels(X: np.ndarray, names: List[str]) -> Tuple[np.ndarray, List[str]]:
    if not names:
        return X, names
    keep_idx = [i for i, nm in enumerate(names) if str(nm).strip().lower() not in GROUND_TOKENS]
    if len(keep_idx) == len(names) or len(keep_idx) == 0:
        return X, names
    X2 = X[keep_idx, :]
    names2 = [names[i] for i in keep_idx]
    return X2, names2

def parse_chs_csv(csv: Optional[str]) -> Optional[List[int]]:
    if not csv:
        return None
    parts = [s.strip() for s in str(csv).split(",") if s.strip()]
    out: List[int] = []
    seen = set()
    for s in parts:
        if s.isdigit():
            idx1 = int(s)
            if idx1 not in seen:
                out.append(idx1 - 1)
                seen.add(idx1)
    return out or None


def analyze_contact(data: np.ndarray, fs: float, names: List[str]) -> List[Dict[str, Any]]:
    C, T = data.shape
    results: List[Dict[str, Any]] = []

    # Compute per-channel metrics
    rms = np.sqrt(np.mean(data ** 2, axis=1) + 1e-12)
    mad = np.median(np.abs(data - np.median(data, axis=1, keepdims=True)), axis=1)

    # Identify mains frequency (closest to 50 or 60)
    mains = 50.0 if abs(band_power(detrend(data[0]), fs, 48, 52) - band_power(detrend(data[0]), fs, 58, 62)) < 0 else 60.0

    baseline_band = (1.0, 45.0)
    line_band = ((48.0, 52.0) if mains == 50.0 else (58.0, 62.0))

    # Compute ratios
    ratios = []
    for ch in range(C):
        x = detrend(data[ch])
        p_base = band_power(x, fs, *baseline_band)
        p_line = band_power(x, fs, *line_band)
        ratio = float(p_line / (p_base + 1e-12)) if math.isfinite(p_line) and math.isfinite(p_base) else float("nan")
        ratios.append(ratio)

    ratios = np.array(ratios, dtype=np.float32)

    # Heuristics
    rms_med = float(np.median(rms)) if C else 0.0
    rms_low_thresh = 0.15 * (rms_med + 1e-9)
    rms_high_thresh = 3.0 * (rms_med + 1e-9)

    for ch in range(C):
        x = data[ch]
        x_i32_like = np.unique(x.astype(np.int32)).size < 0.1 * x.size and (x.dtype != np.float32)
        # Clipping: many samples equal to min or max
        vals, counts = np.unique(x, return_counts=True)
        frac_mode = float(np.max(counts)) / max(1, x.size)
        clipping = frac_mode > 0.2

        # Flatline: very low variation
        is_flat = mad[ch] < 1e-6 or np.std(x) < 1e-6

        # Drift (relative): compare detrended RMS to raw RMS
        x_dt = detrend(x)
        drift_ratio = float((np.sqrt(np.mean((x - x_dt) ** 2)) + 1e-12) / (np.sqrt(np.mean(x ** 2)) + 1e-12))

        info: Dict[str, Any] = {
            "channel": int(ch),
            "name": (names[ch] if 0 <= ch < len(names) else f"ch{ch+1}"),
            "rms": float(rms[ch]),
            "mad": float(mad[ch]),
            "line_ratio": float(ratios[ch]) if np.isfinite(ratios[ch]) else float("nan"),
            "clipping": bool(clipping),
            "flat": bool(is_flat),
            "drift_ratio": float(drift_ratio),
            "flags": [],
        }

        # Flagging rules (unitless / relative heuristics)
        if rms[ch] < rms_low_thresh:
            info["flags"].append("very_low_rms")  # likely poor contact or disconnected
        if rms[ch] > rms_high_thresh:
            info["flags"].append("very_high_rms")  # motion/noise/loose
        if np.isfinite(ratios[ch]) and ratios[ch] > 0.5:
            info["flags"].append("line_noise_dominant")
        if clipping:
            info["flags"].append("clipping")
        if is_flat:
            info["flags"].append("flatline")
        if drift_ratio > 0.5:
            info["flags"].append("strong_drift")

        results.append(info)

    return results


def print_recommendations(res: List[Dict[str, Any]]) -> None:
    """Print actionable recommendations based on per-channel flags.
    Flags considered: very_low_rms, very_high_rms, line_noise_dominant, clipping, flatline, strong_drift
    """
    if not res:
        return

    # Group channels by flag
    flagged: Dict[str, List[Dict[str, Any]]] = {}
    for r in res:
        for f in (r.get("flags") or []):
            flagged.setdefault(f, []).append(r)

    def chlist(items: List[Dict[str, Any]]) -> str:
        return ", ".join(f"{it['channel']}({it['name']})" for it in items[:8]) + ("…" if len(items) > 8 else "")

    print("\nRecommendations:")

    # Severe issues first
    if flagged.get("flatline"):
        print(f"- Flatline on: {chlist(flagged['flatline'])}. Re-seat electrode/lead, verify connector continuity, swap cable/electrode if needed.")
    if flagged.get("clipping"):
        print(f"- Clipping on: {chlist(flagged['clipping'])}. Reduce preamp gain (if available), re-seat to reduce DC offset, ensure no short/bridge due to sweat.")

    # Contact quality
    if flagged.get("very_low_rms"):
        print(f"- Very low RMS on: {chlist(flagged['very_low_rms'])}. Improve contact: re-wet/saline/gel, press to skin, part hair, tighten strap, clean skin (alcohol wipe).")
    if flagged.get("very_high_rms"):
        print(f"- Very high RMS on: {chlist(flagged['very_high_rms'])}. Reduce motion/microphonics, check for cable tug/loose connector, stabilize strap/lead.")

    # Environment / grounding
    if flagged.get("line_noise_dominant"):
        print(f"- Mains (50/60 Hz) dominant on: {chlist(flagged['line_noise_dominant'])}. Improve ground/reference contact, remove cable loops, move away from power supplies/chargers, try on battery power. If device supports it, enable notch.")

    # Baseline stability
    if flagged.get("strong_drift"):
        print(f"- Strong baseline drift on: {chlist(flagged['strong_drift'])}. Wait 30–60 s after re-seating, reduce pressure changes/motion, re-seat with consistent tension; re-wet.")

    # If many channels affected, suggest global steps
    total = len(res)
    affected = sum(1 for r in res if r.get("flags"))
    if affected >= max(2, total // 2):
        print("- Many channels are affected: check reference/ground quality, ensure headband evenly contacts skin, minimize nearby mains and cable loops.")

    print("- Re-run: HOST=<pi_ip> DURATION=15 ./run bci_contact_check to verify improvements. For visual confirmation, run ./run bci_wavelets and look for stable band activity without strong mains stripes.")



async def record_once(host: str, topic: str, epoch: int, duration: float) -> Tuple[np.ndarray, float, List[str]]:

    # Resolve host to IPv4 like the ws_client TCP probe expects
    try:
        host_ip = socket.gethostbyname(host)
    except Exception:
        host_ip = host

    bufs = Buffers()
    got_fs = [None]
    got_names: List[str] = []

    async def on_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
        # Resolve sampling rate
        fs = None
        if meta is not None:
            for k in ("fs", "sample_rate", "sampling_rate", "sr", "hz"):
                if k in meta:
                    try:
                        fs = float(meta[k])
                        break
                    except Exception:
                        pass
        if fs is None:
            for k in ("fs", "sample_rate", "sampling_rate", "sr", "hz"):
                if k in header:
                    try:
                        fs = float(header[k])
                        break
                    except Exception:
                        pass
        if fs is None:
            fs = bufs.fs or 250.0

        # Channel count
        n_ch = int(header.get("num_channels") or (samples.shape[1] if samples.ndim == 2 else 1))

        # Resolve channel names
        names = None
        for d in (meta or {}, header):
            for k in ("chan_names", "channels", "channel_names", "labels"):
                if k in d and isinstance(d[k], (list, tuple)) and len(d[k]) == n_ch:
                    try:
                        names = [str(x) for x in d[k]]
                    except Exception:
                        names = None
                    break
            if names:
                break
        if names is None:
            names = [f"ch{i+1}" for i in range(n_ch)]
        # Remember names for return
        bufs.names = list(names)

        # Buffering and stop condition
        if got_fs[0] is None:
            got_fs[0] = float(fs)
        bufs.ensure(n_ch=n_ch, fs=float(fs), max_seconds=float(duration) + 1.0)
        bufs.ingest(samples)
        total_samples = max(len(d) for d in bufs.bufs) if bufs.bufs else 0
        if total_samples >= int(float(duration) * float(fs)):
            raise asyncio.CancelledError

    async def main():
        try:
            await start_data_ws(host=host_ip, subscriptions=[(topic, epoch)], on_packet=on_packet)
        except asyncio.CancelledError:
            pass

    await main()

    X = bufs.to_array()  # (C, T)
    fs_out = float(got_fs[0] or bufs.fs or 250.0)
    names_out = bufs.names or [f"ch{i+1}" for i in range(X.shape[0])]
    return X, fs_out, names_out


def print_report(res: List[Dict[str, Any]]):
    if not res:
        print("No data collected.")
        return
    # Column headers
    print("\nContact quality report (per channel):")
    print("ch  name         rms      mad      line_ratio  drift  flags")
    for r in res:
        ch = r["channel"]
        nm = r["name"]
        print(
            f"{ch:2d}  {nm:<10s}  "
            f"{r['rms']:.4g}  {r['mad']:.4g}  {r['line_ratio']:.3f}     {r['drift_ratio']:.2f}  "
            f"{','.join(r['flags']) if r['flags'] else '-'}"
        )

    # Recommendations based on results
    print_recommendations(res)


def main():
    ap = argparse.ArgumentParser(description="Quick electrode contact check: RMS, line-noise ratio, flatline/clipping, drift")
    ap.add_argument("--host", default="raspberrypi.local", help="Pi hostname or IP (no ws://)")
    ap.add_argument("--topic", default="eeg_voltage")
    ap.add_argument("--epoch", type=int, default=1)
    ap.add_argument("--duration", type=float, default=12.0, help="Seconds to record before analyzing")
    ap.add_argument("--npz", default="", help="Optional offline .npz to analyze instead of live WS")
    ap.add_argument("--names", default="", help="Comma-separated channel names to override (e.g., 'T8,O2,Oz,O1,T7,Fpz')")
    ap.add_argument("--drop-ground", action="store_true", help="Drop obvious ground/reference channels like Fpz/A1/A2 from analysis")
    args = ap.parse_args()

    if args.npz:
        d = np.load(args.npz, allow_pickle=True)
        data = d.get("data")
        if data is None:
            raise SystemExit("npz missing 'data'")
        fs = float(np.array(d.get("fs", 250.0)).astype(np.float64))
        ch_names = []
        for k in ("ch_names", "channels", "chan_names"):
            if k in d:
                try:
                    ch_names = [str(x) for x in list(d[k].tolist())]
                    break
                except Exception:
                    pass
        if data.ndim == 2 and data.shape[0] < data.shape[1]:
            X = data.astype(np.float32)
        else:
            X = data.T.astype(np.float32)
        # Optional name override and ground drop
        if args.names:
            ch_names = parse_names_csv(args.names, X.shape[0])
        if args.drop_ground:
            X, ch_names = drop_ground_channels(X, ch_names or [f"ch{i+1}" for i in range(X.shape[0])])
        res = analyze_contact(X, fs, ch_names)
        print_report(res)
        return

    # Live mode
    X, fs, names = asyncio.run(record_once(args.host, args.topic, args.epoch, args.duration))
    if X.size == 0:
        print("No data collected.")
        return
    # Optional name override and ground drop
    if args.names:
        names = parse_names_csv(args.names, X.shape[0])
    if args.drop_ground:
        X, names = drop_ground_channels(X, names or [f"ch{i+1}" for i in range(X.shape[0])])
    res = analyze_contact(X, fs, names)
    print_report(res)


if __name__ == "__main__":
    main()

