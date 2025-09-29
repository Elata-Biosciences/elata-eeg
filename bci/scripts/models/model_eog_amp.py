#!/usr/bin/env python3
"""
EOG amplitude-preserving variant.

- Channel selection: prefer frontal Fp1/Fp2 (fallback AF7/AF8, F7/F8)
- Preprocessing per window:
  - Bandpass 0.1–5 Hz (EOG band)
  - Per-window z-score on the bandpassed channels
  - Append per-channel RMS (tiled across time) to preserve absolute amplitude
- Output channels: 2C (e.g., 4 when C=2)

Use with trainer:
  python bci/scripts/models/train.py --model eog_amp --npz <path> [...]
"""
from __future__ import annotations

import numpy as np

from gpt1 import TinyBlinkNet, select_fp_indices, bandpass_filter, standardize_per_window  # type: ignore


def select_channel_indices(chan_names):
    lower = [str(n).lower() for n in (chan_names or [])]
    def find2(a, b):
        try:
            return lower.index(a), lower.index(b)
        except ValueError:
            return None
    for a,b in [("fp1","fp2"),("af7","af8"),("f7","f8")]:
        idx = find2(a,b)
        if idx:
            return [int(idx[0]), int(idx[1])]
    return [0, 1]


def _prep(arr_ct: np.ndarray, fs: float) -> np.ndarray:
    C, T = arr_ct.shape
    bp = bandpass_filter(arr_ct, 0.1, 5.0, float(fs), order=4).astype(np.float32)
    zc = standardize_per_window(bp).astype(np.float32)
    rms = np.sqrt((bp**2).mean(axis=-1, keepdims=True))
    rms = np.maximum(rms, 1e-6).astype(np.float32)
    rms_tiled = np.repeat(rms, T, axis=1)
    return np.concatenate([zc, rms_tiled], axis=0).astype(np.float32)  # (2C, T)


def offline_prepare_X4(XC: np.ndarray, fs: float) -> np.ndarray:
    assert XC.ndim == 3 and XC.shape[1] >= 1
    N, C, T = XC.shape
    out = np.empty((N, 2*C, T), dtype=np.float32)
    for i in range(N):
        out[i] = _prep(XC[i], fs)
    return out


def make_stream_transform(fs: float):
    fs = float(fs)
    def _transform(fp1: np.ndarray, fp2: np.ndarray) -> np.ndarray:
        x = np.stack([np.asarray(fp1, dtype=np.float32), np.asarray(fp2, dtype=np.float32)], axis=0)
        return _prep(x, fs)
    return _transform


def get_inference_spec():
    return {
        "model_class": TinyBlinkNet,
        "offline_prepare_X4": offline_prepare_X4,
        "make_stream_transform": make_stream_transform,
        "select_fp_indices": select_fp_indices,
        "select_channel_indices": select_channel_indices,
    }

