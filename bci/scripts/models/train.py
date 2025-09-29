#!/usr/bin/env python3
"""
Generic trainer for EEG BCI models via registry.

Usage:
  # Train default model (gpt1) with session2.npz
  python bci/scripts/models/train.py --model gpt1 --npz bci/scripts/data/session2.npz

  # Change windows, hops, epochs
  python bci/scripts/models/train.py --model gpt1 --npz bci/scripts/data/session2.npz --window_s 1.0 --hop_s 0.25 --epochs 50

Saves weights to <model>.pt (e.g., gpt1.pt) with metadata including fp_indices and chan_names.
"""

from __future__ import annotations

import argparse
import math
import random
import sys
import time
from pathlib import Path
from typing import Any, Dict, Tuple

import numpy as np
import torch
import torch.nn as nn
from torch.utils.data import Dataset, DataLoader
from sklearn.model_selection import train_test_split
from sklearn.metrics import confusion_matrix, ConfusionMatrixDisplay, accuracy_score, precision_recall_fscore_support, classification_report

# Path setup so running this file directly works
HERE = Path(__file__).resolve().parent                 # .../bci/scripts/models
SCRIPTS_DIR = HERE.parent                              # .../bci/scripts
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

# Registry + tools
from registry import get_spec  # type: ignore

try:
    from tools import load_file, window_stage  # type: ignore
except Exception as e:
    print(f"❌ Could not import tools (load_file, window_stage): {e}")
    print("Ensure PYTHONPATH includes bci/scripts or run from repo root.")
    sys.exit(1)


def set_seed(seed: int = 1337):
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)

def parse_chs_arg(chs: str, total_ch: int):
    """
    Parse --chs hyperparameter.
    - "": returns None (legacy behavior)
    - "all" or "*": returns "all"
    - "1,2,3": returns a list of zero-based indices [0,1,2], filtered to valid range.
    """
    s = (chs or "").strip()
    if not s:
        return None
    s_lower = s.lower()
    if s_lower in ("all", "*"):
        return "all"
    parts = [p.strip() for p in s.split(",") if p.strip()]
    idxs = []
    seen = set()
    for p in parts:
        if not p.isdigit():
            continue
        i1 = int(p)
        i0 = i1 - 1  # convert to 0-based
        if 0 <= i0 < int(total_ch) and i0 not in seen:
            seen.add(i0)
            idxs.append(i0)
    if not idxs:
        return None
    return idxs


class GenericWindows(Dataset):
    """
    Dataset that applies the model's offline preparation pipeline per window.
    Expects X of shape (N, C, T) and y of shape (N,).
    """
    def __init__(self, X: np.ndarray, y: np.ndarray, fs: float, offline_prepare_X4, train: bool):
        assert X.ndim == 3 and X.shape[1] >= 1, "Expected X shape (N, C, T)"
        self.fs = float(fs)
        self.train = bool(train)
        # Apply canonical preprocessing (model-provided)
        Xp = offline_prepare_X4(X, self.fs)  # (N, C, T)
        self.X = Xp.astype(np.float32)
        self.y = y.astype(np.int64)

    def __len__(self):
        return self.X.shape[0]

    def _augment(self, x: np.ndarray) -> np.ndarray:
        # Light, generic time-domain augments
        if np.random.rand() < 0.3:
            x = x + np.random.normal(0, 0.02, size=x.shape).astype(np.float32)
        if np.random.rand() < 0.3:
            g = np.random.uniform(0.9, 1.1)
            x = (x * g).astype(np.float32)
        if np.random.rand() < 0.3:
            # small circular shift up to ±50 ms
            max_shift = int(0.05 * self.fs)
            k = np.random.randint(-max_shift, max_shift + 1)
            x = np.roll(x, k, axis=-1)
        return x

    def __getitem__(self, idx: int):
        x = self.X[idx]
        if self.train:
            x = self._augment(x)
        return torch.from_numpy(x), int(self.y[idx])


def compute_class_weights(y: np.ndarray):
    classes, counts = np.unique(y, return_counts=True)
    total = y.shape[0]
    weights = np.zeros(classes.max() + 1, dtype=np.float32)
    for c, n in zip(classes, counts):
        weights[c] = total / (len(classes) * float(n))
    return torch.tensor(weights, dtype=torch.float32)


def train_one_epoch(model, loader, device, criterion, optimizer):
    model.train()
    total_loss = 0.0
    total_n = 0
    for xb, yb in loader:
        xb = xb.to(device)
        yb = yb.to(device)
        optimizer.zero_grad()
        logits = model(xb)
        loss = criterion(logits, yb)
        loss.backward()
        optimizer.step()
        total_loss += float(loss.item()) * xb.size(0)
        total_n += xb.size(0)
    return total_loss / max(1, total_n)


