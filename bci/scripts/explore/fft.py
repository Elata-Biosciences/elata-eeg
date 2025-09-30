#!/usr/bin/env python3
from __future__ import annotations

"""
Offline FFT/PSD for EEG .npz sessions.

Usage:
  python explore/fft.py data/session.npz [--csv] [--plot] [--fmax 60] [--nperseg 1024] [--noverlap 512] [--units uV|V]

The output power is log10(uV^2/Hz) per channel using Welch's method if SciPy is available,
otherwise a compatible Welch fallback is used.
"""

import argparse
import json
import sys
import pathlib
from typing import List, Tuple, Optional

import numpy as np

try:
    from scipy import signal as spsig  # type: ignore
except Exception:
    spsig = None  # type: ignore

try:
    import matplotlib.pyplot as plt  # type: ignore
except Exception:
    plt = None  # type: ignore


def load_npz_session(path: pathlib.Path) -> Tuple[np.ndarray, float, List[str]]:
    with np.load(path, allow_pickle=True) as npz:
        if "data" not in npz:
            raise ValueError(f"Missing 'data' in {path}")
        data = np.array(npz["data"])
        fs = float(npz["fs"].item()) if "fs" in npz else 250.0
        if "ch_names" in npz:
            ch_names = [str(x) for x in npz["ch_names"].tolist()]
        else:
            C = int(data.shape[0] if data.ndim == 2 else 1)
            ch_names = [f"ch{i+1}" for i in range(C)]

    # Ensure channels-first shape (C, N)
    if data.ndim != 2:
        raise ValueError(f"Expected 2D array for 'data', got shape {data.shape}")
    C, N = data.shape
    # Heuristic: if second dim looks like channels and first is long time, transpose
    # Evaluate: if C > 64 and data.shape[1] <= 64 then transpose
    if C > 64 and data.shape[1] <= 64:
        data = data.T
        C, N = data.shape

    # If ch_names length matches N (time) rather than C, fix by transpose
    if len(ch_names) == data.shape[1] and len(ch_names) != data.shape[0]:
        data = data.T
        C, N = data.shape

    if len(ch_names) != data.shape[0]:
        ch_names = [f"ch{i+1}" for i in range(data.shape[0])]

    return data.astype(np.float64, copy=False), fs, ch_names


def nearest_power_of_two(n: int) -> int:
    # largest power of two <= n
    return 1 << (int(np.floor(np.log2(max(1, n)))))


