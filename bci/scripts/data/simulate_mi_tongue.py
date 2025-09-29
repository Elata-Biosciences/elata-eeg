#!/usr/bin/env python3
"""
Simulate imagined tongue movement data (frontal/temporal band device) and save to NPZ.

- Classes (default): left,right (2-class). You can include 'rest' for 3-class.
- Channels: default 2 (F7/F8-like). You may specify more; only first two carry discriminative signals.
- Sampling rate: default 250 Hz. Duration: --seconds total.
- Output schema (RAW, not windowed):
    data: (C, N) float32
    labels: (N,) int64 [0..K-1]
    fs: float
    ch_names: list[str]
    class_names: list[str]

Example:
  PYTHONPATH=../.. python bci/scripts/data/simulate_mi_tongue.py \
    --out bci/scripts/data/mi_tongue_sim.npz --seconds 60 --labels "left,right" --fs 250 --channels 2
"""
from __future__ import annotations

import argparse
from pathlib import Path
from typing import List
import numpy as np


def pink_noise(n: int, scale: float = 1.0, rng: np.random.Generator | None = None) -> np.ndarray:
    rng = rng or np.random.default_rng()
    # Simple 1/f-ish noise via filtering white noise
    x = rng.standard_normal(n).astype(np.float32)
    # one-pole low-pass to approximate pinkish spectrum
    y = np.empty_like(x)
    a = 0.98
    acc = 0.0
    for i in range(n):
        acc = a * acc + (1 - a) * x[i]
        y[i] = acc
    y *= float(scale)
    return y


def synth_class_signal(label: str, t: np.ndarray, fs: float, rng: np.random.Generator) -> np.ndarray:
    """
    Return base signals for 2 discriminative channels (F7/F8-like):
      - left: 12 Hz on ch0 dominant
      - right: 20 Hz on ch1 dominant
      - rest: low-amplitude noise
    Returns array of shape (2, T)
    """
    T = t.shape[0]
    if label == "left":
        ch0 = 1.5 * np.sin(2 * np.pi * 12.0 * t) + 0.2 * pink_noise(T, rng=rng)
        ch1 = 0.4 * np.sin(2 * np.pi * 12.0 * t + np.pi/4) + 0.2 * pink_noise(T, rng=rng)
    elif label == "right":
        ch0 = 0.4 * np.sin(2 * np.pi * 20.0 * t + np.pi/5) + 0.2 * pink_noise(T, rng=rng)
        ch1 = 1.5 * np.sin(2 * np.pi * 20.0 * t) + 0.2 * pink_noise(T, rng=rng)
    else:  # rest
        ch0 = 0.2 * pink_noise(T, rng=rng)
        ch1 = 0.2 * pink_noise(T, rng=rng)
    return np.stack([ch0.astype(np.float32), ch1.astype(np.float32)], axis=0)


def build_sequence(labels: List[str], fs: float, seconds: float, block_s: float, channels: int, seed: int = 1337):
    rng = np.random.default_rng(seed)
    N = int(round(seconds * fs))
    t = np.arange(N, dtype=np.float32) / float(fs)
    # Build label timeline as blocks cycling through provided labels
    block_len = max(1, int(round(block_s * fs)))
    lab_seq: List[int] = []
    cur = 0
    while len(lab_seq) < N:
        L = min(block_len + rng.integers(low=-int(0.2*block_len), high=int(0.2*block_len)+1), N - len(lab_seq))
        lab_seq.extend([cur] * int(L))
        cur = (cur + 1) % len(labels)
    labels_arr = np.array(lab_seq[:N], dtype=np.int64)

    # Generate base signals for first two channels according to labels
    x2 = np.zeros((2, N), dtype=np.float32)
    i = 0
    while i < N:
        lab_idx = int(labels_arr[i])
        lab = labels[lab_idx]
        j = i
        while j < N and labels_arr[j] == lab_idx:
            j += 1
        seg = slice(i, j)
        t_seg = t[seg]
        x2[:, seg] = synth_class_signal(lab, t_seg, fs, rng)
        i = j

    # Expand to C channels: copy 2 discriminative + add distractor noise channels if needed
    C = int(channels)
    data = np.zeros((C, N), dtype=np.float32)
    data[0:2, :] = x2
    if C > 2:
        for c in range(2, C):
            # low-amplitude pink noise distractors
            data[c, :] = 0.15 * pink_noise(N, rng=rng)

    # Channel names
    ch_names = ["F7", "F8"] + [f"ch{c+1}" for c in range(2, C)]
    return data, labels_arr, ch_names


def main():
    ap = argparse.ArgumentParser(description="Simulate imagined tongue movement dataset → NPZ")
    ap.add_argument("--out", type=str, default="bci/scripts/data/mi_tongue_sim.npz")
    ap.add_argument("--labels", type=str, default="left,right", help="Comma list of labels (order defines class indices)")
    ap.add_argument("--seconds", type=float, default=60.0)
    ap.add_argument("--fs", type=float, default=250.0)
    ap.add_argument("--channels", type=int, default=2)
    ap.add_argument("--block_s", type=float, default=3.0, help="Approx block duration per label")
    ap.add_argument("--seed", type=int, default=1337)
    args = ap.parse_args()

    labels = [s.strip().lower() for s in args.labels.split(",") if s.strip()]
    if len(labels) < 2:
        raise SystemExit("Provide at least two labels, e.g., --labels 'left,right' or 'left,right,rest'")

    data, labels_arr, ch_names = build_sequence(labels, float(args.fs), float(args.seconds), float(args.block_s), int(args.channels), seed=int(args.seed))

    # Save NPZ
    outp = Path(args.out).expanduser().resolve()
    outp.parent.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(
        outp,
        data=data.astype(np.float32),
        labels=labels_arr.astype(np.int64),
        fs=np.array(float(args.fs), dtype=np.float64),
        ch_names=np.array(ch_names, dtype=object),
        class_names=np.array(labels, dtype=object),
    )
    dur = data.shape[1] / float(args.fs)
    print(f"Saved {outp}  CxN={data.shape}  fs={args.fs}Hz  dur={dur:.1f}s  classes={labels}")


if __name__ == "__main__":
    main()

