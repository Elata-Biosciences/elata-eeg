#!/usr/bin/env python3
# model.py — TinyBlinkNet for 2-ch Fp1/Fp2 → (left/right/rest)
# Python 3.9+

import sys, argparse, math, random, time
import numpy as np
import torch
import torch.nn as nn
from torch.utils.data import Dataset, DataLoader
from sklearn.model_selection import train_test_split
from sklearn.metrics import confusion_matrix, ConfusionMatrixDisplay, accuracy_score, precision_recall_fscore_support, classification_report
import matplotlib.pyplot as plt
from scipy.signal import butter, lfilter
import re

# ----------------------------
# Your helpers
# ----------------------------
try:
    from tools import load_file, window_stage
except ImportError:
    print("❌ FATAL: tools.py not found. Put this file next to model.py.")
    sys.exit(1)

# ----------------------------
# Repro
# ----------------------------
def set_seed(seed: int = 1337):
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)

# ----------------------------
# DSP
# ----------------------------
def bandpass_filter(data: np.ndarray, low: float, high: float, fs: float, order: int = 4) -> np.ndarray:
    """data: (C, T) or (T,) — returns same shape, zero-phase not used (IIR lfilter for speed)."""
    nyq = 0.5 * fs
    lo = max(low / nyq, 1e-6)
    hi = min(high / nyq, 0.999999)
    b, a = butter(order, [lo, hi], btype="band")
    if data.ndim == 1:
        return lfilter(b, a, data)
    return lfilter(b, a, data, axis=-1)

def standardize_per_window(x: np.ndarray, eps: float = 1e-6) -> np.ndarray:
    """x: (C, T) → z-score per channel."""
    mu = x.mean(axis=-1, keepdims=True)
    sd = x.std(axis=-1, keepdims=True)
    return (x - mu) / (sd + eps)

# ----------------------------
# Inference pipeline helpers (exported)
# ----------------------------
def select_fp_indices(chan_names):
    """
    Best-effort selection of Fp1/Fp2 channel indices from a list of names.
    Falls back to (0,1) if not found or invalid.
    """
    try:
        lower = [str(n).lower() for n in (chan_names or [])]
        i1 = lower.index("fp1") if "fp1" in lower else 0
        i2 = lower.index("fp2") if "fp2" in lower else (1 if len(lower) > 1 else 0)
        if i1 == i2:
            i1, i2 = 0, 1 if len(lower) > 1 else 0
        return int(i1), int(i2)
    except Exception:
        return 0, 1


def parse_channel_list(ch_text, n_chans):
    """
    Parse a user-provided '--channels' string into two 0-based indices.

    Accepts formats like '4,5' or '4 5'. If both numbers are within 1..n_chans and 0 is not present,
    interpret as 1-based and convert to 0-based. If any 0 is present, treat as 0-based.
    Raises ValueError on invalid input.
    """
    s = str(ch_text).strip()
    if not s:
        raise ValueError("empty --channels")
    parts = [p for p in re.split(r"[,\s]+", s) if p]
    vals = [int(p) for p in parts]
    if len(vals) != 2:
        raise ValueError(f"expected exactly two indices, got {vals}")
    if 0 in vals:
        i, j = vals
    else:
        if 1 <= min(vals) and max(vals) <= n_chans:
            i, j = vals[0] - 1, vals[1] - 1
        else:
            i, j = vals
    if not (0 <= i < n_chans and 0 <= j < n_chans):
        raise ValueError(f"indices out of range for {n_chans} channels: {i},{j}")
    if i == j:
        raise ValueError("indices must be distinct")
    return int(i), int(j)

def expand_to_4ch(fp1, fp2):
    """
    Given two 1-D arrays (T,), compute 4-channel features:
    [Fp1, Fp2, Fp1-Fp2, (Fp1+Fp2)/2] → shape (4, T) float32
    """
    fp1 = np.asarray(fp1, dtype=np.float32)
    fp2 = np.asarray(fp2, dtype=np.float32)
    diff = fp1 - fp2
    sumv = 0.5 * (fp1 + fp2)
    X4 = np.stack([fp1, fp2, diff, sumv], axis=0).astype(np.float32)
    return X4


