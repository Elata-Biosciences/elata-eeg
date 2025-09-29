#!/usr/bin/env python3
"""
Generic streaming inference via model registry.

Usage:
  # Stream inference using gpt1 pipeline
  python bci/scripts/models/infer_stream.py --model gpt1 --weights gpt1.pt --host ws://raspberrypi.local --topic eeg_voltage --epoch 1

  # With smoothing/EMA and explicit channels
  python bci/scripts/models/infer_stream.py --model gpt1 --weights gpt1.pt --smooth 3 --ema_alpha 0.2 --fp1 0 --fp2 1
"""

from __future__ import annotations

import argparse
import asyncio
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
import sys
import time
from typing import Any, Dict, List, Optional, Tuple

import numpy as np
import torch

# Path setup so running this file directly works
HERE = Path(__file__).resolve().parent                 # .../bci/scripts/models
SCRIPTS_DIR = HERE.parent                              # .../bci/scripts
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

# Registry + ws client
from registry import get_spec  # type: ignore
try:
    from ws_client import start_data_ws  # type: ignore
except Exception as e:
    print(f"❌ Could not import start_data_ws from bci/scripts/ws_client.py: {e}")
    print("Ensure PYTHONPATH includes bci/scripts or run from repo root.")
    sys.exit(1)


def extract_fs(header: Dict[str, Any], meta: Optional[Dict[str, Any]], default: float) -> float:
    for d in (header, meta or {}):
        for k in ("fs", "sample_rate", "sampling_rate", "sr", "hz"):
            if k in d:
                try:
                    return float(d[k])
                except Exception:
                    pass
    return default


def extract_names(header: Dict[str, Any], meta: Optional[Dict[str, Any]], num_channels: int) -> List[str]:
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


@dataclass
class StreamState:
    device: torch.device
    model: torch.nn.Module
    transform_factory: Any  # make_stream_transform(fs) -> callable
    select_fp_indices: Any  # function(chan_names) -> (i1, i2)
    window_s: float = 1.0
    hop_s: float = 0.25
    smooth_k: int = 0
    ema_alpha: float = 0.0

    # Preferred FP indices (from weights meta or CLI), optional
    prefer_idx: Optional[Tuple[int, int]] = None

    # Runtime/updatable fields
    fs: float = 250.0
    num_channels: int = 0
    chan_names: List[str] = field(default_factory=list)
    idx_fp1: int = 0
    idx_fp2: int = 1
    window_len: int = 250
    hop_len: int = 62

    # Buffers store raw stream values
    buf_fp1: List[float] = field(default_factory=list)
    buf_fp2: List[float] = field(default_factory=list)
    next_start: int = 0  # next window start index (in samples) for inference
    pred_hist: deque = field(default_factory=deque)

    # Cached transform and EMA state
    transform: Optional[Any] = None
    prob_ema: Optional[np.ndarray] = None

    def update_fs_and_channels(self, header: Dict[str, Any], meta: Optional[Dict[str, Any]], samples: np.ndarray):
        num_ch = int(header.get("num_channels") or (samples.shape[1] if samples.ndim == 2 else 1))
        fs_new = extract_fs(header, meta, self.fs)
        names = extract_names(header, meta, num_ch)

        changed = (abs(fs_new - self.fs) > 1e-6) or (num_ch != self.num_channels) or (names != self.chan_names)
        if changed:
            self.fs = float(fs_new)
            self.num_channels = int(num_ch)
            self.chan_names = names

            # Choose FP indices: prefer checkpoint/CLI-provided if valid, else heuristic
            if self.prefer_idx is not None:
                i1, i2 = int(self.prefer_idx[0]), int(self.prefer_idx[1])
                if 0 <= i1 < self.num_channels and 0 <= i2 < self.num_channels and i1 != i2:
                    self.idx_fp1, self.idx_fp2 = i1, i2
                else:
                    self.idx_fp1, self.idx_fp2 = self.select_fp_indices(self.chan_names)
            else:
                self.idx_fp1, self.idx_fp2 = self.select_fp_indices(self.chan_names)

            self.window_len = max(1, int(round(self.window_s * self.fs)))
            self.hop_len = max(1, int(round(self.hop_s * self.fs)))
            self.next_start = max(0, len(self.buf_fp1) - (len(self.buf_fp1) % self.hop_len))  # align to hop

            # Reset transform and EMA on fs change
            self.transform = self.transform_factory(self.fs)
            self.prob_ema = None

            print(
                f"[stream] fs={self.fs:.2f}Hz | chans={self.num_channels} {self.chan_names} "
                f"| Fp1={self.idx_fp1}, Fp2={self.idx_fp2} | win={self.window_len} hop={self.hop_len}"
            )

    def append_samples(self, samples: np.ndarray):
        if samples.ndim == 1:
            # (batch,) -> assume single channel; duplicate
            x = samples.astype(np.float32)
            self.buf_fp1.extend(x.tolist())
            self.buf_fp2.extend(x.tolist())
            return

        # (batch, channels)
        if self.idx_fp1 >= samples.shape[1] or self.idx_fp2 >= samples.shape[1]:
            return
        fp1 = samples[:, self.idx_fp1].astype(np.float32)
        fp2 = samples[:, self.idx_fp2].astype(np.float32)
        self.buf_fp1.extend(fp1.tolist())
        self.buf_fp2.extend(fp2.tolist())

    def available(self) -> int:
        return max(0, min(len(self.buf_fp1), len(self.buf_fp2)) - self.next_start)

    def next_window(self) -> Optional[Tuple[np.ndarray, np.ndarray]]:
        if self.available() < self.window_len:
            return None
        s = self.next_start
        e = s + self.window_len
        fp1 = np.asarray(self.buf_fp1[s:e], dtype=np.float32)
        fp2 = np.asarray(self.buf_fp2[s:e], dtype=np.float32)
        self.next_start += self.hop_len

        # Periodically prune buffers to cap memory
        if self.next_start > (self.window_len * 4):
            drop = self.next_start - self.window_len  # keep one full window before next_start
            if drop > 0:
                self.buf_fp1 = self.buf_fp1[drop:]
                self.buf_fp2 = self.buf_fp2[drop:]
                self.next_start -= drop
        return fp1, fp2

    def infer_window(self, fp1: np.ndarray, fp2: np.ndarray) -> Tuple[int, np.ndarray, np.ndarray]:
        assert self.transform is not None, "Transform not initialized"
        X4 = self.transform(fp1, fp2).astype(np.float32)  # (4,T)
        xt = torch.from_numpy(X4[None, :, :]).to(self.device)  # (1,4,T)
        with torch.no_grad():
            logits = self.model(xt)
            probs = torch.softmax(logits, dim=1).detach().cpu().numpy()[0]
            pred = int(np.argmax(probs))
        return pred, probs, X4

    def update_ema(self, probs: np.ndarray) -> Tuple[np.ndarray, int]:
        if self.ema_alpha <= 0.0:
            return probs, int(np.argmax(probs))
        if self.prob_ema is None or self.prob_ema.shape != probs.shape:
            self.prob_ema = probs.astype(np.float32)
        else:
            a = float(self.ema_alpha)
            self.prob_ema = (1.0 - a) * self.prob_ema + a * probs.astype(np.float32)
        return self.prob_ema, int(np.argmax(self.prob_ema))

    def smooth_pred(self, pred: int) -> int:
        if self.smooth_k <= 1:
            return pred
        self.pred_hist.append(pred)
        while len(self.pred_hist) > self.smooth_k:
            self.pred_hist.popleft()
        vals, cnts = np.unique(np.fromiter(self.pred_hist, dtype=np.int64), return_counts=True)
        return int(vals[int(np.argmax(cnts))])


