#!/usr/bin/env bash
# Record a manual data acquisition session (you type labels) to NPZ.
# Usage:
#   ./run bci_record_manual
# Configure via environment variables if needed:
#   HOST=raspberrypi.local TOPIC=eeg_voltage EPOCH=1 \
#   LABELS="left,right,up,down" LABEL_SHIFT=0.6 CHS="" BEEP=0 OUT="" \
#   ./run bci_record_manual
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

HOST=${HOST:-raspberrypi.local}
TOPIC=${TOPIC:-eeg_voltage}
EPOCH=${EPOCH:-1}
LABELS=${LABELS:-left,right,up,down}
LABEL_SHIFT=${LABEL_SHIFT:-0.6}
CHS=${CHS:-}
BEEP=${BEEP:-0}

STAMP=$(date +"%Y%m%d_%H%M%S")
OUT=${OUT:-bci/scripts/data/session_${STAMP}.npz}

echo "==> Recording manual session"
echo "    host=${HOST} topic=${TOPIC} epoch=${EPOCH}"
echo "    labels=${LABELS} label_shift=${LABEL_SHIFT}s channels=${CHS:-ALL} beep=${BEEP}"
echo "    out=${OUT}"

echo "Instructions: type one of the labels to switch (e.g., 'left'), or 'mark <text>' to annotate, 'status' to print, 'quit' to finish.\n"

ARGS=(--host "$HOST" --topic "$TOPIC" --epoch "$EPOCH" \
      --out "$OUT" --labels "$LABELS" --label-shift "$LABEL_SHIFT")
if [ -n "$CHS" ]; then
  ARGS+=(--chs "$CHS")
fi
if [ "$BEEP" -eq 1 ]; then
  ARGS+=(--beep)
fi

python3 bci/scripts/gather_data/record_epochs.py "${ARGS[@]}"

echo "Saved: $OUT"
