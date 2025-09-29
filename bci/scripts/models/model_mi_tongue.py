#!/usr/bin/env python3
"""
Imagined tongue movement / facial EMG oriented variant for band devices.

- Channel selection: prefer lateral frontal/temporal pairs (F7/F8, then AF7/AF8, then Fp1/Fp2)
- Preprocessing: bandpass 8–30 Hz (mu/beta) to emphasize motor rhythms; per-window z-score
- Output channel count: equals selected C

Use with trainer:
  python bci/scripts/models/train.py --model mi_tongue --npz <path> [...]
"""
from __future__ import annotations

import numpy as np

from gpt1 import TinyBlinkNet, bandpass_filter, standardize_per_window, select_fp_indices  # type: ignore


def select_channel_indices(chan_names):
    lower = [str(n).lower() for n in (chan_names or [])]
    def find2(a, b):
        try:
            return lower.index(a), lower.index(b)
        except ValueError:
            return None
    for a,b in [("f7","f8"),("af7","af8"),("fp1","fp2")]:
        idx = find2(a,b)
        if idx:
            return [int(idx[0]), int(idx[1])]
    return [0, 1]


def offline_prepare_X4(XC: np.ndarray, fs: float) -> np.ndarray:
    assert XC.ndim == 3 and XC.shape[1] >= 1
    Xp = XC.astype(np.float32, copy=True)
    for i in range(Xp.shape[0]):
        Xp[i] = bandpass_filter(Xp[i], 8.0, 30.0, float(fs), order=4).astype(np.float32)
        Xp[i] = standardize_per_window(Xp[i]).astype(np.float32)
    return Xp


def make_stream_transform(fs: float):
    fs = float(fs)
    def _transform(fp1: np.ndarray, fp2: np.ndarray) -> np.ndarray:
        x = np.stack([np.asarray(fp1, dtype=np.float32), np.asarray(fp2, dtype=np.float32)], axis=0)
        x = bandpass_filter(x, 8.0, 30.0, fs, order=4).astype(np.float32)
        x = standardize_per_window(x).astype(np.float32)
        return x
    return _transform


def get_inference_spec():
    return {
        "model_class": TinyBlinkNet,
        "offline_prepare_X4": offline_prepare_X4,
        "make_stream_transform": make_stream_transform,
        "select_fp_indices": select_fp_indices,
        "select_channel_indices": select_channel_indices,
    }

