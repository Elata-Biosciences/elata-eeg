#!/usr/bin/env python3
"""
Quick channel sweep utility.
Evaluates validation accuracy across candidate 2‑channel pairs for a given model/data.

Usage (from repo root or bci/scripts):
  PYTHONPATH=. python bci/scripts/models/sweep_channels.py --model eog --npz bci/scripts/data/session2.npz
Options:
  --pairs auto|all  (default auto: tries known frontal/temporal pairs first)
  --max_pairs N     (limit when using --pairs all)
  --epochs E        (default 10)
  --batch B         (default 64)
  --lr LR           (default 3e-4)
  --seed S          (default 1337)
  --window_s, --hop_s (default 1.0, 0.25)
  --norm per_window|dataset (default per_window)
"""
from __future__ import annotations

import argparse
import itertools
import sys
from pathlib import Path
from typing import List, Tuple

import numpy as np
import torch
import torch.nn as nn
from torch.utils.data import DataLoader

HERE = Path(__file__).resolve().parent
SCRIPTS_DIR = HERE.parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from registry import get_spec  # type: ignore
from tools import load_file, window_stage  # type: ignore
from train import GenericWindows, compute_class_weights, train_one_epoch, eval_model  # type: ignore


def candidate_pairs(ch_names: List[str], mode: str = "auto") -> List[Tuple[int,int]]:
    n = len(ch_names)
    lower = [str(nm).lower() for nm in (ch_names or [])]
    def idx(name: str):
        return lower.index(name) if name in lower else -1
    pairs = []
    if mode == "auto":
        for a,b in [("fp1","fp2"),("af7","af8"),("f7","f8"),("af3","af4")]:
            ia, ib = idx(a), idx(b)
            if ia >= 0 and ib >= 0:
                pairs.append((ia, ib))
        # Fallback: first 4 channels all combos
        if not pairs and n >= 2:
            for i in range(min(n,4)):
                for j in range(i+1, min(n,4)):
                    pairs.append((i,j))
    else:  # all
        for i in range(n):
            for j in range(i+1, n):
                pairs.append((i,j))
    # De-dup and keep order
    seen = set()
    out = []
    for p in pairs:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


def main():
    ap = argparse.ArgumentParser(description="Channel sweep for 2‑channel pairs")
    ap.add_argument("--model", type=str, default="eog")
    ap.add_argument("--npz", type=str, required=True)
    ap.add_argument("--pairs", type=str, default="auto", choices=["auto","all"])
    ap.add_argument("--max_pairs", type=int, default=30)
    ap.add_argument("--epochs", type=int, default=10)
    ap.add_argument("--batch", type=int, default=64)
    ap.add_argument("--lr", type=float, default=3e-4)
    ap.add_argument("--seed", type=int, default=1337)
    ap.add_argument("--window_s", type=float, default=1.0)
    ap.add_argument("--hop_s", type=float, default=0.25)
    ap.add_argument("--norm", type=str, default="per_window", choices=["per_window","dataset"])
    args = ap.parse_args()

    np.random.seed(args.seed)
    torch.manual_seed(args.seed)

    spec = get_spec(args.model)
    ModelClass = spec["model_class"]
    offline_prepare_X4 = spec["offline_prepare_X4"]

    print(f"🧠 Loading {args.npz} ...")
    d = load_file(args.npz)
    fs = float(d.fs)
    cont = d.data.astype(np.float32)

    # Windowing
    X, y = window_stage(cont, d.labels, fs, window_s=args.window_s, hop_s=args.hop_s)
    ch_names = list(map(str, getattr(d, "chan_names", [f"ch{i}" for i in range(cont.shape[0])])))

    pairs = candidate_pairs(ch_names, mode=args.pairs)
    if args.pairs == "all" and len(pairs) > args.max_pairs:
        pairs = pairs[:args.max_pairs]
    print(f"🔎 Evaluating {len(pairs)} pairs...")

    results = []
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    for (i, j) in pairs:
        Xp = X[:, [i, j], :]
        # Split
        idx = np.random.permutation(len(y))
        n_val = max(1, int(0.2 * len(y)))
        val_idx = idx[:n_val]
        tr_idx = idx[n_val:]
        X_tr, y_tr = Xp[tr_idx], y[tr_idx]
        X_va, y_va = Xp[val_idx], y[val_idx]

        # Dataset-level normalization option
        if args.norm == "dataset":
            X_tr_prep = offline_prepare_X4(X_tr, fs)
            X_va_prep = offline_prepare_X4(X_va, fs)
            mu = X_tr_prep.mean(axis=(0,2), keepdims=True)
            sd = np.maximum(X_tr_prep.std(axis=(0,2), keepdims=True), 1e-6)
            X_tr_p = (X_tr_prep - mu) / sd
            X_va_p = (X_va_prep - mu) / sd
            identity = lambda X, fs: X
            tr_ds = GenericWindows(X_tr_p, y_tr, fs=fs, offline_prepare_X4=identity, train=True)
            va_ds = GenericWindows(X_va_p, y_va, fs=fs, offline_prepare_X4=identity, train=False)
        else:
            tr_ds = GenericWindows(X_tr, y_tr, fs=fs, offline_prepare_X4=offline_prepare_X4, train=True)
            va_ds = GenericWindows(X_va, y_va, fs=fs, offline_prepare_X4=offline_prepare_X4, train=False)

        tr_ld = DataLoader(tr_ds, batch_size=args.batch, shuffle=True)
        va_ld = DataLoader(va_ds, batch_size=args.batch, shuffle=False)

        try:
            model = ModelClass(n_classes=int(np.unique(y).size), n_in_ch=int(tr_ds.X.shape[1])).to(device)
        except TypeError:
            model = ModelClass(n_classes=int(np.unique(y).size)).to(device)
        criterion = nn.CrossEntropyLoss(weight=compute_class_weights(y_tr).to(device))
        optim = torch.optim.AdamW(model.parameters(), lr=args.lr)

        best_acc = 0.0
        for ep in range(args.epochs):
            _ = train_one_epoch(model, tr_ld, device, criterion, optim)
            va_loss, va_acc, _, _ = eval_model(model, va_ld, device, criterion)
            best_acc = max(best_acc, va_acc)
        results.append(((i,j), best_acc))
        print(f"pair ({i},{j}) {ch_names[i]},{ch_names[j]} -> best Val Acc={best_acc*100:.2f}%")

    results.sort(key=lambda x: x[1], reverse=True)
    print("\n=== Top pairs ===")
    for (i,j), acc in results[:10]:
        print(f"({i},{j}) {ch_names[i]},{ch_names[j]} : {acc*100:.2f}%")


if __name__ == "__main__":
    main()

