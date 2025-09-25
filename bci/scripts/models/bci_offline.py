#!/usr/bin/env python3
# bci_offline.py — train/eval on .npz recorded by record_epochs.py
# Python 3.9+
#
# Usage examples (commented intentionally):
#   # classical baseline:
#   # python bci_offline.py fit  --npz ./session_cued.npz --model classical --out ./classic_model
#   # evaluate classical:
#   # python bci_offline.py eval --npz ./session_cued.npz --model classical --model-path ./classic_model/model.joblib
#   # eegnet (needs torch + braindecode):
#   # python bci_offline.py fit  --npz ./session_cued.npz --model eegnet    --out ./eegnet_model
#   # python bci_offline.py eval --npz ./session_cued.npz --model eegnet    --model-path ./eegnet_model/eegnet.pt

import argparse
import pathlib
from typing import List, Tuple, Optional, Dict

import numpy as np
from scipy.signal import butter, sosfiltfilt, iirnotch, filtfilt, welch

# Optional torch stack (only used for --model eegnet)
try:
    import torch
    from torch.utils.data import TensorDataset, DataLoader
    from braindecode.models import EEGNetv4
    TORCH_OK = True
except Exception:
    TORCH_OK = False

# -------------------- config --------------------
WINDOW_S = 1.25
HOP_S    = 0.25
BP       = (1.0, 45.0)
NOTCHS   = [(60.0, 40.0), (120.0, 40.0)]
DEFAULT_CHANNELS_HINT = ["ch1","ch2","ch3","ch4","ch5","ch6","ch7","ch8"]

BATCH    = 64
LR       = 1e-3
EPOCHS   = 30

LABELS = {"down":0,"up":1,"rest":2}
INV_LABELS = {v:k for k,v in LABELS.items()}

# -------------------- cleaning --------------------
_sos_cache: Dict[float, np.ndarray] = {}
def bp_filter(x: np.ndarray, fs: float) -> np.ndarray:
    sos = _sos_cache.get(fs)
    if sos is None:
        sos = butter(4, [BP[0]/(fs/2), BP[1]/(fs/2)], btype="band", output="sos")
        _sos_cache[fs] = sos
    return sosfiltfilt(sos, x, axis=1)

def iir_notch_zero_phase(x: np.ndarray, fs: float, f0: float, q: float) -> np.ndarray:
    b, a = iirnotch(w0=f0, Q=q, fs=fs)
    return filtfilt(b, a, x, axis=1)

def clean(x: np.ndarray, fs: float) -> np.ndarray:
    y = bp_filter(x, fs)
    for f0, q in NOTCHS:
        y = iir_notch_zero_phase(y, fs, f0, q)
    return y

# -------------------- features (classical) --------------------
def bandpower_features(x: np.ndarray, fs: float, bands=((4,7),(8,12),(13,30),(30,45))) -> np.ndarray:
    """Compute Welch bandpowers per-channel across bands; x: (C, T)."""
    nperseg = max(8, int(0.5 * fs))
    noverlap = int(0.25 * fs)
    feats = []
    for ch in range(x.shape[0]):
        f, Pxx = welch(x[ch], fs=fs, nperseg=nperseg, noverlap=noverlap)
        for lo, hi in bands:
            mask = (f >= lo) & (f <= hi)
            feats.append(float(np.trapz(Pxx[mask], f[mask])))
    return np.array(feats, dtype=np.float32)

# -------------------- io + windowing --------------------
def load_npz(path: pathlib.Path):
    z = np.load(path, allow_pickle=True)
    data   = np.asarray(z["data"])          # (C, N)
    labels = np.asarray(z["labels"])        # (N,)
    fs     = float(np.asarray(z["fs"]))
    chn    = list(np.asarray(z["ch_names"]).tolist())
    events = np.asarray(z["events"]) if "events" in z else None
    return data, labels, fs, chn, events

def choose_channels(ch_names: List[str]) -> List[int]:
    idx = [ch_names.index(c) for c in DEFAULT_CHANNELS_HINT if c in ch_names]
    return idx if idx else list(range(len(ch_names)))