def load_weights(weights_path: Path) -> Tuple[Dict[str, Any], Dict[str, Any]]:
    obj = torch.load(str(weights_path), map_location="cpu")
    meta: Dict[str, Any] = {}
    if isinstance(obj, dict) and "state_dict" in obj:
        state_dict = obj["state_dict"]
        meta = (obj.get("meta") or {})
    else:
        # raw state dict
        state_dict = obj
    return state_dict, meta


async def stream_infer(args):
    # Load weights/meta first to resolve model and hyperparameters
    state_dict, meta = load_weights(Path(args.weights))
    h = meta.get("hparams") if isinstance(meta.get("hparams"), dict) else {}

    # Resolve model key (auto from checkpoint unless explicitly provided)
    if args.model and args.model.lower() != "auto":
        model_key = args.model
    else:
        model_key = str(h.get("model_key") or meta.get("model") or "gpt1")

    # Resolve model spec
    spec = get_spec(model_key)
    ModelClass = spec["model_class"]
    transform_factory = spec["make_stream_transform"]
    select_fp_indices = spec["select_fp_indices"]

    # Determine classes
    if "classifier.weight" in state_dict:
        n_classes = int(state_dict["classifier.weight"].shape[0])
    else:
        classes = (h.get("classes") if isinstance(h, dict) else meta.get("classes", []))
        n_classes = int(len(classes) or 3)

    # Device/model
    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model = ModelClass(n_classes=n_classes).to(device)
    try:
        model.load_state_dict(state_dict, strict=True)
    except Exception as e:
        print(f"[warn] strict load failed ({e}); retrying with strict=False")
        model.load_state_dict(state_dict, strict=False)
    model.eval()

    # Use checkpoint meta values by default unless --no_meta
    use_meta = not bool(args.no_meta)
    if use_meta:
        window_s = float(h.get("window_s", meta.get("window_s", args.window_s)))
        hop_s = float(h.get("hop_s", meta.get("hop_s", args.hop_s)))
    else:
        window_s = float(args.window_s)
        hop_s = float(args.hop_s)

    # Preferred FP indices from meta or CLI
    prefer_idx = None
    if use_meta:
        fp = (h.get("fp_indices") if isinstance(h, dict) else None)
        if not isinstance(fp, (list, tuple)):
            fp = meta.get("fp_indices")
        if isinstance(fp, (list, tuple)) and len(fp) == 2:
            prefer_idx = (int(fp[0]), int(fp[1]))
    if getattr(args, "fp1", None) is not None and args.fp1 >= 0 and getattr(args, "fp2", None) is not None and args.fp2 >= 0:
        prefer_idx = (int(args.fp1), int(args.fp2))

    state = StreamState(
        device=device,
        model=model,
        transform_factory=transform_factory,
        select_fp_indices=select_fp_indices,
        window_s=float(window_s),
        hop_s=float(hop_s),
        smooth_k=int(args.smooth) if args.smooth and args.smooth > 0 else 0,
        ema_alpha=float(args.ema_alpha) if args.ema_alpha and args.ema_alpha > 0 else 0.0,
        prefer_idx=prefer_idx,
    )

    last_print_ts = 0.0

    async def on_packet(header: Dict[str, Any], samples: np.ndarray, meta_pkt: Optional[Dict[str, Any]]):
        nonlocal last_print_ts

        # Initialize/update runtime params
        state.update_fs_and_channels(header, meta_pkt, samples)

        # Append streaming samples
        state.append_samples(samples)

        # Consume as many hop windows as available
        while True:
            pair = state.next_window()
            if pair is None:
                break
            fp1, fp2 = pair
            raw_pred, probs, X4 = state.infer_window(fp1, fp2)
            if args.debug:
                fp1_std = float(np.std(fp1))
                fp2_std = float(np.std(fp2))
                diff_std = float(np.std(fp1 - fp2))
                x4_rms = [float(np.sqrt(np.mean(X4[i] ** 2))) for i in range(X4.shape[0])]
                print(f"[dbg] next_start={state.next_start} len={len(state.buf_fp1)} "
                      f"fp1_std={fp1_std:.6f} fp2_std={fp2_std:.6f} diff_std={diff_std:.6f} "
                      f"X4_rms={[round(v,6) for v in x4_rms]} "
                      f"FpIdx=({state.idx_fp1},{state.idx_fp2})")
                if diff_std < 1e-6:
                    print("[warn] Very low diff_std; check FP indices or signal source (possible flat/noise-only input).")
            ema_probs, ema_pred = state.update_ema(probs)
            base_pred = ema_pred if state.ema_alpha > 0 else raw_pred
            smoothed = state.smooth_pred(base_pred)

            now = time.time()
            # Throttle prints to ~10Hz (but print every hop if small rate)
            if now - last_print_ts > max(0.01, state.hop_len / max(1.0, state.fs) * 0.5):
                msg = (
                    f"[pred] raw={raw_pred} ema={ema_pred if state.ema_alpha>0 else '-'} smoothed={smoothed} "
                    f"probs={[round(float(x), 3) for x in probs.tolist()]} "
                    f"| fs={state.fs:.1f}Hz win={state.window_len} hop={state.hop_len} "
                    f"| Fp1={state.chan_names[state.idx_fp1] if state.chan_names else state.idx_fp1} "
                    f"Fp2={state.chan_names[state.idx_fp2] if state.chan_names else state.idx_fp2}"
                )
                print(msg)
                last_print_ts = now

    print(
        f"🚀 Streaming inference: host={args.host} topic={args.topic} epoch={args.epoch} "
        f"| weights={args.weights} | device={device} | model={model_key} | win={window_s} hop={hop_s}"
    )
    await start_data_ws(args.host, [(args.topic, int(args.epoch))], on_packet)


