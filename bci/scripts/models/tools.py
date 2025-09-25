#!/usr/bin/env python3
# windowed_loader.py — return (X, y, fs, ch_names) as NumPy arrays.
# - If NPZ is RAW (data, labels, fs, ch_names): do NO FILTERING, just windowing.
# - If NPZ is already WINDOWED (X, y, fs, ch_names): return as-is.

import numpy as np
from scipy.signal import butter, lfilter

def load_file(npz_path: str):
    """
    Load a RAW EEG NPZ file.

    Parameters
    ----------
    npz_path : str
        Path to NPZ file containing {data, labels, fs, ch_names}.

    Returns
    -------
    SimpleNamespace
        .data   : np.ndarray, shape (C, N), EEG channels x samples
        .labels : np.ndarray, shape (N,), per-sample int labels
        .fs     : float, sampling rate (Hz)
        .ch_names : list of str, channel names

    Raises
    ------
    ValueError : If shapes are inconsistent.
    KeyError   : If NPZ schema is unrecognized or already windowed.
    """
    from types import SimpleNamespace
    import numpy as np
    z = np.load(npz_path, allow_pickle=True)
    keys = set(z.keys())
    if {"data", "labels", "fs", "ch_names"} <= keys:
        data   = np.asarray(z["data"], dtype=np.float32)
        labels = np.asarray(z["labels"], dtype=np.int64)
        fs     = float(np.asarray(z["fs"]))
        ch     = list(np.asarray(z["ch_names"]).tolist())
        if data.ndim != 2 or labels.ndim != 1 or labels.shape[0] != data.shape[1]:
            raise ValueError("Bad shapes: expect data (C,N), labels (N,).")
        return SimpleNamespace(data=data, labels=labels, fs=fs, ch_names=ch)
    if {"X", "y", "fs", "ch_names"} <= keys:
        raise ValueError("File is already windowed; use directly with the model.")
    raise KeyError(f"Unrecognized NPZ schema: {sorted(keys)}")

def window_stage(data, labels, fs, *,
                 window_s=1.25, hop_s=0.25, label_mode="majority",
                 label_offset_ms=0, majority=0.70, keep=None, out_order="CT"):
    """
    Slice continuous EEG into fixed windows (no preprocessing).

    Parameters
    ----------
    data : np.ndarray, shape (C, N)
        EEG data (channels x samples).
    labels : np.ndarray, shape (N,) or None
        Per-sample int labels; if None, filled with -1.
    fs : float
        Sampling rate (Hz).
    window_s : float
        Window length in seconds (default 1.25).
    hop_s : float
        Hop length in seconds (default 0.25).
    label_mode : {"majority", "center"}
        How to assign a label to each window.
    label_offset_ms : int
        Shift labels by this many ms to account for cue lag.
    majority : float
        Fraction threshold for majority voting (if mode="majority").
    keep : list[int] or None
        Optional class IDs to retain.
    out_order : {"CT", "TC"}
        Axis order of each window: (C,T) or (T,C).

    Returns
    -------
    X : np.ndarray, shape (W, C, T) or (W, T, C)
        Windowed EEG data.
    y : np.ndarray, shape (W,)
        Labels per window.

    Raises
    ------
    ValueError  : On bad input shapes or parameters.
    RuntimeError: If no windows are produced.
    """
    import numpy as np
    if data.ndim != 2:
        raise ValueError("data must be (C,N)")
    if labels is None:
        labels = np.full(data.shape[1], -1, dtype=np.int64)
    labels = np.asarray(labels, dtype=np.int64)
    C, N = data.shape
    off = int(round((label_offset_ms / 1000.0) * fs))
    labels_shift = np.roll(labels, -off) if off != 0 else labels
    T = int(round(window_s * fs))
    H = int(round(hop_s * fs))
    if T <= 0 or H <= 0 or T > N:
        raise ValueError(f"Bad window/hop (T={T}, H={H}) for N={N}, fs={fs}")
    Xs, ys = [], []
    for beg in range(0, N - T + 1, H):
        sl = slice(beg, beg + T)
        win_lab = labels_shift[sl]
        if label_mode == "center":
            y = int(win_lab[T // 2]) if T > 0 else int(win_lab[0])
            if keep is not None and y not in keep:
                continue
        else:  # majority
            valid = win_lab[win_lab >= 0] if win_lab.min() < 0 else win_lab
            if valid.size == 0:
                continue
            cnt = np.bincount(valid)
            y = int(cnt.argmax())
            if (cnt[y] / valid.size) < majority:
                continue
            if keep is not None and y not in keep:
                continue
        Xs.append(data[:, sl])
        ys.append(y)
    if not Xs:
        raise RuntimeError("No windows produced (adjust window/hop/majority/keep/offset).")
    X = np.stack(Xs).astype(np.float32)
    if out_order.upper() == "TC":
        X = np.transpose(X, (0, 2, 1))
    return X, np.asarray(ys, dtype=np.int64)

def load_windowed2(npz_path: str, window_s: float = 1.25, hop_s: float = 0.25, majority: float = 0.70):
    """
    Returns:
      X: (W, C, T) float32
      y: (W,) int64
      fs: float
      ch_names: list[str]
    """
    z = np.load(npz_path, allow_pickle=True)
    keys = set(z.keys())

    # Already windowed
    if {"X", "y", "fs", "ch_names"} <= keys:
        X = np.asarray(z["X"], dtype=np.float32)
        y = np.asarray(z["y"], dtype=np.int64)
        fs = float(np.asarray(z["fs"]))
        ch_names = list(np.asarray(z["ch_names"]).tolist())
        return X, y, fs, ch_names

    # RAW -> window (no filtering)
    if {"data", "labels", "fs", "ch_names"} <= keys:
        data   = np.asarray(z["data"], dtype=np.float32)   # (C, N)
        labels = np.asarray(z["labels"], dtype=np.int64)   # (N,)
        fs     = float(np.asarray(z["fs"]))
        ch_names = list(np.asarray(z["ch_names"]).tolist())

        C, N = data.shape
        m = int(round(window_s * fs))
        h = int(round(hop_s * fs))
        if m <= 0 or h <= 0 or m > N:
            raise ValueError(f"Bad window/hop ({m},{h}) for N={N}, fs={fs}")

        X_list, y_list = [], []
        kmax = int(labels.max()) + 1
        for beg in range(0, N - m + 1, h):
            sl = slice(beg, beg + m)
            cnt = np.bincount(labels[sl], minlength=kmax)
            lab = int(cnt.argmax())
            if majority > 0 and cnt.sum() and (cnt[lab] / cnt.sum() < majority):
                continue  # drop near-transition windows
            X_list.append(data[:, sl])   # NO filtering
            y_list.append(lab)

        if not X_list:
            raise RuntimeError("No windows produced. Try smaller --majority or different window sizes.")

        X = np.stack(X_list).astype(np.float32)  # (W, C, T)
        y = np.array(y_list, dtype=np.int64)
        return X, y, fs, ch_names

    raise KeyError(f"Unrecognized NPZ schema: keys={sorted(keys)}")


def bandpass_filter(data, lowcut, highcut, fs, order=5):
    """Applies a bandpass filter to the data."""
    nyq = 0.5 * fs
    low = lowcut / nyq
    high = highcut / nyq
    b, a = butter(order, [low, high], btype='band')
    # Apply the filter to the last axis (time dimension)
    y = lfilter(b, a, data, axis=-1)
    return y

__all__ = ["load_file", "window_stage", "bandpass_filter"]
