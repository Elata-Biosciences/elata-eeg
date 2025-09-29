#!/usr/bin/env python3
"""
Experimental amplitude-preserving variant of gpt1 preprocessing.

- Keeps per-window amplitude information via per-channel RMS features
- Also applies bandpass (0.1–15 Hz) and per-window z-score to the raw channels
- Final channels per window: [zscored_channels (C), rms_channels (C)] -> (2C, T)

Use with the generic trainer:
  python bci/scripts/models/train.py --model gpt1_amp --npz <path> [...]
"""
from __future__ import annotations

import numpy as np

# Reuse core pieces from gpt1 (absolute import so tests can import directly)
from gpt1 import TinyBlinkNet, select_fp_indices, bandpass_filter, standardize_per_window  # type: ignore


def _prep_window(arr_ct: np.ndarray, fs: float) -> np.ndarray:
    """arr_ct: (C,T) -> (2C, T) with zscored time series and RMS-as-constant features."""
    C, T = arr_ct.shape
    # Bandpass first
    bp = bandpass_filter(arr_ct, 0.1, 15.0, float(fs), order=4).astype(np.float32)
    # Per-window z-score on bandpassed channels
    zc = standardize_per_window(bp).astype(np.float32)  # (C, T)
    # Per-channel RMS on bandpassed data, then tile across time to (C, T)
    rms = np.sqrt((bp ** 2).mean(axis=-1, keepdims=True))  # (C,1)
    # Avoid zero RMS leading to all-zero constant channel ambiguity
    rms = np.maximum(rms, 1e-6).astype(np.float32)
    rms_tiled = np.repeat(rms, T, axis=1).astype(np.float32)  # (C, T)
    # Concatenate along channel axis: (2C, T)
    out = np.concatenate([zc, rms_tiled], axis=0).astype(np.float32)
    return out


def offline_prepare_X4(XC: np.ndarray, fs: float) -> np.ndarray:
    """
    XC: (N, C, T) -> (N, 2C, T)
    Applies bandpass + per-window z-score to the time-series channels and appends
    per-channel RMS (tiled across time) to preserve amplitude.
    """
    assert XC.ndim == 3 and XC.shape[1] >= 1, "Expected X shape (N, C, T)"
    N, C, T = int(XC.shape[0]), int(XC.shape[1]), int(XC.shape[2])
    out = np.empty((N, 2 * C, T), dtype=np.float32)
    for i in range(N):
        out[i] = _prep_window(XC[i], fs)
    return out


def make_stream_transform(fs: float):
    """
    Returns a callable(fp1, fp2) -> (4, T) for 2-channel streams (Fp1/Fp2),
    matching the offline_prepare_X4 when C=2.
    """
    fs = float(fs)

    def _transform(fp1: np.ndarray, fp2: np.ndarray) -> np.ndarray:
        x = np.stack([np.asarray(fp1, dtype=np.float32), np.asarray(fp2, dtype=np.float32)], axis=0)
        return _prep_window(x, fs)

    return _transform


def get_inference_spec():
    return {
        "model_class": TinyBlinkNet,
        "offline_prepare_X4": offline_prepare_X4,
        "make_stream_transform": make_stream_transform,
        "select_fp_indices": select_fp_indices,
    }

