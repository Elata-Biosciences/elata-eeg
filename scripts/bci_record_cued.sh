#!/usr/bin/env bash
# Record a cued data acquisition session to NPZ using the existing Python recorder.
# Usage:
#   ./run bci_record_cued
# Configure via environment variables if needed:
#   HOST=ws://raspberrypi.local TOPIC=eeg_voltage EPOCH=1 \
#   LABELS="left,right,up,down" MINUTES=5 BLOCK=5 LABEL_SHIFT=0.6 \
#   CHS="" BEEP=1 OUT="" \
#   ./run bci_record_cued
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

HOST=${HOST:-ws://raspberrypi.local}
TOPIC=${TOPIC:-eeg_voltage}
EPOCH=${EPOCH:-1}
LABELS=${LABELS:-left,right,up,down}
MINUTES=${MINUTES:-5}
BLOCK=${BLOCK:-5}
LABEL_SHIFT=${LABEL_SHIFT:-0.6}
CHS=${CHS:-}
BEEP=${BEEP:-0}

STAMP=$(date +"%Y%m%d_%H%M%S")
OUT=${OUT:-bci/scripts/data/session_${STAMP}.npz}

echo "==> Recording cued session"
echo "    host=${HOST} topic=${TOPIC} epoch=${EPOCH}"
echo "    labels=${LABELS} minutes=${MINUTES} block=${BLOCK}s label_shift=${LABEL_SHIFT}s"
echo "    channels=${CHS:-ALL} beep=${BEEP}"
echo "    out=${OUT}"

ARGS=(--host "$HOST" --topic "$TOPIC" --epoch "$EPOCH" \
      --out "$OUT" --labels "$LABELS" --cued --minutes "$MINUTES" --block "$BLOCK" --label-shift "$LABEL_SHIFT")
if [ -n "$CHS" ]; then
  ARGS+=(--chs "$CHS")
fi
if [ "$BEEP" -eq 1 ]; then
  ARGS+=(--beep)
fi

python3 bci/scripts/gather_data/record_epochs.py "${ARGS[@]}"

echo "Saved: $OUT"
