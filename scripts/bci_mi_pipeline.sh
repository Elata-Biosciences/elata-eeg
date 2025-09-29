#!/usr/bin/env bash
# Simplified pipeline for imagined tongue movement using simulated data.
# Usage:
#   ./run bci_mi_pipeline
# Optional environment overrides:
#   SECONDS=60 LABELS="left,right" FS=250 CHANNELS=2 \
#   EPOCHS=10 BATCH=64 LR=3e-3 SEED=1337 CHS=1,2 \
#   WINDOW_S=1.0 HOP_S=0.25 \
#   SIM_OUT=bci/scripts/data/mi_tongue_sim.npz SAVE=/tmp/mi_tongue_sim.pt \
#   ./run bci_mi_pipeline
set -euo pipefail

# Ensure repo root on sys.path
export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

SIM_OUT=${SIM_OUT:-bci/scripts/data/mi_tongue_sim.npz}
SIM_SECONDS=${SIM_SECONDS:-60}
LABELS=${LABELS:-left,right}
FS=${FS:-250}
CHANNELS=${CHANNELS:-2}

EPOCHS=${EPOCHS:-10}
BATCH=${BATCH:-64}
LR=${LR:-3e-3}
SEED=${SEED:-1337}
CHS=${CHS:-1,2}
WINDOW_S=${WINDOW_S:-1.0}
HOP_S=${HOP_S:-0.25}
SAVE=${SAVE:-/tmp/mi_tongue_sim.pt}

echo "==> [1/3] Generate simulated dataset: ${SIM_OUT}"
python3 bci/scripts/data/simulate_mi_tongue.py \
  --out "${SIM_OUT}" --seconds "${SIM_SECONDS}" --labels "${LABELS}" --fs "${FS}" --channels "${CHANNELS}"

echo "==> [2/3] Train mi_tongue on simulated data (epochs=${EPOCHS})"
python3 bci/scripts/models/train.py \
  --model mi_tongue --npz "${SIM_OUT}" --chs ${CHS} \
  --window_s "${WINDOW_S}" --hop_s "${HOP_S}" \
  --epochs "${EPOCHS}" --batch "${BATCH}" --lr "${LR}" --seed "${SEED}" \
  --save "${SAVE}"

echo "==> [3/3] Offline inference summary"
python3 bci/scripts/models/infer.py --model auto --weights "${SAVE}" --npz "${SIM_OUT}"

echo "Pipeline complete. Weights: ${SAVE} | Data: ${SIM_OUT}"
