#!/usr/bin/env python3
"""
infer_stream_gpt1.py — Stream EEG via WS and run TinyBlinkNet inference.

Usage examples:
  # Default host/topic/epoch; looks for blinknet.pt in CWD
  python bci/scripts/models/infer_stream_gpt1.py

  # Explicit weights and device
  python bci/scripts/models/infer_stream_gpt1.py --weights blinknet.pt --device cpu

  # Custom host/topic and hop/window
  python bci/scripts/models/infer_stream_gpt1.py --host raspberrypi.local --topic eeg_voltage --epoch 1 --window_s 1.0 --hop_s 0.25
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

# Make imports work when running this file directly
HERE = Path(__file__).resolve().parent                 # .../bci/scripts/models
SCRIPTS_DIR = HERE.parent                              # .../bci/scripts
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

# Import model/DSP helpers and websocket client
try:
    from gpt1 import TinyBlinkNet, make_stream_transform, select_fp_indices, bandpass_filter, standardize_per_window, expand_to_4ch  # type: ignore
except Exception as e:
    print(f"❌ Could not import TinyBlinkNet/DSP from gpt1.py: {e}")
    print("Ensure PYTHONPATH includes bci/scripts/models or run from repo root.")
    sys.exit(1)

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


def find_fp_indices(names: List[str]) -> Tuple[int, int]:
    # Prefer exact "Fp1"/"Fp2", else try case-insensitive contains
    lower = [n.lower() for n in names]
    try:
        i1 = lower.index("fp1")
    except ValueError:
        i1 = 0
    try:
        i2 = lower.index("fp2")
    except ValueError:
        i2 = 1 if len(names) > 1 else 0
    if i1 == i2:
        # Fall back to first two distinct channels if possible
        i1, i2 = 0, 1 if len(names) > 1 else 0
    return i1, i2


@dataclass
class StreamState:
    device: torch.device
    model: TinyBlinkNet
    window_s: float = 1.0
    hop_s: float = 0.25
    smooth_k: int = 0  # rolling majority vote window (0 disables)
    ema_alpha: float = 0.0  # EMA of probabilities (0 disables)
    compat_double: bool = False  # emulate legacy double-filtered training distribution

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

    # Preferred FP indices from checkpoint meta (optional)
    prefer_idx: Optional[Tuple[int, int]] = None

    # Preprocessing transform cached per fs
    transform: Optional[Any] = None

    # EMA state
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

            # Choose FP indices: prefer checkpoint-provided if valid, else heuristic
            if self.prefer_idx is not None:
                i1, i2 = int(self.prefer_idx[0]), int(self.prefer_idx[1])
                if 0 <= i1 < self.num_channels and 0 <= i2 < self.num_channels and i1 != i2:
                    self.idx_fp1, self.idx_fp2 = i1, i2
                else:
                    self.idx_fp1, self.idx_fp2 = select_fp_indices(self.chan_names)
            else:
                self.idx_fp1, self.idx_fp2 = select_fp_indices(self.chan_names)

            self.window_len = max(1, int(round(self.window_s * self.fs)))
            self.hop_len = max(1, int(round(self.hop_s * self.fs)))
            self.next_start = max(0, len(self.buf_fp1) - (len(self.buf_fp1) % self.hop_len))  # align to hop

            # Reset transform and EMA on fs change
            self.transform = make_stream_transform(self.fs)
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
        fp1 = samples[:, self.idx_fp1].astype(np.float32)
        fp2 = samples[:, self.idx_fp2].astype(np.float32)
        self.buf_fp1.extend(fp1.tolist())
        self.buf_fp2.extend(fp2.tolist())

    def available(self) -> int:
        return max(0, min(len(self.buf_fp1), len(self.buf_fp2)) - self.next_start)

    def next_window(self) -> Optional[Tuple[np.ndarray, np.ndarray]]:
        # Provide the next [0:window_len] slice starting at next_start, if available
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
        if not self.compat_double:
            X4 = self.transform(fp1, fp2).astype(np.float32)  # (4,T)
        else:
            # Legacy-compat: approximate training-time double filtering (continuous + window) by filtering twice per window.
            X4 = expand_to_4ch(fp1, fp2).astype(np.float32)  # (4,T)
            X4 = bandpass_filter(X4, 0.1, 15.0, self.fs, order=4).astype(np.float32)
            X4 = bandpass_filter(X4, 0.1, 15.0, self.fs, order=4).astype(np.float32)
            X4 = standardize_per_window(X4).astype(np.float32)
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


async def stream_infer(args):
    # Load checkpoint (supports {"state_dict","meta"} or raw state_dict)
    ckpt = torch.load(args.weights, map_location="cpu")
    if isinstance(ckpt, dict) and "state_dict" in ckpt:
        state_dict = ckpt["state_dict"]
        meta = ckpt.get("meta", {}) or {}
    else:
        state_dict = ckpt
        meta = {}

    # Determine n_classes
    if "classifier.weight" in state_dict:
        n_classes = int(state_dict["classifier.weight"].shape[0])
    elif isinstance(meta.get("classes"), (list, tuple)) and len(meta["classes"]) > 0:
        n_classes = int(len(meta["classes"]))
    else:
        n_classes = 3

    # Preferred FP indices from meta (optional)
    prefer_idx = None
    if isinstance(meta.get("fp_indices"), (list, tuple)) and len(meta["fp_indices"]) == 2:
        prefer_idx = (int(meta["fp_indices"][0]), int(meta["fp_indices"][1]))
    # CLI override if provided
    if getattr(args, "fp1", None) is not None and args.fp1 >= 0 and getattr(args, "fp2", None) is not None and args.fp2 >= 0:
        prefer_idx = (int(args.fp1), int(args.fp2))

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model = TinyBlinkNet(n_classes=n_classes).to(device)

    # Load with BN→GN compatibility: remap keys and drop BN running buffers
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

    # Warn if channel names don't include Fp1/Fp2 and no indices provided
    try:
        chn = meta.get("chan_names") if isinstance(meta.get("chan_names"), (list, tuple)) else None
        if prefer_idx is None and not (chn and any(str(x).lower() == "fp1" for x in chn) and any(str(x).lower() == "fp2" for x in chn)):
            print("[warn] No fp_indices in checkpoint and channel names do not include Fp1/Fp2. "
                  "Consider passing --fp1/--fp2 to select the correct channels.")
    except Exception:
        pass

    state = StreamState(
        device=device,
        model=model,
        window_s=float(args.window_s),
        hop_s=float(args.hop_s),
        smooth_k=int(args.smooth) if args.smooth and args.smooth > 0 else 0,
        ema_alpha=float(args.ema_alpha) if args.ema_alpha and args.ema_alpha > 0 else 0.0,
        prefer_idx=prefer_idx,
        compat_double=bool(args.compat_double_filter),
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
                      f"FpIdx=({state.idx_fp1},{state.idx_fp2}) compat_double={state.compat_double}")
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
        f"| weights={args.weights} | device={device}"
    )
    await start_data_ws(args.host, [(args.topic, int(args.epoch))], on_packet)


def main():
    ap = argparse.ArgumentParser(description="Streaming inference for TinyBlinkNet over WS")
    ap.add_argument("--host", type=str, default="raspberrypi.local", help="Data host")
    ap.add_argument("--topic", type=str, default="eeg_voltage", help="Topic name to subscribe to")
    ap.add_argument("--epoch", type=int, default=1, help="Epoch to subscribe with")
    ap.add_argument("--weights", type=str, default="blinknet.pt", help="Path to model weights")
    ap.add_argument("--window_s", type=float, default=1.0, help="Window length (seconds)")
    ap.add_argument("--hop_s", type=float, default=0.25, help="Hop length (seconds)")
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")
    ap.add_argument("--smooth", type=int, default=0, help="Majority-vote window over last K preds (0=off)")
    ap.add_argument("--ema_alpha", type=float, default=0.0, help="EMA alpha for probability smoothing (0=off)")
    ap.add_argument("--fp1", type=int, default=-1, help="Force FP1 channel index (overrides meta)")
    ap.add_argument("--fp2", type=int, default=-1, help="Force FP2 channel index (overrides meta)")
    ap.add_argument("--compat_double_filter", action="store_true", help="Approximate legacy double filtering per window")
    ap.add_argument("--debug", action="store_true", help="Enable verbose per-window diagnostics")
    args = ap.parse_args()

    try:
        asyncio.run(stream_infer(args))
    except KeyboardInterrupt:
        print("\nInterrupted. Exiting.")


if __name__ == "__main__":
    main()