def offline_prepare_X4(X2, fs):
    """
    X2: (N, 2, T) → expands to (N, 4, T), bandpass 0.1–15 Hz, z-score per window (matches training).
    """
    assert X2.ndim == 3 and X2.shape[1] == 2, "Expected X shape (N, 2, T)"
    N = X2.shape[0]
    fp1 = X2[:, 0, :]
    fp2 = X2[:, 1, :]
    diff = fp1 - fp2
    sumv = 0.5 * (fp1 + fp2)
    X4 = np.stack([fp1, fp2, diff, sumv], axis=1).astype(np.float32)  # (N,4,T)

    # Bandpass + standardize per window
    for i in range(N):
        X4[i] = bandpass_filter(X4[i], 0.1, 15.0, float(fs), order=4).astype(np.float32)
        X4[i] = standardize_per_window(X4[i]).astype(np.float32)
    return X4.astype(np.float32)


def make_stream_transform(fs):
    """
    Returns a callable transform(fp1, fp2) -> (4, T) float32 that:
    - builds 4ch features,
    - bandpasses to 0.1–15 Hz,
    - standardizes per window.
    """
    fs = float(fs)
    def _transform(fp1, fp2):
        X4 = expand_to_4ch(fp1, fp2)
        X4 = bandpass_filter(X4, 0.1, 15.0, fs, order=4).astype(np.float32)
        X4 = standardize_per_window(X4).astype(np.float32)
        return X4
    return _transform


def get_inference_spec():
    """
    Export a simple contract so generic inference tools can discover the pipeline:
      - model_class: constructor for the model (expects n_classes kw or arg)
      - offline_prepare_X4(X2, fs): (N,2,T) -> (N,4,T) preprocessed
      - make_stream_transform(fs): () -> callable(fp1, fp2) -> (4,T) preprocessed
      - select_fp_indices(chan_names): returns (idx_fp1, idx_fp2)
    """
    return {
        "model_class": TinyBlinkNet,
        "offline_prepare_X4": offline_prepare_X4,
        "make_stream_transform": make_stream_transform,
        "select_fp_indices": select_fp_indices,
    }

# ----------------------------
# Dataset
# ----------------------------
class BlinkWindows(Dataset):
    def __init__(self, X: np.ndarray, y: np.ndarray, fs: float, train: bool):
        """
        X: (N, C(=2), T) with channels [Fp1, Fp2]
        y: (N,)
        We expand to 4 channels: [Fp1, Fp2, Fp1-Fp2, (Fp1+Fp2)/2], bandpass 0.1–15 Hz, z-score per window.
        """
        self.fs = float(fs)
        self.train = train

        assert X.ndim == 3 and X.shape[1] == 2, "Expected X shape (N, 2, T)"
        fp1 = X[:, 0, :]
        fp2 = X[:, 1, :]
        diff = fp1 - fp2           # horizontal EOG (lateralization)
        sumv = 0.5 * (fp1 + fp2)   # vertical EOG (blink strength)
        X4 = np.stack([fp1, fp2, diff, sumv], axis=1)  # (N, 4, T)

        # Bandpass to blink band
        for i in range(X4.shape[0]):
            X4[i] = bandpass_filter(X4[i], 0.1, 15.0, self.fs, order=4)

        # Standardize per window
        for i in range(X4.shape[0]):
            X4[i] = standardize_per_window(X4[i])

        self.X = X4.astype(np.float32)     # (N, 4, T)
        self.y = y.astype(np.int64)

    def __len__(self):
        return self.X.shape[0]

    def _augment(self, x: np.ndarray) -> np.ndarray:
        """Light time-domain augments: jitter, gain, shift (keep labels)."""
        # x: (4, T)
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

