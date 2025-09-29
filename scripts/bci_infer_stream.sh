#!/usr/bin/env bash
# Simplified streaming inference using infer_stream.py
# Usage:
#   WEIGHTS=/path/to/model.pt ./run bci_infer_stream
# Optional env:
#   MODEL=auto HOST=ws://raspberrypi.local TOPIC=eeg_voltage EPOCH=1 \
#   FP1=-1 FP2=-1 SMOOTH=3 EMA=0.2 WINDOW_S=1.0 HOP_S=0.25 NO_META=0 DEVICE=
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

WEIGHTS=${WEIGHTS:-}
if [ -z "${WEIGHTS}" ]; then
  echo "Set WEIGHTS=/path/to/model.pt" >&2
  exit 2
fi
MODEL=${MODEL:-auto}
HOST=${HOST:-ws://raspberrypi.local}
TOPIC=${TOPIC:-eeg_voltage}
EPOCH=${EPOCH:-1}
SMOOTH=${SMOOTH:-3}
EMA=${EMA:-0.2}
FP1=${FP1:--1}
FP2=${FP2:--1}
WINDOW_S=${WINDOW_S:-1.0}
HOP_S=${HOP_S:-0.25}
NO_META=${NO_META:-0}
DEVICE=${DEVICE:-}

ARGS=(--model "$MODEL" --weights "$WEIGHTS" --host "$HOST" --topic "$TOPIC" --epoch "$EPOCH" --smooth "$SMOOTH" --ema_alpha "$EMA")
if [ "$FP1" -ge 0 ] && [ "$FP2" -ge 0 ]; then
  ARGS+=(--fp1 "$FP1" --fp2 "$FP2")
fi
if [ "$NO_META" -eq 1 ]; then
  ARGS+=(--no_meta --window_s "$WINDOW_S" --hop_s "$HOP_S")
fi
if [ -n "$DEVICE" ]; then
  ARGS+=(--device "$DEVICE")
fi

echo "> Streaming with: ${ARGS[*]}"
python3 bci/scripts/models/infer_stream.py "${ARGS[@]}"
