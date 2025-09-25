# EEG Model Authoring Notes

This brief defines the exact contracts for writing a new training script (e.g. `models/new_model.py`).  
Goal: no guesswork when generating code (whether by another engineer or an AI).

---

## Contract (TL;DR for `new_model.py`)

- **Input data**: `.npz` with keys:
  - `data`: (C, N) float32 EEG
  - `labels`: (N,) int64, class ids
  - `fs`: scalar Hz
  - `ch_names`: list[str]
- **Classes**: 0=down, 1=up, 2=rest
- **Optional pre-windowed**: `{X:(W,C,T), y:(W,)}` format
- **Preproc**: optional bandpass 1–40 Hz; optional `keep=["Fp1"]`
- **Windowing**: `window_s=1.25`, `hop_s=0.25`, `label_mode="majority"`, `label_offset_ms=0`
- **Tensor shape**: (B,1,C,T) for PyTorch model
- **Split**: stratified 80/20; avoid temporal leakage across same gesture
- **Train**: CrossEntropy, Adam, constants at top (`EPOCHS`, `BATCH_SIZE`, `LR`)
- **Report**: per-epoch train/val acc, final confusion matrix
- **Save**: `models/checkpoints/new_model.pt` + `metadata.json`  
  (JSON must log: class order, fs, C, T)

---

## Runtime Assumptions
- Python ≥ 3.9  
- Dependencies: `pip install -r bci/requirements.txt torch`  
- Working dir: `bci/scripts`  
- Data: `bci/scripts/data/` (git-ignored)

---

## Helper Functions (`models/tools.py`)

These helpers are already implemented and must be used.

```python
from tools import load_file, window_stage, bandpass_filter
````

### `load_file(npz_path: str) -> SimpleNamespace`

* **Args**: `npz_path`: path to `.npz` file
* **Returns**: object with fields:

  * `.data`: np.ndarray, shape (C, N), float32
  * `.labels`: np.ndarray, shape (N,), int64
  * `.fs`: float, sampling rate Hz
  * `.ch_names`: list\[str]
* **Raises**: if schema is unexpected or already windowed

---

### \`window\_stage(

```
data: np.ndarray,
labels: np.ndarray,
fs: float,
*,
window_s: float = 1.25,
hop_s: float = 0.25,
label_mode: str = "majority",
label_offset_ms: int = 0,
majority: float = 0.70,
keep: list[str] | None = None,
out_order: str = "CT"
```

) -> tuple\[np.ndarray, np.ndarray]\`

* **Args**:

  * `window_s`: seconds per window
  * `hop_s`: stride in seconds
  * `label_mode`: how to assign labels (e.g. `"majority"`)
  * `label_offset_ms`: shift labels forward/backward
  * `majority`: fraction threshold for `"majority"` mode
  * `keep`: optional list of channel names to keep
  * `out_order`: `"CT"` = (windows, C, T), `"TC"` = (windows, T, C)
* **Returns**:

  * `X`: np.ndarray, shape (W, C, T) or (W, T, C)
  * `y`: np.ndarray, shape (W,), int64 window labels

---

### \`bandpass\_filter(

```
data: np.ndarray,
lowcut: float,
highcut: float,
fs: float,
order: int = 5
```

) -> np.ndarray\`

* **Args**:

  * `data`: np.ndarray, shape (C, N)
  * `lowcut`, `highcut`: cutoff frequencies in Hz
  * `fs`: sampling rate
  * `order`: Butterworth filter order
* **Returns**: filtered array, same shape as input
* **Notes**: Applies band-pass along the time axis

---

## Required Script Structure

1. Define constants:
   `NPZ_PATH`, `WINDOW_S`, `HOP_S`, `EPOCHS`, `BATCH_SIZE`, `LEARNING_RATE`
2. Load data with `load_file`, print stats
3. Optionally apply `bandpass_filter`
4. Window with `window_stage` (skip if already windowed)
5. Convert to tensors `(B,1,C,T)`
   Split train/val (stratified, no leakage across gestures)
6. Define model (EEGNet variant or specified alt)
7. Train with CE loss + Adam; log train/val metrics each epoch
8. Report final accuracy + confusion matrix
9. Save `state_dict` and `metadata.json`

---

## Notes for AI Assistants

* Always confirm dataset schema (`{data, labels, fs, ch_names}` vs `{X,y}`)
* Clarify: channel subset, band-pass range, label mapping, target architecture
* Scripts must set random seeds and print them
* Runtime should be acceptable on laptop GPU/CPU (few minutes per epoch)
