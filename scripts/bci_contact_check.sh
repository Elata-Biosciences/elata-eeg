#!/usr/bin/env bash
# Quick electrode contact check: runs contact_check.py and prints per-channel metrics
# Usage:
#   ./run bci_contact_check
# Optional env:
#   HOST=raspberrypi.local TOPIC=eeg_voltage EPOCH=1 DURATION=12 NPZ=
#   NAMES="T8,O2,Oz,O1,T7,Fpz" DROP_GROUND=1
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

HOST=${HOST:-raspberrypi.local}
TOPIC=${TOPIC:-eeg_voltage}
EPOCH=${EPOCH:-1}
DURATION=${DURATION:-12}
NPZ=${NPZ:-}
NAMES=${NAMES:-}
DROP_GROUND=${DROP_GROUND:-0}

ARGS=()
if [ -n "$NPZ" ]; then
  ARGS+=(--npz "$NPZ")
else
  ARGS+=(--host "$HOST" --topic "$TOPIC" --epoch "$EPOCH" --duration "$DURATION")
fi
if [ -n "$NAMES" ]; then
  ARGS+=(--names "$NAMES")
fi
if [ "$DROP_GROUND" = "1" ] || [ "$DROP_GROUND" = "true" ]; then
  ARGS+=(--drop-ground)
fi

python3 bci/scripts/explore/contact_check.py "${ARGS[@]}"

