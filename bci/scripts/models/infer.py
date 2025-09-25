#!/usr/bin/env python3
"""
infer.py — Generic inference launcher that loads a model/pipeline module and runs:
  - offline inference on an .npz (windowed)
  - streaming inference from the daemon's WebSocket

Examples:
  # Use GPT1 pipeline for offline inference
  python bci/scripts/models/infer.py --file bci/scripts/models/gpt1.py --mode offline --npz bci/scripts/data/session2.npz --weights blinknet.pt

  # Use GPT1 pipeline for streaming inference
  python bci/scripts/models/infer.py --file bci/scripts/models/gpt1.py --mode stream --host raspberrypi.local --topic eeg_voltage --epoch 1 --weights blinknet.pt

Contract expected from the model file (e.g., gpt1.py):
  - get_inference_spec() -> dict with:
      - "model_class": a torch.nn.Module class accepting n_classes
      - "offline_prepare_X4"(X2, fs) -> (N,4,T) preprocessed
      - "make_stream_transform"(fs) -> callable(fp1, fp2) -> (4,T) preprocessed
      - "select_fp_indices"(chan_names) -> (idx_fp1, idx_fp2)
  - It should also provide load_file, window_stage, and (optionally) bandpass_filter
"""

from __future__ import annotations

import argparse
import asyncio
import importlib.util
import sys
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import numpy as np
import torch
from scipy.signal import butter, lfilter, lfilter_zi


