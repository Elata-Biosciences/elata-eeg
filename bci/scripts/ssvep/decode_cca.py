#!/usr/bin/env python3
"""
Training-free SSVEP decoder using Canonical Correlation Analysis (CCA).

- Offline: reads a labeled NPZ recording and predicts the class per window
- No model training needed; uses sin/cos templates at target frequencies (with harmonics)
- Reports accuracy and per-class metrics; optional CSV of predictions

Example:
  PYTHONPATH=. python3 bci/scripts/ssvep/decode_cca.py \
    --npz bci/scripts/data/session_XXXX.npz \
    --freqs 15,12,10,7.5 --labels left,right,up,down \
    --chs 2,3,4 --window_s 1.0 --hop_s 0.25 --harmonics 3

Notes:
- Choose window_s and hop_s to balance responsiveness and frequency resolution.
- 1.0 s windows are a good starting point on a 60 Hz display; 0.5 s also works.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from typing import List, Tuple
import numpy as np
import sys

# Path setup so running this file directly works when invoked from repo root
import pathlib
HERE = pathlib.Path(__file__).resolve().parent              # .../bci/scripts/ssvep
SCRIPTS_DIR = HERE.parent                                   # .../bci/scripts
MODELS_DIR = SCRIPTS_DIR / "models"                        # .../bci/scripts/models
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))
if str(MODELS_DIR) not in sys.path:
    sys.path.insert(0, str(MODELS_DIR))

from tools import load_file, window_stage  # type: ignore
from sklearn.cross_decomposition import CCA  # type: ignore
from sklearn.metrics import classification_report, confusion_matrix


def parse_number_list(s: str | None) -> List[float]:
    if not s:
        return []
    out: List[float] = []
    for p in s.split(','):
        p = p.strip()
        if not p:
            continue
        try:
            v = float(p)
            if v > 0:
                out.append(v)
        except Exception:
            pass
    return out


def parse_string_list(s: str | None) -> List[str]:
    if not s:
        return []
    out = [p.strip() for p in s.split(',') if p.strip()]
    return out


def make_templates(freqs: List[float], fs: float, T: int, harmonics: int) -> List[np.ndarray]:
    """Create sin/cos templates per frequency. Returns list of (T, 2*harmonics) arrays."""
    t = np.arange(T, dtype=np.float64) / float(fs)
    Ts: List[np.ndarray] = []
    for f in freqs:
        cols = []
        for k in range(1, harmonics + 1):
            w = 2.0 * np.pi * (k * f)
            cols.append(np.sin(w * t))
            cols.append(np.cos(w * t))
        Tmat = np.stack(cols, axis=1)  # (T, 2*H)
        # Standardize columns to unit variance to stabilize CCA
        Tmat = (Tmat - Tmat.mean(axis=0, keepdims=True)) / (Tmat.std(axis=0, keepdims=True) + 1e-8)
        Ts.append(Tmat.astype(np.float32))
    return Ts


def pick_channels(chs: str, total: int) -> List[int] | str | None:
    s = (chs or '').strip()
    if not s:
        return None
    s_lower = s.lower()
    if s_lower in ('all', '*'):
        return 'all'
    idxs: List[int] = []
    seen = set()
    for p in s.split(','):
        p = p.strip()
        if p.isdigit():
            i1 = int(p)
            i0 = i1 - 1
            if 0 <= i0 < int(total) and i0 not in seen:
                seen.add(i0)
                idxs.append(i0)
    return idxs or None


@dataclass
class Args:
    npz: str
    freqs: str
    labels: str
    chs: str
    window_s: float
    hop_s: float
    harmonics: int
    save_csv: str


def decode(args: Args) -> int:
    d = load_file(args.npz)
    fs = float(d.fs)
    cont = d.data.astype(np.float32)
    X, y = window_stage(cont, d.labels, fs, window_s=float(args.window_s), hop_s=float(args.hop_s))  # (N,C,T)
    N, C, T = X.shape
    print(f"NPZ={args.npz} | fs={fs:.1f}Hz | windows={N} | shape={X.shape}")

    # Channels
    ch_sel = pick_channels(args.chs, total=C)
    if ch_sel == 'all':
        pass
    elif isinstance(ch_sel, list) and len(ch_sel) > 0:
        X = X[:, ch_sel, :]
        C = X.shape[1]
    else:
        # default: keep as-is; user can specify --chs
        pass

    # Prepare templates
    freqs = parse_number_list(args.freqs)
    if not freqs:
        raise SystemExit("--freqs is required (e.g., 15,12,10,7.5)")
    labels = parse_string_list(args.labels)
    if labels and len(labels) != len(freqs):
        print(f"[warn] labels count ({len(labels)}) != freqs count ({len(freqs)}); using indices as labels")
        labels = []
    templates = make_templates(freqs, fs=fs, T=T, harmonics=int(args.harmonics))

    preds = np.zeros((N,), dtype=np.int64)
    scores = np.zeros((N, len(freqs)), dtype=np.float32)

    # For each window, compute CCA correlation score to each template
    for i in range(N):
        Xi = X[i].T.astype(np.float32)  # (T,C)
        # Standardize per channel to zero mean, unit variance
        Xi = (Xi - Xi.mean(axis=0, keepdims=True)) / (Xi.std(axis=0, keepdims=True) + 1e-6)
        for j, Tj in enumerate(templates):
            try:
                cca = CCA(n_components=1, max_iter=200)
                U, V = cca.fit_transform(Xi, Tj)
                # Correlation of first canonical variates
                num = float(np.corrcoef(U[:, 0], V[:, 0])[0, 1])
                scores[i, j] = num if np.isfinite(num) else 0.0
            except Exception as e:
                scores[i, j] = 0.0
        preds[i] = int(np.nanargmax(scores[i]))

    # Metrics
    if y is not None and y.size == preds.size:
        from sklearn.metrics import precision_recall_fscore_support, accuracy_score
        acc = accuracy_score(y, preds)
        print(f"Accuracy: {acc*100:.2f}%")
        print("Per-class:")
        labs = list(range(len(freqs)))
        prec, rec, f1, sup = precision_recall_fscore_support(y, preds, labels=labs, zero_division=0)
        for k in labs:
            name = labels[k] if (labels and k < len(labels)) else str(k)
            print(f"  {name:>6s}: P={prec[k]:.3f} R={rec[k]:.3f} F1={f1[k]:.3f} N={sup[k]}")
        print("Confusion matrix (rows=true, cols=pred):")
        cm = confusion_matrix(y, preds, labels=labs)
        with np.printoptions(suppress=True):
            print(cm)
    else:
        print("(No labels found; skipped metrics)")

    # Optional CSV
    if args.save_csv:
        np.savetxt(args.save_csv, preds, fmt="%d", delimiter=",")
        print(f"Saved predictions to {args.save_csv}")

    # Preview first 20
    print("First 20 preds:", preds[:20].tolist())

    return 0


def main():
    ap = argparse.ArgumentParser(description="Training-free SSVEP decoder via CCA (offline)")
    ap.add_argument("--npz", type=str, required=True, help="Input NPZ path (recorded session)")
    ap.add_argument("--freqs", type=str, required=True, help="Comma-separated target freqs (Hz)")
    ap.add_argument("--labels", type=str, default="", help="Comma-separated labels for targets (optional)")
    ap.add_argument("--chs", type=str, default="2,3,4", help="Channels to use (1-based comma list or 'all')")
    ap.add_argument("--window_s", type=float, default=1.0)
    ap.add_argument("--hop_s", type=float, default=0.25)
    ap.add_argument("--harmonics", type=int, default=3, help="Number of harmonics to include in templates")
    ap.add_argument("--save_csv", type=str, default="", help="Optional: path to save predictions as CSV")
    args = ap.parse_args()
    code = decode(Args(**vars(args)))
    raise SystemExit(code)


if __name__ == "__main__":
    main()