@torch.no_grad()
def eval_model(model, loader, device, criterion):
    model.eval()
    total_loss = 0.0
    total_n = 0
    all_y = []
    all_p = []
    for xb, yb in loader:
        xb = xb.to(device)
        yb = yb.to(device)
        logits = model(xb)
        loss = criterion(logits, yb)
        total_loss += float(loss.item()) * xb.size(0)
        total_n += xb.size(0)
        preds = logits.argmax(dim=1).cpu().numpy()
        all_p.append(preds)
        all_y.append(yb.cpu().numpy())
    y_true = np.concatenate(all_y) if all_y else np.array([])
    y_pred = np.concatenate(all_p) if all_p else np.array([])
    acc = float((y_true == y_pred).mean()) if y_true.size else 0.0
    return total_loss / max(1, total_n), acc, y_true, y_pred


def main():
    ap = argparse.ArgumentParser(description="Generic trainer for EEG BCI models via registry")
    ap.add_argument("--model", type=str, default="gpt1", help="Model key (e.g., gpt1, gpt2)")
    ap.add_argument("--npz", type=str, default="bci/scripts/data/session2.npz", help="Input npz path")
    ap.add_argument("--window_s", type=float, default=1.0, help="Window length (s)")
    ap.add_argument("--hop_s", type=float, default=0.25, help="Hop length (s)")
    ap.add_argument("--epochs", type=int, default=50)
    ap.add_argument("--batch", type=int, default=64)
    ap.add_argument("--lr", type=float, default=3e-4)
    ap.add_argument("--seed", type=int, default=1337)
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")
    ap.add_argument("--save", type=str, default="", help="Output weights path (default: <model>.pt)")
    ap.add_argument("--chs", type=str, default="", help="Channel selection hyperparameter: 'all' or comma-separated 1-based indices (e.g., '1,2,3,4'). Empty=legacy behavior")
    args = ap.parse_args()

    set_seed(args.seed)

    # Resolve model spec
    spec = get_spec(args.model)
    ModelClass = spec["model_class"]
    offline_prepare_X4 = spec["offline_prepare_X4"]
    make_stream_transform = spec["make_stream_transform"]  # not used here but validated by registry
    select_fp_indices = spec["select_fp_indices"]

    # Load continuous data
    print(f"🧠 Loading {args.npz} ...")
    d = load_file(args.npz)  # expects fields: data (C,T), labels (list of (t_start_s, t_end_s, label_idx)), fs
    fs = float(d.fs)
    print(f"   fs={fs:.1f} Hz, seconds={d.data.shape[1]/fs:.1f}, chans={d.data.shape[0]}")

    # Window the data (no continuous pre-filter; per-window preprocessing applied in dataset)
    cont = d.data.astype(np.float32)  # (C,T)
    print("🔪 Windowing...")
    X, y = window_stage(cont, d.labels, fs, window_s=args.window_s, hop_s=args.hop_s)  # X:(N, C, T), y:(N,)
    print(f"   Windows: {X.shape[0]}, shape={X.shape}")
    
    # Channel selection
    try:
        ch_names = getattr(d, "chan_names", None)
    except Exception:
        ch_names = None

    total_ch = X.shape[1]
    def _safe_names(indices):
        if ch_names:
            return [str(ch_names[i]) if 0 <= i < len(ch_names) else f"ch{i}" for i in indices]
        return [f"ch{i}" for i in indices]

    chs_arg = parse_chs_arg(args.chs, total_ch=total_ch)
    used_fp = False
    if chs_arg is None:
        # Legacy behavior: pick Fp1/Fp2 (falls back to 0,1)
        idx_fp1, idx_fp2 = select_fp_indices(ch_names)
        chs_used = [int(idx_fp1), int(idx_fp2)]
        used_fp = True
    elif chs_arg == "all":
        chs_used = list(range(total_ch))
    else:
        chs_used = list(map(int, chs_arg))

    X = X[:, chs_used, :]
    print(f"   Using channels: indices={chs_used} names={_safe_names(chs_used)}")

    # Split
    X_train, X_val, y_train, y_val = train_test_split(
        X, y, test_size=0.2, stratify=y, random_state=args.seed
    )

    # Datasets/Loaders
    tr_ds = GenericWindows(X_train, y_train, fs=fs, offline_prepare_X4=offline_prepare_X4, train=True)
    va_ds = GenericWindows(X_val, y_val, fs=fs, offline_prepare_X4=offline_prepare_X4, train=False)
    tr_ld = DataLoader(tr_ds, batch_size=args.batch, shuffle=True, drop_last=False)
    va_ld = DataLoader(va_ds, batch_size=args.batch, shuffle=False, drop_last=False)

    # Model & training setup
    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    print(f"🚀 Device: {device}")
    
    n_classes = int(np.unique(y).size)
    try:
        n_in_ch = int(tr_ds.X.shape[1])  # set below after dataset creation
    except NameError:
        n_in_ch = int(X.shape[1])  # fallback
    try:
        model = ModelClass(n_classes=n_classes, n_in_ch=n_in_ch).to(device)
    except TypeError:
        model = ModelClass(n_classes=n_classes).to(device)

    class_w = compute_class_weights(y_train).to(device)
    criterion = nn.CrossEntropyLoss(weight=class_w)

    optimizer = torch.optim.AdamW(model.parameters(), lr=args.lr, betas=(0.9, 0.999), weight_decay=1e-4)
    scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(optimizer, T_max=args.epochs)

    best_acc = 0.0
    best_state = None

    print("🏋️ Training...")
    for epoch in range(1, args.epochs + 1):
        tr_loss = train_one_epoch(model, tr_ld, device, criterion, optimizer)
        va_loss, va_acc, _, _ = eval_model(model, va_ld, device, criterion)
        scheduler.step()

        print(f"Epoch [{epoch:02d}/{args.epochs}] | Train Loss: {tr_loss:.4f} | Val Loss: {va_loss:.4f} | Val Acc: {va_acc*100:.2f}%")

        if va_acc > best_acc:
            best_acc = va_acc
            best_state = {k: v.detach().cpu().clone() for k, v in model.state_dict().items()}

    if best_state is not None:
        model.load_state_dict(best_state)

    # Save trained weights + metadata (checkpoint-driven hyperparameters)
    out_path = args.save.strip() or f"{args.model}.pt"
    try:
        bandpass = [0.1, 15.0, 4]
        hparams: Dict[str, Any] = {
            "model_key": str(args.model),
            "pipeline_version": "1.0",
            "window_s": float(args.window_s),
            "hop_s": float(args.hop_s),
            "bandpass": bandpass,
            "norm": "zscore_per_window",
            "features": "per_channel_bandpass_zscore",
            "seed": int(args.seed),
            "classes": [int(c) for c in sorted(np.unique(y))],
            "chs_indices": [int(i) for i in chs_used],
            "n_in_ch": int(tr_ds.X.shape[1]),
            "chan_names": list(map(str, ch_names)) if ch_names is not None else None,
        }
        if 'idx_fp1' in locals() and 'idx_fp2' in locals() and used_fp:
            hparams["fp_indices"] = [int(idx_fp1), int(idx_fp2)]

        save_obj = {
            "state_dict": model.state_dict(),
            "meta": {
                # Back-compat top-level keys
                "fs": fs,
                "window_s": hparams["window_s"],
                "hop_s": hparams["hop_s"],
                "classes": hparams["classes"],
                "model": hparams["model_key"],
                "timestamp": time.time(),
                "seed": hparams["seed"],
                "fp_indices": hparams.get("fp_indices"),
                "chan_names": hparams.get("chan_names"),
                "bandpass": bandpass,
                # New structured hparams block
                "hparams": hparams,
            },
        }
        torch.save(save_obj, out_path)
        print(f"💾 Saved model weights and metadata to {out_path}")
    except Exception as e:
        print(f"Failed to save model: {e}")

    # Final eval + confusion matrix
    va_loss, va_acc, y_true, y_pred = eval_model(model, va_ld, device, criterion)
    print("\n--- Training Finished ---")
    print(f"Final Validation Accuracy: {va_acc*100:.2f}%")

    try:
        labels_sorted = [int(c) for c in sorted(np.unique(y_true))]
        prec, rec, f1, support = precision_recall_fscore_support(
            y_true, y_pred, labels=labels_sorted, zero_division=0
        )
        print("Per-class metrics (precision, recall, f1, support):")
        for i, c in enumerate(labels_sorted):
            print(f"  class {int(c)}: P={prec[i]:.3f} R={rec[i]:.3f} F1={f1[i]:.3f} N={int(support[i])}")
        print(f"Macro F1: {float(f1.mean()):.3f}")
        print("\nClassification report:\n", classification_report(y_true, y_pred, zero_division=0))
    except Exception as e:
        print(f"(skipping detailed metrics: {e})")

    try:
        labels_sorted = [int(c) for c in sorted(np.unique(y_true))]
        cm = confusion_matrix(y_true, y_pred, labels=labels_sorted)
        disp = ConfusionMatrixDisplay(confusion_matrix=cm, display_labels=labels_sorted)
        disp.plot(values_format="d")
        import matplotlib.pyplot as plt  # delayed import to avoid headless issues
        plt.title("Validation Confusion Matrix")
        plt.tight_layout()
        plt.show()
    except Exception as e:
        print(f"(skipping confusion matrix plot: {e})")


if __name__ == "__main__":
    main()