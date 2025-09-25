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
    from gpt1 import TinyBlinkNet, offline_prepare_X4, select_fp_indices, load_file, window_stage
except Exception as e:
    print(f"❌ Could not import from gpt1.py: {e}")
    print("Run from the repo root or set PYTHONPATH to include bci/scripts/models")
    sys.exit(1)

def prepare_windows(d, window_s=1.0, hop_s=0.25, fp_indices=None):
    fs = float(d.fs)
    cont = d.data.astype(np.float32)  # no pre-filter; per-window preprocessing will be applied
    X, y = window_stage(cont, d.labels, fs, window_s=window_s, hop_s=hop_s)
    # Select Fp1/Fp2 — prefer provided indices from checkpoint meta; else infer
    try:
        ch_names = getattr(d, "chan_names", None)
    except Exception:
        ch_names = None
    if fp_indices is None:
        idx_fp1, idx_fp2 = select_fp_indices(ch_names)
    else:
        idx_fp1, idx_fp2 = int(fp_indices[0]), int(fp_indices[1])
        if idx_fp1 == idx_fp2 or idx_fp1 < 0 or idx_fp2 < 0 or idx_fp1 >= X.shape[1] or idx_fp2 >= X.shape[1]:
            idx_fp1, idx_fp2 = select_fp_indices(ch_names)
    X = X[:, [idx_fp1, idx_fp2], :]
    # Apply the exact same per-window pipeline as training/inference helpers
    X4 = offline_prepare_X4(X, fs)  # (N,4,T)
    return X4, y, fs, (idx_fp1, idx_fp2)

def infer(args):
    npz_path = Path(args.npz)
    if not npz_path.exists():
        print(f"❌ NPZ not found: {npz_path}")
        sys.exit(2)

    # Load checkpoint (supports {"state_dict", "meta"} or raw state_dict)
    ckpt = torch.load(args.weights, map_location="cpu")
    if isinstance(ckpt, dict) and "state_dict" in ckpt:
        state_dict = ckpt["state_dict"]
        meta = ckpt.get("meta", {}) or {}
    else:
        state_dict = ckpt
        meta = {}

    # Determine classes and fp indices from checkpoint if available
    if "classifier.weight" in state_dict:
        n_classes = int(state_dict["classifier.weight"].shape[0])
    elif "classes" in meta and isinstance(meta["classes"], (list, tuple)) and len(meta["classes"]) > 0:
        n_classes = int(len(meta["classes"]))
    else:
        n_classes = 3

    fp_indices = None
    if "fp_indices" in meta and isinstance(meta["fp_indices"], (list, tuple)) and len(meta["fp_indices"]) == 2:
        fp_indices = (int(meta["fp_indices"][0]), int(meta["fp_indices"][1]))

    # Load data and prepare windows with identical preprocessing
    d = load_file(str(npz_path))
    X4, y, fs, used_idx = prepare_windows(d, args.window_s, args.hop_s, fp_indices=fp_indices)

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model = TinyBlinkNet(n_classes=n_classes).to(device)

    # Migrate BN→GN keys and drop BN running buffers for backward compat
    sd = {}
    for k, v in state_dict.items():
        if any(s in k for s in ("running_mean", "running_var", "num_batches_tracked")):
            continue
        nk = k.replace(".bn.", ".gn.").replace("head_bn.", "head_gn.")
        sd[nk] = v
    load_res = model.load_state_dict(sd, strict=False)
    model.eval()
    try:
        missing = getattr(load_res, "missing_keys", [])
        unexpected = getattr(load_res, "unexpected_keys", [])
        if missing or unexpected:
            print(f"[warn] load_state: missing={missing} unexpected={unexpected}")
    except Exception:
        pass

    X_tensor = torch.from_numpy(X4).to(device)
    with torch.no_grad():
        logits = model(X_tensor)
        preds = torch.argmax(logits, dim=1).cpu().numpy()
        probs = torch.softmax(logits, dim=1).cpu().numpy()

    print(f"Windows: {len(preds)}  fs={fs:.1f}Hz  classes={n_classes}  FpIdx={used_idx}")
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
    p.add_argument("--smooth", type=int, default=0, help="Majority-vote window K over preds (0=off)")
    p.add_argument("--ema_alpha", type=float, default=0.0, help="EMA alpha for probability smoothing (0=off)")
    args = p.parse_args()
    sys.exit(infer(args))

if __name__ == "__main__":
    main()