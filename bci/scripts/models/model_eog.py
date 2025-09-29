#!/usr/bin/env python3
"""
EOG-focused model spec:
- Bandpass 0.1–5 Hz
- No per-window z-score by default (use dataset-level normalization in trainer if desired)
- Frontal channel selection via select_fp_indices
"""
from __future__ import annotations

import numpy as np
import torch
import torch.nn as nn

from gpt1 import TinyBlinkNet, select_fp_indices, bandpass_filter  # type: ignore


def select_channel_indices(chan_names):
    """Prefer frontal pairs for EOG: Fp1/Fp2, AF7/AF8, F7/F8; fallback [0,1]."""
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


def offline_prepare_X4(XC: np.ndarray, fs: float) -> np.ndarray:
    """
    Bandpass only (0.1–5 Hz). Leaves amplitude intact.
    XC: (N, C, T) -> (N, C, T)
    """
    assert XC.ndim == 3 and XC.shape[1] >= 1
    N = XC.shape[0]
    out = XC.astype(np.float32, copy=True)
    for i in range(N):
        out[i] = bandpass_filter(out[i], 0.1, 5.0, float(fs), order=4)
    return out.astype(np.float32)


def make_stream_transform(fs: float):
    fs = float(fs)
    def _transform(fp1: np.ndarray, fp2: np.ndarray) -> np.ndarray:
        x = np.stack([np.asarray(fp1, dtype=np.float32), np.asarray(fp2, dtype=np.float32)], axis=0)
        y = bandpass_filter(x, 0.1, 5.0, fs, order=4).astype(np.float32)
        return y
    return _transform


def get_inference_spec():
    return {
        "model_class": TinyBlinkNet,
        "offline_prepare_X4": offline_prepare_X4,
        "make_stream_transform": make_stream_transform,
        "select_fp_indices": select_fp_indices,
    }

