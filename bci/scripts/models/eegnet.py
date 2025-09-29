#!/usr/bin/env python3
"""
EEGNet (simplified 1D) baseline for MI tasks.
- Input: (B, C, T)
- Preprocessing: model-provided bandpass 8–30 Hz + per-window z-score
"""
from __future__ import annotations

import numpy as np
import torch
import torch.nn as nn

from gpt1 import select_fp_indices, bandpass_filter, standardize_per_window  # type: ignore


class EEGNet1D(nn.Module):
    def __init__(self, n_classes: int = 4, n_in_ch: int = 2, T: int = 250):
        super().__init__()
        # Temporal conv
        self.temporal = nn.Sequential(
            nn.Conv1d(n_in_ch, 16, kernel_size=31, padding=15, bias=False),
            nn.BatchNorm1d(16),
            nn.ELU(),
        )
        # Depthwise spatial conv
        self.spatial = nn.Sequential(
            nn.Conv1d(16, 32, kernel_size=1, groups=16, bias=False),
            nn.BatchNorm1d(32),
            nn.ELU(),
            nn.Dropout(0.25),
        )
        # Separable conv
        self.separable = nn.Sequential(
            nn.Conv1d(32, 32, kernel_size=15, padding=7, groups=32, bias=False),
            nn.Conv1d(32, 64, kernel_size=1, bias=False),
            nn.BatchNorm1d(64),
            nn.ELU(),
            nn.Dropout(0.25),
        )
        self.classifier = nn.Linear(64, n_classes)

    def forward(self, x):
        x = self.temporal(x)
        x = self.spatial(x)
        x = self.separable(x)
        x = x.mean(dim=-1)
        return self.classifier(x)


def offline_prepare_X4(XC: np.ndarray, fs: float) -> np.ndarray:
    """Bandpass 8–30 Hz and per-window z-score (MI typical)."""
    assert XC.ndim == 3 and XC.shape[1] >= 1
    N = XC.shape[0]
    out = XC.astype(np.float32, copy=True)
    for i in range(N):
        out[i] = bandpass_filter(out[i], 8.0, 30.0, float(fs), order=4)
        out[i] = standardize_per_window(out[i])
    return out.astype(np.float32)


def make_stream_transform(fs: float):
    fs = float(fs)
    def _transform(fp1: np.ndarray, fp2: np.ndarray) -> np.ndarray:
        x = np.stack([np.asarray(fp1, dtype=np.float32), np.asarray(fp2, dtype=np.float32)], axis=0)
        x = bandpass_filter(x, 8.0, 30.0, fs, order=4).astype(np.float32)
        x = standardize_per_window(x)
        return x
    return _transform


def get_inference_spec():
    return {
        "model_class": EEGNet1D,
        "offline_prepare_X4": offline_prepare_X4,
        "make_stream_transform": make_stream_transform,
        "select_fp_indices": select_fp_indices,
    }