# ----------------------------
# Model: TinyBlinkNet
# ----------------------------
class DepthwiseSeparableConv1d(nn.Module):
    def __init__(self, in_ch, out_ch, kernel_size, dilation=1):
        super().__init__()
        padding = (kernel_size // 2) * dilation
        self.depth = nn.Conv1d(in_ch, in_ch, kernel_size=kernel_size, padding=padding,
                               dilation=dilation, groups=in_ch, bias=False)
        self.point = nn.Conv1d(in_ch, out_ch, kernel_size=1, bias=False)
        self.bn = nn.BatchNorm1d(out_ch)
        self.act = nn.GELU()

    def forward(self, x):
        x = self.depth(x)
        x = self.point(x)
        x = self.bn(x)
        x = self.act(x)
        return x

class TinyBlinkNet(nn.Module):
    """
    Input: (B, 4, T)  [Fp1, Fp2, diff, sum]
    Stack of temporal depthwise separable convs with increasing receptive field.
    Global average pool over time → linear → 3 classes.
    """
    def __init__(self, n_classes: int = 3):
        super().__init__()
        self.stem = nn.Sequential(
            nn.Conv1d(4, 16, kernel_size=11, padding=5, bias=False),
            nn.BatchNorm1d(16),
            nn.GELU(),
            nn.Dropout(0.1),
        )
        self.block1 = DepthwiseSeparableConv1d(16, 32, kernel_size=31, dilation=1)
        self.block2 = DepthwiseSeparableConv1d(32, 48, kernel_size=41, dilation=2)
        self.block3 = DepthwiseSeparableConv1d(48, 64, kernel_size=51, dilation=3)
        self.head_bn = nn.BatchNorm1d(64)
        self.head_act = nn.GELU()
        self.dropout = nn.Dropout(0.25)
        self.classifier = nn.Linear(64, n_classes)

    def forward(self, x):               # x: (B, 4, T)
        x = self.stem(x)
        x = self.block1(x)
        x = self.block2(x)
        x = self.block3(x)
        x = self.head_bn(x)
        x = self.head_act(x)
        x = x.mean(dim=-1)              # GAP over time -> (B, 64)
        x = self.dropout(x)
        return self.classifier(x)

# ----------------------------
# Training
# ----------------------------
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

# ----------------------------
# Main
# ----------------------------
def main():
    parser = argparse.ArgumentParser(description="TinyBlinkNet for Fp1/Fp2 blink control (left/right/rest)")
    parser.add_argument("--npz", type=str, default="data/session2.npz", help="Input npz path")
    parser.add_argument("--window_s", type=float, default=1.0, help="Window length (s)")
    parser.add_argument("--hop_s", type=float, default=0.25, help="Hop length (s)")
    parser.add_argument("--epochs", type=int, default=50)
    parser.add_argument("--batch", type=int, default=64)
    parser.add_argument("--lr", type=float, default=3e-4)
    parser.add_argument("--seed", type=int, default=1337)
    parser.add_argument("--channels", type=str, default=None,
                        help="Two channel indices to use, e.g., '4,5' or '3 4'. Accepts 1-based or 0-based; converted internally to 0-based.")
    args = parser.parse_args()

    set_seed(args.seed)

    # 1) Load continuous data
    print(f"🧠 Loading {args.npz} ...")
    d = load_file(args.npz)  # expects fields: data (C,T), labels (list of (t_start_s, t_end_s, label_idx)), fs
    fs = float(d.fs)
    print(f"   fs={fs:.1f} Hz, seconds={d.data.shape[1]/fs:.1f}, chans={d.data.shape[0]}")
    # Inspect raw label distribution before windowing
    try:
        raw_u, raw_c = np.unique(d.labels, return_counts=True)
        print(f"   Raw labels: {raw_u.tolist()} counts: {raw_c.tolist()} (num_classes={int(raw_u.size)})")
    except Exception as e:
        print(f"   (unable to inspect raw labels: {e})")

    # 2) Window the filtered data
    #    We’ll prefilter before windowing so edges are consistent
    print("🔧 Bandpass 0.1–15 Hz on continuous...")
    cont = bandpass_filter(d.data, 0.1, 15.0, fs, order=4).astype(np.float32)  # (C,T)
    print("🔪 Windowing...")
    X, y = window_stage(cont, d.labels, fs, window_s=args.window_s, hop_s=args.hop_s)  # X:(N, C, T), y:(N,)
    print(f"   Windows: {X.shape[0]}, shape={X.shape}")

    # Inspect windowed label distribution and normalize to 0..K-1
    try:
        win_u, win_c = np.unique(y, return_counts=True)
        print(f"   Windowed labels (pre-remap): {win_u.tolist()} counts: {win_c.tolist()} (num_classes={int(win_u.size)})")
    except Exception as e:
        print(f"   (unable to inspect windowed labels: {e})")

    # Build remap LUT from original labels to 0..K-1
    classes_original = np.array(sorted(np.unique(y)), dtype=np.int64)
    class_map = {int(c): int(i) for i, c in enumerate(classes_original)}
    if not np.array_equal(classes_original, np.arange(classes_original.size, dtype=np.int64)):
        y = np.array([class_map[int(v)] for v in y], dtype=np.int64)
        print(f"   Remapped labels to 0..{classes_original.size-1}; classes_original={classes_original.tolist()} map={class_map}")
    else:
        print("   Labels already 0-based consecutive; no remap needed.")

    try:
        win_u2, win_c2 = np.unique(y, return_counts=True)
        print(f"   Windowed labels (post-remap): {win_u2.tolist()} counts: {win_c2.tolist()}")
    except Exception as e:
        print(f"   (unable to inspect remapped windowed labels: {e})")

    # Choose two channels:
    # Priority: --channels if provided; otherwise best-effort Fp1/Fp2 by name, else (0,1).
    n_total_ch = int(d.data.shape[0])
    idx_fp1, idx_fp2 = None, None

    if args.channels:
        try:
            idx_fp1, idx_fp2 = parse_channel_list(args.channels, n_total_ch)
            print(f"📌 Using --channels -> 0-based indices [{idx_fp1}, {idx_fp2}] out of {n_total_ch} total")
        except Exception as e:
            print(f"⚠️ Invalid --channels '{args.channels}': {e}. Falling back to Fp1/Fp2 selection.")

    if idx_fp1 is None or idx_fp2 is None:
        try:
            ch_names = getattr(d, "chan_names", None)
        except Exception:
            ch_names = None
        idx_fp1, idx_fp2 = select_fp_indices(ch_names)
        print(f"📌 Selected Fp1/Fp2 indices by name or fallback -> [{idx_fp1}, {idx_fp2}]")

    X = X[:, [idx_fp1, idx_fp2], :]  # keep 2-ch

    # 3) Split
    X_train, X_val, y_train, y_val = train_test_split(
        X, y, test_size=0.2, stratify=y, random_state=args.seed
    )
    # Show split distributions
    try:
        tr_u, tr_c = np.unique(y_train, return_counts=True)
        va_u, va_c = np.unique(y_val, return_counts=True)
        print(f"   Train labels: {tr_u.tolist()} counts: {tr_c.tolist()}")
        print(f"   Val   labels: {va_u.tolist()} counts: {va_c.tolist()}")
    except Exception as e:
        print(f"   (unable to inspect split label distributions: {e})")

    # 4) Datasets/Loaders
    tr_ds = BlinkWindows(X_train, y_train, fs=fs, train=True)
    va_ds = BlinkWindows(X_val, y_val, fs=fs, train=False)
    tr_ld = DataLoader(tr_ds, batch_size=args.batch, shuffle=True, drop_last=False)
    va_ld = DataLoader(va_ds, batch_size=args.batch, shuffle=False, drop_last=False)

    # 5) Model & training setup
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    print(f"🚀 Device: {device}")

    n_classes = int(np.unique(y).size)
    model = TinyBlinkNet(n_classes=n_classes).to(device)

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

    # 6) Save trained weights + metadata
    try:
        save_obj = {
            "state_dict": model.state_dict(),
            "meta": {
                "fs": fs,
                "window_s": float(args.window_s),
                "hop_s": float(args.hop_s),
                "bandpass": [0.1, 15.0, 4],
                "classes": [int(c) for c in sorted(np.unique(y))],  # post-remap classes (0..K-1)
                "classes_original": [int(c) for c in classes_original.tolist()],
                "class_map": {int(k): int(v) for k, v in class_map.items()},
                "n_classes": int(n_classes),
                "model": "TinyBlinkNet",
                "timestamp": time.time(),
                "seed": int(args.seed),
            },
        }
        torch.save(save_obj, "blinknet.pt")
        print("💾 Saved model weights and metadata to blinknet.pt")
    except Exception as e:
        print(f"Failed to save model: {e}")

    # 7) Final eval + confusion matrix
    va_loss, va_acc, y_true, y_pred = eval_model(model, va_ld, device, criterion)
    print("\n--- Training Finished ---")
    print(f"Final Validation Accuracy: {va_acc*100:.2f}%")

    # Detailed metrics
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

    # Confusion matrix
    try:
        labels_sorted = [int(c) for c in sorted(np.unique(y_true))]
        cm = confusion_matrix(y_true, y_pred, labels=labels_sorted)
        disp = ConfusionMatrixDisplay(confusion_matrix=cm, display_labels=labels_sorted)
        disp.plot(values_format="d")
        plt.title("Validation Confusion Matrix")
        plt.tight_layout()
        plt.show()
    except Exception as e:
        print(f"(skipping confusion matrix plot: {e})")


if __name__ == "__main__":
    main()