def windows_from_npz(path: pathlib.Path, keep_labels: Optional[List[int]]=None,
                     artifact_gate: bool=True) -> Tuple[np.ndarray, np.ndarray, float, List[str], np.ndarray]:
    """
    Returns:
      X: (N, C, T) float32  (cleaned)
      y: (N,) int64
      fs: float
      ch_names: list[str]
      blk_ids: (N,) int64   contiguous segment ids for block-wise splitting
    """
    data, labels, fs, ch_names, _events = load_npz(path)
    C, N = data.shape
    sel = choose_channels(ch_names)
    data = data[sel, :]
    ch_names = [ch_names[i] for i in sel]

    # contiguous-label blocks (per-sample)
    blk_ids = np.zeros(N, dtype=np.int64)
    cur = 0
    blk = 0
    while cur < N:
        start = cur
        lab = labels[cur]
        while cur < N and labels[cur] == lab:
            cur += 1
        blk_ids[start:cur] = blk
        blk += 1

    m = int(round(WINDOW_S * fs))
    h = int(round(HOP_S * fs))
    if m <= 0 or h <= 0 or m > N:
        raise SystemExit(f"Bad window/hop ({m},{h}) for N={N}, fs={fs}")

    xs = []
    ys = []
    wblk = []
    p_hist = []
    for beg in range(0, N - m + 1, h):
        end = beg + m
        xw  = data[:, beg:end]            # (C, m)
        lab = int(np.bincount(labels[beg:end]).argmax())  # majority
        if keep_labels is not None and lab not in keep_labels:
            continue
        xf = clean(xw, fs).astype(np.float32)
        if artifact_gate:
            p = float(np.sum(xf ** 2))
            p_hist.append(p)
            if len(p_hist) > 32:
                med = float(np.median(p_hist))
                if med > 0 and (p > 5 * med or p < 0.1 * med):
                    continue
        xs.append(xf)
        ys.append(lab)
        center = beg + m // 2
        wblk.append(int(blk_ids[center]))

    if not xs:
        raise SystemExit("[bci_offline] No usable windows from file.")

    X = np.stack(xs)               # (Nw, C, m)
    y = np.array(ys, dtype=np.int64)
    blk = np.array(wblk, dtype=np.int64)
    return X, y, fs, ch_names, blk

# -------------------- classical model --------------------
def fit_classical(X: np.ndarray, y: np.ndarray, blk: np.ndarray, out_dir: pathlib.Path):
    from sklearn.preprocessing import StandardScaler
    from sklearn.linear_model import LogisticRegression
    from sklearn.metrics import accuracy_score, confusion_matrix, classification_report
    from joblib import dump

    # compute Welch bandpowers per window using estimated fs from window length
    fs_est = float(X.shape[2] / WINDOW_S)
    feats = np.stack([bandpower_features(x, fs=fs_est) for x in X], axis=0)

    classes = sorted(set(y.tolist()))
    print("[classical] classes:", {c: INV_LABELS.get(int(c), c) for c in classes})

    # block-wise split
    uniq = sorted(set(blk.tolist()))
    cut = max(1, int(round(0.8 * len(uniq))))
    if cut >= len(uniq):
        cut = len(uniq) - 1
    tr_blocks, te_blocks = uniq[:cut], uniq[cut:]
    tr = np.isin(blk, tr_blocks)
    te = np.isin(blk, te_blocks)

    scaler = StandardScaler()
    Xtr = scaler.fit_transform(feats[tr])
    Xte = scaler.transform(feats[te])

    clf = LogisticRegression(max_iter=500, multi_class="auto", class_weight="balanced")
    clf.fit(Xtr, y[tr])

    yhat = clf.predict(Xte)
    acc  = accuracy_score(y[te], yhat)
    cm   = confusion_matrix(y[te], yhat, labels=classes)
    print(f"[classical] holdout acc: {acc:.3f}")
    print("[classical] confusion (rows=true, cols=pred):\n", cm)
    print("[classical] report:\n", classification_report(
        y[te], yhat, labels=classes,
        target_names=[INV_LABELS.get(int(c), str(c)) for c in classes]
    ))

    out_dir.mkdir(parents=True, exist_ok=True)
    dump({"scaler": scaler, "clf": clf, "classes": classes,
          "win_s": WINDOW_S, "hop_s": HOP_S}, out_dir / "model.joblib")
    print("[classical] saved", out_dir / "model.joblib")