def welch_fallback(x: np.ndarray, fs: float, nperseg: int, noverlap: int) -> Tuple[np.ndarray, np.ndarray]:
    # x: 1D signal (float), returns f, Pxx (density)
    if nperseg <= 1 or nperseg > x.size:
        nperseg = min(max(256, x.size), x.size)
    step = max(1, nperseg - noverlap)
    if step <= 0:
        step = nperseg // 2
    # Build segments
    idxs = np.arange(0, x.size - nperseg + 1, step, dtype=int)
    if idxs.size == 0:
        idxs = np.array([0], dtype=int)
    w = np.hanning(nperseg)
    # Normalization factor U = sum(w^2)
    U = np.sum(w**2)
    K = 0
    P_acc = None
    for i0 in idxs:
        seg = x[i0:i0+nperseg]
        if seg.size < nperseg:
            break
        xw = seg * w
        X = np.fft.rfft(xw)
        P = (np.abs(X)**2) / (fs * U)
        # One-sided correction (double all except DC and Nyquist)
        if P.size > 2:
            P[1:-1] *= 2.0
        elif P.size == 2:
            P[1] *= 2.0
        if P_acc is None:
            P_acc = P
        else:
            P_acc += P
        K += 1
    if K == 0:
        K = 1
    Pxx = P_acc / K if P_acc is not None else np.zeros(nperseg//2 + 1, dtype=float)
    f = np.fft.rfftfreq(nperseg, d=1.0/fs)
    return f, Pxx


def compute_psd_all(data_uv: np.ndarray, fs: float, nperseg: int, noverlap: int) -> Tuple[np.ndarray, np.ndarray]:
    # data_uv: (C, N)
    C, N = data_uv.shape
    if nperseg is None or nperseg <= 0 or nperseg > N:
        target = int(min(N, max(256, int(round(fs*4.0)))))
        nperseg = nearest_power_of_two(target)
    if noverlap is None or noverlap < 0 or noverlap >= nperseg:
        noverlap = nperseg // 2
    if spsig is not None:
        f, P = spsig.welch(
            data_uv,
            fs=fs,
            window="hann",
            nperseg=nperseg,
            noverlap=noverlap,
            detrend="constant",
            return_onesided=True,
            scaling="density",
            axis=-1,
        )
        # SciPy welch with axis=-1 expects shape (..., N); our data is (C, N) so returns (C, F)
        return f, P
    # Fallback: compute per channel
    f0, P0 = welch_fallback(data_uv[0], fs, nperseg, noverlap)
    P_all = np.empty((data_uv.shape[0], P0.size), dtype=float)
    P_all[0, :] = P0
    for c in range(1, data_uv.shape[0]):
        _, Pc = welch_fallback(data_uv[c], fs, nperseg, noverlap)
        P_all[c, :] = Pc
    return f0, P_all


def main():
    ap = argparse.ArgumentParser(description="Compute per-channel PSD (log10(uV^2/Hz)) from an EEG .npz session.")
    ap.add_argument("npz_path", help="Path to .npz file (e.g., data/session.npz)")
    ap.add_argument("--csv", action="store_true", help="Save CSV with freq and per-channel PSD log10(uV^2/Hz)")
    ap.add_argument("--plot", action="store_true", help="Plot per-channel PSD curves")
    ap.add_argument("--fmax", type=float, default=60.0, help="Max frequency to display/save")
    ap.add_argument("--nperseg", type=int, default=0, help="Welch nperseg (0=auto ~4s, pow2)")
    ap.add_argument("--noverlap", type=int, default=-1, help="Welch noverlap (-1=auto 50%)")
    ap.add_argument("--units", type=str, default="V", choices=["uV", "V"], help="Units of time-series in the file; will convert to uV if needed.")
    args = ap.parse_args()

    npz_path = pathlib.Path(args.npz_path).expanduser()
    if not npz_path.exists():
        print(f"[error] File not found: {npz_path}", file=sys.stderr)
        sys.exit(2)

    data, fs, ch_names = load_npz_session(npz_path)
    C, N = data.shape

    # Convert to microvolts if needed
    if args.units.lower() == "v":
        data_uv = data * 1e6
    else:
        data_uv = data

    nperseg = int(args.nperseg) if args.nperseg and args.nperseg > 0 else 0
    noverlap = int(args.noverlap) if args.noverlap and args.noverlap >= 0 else -1

    f, P_uv2_per_hz = compute_psd_all(
        data_uv,
        fs,
        nperseg,
        noverlap if noverlap >= 0 else None if nperseg == 0 else nperseg//2,
    )

    # Convert to log10(uV^2/Hz)
    with np.errstate(divide="ignore"):
        psd_log10 = np.log10(np.maximum(P_uv2_per_hz, 1e-20))

    # Limit frequency range for output if requested
    mask = (f <= args.fmax) if args.fmax is not None else np.ones_like(f, dtype=bool)

    print(f"[fft] file={npz_path.name} fs={fs:.2f} Hz channels={C} samples={N} method={'scipy.welch' if spsig is not None else 'welch_fallback'}")
    # Quick summary: alpha peak 8-13 Hz per channel
    alpha_lo, alpha_hi = 8.0, 13.0
    for i in range(C):
        m = (f >= alpha_lo) & (f <= alpha_hi)
        if np.any(m):
            idx = np.argmax(psd_log10[i, m])
            f_alpha = f[m][idx]
            val = psd_log10[i, m][idx]
            print(f"  ch{i+1:02d} {ch_names[i]:<8} alpha_peak ~ {f_alpha:5.2f} Hz | log10(uV^2/Hz)={val: .3f}")

    if args.csv:
        out_csv = npz_path.with_suffix("").with_name(npz_path.stem + "_psd.csv")
        # Compose table
        freq_col = f[mask][:, None]
        data_cols = psd_log10[:, mask].T  # shape (F, C)
        table = np.concatenate([freq_col, data_cols], axis=1)
        header = "freq_hz," + ",".join(ch_names)
        np.savetxt(out_csv, table, delimiter=",", header=header, comments="", fmt="%.6f")
        print(f"[fft] wrote {out_csv}")

    if args.plot:
        if plt is None:
            print("[warn] matplotlib not available; cannot plot.", file=sys.stderr)
        else:
            import matplotlib as mpl  # type: ignore
            plt.figure(figsize=(9, 6))
            for i in range(C):
                plt.plot(f[mask], psd_log10[i, mask], lw=1.2, label=f"{ch_names[i]}")
            plt.xlabel("Frequency (Hz)")
            plt.ylabel("log10(uV^2/Hz)")
            plt.title(f"PSD (Welch) — {npz_path.name} — fs={fs:.1f} Hz")
            plt.grid(True, which="both", ls="--", alpha=0.4)
            # Legend outside if many channels
            if C <= 10:
                plt.legend()
            else:
                plt.legend(ncol=2, fontsize=8, loc="upper right")
            plt.tight_layout()
            plt.show()


if __name__ == "__main__":
    main()