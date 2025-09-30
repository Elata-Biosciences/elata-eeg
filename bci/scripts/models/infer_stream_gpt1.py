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
    from gpt1 import TinyBlinkNet, bandpass_filter, standardize_per_window, parse_channel_list, make_stream_transform  # type: ignore
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


def unwrap_state_dict(ckpt: Any) -> Dict[str, Any]:
    """
    Accepts torch.load() output and returns a clean state_dict suitable for model.load_state_dict().
    Handles wrappers like {'state_dict': ..., 'meta': ...} and common prefixes: 'module.', 'model.'.
    Also tries common alternative keys: 'model_state_dict', 'weights', 'params', 'model'.
    """
    from collections import OrderedDict

    sd = ckpt
    if isinstance(ckpt, dict):
        # Preferred known keys
        for key in ("state_dict", "model_state_dict", "weights", "params", "model"):
            if key in ckpt and isinstance(ckpt[key], (dict, OrderedDict)):
                sd = ckpt[key]
                break
        else:
            # Heuristic: if exactly one nested dict looks like a state_dict, use it
            nested = [v for v in ckpt.values() if isinstance(v, (dict, OrderedDict))]
            if len(nested) == 1:
                sd = nested[0]

    # Strip known prefixes
    cleaned = OrderedDict()
    for k, v in (sd.items() if isinstance(sd, (dict, OrderedDict)) else []):
        name = str(k)
        if name.startswith("module."):
            name = name[len("module."):]
        if name.startswith("model."):
            name = name[len("model."):]
        cleaned[name] = v
    return cleaned if cleaned else sd

@dataclass
class StreamState:
    device: torch.device
    model: TinyBlinkNet
    window_s: float = 1.0
    hop_s: float = 0.25
    smooth_k: int = 0  # rolling majority window over last K preds (0 disables)
    # Optional user override for channel selection (parsed when num_ch known)
    override_channels_text: Optional[str] = None
    override_indices: Optional[Tuple[int, int]] = None

    # Runtime/updatable fields
    fs: float = 250.0
    num_channels: int = 0
    chan_names: List[str] = field(default_factory=list)
    idx_fp1: int = 0
    idx_fp2: int = 1
    window_len: int = 250
    hop_len: int = 62
    # Preprocessing transform matching training (initialized when fs known)
    transform: Any = None
    # Buffers store raw stream values
    buf_fp1: List[float] = field(default_factory=list)
    buf_fp2: List[float] = field(default_factory=list)
    next_start: int = 0  # next window start index (in samples) for inference
    pred_hist: deque = field(default_factory=deque)

    def update_fs_and_channels(self, header: Dict[str, Any], meta: Optional[Dict[str, Any]], samples: np.ndarray):
        num_ch = int(header.get("num_channels") or (samples.shape[1] if samples.ndim == 2 else 1))
        fs_new = extract_fs(header, meta, self.fs)
        names = extract_names(header, meta, num_ch)

        changed = (abs(fs_new - self.fs) > 1e-6) or (num_ch != self.num_channels) or (names != self.chan_names)
        if changed:
            self.fs = float(fs_new)
            self.num_channels = int(num_ch)
            self.chan_names = names

            # Parse --channels override once when num_ch is known
            if self.override_indices is None and self.override_channels_text:
                try:
                    self.override_indices = parse_channel_list(self.override_channels_text, self.num_channels)
                    print(f"[stream] Using --channels override -> indices {self.override_indices} of {self.num_channels} total")
                except Exception as e:
                    print(f"[stream] ⚠️ Invalid --channels '{self.override_channels_text}': {e}. Falling back to Fp1/Fp2 selection.")
                    self.override_indices = None

            if self.override_indices is not None:
                self.idx_fp1, self.idx_fp2 = self.override_indices
            else:
                self.idx_fp1, self.idx_fp2 = find_fp_indices(names)

            self.window_len = max(1, int(round(self.window_s * self.fs)))
            self.hop_len = max(1, int(round(self.hop_s * self.fs)))
            # Initialize two-pass preprocessing transform to match training
            self.transform = make_stream_transform(self.fs)
            self.next_start = max(0, len(self.buf_fp1) - (len(self.buf_fp1) % self.hop_len))  # align to hop
            print(
                f"[stream] fs={self.fs:.2f}Hz | chans={self.num_channels} {self.chan_names} "
                f"| Fp1={self.idx_fp1}, Fp2={self.idx_fp2} | win={self.window_len} hop={self.hop_len}"
            )

    def append_samples(self, samples: np.ndarray):
        if samples.ndim == 1:
            # (batch,) -> assume single channel; duplicate or treat as both?
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

    def infer_window(self, fp1: np.ndarray, fp2: np.ndarray) -> Tuple[int, np.ndarray]:
        # Preprocess to 4-ch window using the same two-pass pipeline as training
        if self.transform is not None:
            X4 = self.transform(fp1, fp2).astype(np.float32)  # already bandpassed (2-ch then 4-ch) and standardized
        else:
            # Fallback: previous per-window-only pipeline
            diff = fp1 - fp2
            sumv = 0.5 * (fp1 + fp2)
            X4 = np.stack([fp1, fp2, diff, sumv], axis=0).astype(np.float32)
            X4 = bandpass_filter(X4, 0.1, 15.0, self.fs, order=4).astype(np.float32)
            X4 = standardize_per_window(X4).astype(np.float32)

        xt = torch.from_numpy(X4[None, :, :]).to(self.device)  # (1,4,T)
        with torch.no_grad():
            logits = self.model(xt)
            probs = torch.softmax(logits, dim=1).detach().cpu().numpy()[0]
            pred = int(np.argmax(probs))
        return pred, probs

    def smooth_pred(self, raw_pred: int) -> int:
        if self.smooth_k <= 1:
            return raw_pred
        self.pred_hist.append(raw_pred)
        while len(self.pred_hist) > self.smooth_k:
            self.pred_hist.popleft()
        # Majority vote
        vals, cnts = np.unique(np.fromiter(self.pred_hist, dtype=np.int64), return_counts=True)
        return int(vals[int(np.argmax(cnts))])


