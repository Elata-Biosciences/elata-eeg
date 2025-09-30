#!/usr/bin/env python3
"""
infer_gpt1.py — Load TinyBlinkNet weights and run inference on a new npz.

Usage:
  python bci/scripts/models/infer_gpt1.py --npz bci/scripts/data/session2.npz --weights blinknet.pt
"""
import argparse
import sys
import os
from pathlib import Path
import numpy as np
import torch
import torch.nn as nn

# Make imports work when running this file directly
HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

# Import model and DSP helpers from gpt1.py and tools.py via gpt1
try:
    from gpt1 import TinyBlinkNet, bandpass_filter, standardize_per_window, load_file, window_stage, select_fp_indices
except Exception as e:
    print(f"❌ Could not import from gpt1.py: {e}")
    print("Run from the repo root or set PYTHONPATH to include bci/scripts/models")
    sys.exit(1)

def prepare_windows(d, window_s=1.0, hop_s=0.25):
    fs = float(d.fs)
    cont = bandpass_filter(d.data, 0.1, 15.0, fs, order=4).astype(np.float32)
    X, y = window_stage(cont, d.labels, fs, window_s=window_s, hop_s=hop_s)
    # Select Fp1/Fp2
    try:
        ch_names = getattr(d, "chan_names", None)
    except Exception:
        ch_names = None
    i1, i2 = select_fp_indices(ch_names)
    X = X[:, [i1, i2], :]
    # Expand to 4 channels per window
    fp1 = X[:, 0, :]
    fp2 = X[:, 1, :]
    diff = fp1 - fp2
    sumv = 0.5 * (fp1 + fp2)
    X4 = np.stack([fp1, fp2, diff, sumv], axis=1).astype(np.float32)
    # Bandpass per window on 4-ch to match training
    for i in range(X4.shape[0]):
        X4[i] = bandpass_filter(X4[i], 0.1, 15.0, fs, order=4).astype(np.float32)
    # Standardize per window
    for i in range(X4.shape[0]):
        X4[i] = standardize_per_window(X4[i]).astype(np.float32)
    return X4, y, fs

def infer(args):
    npz_path = Path(args.npz)
    if not npz_path.exists():
        print(f"❌ NPZ not found: {npz_path}")
        sys.exit(2)
    d = load_file(str(npz_path))
    X4, y, fs = prepare_windows(d, args.window_s, args.hop_s)

    # Infer n_classes from checkpoint
    ckpt = torch.load(args.weights, map_location="cpu")
    # Unwrap state_dict from saved object if needed
    if isinstance(ckpt, dict) and "state_dict" in ckpt:
        state_dict = ckpt["state_dict"]
        meta = ckpt.get("meta", {})
    else:
        state_dict = ckpt
        meta = {}
    # Infer classes
    if isinstance(state_dict, dict) and "classifier.weight" in state_dict:
        n_classes = int(state_dict["classifier.weight"].shape[0])
    else:
        n_classes = int(meta.get("n_classes", 3))

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model = TinyBlinkNet(n_classes=n_classes).to(device)
    model.load_state_dict(state_dict, strict=False)
    model.eval()

    X_tensor = torch.from_numpy(X4).to(device)
    with torch.no_grad():
        logits = model(X_tensor)
        preds = torch.argmax(logits, dim=1).cpu().numpy()
        probs = torch.softmax(logits, dim=1).cpu().numpy()

    print(f"Windows: {len(preds)}  fs={fs:.1f}Hz  classes={n_classes}")
    uniq, cnts = np.unique(preds, return_counts=True)
    dist = {int(u): int(c) for u, c in zip(uniq, cnts)}
    print("First 20 preds:", preds[:20])
    print("Class histogram:", dist)

    if args.save_csv:
        out = Path(args.save_csv)
        np.savetxt(out, preds, fmt="%d", delimiter=",")
        print(f"Saved predictions to {out}")

    return 0

def main():
    p = argparse.ArgumentParser(description="Inference for TinyBlinkNet")
    p.add_argument("--npz", type=str, default=str(HERE.parent / "data" / "session2.npz"))
    p.add_argument("--weights", type=str, default="blinknet.pt")
    p.add_argument("--window_s", type=float, default=1.0)
    p.add_argument("--hop_s", type=float, default=0.25)
    p.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")
    p.add_argument("--save_csv", type=str, default="", help="Optional path to save predictions as CSV")
    args = p.parse_args()
    sys.exit(infer(args))

if __name__ == "__main__":
    main()