def eval_classical(npz_path: pathlib.Path, model_path: pathlib.Path):
    from joblib import load
    from sklearn.metrics import accuracy_score, confusion_matrix, classification_report

    X, y, fs, ch_names, blk = windows_from_npz(npz_path)
    fs_est = float(X.shape[2] / WINDOW_S)
    feats = np.stack([bandpower_features(x, fs=fs_est) for x in X], axis=0)

    obj = load(model_path)
    scaler, clf, classes = obj["scaler"], obj["clf"], obj["classes"]

    Xn = scaler.transform(feats)
    yhat = clf.predict(Xn)
    acc = accuracy_score(y, yhat)
    cm  = confusion_matrix(y, yhat, labels=classes)
    print(f"[classical/eval] acc on file: {acc:.3f}")
    print("[classical/eval] confusion:\n", cm)
    print("[classical/eval] report:\n", classification_report(
        y, yhat, labels=classes,
        target_names=[INV_LABELS.get(int(c), str(c)) for c in classes]
    ))

# -------------------- EEGNet model --------------------
def fit_eegnet(X: np.ndarray, y: np.ndarray, blk: np.ndarray, out_dir: pathlib.Path):
    if not TORCH_OK:
        raise SystemExit("Install torch + braindecode for --model eegnet")

    n_ch, n_t = X.shape[1], X.shape[2]
    classes = sorted(set(y.tolist()))
    n_out = len(classes)

    # remap labels to 0..n_out-1
    lab2idx = {lab: i for i, lab in enumerate(classes)}
    y_idx = np.array([lab2idx[int(u)] for u in y], dtype=np.int64)

    uniq = sorted(set(blk.tolist()))
    cut = max(1, int(round(0.8 * len(uniq))))
    if cut >= len(uniq):
        cut = len(uniq) - 1
    tr_blocks, te_blocks = uniq[:cut], uniq[cut:]
    tr = np.isin(blk, tr_blocks)
    te = np.isin(blk, te_blocks)

    device = "cuda" if torch.cuda.is_available() else "cpu"
    model = EEGNetv4(n_chans=n_ch, n_outputs=n_out, input_window_samples=n_t, final_conv_length="auto").to(device)
    opt = torch.optim.Adam(model.parameters(), lr=LR, weight_decay=1e-4)
    loss_fn = torch.nn.CrossEntropyLoss()

    tr_loader = DataLoader(TensorDataset(torch.from_numpy(X[tr]), torch.from_numpy(y_idx[tr])),
                           batch_size=BATCH, shuffle=True, drop_last=True)
    te_loader = DataLoader(TensorDataset(torch.from_numpy(X[te]), torch.from_numpy(y_idx[te])),
                           batch_size=BATCH, shuffle=False)

    def run_epoch(loader, train=True):
        model.train(train)
        tot = 0
        correct = 0
        loss_sum = 0.0
        for xb, yb in loader:
            xb, yb = xb.to(device), yb.to(device)
            if train:
                opt.zero_grad()
            out = model(xb)
            loss = loss_fn(out, yb)
            if train:
                loss.backward()
                torch.nn.utils.clip_grad_norm_(model.parameters(), 5.0)
                opt.step()
            pred = out.argmax(1)
            tot += yb.numel()
            correct += int((pred == yb).sum())
            loss_sum += float(loss.item()) * int(yb.numel())
        return (correct / max(1, tot)), (loss_sum / max(1, tot))

    for ep in range(1, EPOCHS + 1):
        ta, tl = run_epoch(tr_loader, True)
        va, vl = run_epoch(te_loader, False)
        print(f"[eegnet] epoch {ep:02d}  train {ta:.3f}/{tl:.3f}  val {va:.3f}/{vl:.3f}")

    out_dir.mkdir(parents=True, exist_ok=True)
    torch.save({
        "state_dict": model.state_dict(),
        "n_ch": n_ch, "n_t": n_t,
        "classes": classes
    }, out_dir / "eegnet.pt")
    print("[eegnet] saved", out_dir / "eegnet.pt")