async def stream_infer(args):
    # Load weights (robust to wrapped checkpoints and prefixed keys)
    ckpt = torch.load(args.weights, map_location="cpu")
    state_dict = unwrap_state_dict(ckpt)

    # Infer number of classes if present; else fallback
    n_classes = int(state_dict["classifier.weight"].shape[0]) if isinstance(state_dict, dict) and "classifier.weight" in state_dict else 3

    device = torch.device(args.device if args.device else ("cuda" if torch.cuda.is_available() else "cpu"))
    model = TinyBlinkNet(n_classes=n_classes).to(device)
    # Load non-strict to handle classifier mismatches gracefully
    res = model.load_state_dict(state_dict, strict=False) if isinstance(state_dict, dict) else None
    if res is not None:
        missing = list(getattr(res, "missing_keys", []))
        unexpected = list(getattr(res, "unexpected_keys", []))
        if missing or unexpected:
            print(f"[weights] Loaded with missing={missing} unexpected={unexpected}")
    model.eval()

    state = StreamState(
        device=device,
        model=model,
        window_s=float(args.window_s),
        hop_s=float(args.hop_s),
        smooth_k=int(args.smooth) if args.smooth and args.smooth > 0 else 0,
        override_channels_text=args.channels,
    )

    last_print_ts = 0.0

    async def on_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
        nonlocal last_print_ts

        # Initialize/update runtime params
        state.update_fs_and_channels(header, meta, samples)

        # Append streaming samples
        state.append_samples(samples)

        # Consume as many hop windows as available
        made = 0
        while True:
            pair = state.next_window()
            if pair is None:
                break
            fp1, fp2 = pair
            pred, probs = state.infer_window(fp1, fp2)
            smoothed = state.smooth_pred(pred)

            now = time.time()
            # Throttle prints to ~10Hz (but print every hop if small rate)
            if now - last_print_ts > max(0.01, state.hop_len / max(1.0, state.fs) * 0.5):
                msg = (
                    f"[pred] raw={pred} smoothed={smoothed} "
                    f"probs={[round(float(x), 3) for x in probs.tolist()]} "
                    f"| fs={state.fs:.1f}Hz win={state.window_len} hop={state.hop_len} "
                    f"| Fp1={state.chan_names[state.idx_fp1] if state.chan_names else state.idx_fp1} "
                    f"Fp2={state.chan_names[state.idx_fp2] if state.chan_names else state.idx_fp2}"
                )
                print(msg)
                last_print_ts = now
            made += 1

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
    ap.add_argument("--channels", type=str, default=None, help="Two channel indices to use (e.g., '1 2' or '0,1'). Accepts 1-based unless any 0 present.")
    ap.add_argument("--device", type=str, default=None, help="cuda|cpu (auto if not set)")
    ap.add_argument("--smooth", type=int, default=0, help="Majority-vote window over last K preds (0=off)")
    args = ap.parse_args()

    try:
        asyncio.run(stream_infer(args))
    except KeyboardInterrupt:
        print("\nInterrupted. Exiting.")


if __name__ == "__main__":
    main()