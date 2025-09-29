# BCI Model Training and Development

This directory contains Python scripts and resources for training and running Brain-Computer Interface (BCI) models with the Elata EEG device.

## Getting Started

First, ensure you are in the `bci` directory. Then, install the required Python packages:

```bash
pip install -r requirements.txt
```

## BCI Pipeline Workflow

The end-to-end pipeline involves three main stages:

1.  **Data Acquisition**: Collect labeled EEG data using scripts that connect to the EEG daemon.
2.  **Model Training**: Train a model on the collected data.
3.  **Inference**: Use the trained model to make real-time predictions from a live data stream.

All commands should be run from the `bci/scripts/` directory.

---

### 1. Data Acquisition

Use `gather_data/record_epochs.py` to record labeled EEG from the daemon’s WebSocket into an `.npz` file. You can run in cued mode (auto-cycling labels) or manual mode (type labels in the terminal).

- Cued mode (recommended to start):
```bash
# From the bci/scripts/ directory
PYTHONPATH=../.. python gather_data/record_epochs.py \
  --host ws://raspberrypi.local --topic eeg_voltage --epoch 1 \
  --out data/rez1.npz \
  --labels "left,right,up,down" \
  --cued --minutes 5 --block 5 \
  --label-shift 0.6 \
  --chs 1,2
```
  - --labels: comma list; order defines class indices
  - --cued: cycles labels every --block seconds for --minutes total
  - --label-shift: seconds to shift labels forward to compensate reaction/latency
  - --chs: optional 1-based indices to record subset of channels; omit to record all

- Manual mode (type labels live; commands: the label names, `mark <note>`, `status`, `quit`):
```bash
PYTHONPATH=../.. python gather_data/record_epochs.py \
  --host ws://raspberrypi.local --topic eeg_voltage --epoch 1 \
  --out data/rez1.npz \
  --labels "left,right,up,down"
```

---

### 2. Model Training

Once you have collected and prepared your dataset, you can train a model using the `models/train.py` script.

**Example Training Command:**
The following command trains a `gpt1` model on the dataset `data/rez1.npz`.

```bash
# From the bci/scripts/ directory
PYTHONPATH=../.. python models/train.py --model gpt1 --npz ../data/rez1.npz --chs all --window_s 1.0 --hop_s 0.25 --epochs 50 --batch 64 --lr 3e-4 --seed 1337
```

**Key Parameters:**
- `--model`: The model architecture to use (e.g., `gpt1`).
- `--npz`: Path to the input `.npz` data file.
- `--chs`: EEG channels to use (`all` or a specific subset).
- `--window_s`: Duration of the sliding window in seconds.
- `--hop_s`: Step size of the sliding window in seconds.
- `--epochs`: Number of training epochs.
- `--batch`: Batch size for training.
- `--lr`: Learning rate.
- `--seed`: Random seed for reproducibility.

---

### 3. Inference

You can perform streaming inference from the live daemon or offline inference on a saved `.npz`.

- Streaming inference (reads model spec from checkpoint by default):
```bash
# From the bci/scripts/ directory
PYTHONPATH=../.. python models/infer_stream.py \
  --model auto \
  --weights /path/to/model.pt \
  --host ws://raspberrypi.local --topic eeg_voltage --epoch 1 \
  --smooth 3 --ema_alpha 0.2
```
  - Optional: force frontal pair if needed: `--fp1 0 --fp2 1`
  - Use `--no_meta` to ignore checkpoint window/hop and supply `--window_s/--hop_s`

- Offline inference on a dataset (windows through the file and prints preds):
```bash
PYTHONPATH=../.. python models/infer.py \
  --model auto \
  --weights /path/to/model.pt \
  --npz data/rez1.npz
```
  - Optional: `--chs all` or `--chs 1,2` to select channels; `--save_csv preds.csv` to export

Note: If you trained with `--norm dataset`, the checkpoint stores normalization stats; streaming currently uses the models standard transform. Matching dataset-level normalization in streaming will be added soon.



## Electrode placement guide (band devices)

When using a frontal band (not a full 10–20 cap), you can still target several useful tasks. Place dry electrodes firmly against skin, and keep hair away from the contacts.

- Eye blinks / eye direction (EOG)
  - Primary electrodes: Fp1 (above left eyebrow), Fp2 (above right eyebrow)
  - Optional lateral support: AF7/AF8 or F7/F8 near the temples (helps horizontal saccades)
  - Reference/ground: on the mastoid (behind ear) or a neutral forehead location
  - Recommended preprocessing/model: `--model eog` (band 0.1–5 Hz; amplitude preserved)

- Imagined tongue movement / jaw EMG style tasks with a band device
  - With a frontal band, you won’t have central (C3/C4) coverage; use lateral frontal/temporal positions
  - Primary electrodes: F7 (left temple) and F8 (right temple); alternatives: AF7/AF8; fallback: Fp1/Fp2
  - These positions capture facial EMG and some low‑frequency components linked to orofacial activity
  - Recommended preprocessing/model: `--model mi_tongue` (band 8–30 Hz)

- General tips
  - Clean skin with alcohol wipes; ensure steady pressure; reduce motion
  - Keep cables stable; reduce jaw and face movement unless part of the task
  - Label timing matters; try `--label_offset_ms 300..800` to align windows with actual responses

## Channel selection and task modes

You can let the model choose sensible defaults or pick channels explicitly.

- Automatic (no `--chs` provided)
  - `--model eog`: prefers Fp1/Fp2; falls back to AF7/AF8 or F7/F8
  - `--model mi_tongue`: prefers F7/F8; falls back to AF7/AF8; then Fp1/Fp2
  - `--model gpt1` (generic): falls back to Fp1/Fp2

- Manual: e.g., `--chs 1,2` for 1‑based indices or `--chs all`

## Example training commands

From `bci/scripts`:

- Eye blinks / eye direction (automatic frontal selection and EOG band):
  - `PYTHONPATH=../.. python models/train.py --model eog --npz data/rez1.npz --window_s 1.0 --hop_s 0.25 --epochs 50 --batch 64 --lr 3e-4 --seed 1337`

- Imagined tongue movement pipeline (with simulated data first):
  1) Generate synthetic dataset:
     - `PYTHONPATH=../.. python data/simulate_mi_tongue.py --out data/mi_tongue_sim.npz --seconds 60 --labels "left,right" --fs 250 --channels 2`
  2) Train mi_tongue on the simulated data:
     - `PYTHONPATH=../.. python models/train.py --model mi_tongue --npz data/mi_tongue_sim.npz --chs 1,2 --window_s 1.0 --hop_s 0.25 --epochs 20 --batch 64 --lr 3e-3 --seed 1337`
  3) Offline evaluate with the checkpoint:
     - `PYTHONPATH=../.. python models/infer.py --model auto --weights /tmp/mi_tongue_sim.pt --npz data/mi_tongue_sim.npz`

- Real data imagined tongue movement (lateral frontal/temporal, mu/beta band):
  - `PYTHONPATH=../.. python models/train.py --model mi_tongue --npz data/rez1.npz --window_s 1.0 --hop_s 0.25 --epochs 50 --batch 64 --lr 3e-4 --seed 1337`

- If you know the exact channels (e.g., channels 1 and 2 correspond to F7/F8 or AF7/AF8):
  - `PYTHONPATH=../.. python models/train.py --model mi_tongue --npz data/rez1.npz --chs 1,2 --epochs 50`

- Try label lag compensation (example +600 ms):
  - `PYTHONPATH=../.. python models/train.py --model eog --npz data/rez1.npz --label_offset_ms 600 --epochs 50`