def parse_args():
    ap = argparse.ArgumentParser(description="Generic streaming inference via model registry")
    ap.add_argument("--model", type=str, default="auto", help="Model key (e.g., gpt1, gpt2) or 'auto' to read from checkpoint")
    ap.add_argument("--weights", type=str, default="gpt1.pt", help="Path to model weights .pt")
    ap.add_argument("--host", type=str, default="raspberrypi.local")
    ap.add_argument("--topic", type=str, default="eeg_voltage")
    ap.add_argument("--epoch", type=int, default=1)
    ap.add_argument("--window_s", type=float, default=1.0, help="Window length (seconds)")
    ap.add_argument("--hop_s", type=float, default=0.25, help="Hop length (seconds)")
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")
    ap.add_argument("--smooth", type=int, default=0, help="Majority-vote window over last K preds (0=off)")
    ap.add_argument("--ema_alpha", type=float, default=0.0, help="EMA alpha for probability smoothing (0=off)")
    ap.add_argument("--fp1", type=int, default=-1, help="Force FP1 channel index (overrides meta)")
    ap.add_argument("--fp2", type=int, default=-1, help="Force FP2 channel index (overrides meta)")
    ap.add_argument("--debug", action="store_true", help="Enable verbose per-window diagnostics")
    ap.add_argument("--no_meta", action="store_true", help="Disable reading hyperparameters from checkpoint meta")
    return ap.parse_args()


def main():
    args = parse_args()
    try:
        asyncio.run(stream_infer(args))
    except KeyboardInterrupt:
        print("\nInterrupted. Exiting.")


if __name__ == "__main__":
    main()