def dynamic_import(path: Path):
    """Import a Python module by absolute file path, ensuring its folder is on sys.path for relative imports."""
    path = path.resolve()
    folder = str(path.parent)
    if folder not in sys.path:
        sys.path.insert(0, folder)
    spec = importlib.util.spec_from_file_location(path.stem, str(path))
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Cannot import module at: {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def load_model_and_spec(model_file: Path):
    mod = dynamic_import(model_file)
    if not hasattr(mod, "get_inference_spec"):
        raise RuntimeError(f"Module {model_file} does not export get_inference_spec()")
    spec = mod.get_inference_spec()
    # Validate expected keys
    for k in ("model_class", "offline_prepare_X4", "make_stream_transform", "select_fp_indices"):
        if k not in spec:
            raise RuntimeError(f"get_inference_spec() missing key: {k}")
    # Also verify data helpers exist
    for k in ("load_file", "window_stage"):
        if not hasattr(mod, k):
            raise RuntimeError(f"Module {model_file} must export {k}")
    return mod, spec


def build_model(weights_path: Path, model_class, device: torch.device):
    obj = torch.load(str(weights_path), map_location="cpu")
    meta = {}
    if isinstance(obj, dict) and "state_dict" in obj:
        state = obj["state_dict"]
        meta = (obj.get("meta") or {})
    else:
        state = obj

    # Infer classes from last layer shape if possible, otherwise from meta, else fallback
    if isinstance(state, dict) and "classifier.weight" in state:
        n_classes = int(state["classifier.weight"].shape[0])
    else:
        n_classes = int(len(meta.get("classes", [])) or 3)

    model = model_class(n_classes=n_classes).to(device)
    model.load_state_dict(state)
    model.eval()
    setattr(model, "_meta", meta)
    return model, n_classes, meta


def run_offline(args):
    model_file = Path(args.file)
    mod, spec = load_model_and_spec(model_file)

    # Load data and create windows (continuous → windowed)
    npz_path = Path(args.npz).resolve()
    if not npz_path.exists():
        raise SystemExit(f"NPZ not found: {npz_path}")
    d = mod.load_file(str(npz_path))
    fs = float(d.fs)

    # Match training's preprocessing: bandpass continuous before windowing if available
    cont = getattr(mod, "bandpass_filter", lambda x, lo, hi, fs, order=4: x)(d.data, 0.1, 15.0, fs, order=4).astype(np.float32)
    X, y = mod.window_stage(cont, d.labels, fs, window_s=args.window_s, hop_s=args.hop_s)

    # Select Fp1/Fp2 channels via pipeline contract
    try:
        ch_names = getattr(d, "chan_names", None)
    except Exception:
        ch_names = None
    idx_fp1, idx_fp2 = spec["select_fp_indices"](ch_names)
    X2 = X[:, [idx_fp1, idx_fp2], :]  # (N,2,T)

    # Pipeline preprocessing → (N,4,T)
    X4 = spec["offline_prepare_X4"](X2, fs)

    # Model
    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model, n_classes, meta = build_model(Path(args.weights), spec["model_class"], device)
    if meta:
        print(f"Loaded weights meta: fs={meta.get('fs','?')}Hz window_s={meta.get('window_s','?')} hop_s={meta.get('hop_s','?')} band={meta.get('bandpass','?')} classes={meta.get('classes','?')}")

    # Inference
    X_tensor = torch.from_numpy(X4.astype(np.float32)).to(device)
    with torch.no_grad():
        logits = model(X_tensor)
        preds = torch.argmax(logits, dim=1).cpu().numpy()
        probs = torch.softmax(logits, dim=1).cpu().numpy()

    print(f"Windows: {len(preds)} | fs={fs:.1f}Hz | classes={n_classes}")
    uniq, cnts = np.unique(preds, return_counts=True)
    hist = {int(u): int(c) for u, c in zip(uniq, cnts)}
    print("First 20 preds:", preds[:20])
    print("Class histogram:", hist)

    if args.save_csv:
        out = Path(args.save_csv)
        np.savetxt(out, preds, fmt="%d", delimiter=",")
        print(f"Saved predictions to {out}")


# ------------- Streaming -------------


def ensure_ws_client_on_path(model_file: Path):
    # ws_client.py is in bci/scripts, parent of models/
    scripts_dir = model_file.parent.parent
    if str(scripts_dir) not in sys.path:
        sys.path.insert(0, str(scripts_dir))
    try:
        from ws_client import start_data_ws  # noqa: F401
    except Exception as e:
        raise RuntimeError(f"Failed to import ws_client from {scripts_dir}: {e}")


@dataclass
class StreamState:
    device: torch.device
    model: torch.nn.Module
    transform_factory: Any  # make_stream_transform(fs) -> callable
    select_fp_indices: Any  # function(chan_names) -> (i1, i2)
    window_s: float = 1.0
    hop_s: float = 0.25
    smooth_k: int = 0
    fp1_idx_override: Optional[int] = None
    fp2_idx_override: Optional[int] = None
    # Continuous prefilter (match training's pre-window bandpass)
    filt_b: Optional[np.ndarray] = None
    filt_a: Optional[np.ndarray] = None
    filt_zi_fp1: Optional[np.ndarray] = None
    filt_zi_fp2: Optional[np.ndarray] = None

    # Runtime
    fs: float = 250.0
    num_channels: int = 0
    chan_names: List[str] = field(default_factory=list)
    idx_fp1: int = 0
    idx_fp2: int = 1
    window_len: int = 250
    hop_len: int = 62
    transform: Optional[Any] = None
    buf_fp1: List[float] = field(default_factory=list)
    buf_fp2: List[float] = field(default_factory=list)
    next_start: int = 0
    pred_hist: deque = field(default_factory=deque)

    def on_meta(self, header: Dict[str, Any], meta: Optional[Dict[str, Any]], samples: np.ndarray):
        # Determine fs
        fs_new = self._extract_fs(header, meta, self.fs)
        num_ch = int(header.get("num_channels") or (samples.shape[1] if samples.ndim == 2 else 1))
        names = self._extract_names(header, meta, num_ch)

        changed = (abs(fs_new - self.fs) > 1e-6) or (num_ch != self.num_channels) or (names != self.chan_names)
        if changed:
            self.fs = float(fs_new)
            self.num_channels = int(num_ch)
            self.chan_names = names
            idx1, idx2 = self.select_fp_indices(names)
            self.idx_fp1 = int(self.fp1_idx_override) if (self.fp1_idx_override is not None and 0 <= self.fp1_idx_override < self.num_channels) else int(idx1)
            self.idx_fp2 = int(self.fp2_idx_override) if (self.fp2_idx_override is not None and 0 <= self.fp2_idx_override < self.num_channels) else int(idx2)
            self.window_len = max(1, int(round(self.window_s * self.fs)))
            self.hop_len = max(1, int(round(self.hop_s * self.fs)))
            self.transform = self.transform_factory(self.fs)

            # Design continuous prefilter to match training's pre-window bandpass (0.1–15 Hz, order 4)
            nyq = 0.5 * self.fs
            lo = max(0.1 / nyq, 1e-6)
            hi = min(15.0 / nyq, 0.999999)
            self.filt_b, self.filt_a = butter(4, [lo, hi], btype="band")
            # Reset filter state on reconfig
            self.filt_zi_fp1 = None
            self.filt_zi_fp2 = None

            # Align next_start to hop
            self.next_start = max(0, len(self.buf_fp1) - (len(self.buf_fp1) % self.hop_len))
            print(
                f"[stream] fs={self.fs:.2f}Hz | chans={self.num_channels} {self.chan_names} "
                f"| Fp1={self.idx_fp1}, Fp2={self.idx_fp2} | win={self.window_len} hop={self.hop_len}"
            )

    def append(self, samples: np.ndarray):
        """Append samples to buffers after applying continuous prefilter."""
        f1, f2 = self._filter_pair(samples)
        if f1.size:
            self.buf_fp1.extend(f1.astype(np.float32).tolist())
        if f2.size:
            self.buf_fp2.extend(f2.astype(np.float32).tolist())

    def _filter_pair(self, samples: np.ndarray) -> Tuple[np.ndarray, np.ndarray]:
        """Return filtered (fp1, fp2) arrays for the incoming packet, maintaining filter state."""
        if samples is None:
            return np.array([], dtype=np.float32), np.array([], dtype=np.float32)

        # Extract selected channels
        if samples.ndim == 1:
            fp1 = samples.astype(np.float32)
            fp2 = samples.astype(np.float32)
        else:
            if self.idx_fp1 >= samples.shape[1]:
                return np.array([], dtype=np.float32), np.array([], dtype=np.float32)
            if self.idx_fp2 >= samples.shape[1]:
                return np.array([], dtype=np.float32), np.array([], dtype=np.float32)
            fp1 = samples[:, self.idx_fp1].astype(np.float32)
            fp2 = samples[:, self.idx_fp2].astype(np.float32)

        if fp1.size == 0:
            return fp1, fp2

        # If filter not configured yet, pass-through
        if self.filt_b is None or self.filt_a is None:
            return fp1, fp2

        # Initialize zi on first use for each channel to reduce startup transient
        if self.filt_zi_fp1 is None:
            try:
                zi1 = lfilter_zi(self.filt_b, self.filt_a).astype(np.float32) * fp1[0]
            except Exception:
                zi1 = np.zeros(max(len(self.filt_a), len(self.filt_b)) - 1, dtype=np.float32)
            self.filt_zi_fp1 = zi1
        if self.filt_zi_fp2 is None:
            try:
                zi2 = lfilter_zi(self.filt_b, self.filt_a).astype(np.float32) * fp2[0]
            except Exception:
                zi2 = np.zeros(max(len(self.filt_a), len(self.filt_b)) - 1, dtype=np.float32)
            self.filt_zi_fp2 = zi2

        y1, zi1 = lfilter(self.filt_b, self.filt_a, fp1, zi=self.filt_zi_fp1)
        y2, zi2 = lfilter(self.filt_b, self.filt_a, fp2, zi=self.filt_zi_fp2)
        self.filt_zi_fp1 = zi1
        self.filt_zi_fp2 = zi2
        return y1.astype(np.float32), y2.astype(np.float32)

    def next_window(self) -> Optional[Tuple[np.ndarray, np.ndarray]]:
        have = max(0, min(len(self.buf_fp1), len(self.buf_fp2)) - self.next_start)
        if have < self.window_len:
            return None
        s = self.next_start
        e = s + self.window_len
        fp1 = np.asarray(self.buf_fp1[s:e], dtype=np.float32)
        fp2 = np.asarray(self.buf_fp2[s:e], dtype=np.float32)
        self.next_start += self.hop_len
        # Prune
        if self.next_start > (self.window_len * 4):
            drop = self.next_start - self.window_len
            if drop > 0:
                self.buf_fp1 = self.buf_fp1[drop:]
                self.buf_fp2 = self.buf_fp2[drop:]
                self.next_start -= drop
        return fp1, fp2

    def infer_window(self, fp1: np.ndarray, fp2: np.ndarray) -> Tuple[int, np.ndarray]:
        assert self.transform is not None
        X4 = self.transform(fp1, fp2)  # (4,T)
        xt = torch.from_numpy(X4[None, :, :]).to(self.device)
        with torch.no_grad():
            logits = self.model(xt)
            probs = torch.softmax(logits, dim=1).detach().cpu().numpy()[0]
            pred = int(np.argmax(probs))
        return pred, probs

    def smooth(self, pred: int) -> int:
        if self.smooth_k <= 1:
            return pred
        self.pred_hist.append(pred)
        while len(self.pred_hist) > self.smooth_k:
            self.pred_hist.popleft()
        vals, cnts = np.unique(np.fromiter(self.pred_hist, dtype=np.int64), return_counts=True)
        return int(vals[int(np.argmax(cnts))])

    @staticmethod
    def _extract_fs(header: Dict[str, Any], meta: Optional[Dict[str, Any]], default: float) -> float:
        for d in (header, meta or {}):
            for k in ("fs", "sample_rate", "sampling_rate", "sr", "hz"):
                if k in d:
                    try:
                        return float(d[k])
                    except Exception:
                        pass
        return default

    @staticmethod
    def _extract_names(header: Dict[str, Any], meta: Optional[Dict[str, Any]], num_channels: int) -> List[str]:
        names = None
        for d in (meta or {}, header):
            for k in ("channels", "channel_names", "labels", "ch_names"):
                if k in d and isinstance(d[k], (list, tuple)) and len(d[k]) > 0:
                    names = [str(x) for x in d[k]]
                    break
            if names:
                break
        if not names or len(names) != num_channels:
            names = [f"ch{i+1}" for i in range(num_channels)]
        return names


async def run_stream(args):
    model_file = Path(args.file)
    mod, spec = load_model_and_spec(model_file)
    ensure_ws_client_on_path(model_file)
    from ws_client import start_data_ws  # type: ignore

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model, n_classes, meta = build_model(Path(args.weights), spec["model_class"], device)
    if meta:
        print(f"[weights] meta: fs={meta.get('fs','?')}Hz window_s={meta.get('window_s','?')} hop_s={meta.get('hop_s','?')} band={meta.get('bandpass','?')} classes={meta.get('classes','?')}")

    state = StreamState(
        device=device,
        model=model,
        transform_factory=spec["make_stream_transform"],
        select_fp_indices=spec["select_fp_indices"],
        window_s=float(args.window_s),
        hop_s=float(args.hop_s),
        smooth_k=int(args.smooth) if args.smooth and args.smooth > 0 else 0,
        fp1_idx_override=(int(args.fp1_idx) if getattr(args, "fp1_idx", -1) is not None and int(args.fp1_idx) >= 0 else None),
        fp2_idx_override=(int(args.fp2_idx) if getattr(args, "fp2_idx", -1) is not None and int(args.fp2_idx) >= 0 else None),
    )

    last_print_ts = 0.0

    async def on_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
        nonlocal last_print_ts
        state.on_meta(header, meta, samples)
        state.append(samples)

        while True:
            pair = state.next_window()
            if pair is None:
                break
            fp1, fp2 = pair
            pred, probs = state.infer_window(fp1, fp2)
            smoothed = state.smooth(pred)

            # Print throttled by hop size
            import time as _t
            now = _t.time()
            if now - last_print_ts > max(0.01, state.hop_len / max(1.0, state.fs) * 0.5):
                print(
                    f"[pred] raw={pred} smoothed={smoothed} "
                    f"probs={[round(float(x), 3) for x in probs.tolist()]} "
                    f"| fs={state.fs:.1f}Hz win={state.window_len} hop={state.hop_len}"
                )
                last_print_ts = now

    print(
        f"🚀 Streaming inference: host={args.host} topic={args.topic} epoch={args.epoch} "
        f"| weights={args.weights} | device={device} | classes=auto"
    )
    await start_data_ws(args.host, [(args.topic, int(args.epoch))], on_packet)


def parse_args():
    ap = argparse.ArgumentParser(description="Generic inference using a model pipeline file")
    ap.add_argument("--file", type=str, default=str(Path(__file__).resolve().parent / "gpt1.py"), help="Path to model pipeline file (e.g., gpt1.py)")
    ap.add_argument("--mode", type=str, choices=["offline", "stream"], default="offline", help="Inference mode")
    ap.add_argument("--weights", type=str, default="blinknet.pt", help="Path to model weights .pt")
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")

    # Offline
    ap.add_argument("--npz", type=str, default=str(Path(__file__).resolve().parents[1] / "data" / "session2.npz"))
    ap.add_argument("--window_s", type=float, default=1.0)
    ap.add_argument("--hop_s", type=float, default=0.25)
    ap.add_argument("--save_csv", type=str, default="")

    # Stream
    ap.add_argument("--host", type=str, default="raspberrypi.local")
    ap.add_argument("--topic", type=str, default="eeg_voltage")
    ap.add_argument("--epoch", type=int, default=1)
    ap.add_argument("--fp1_idx", type=int, default=-1, help="Override index for Fp1 channel (0-based); -1=auto")
    ap.add_argument("--fp2_idx", type=int, default=-1, help="Override index for Fp2 channel (0-based); -1=auto")
    ap.add_argument("--smooth", type=int, default=0, help="Majority vote over last K predictions (0=off)")
    return ap.parse_args()


def main():
    args = parse_args()
    if args.mode == "offline":
        run_offline(args)
    else:
        try:
            asyncio.run(run_stream(args))
        except KeyboardInterrupt:
            print("\nInterrupted. Exiting.")


if __name__ == "__main__":
    main()