def eval_eegnet(npz_path: pathlib.Path, model_path: pathlib.Path):
    if not TORCH_OK:
        raise SystemExit("Install torch + braindecode for --model eegnet")

    X, y, fs, ch_names, blk = windows_from_npz(npz_path)

    ckpt = torch.load(model_path, map_location="cpu")
    n_ch, n_t = int(ckpt["n_ch"]), int(ckpt["n_t"])
    classes = list(map(int, ckpt.get("classes", sorted(set(y.tolist())))))
    lab2idx = {lab: i for i, lab in enumerate(classes)}
    y_idx = np.array([lab2idx.get(int(u), 0) for u in y], dtype=np.int64)

    # adjust length if mismatched
    if X.shape[1] != n_ch or X.shape[2] != n_t:
        if X.shape[2] > n_t:
            X = X[:, :, :n_t]
        else:
            pad = n_t - X.shape[2]
            X = np.pad(X, ((0, 0), (0, 0), (0, pad)), mode="constant")

    device = "cuda" if torch.cuda.is_available() else "cpu"
    model = EEGNetv4(n_chans=n_ch, n_outputs=len(classes), input_window_samples=n_t, final_conv_length="auto")
    model.load_state_dict(ckpt["state_dict"])
    model.eval()
    model.to(device)

    loader = DataLoader(TensorDataset(torch.from_numpy(X), torch.from_numpy(y_idx)),
                        batch_size=BATCH, shuffle=False)
    tot = 0
    correct = 0
    with torch.no_grad():
        for xb, yb in loader:
            xb, yb = xb.to(device), yb.to(device)
            pred = model(xb).argmax(1)
            tot += yb.numel()
            correct += int((pred == yb).sum())
    acc = correct / max(1, tot)
    print(f"[eegnet/eval] acc on file: {acc:.3f}  (classes={ {c:INV_LABELS.get(int(c),c) for c in classes} })")

# -------------------- CLI --------------------
def main():
    ap = argparse.ArgumentParser(description="Offline BCI train/eval on .npz recorded by record_epochs.py")
    sub = ap.add_subparsers(dest="cmd", required=True)

    fitp = sub.add_parser("fit")
    fitp.add_argument("--npz", required=True, help="Path to session .npz")
    fitp.add_argument("--model", choices=["classical", "eegnet"], default="classical")
    fitp.add_argument("--out", default="./bci_model")

    evp = sub.add_parser("eval")
    evp.add_argument("--npz", required=True)
    evp.add_argument("--model", choices=["classical", "eegnet"], default="classical")
    evp.add_argument("--model-path", required=True)

    args = ap.parse_args()
    npz_path = pathlib.Path(args.npz)

    if args.cmd == "fit":
        X, y, fs, ch_names, blk = windows_from_npz(npz_path)
        print(f"[fit] NPZ={npz_path.name}  X={X.shape} fs={fs}  classes={sorted(set(y.tolist()))}")
        out_dir = pathlib.Path(args.out)
        if args.model == "classical":
            fit_classical(X, y, blk, out_dir)
        else:
            fit_eegnet(X, y, blk, out_dir)
        return

    if args.cmd == "eval":
        if args.model == "classical":
            eval_classical(npz_path, pathlib.Path(args.model_path))
        else:
            eval_eegnet(npz_path, pathlib.Path(args.model_path))
        return

if __name__ == "__main__":
    main()
