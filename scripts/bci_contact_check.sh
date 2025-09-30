#!/usr/bin/env bash
# Quick electrode contact check: runs contact_check.py and prints per-channel metrics
# Usage:
#   ./run bci_contact_check
# Optional env:
#   HOST=raspberrypi.local TOPIC=eeg_voltage EPOCH=1 DURATION=12 NPZ=
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

HOST=${HOST:-raspberrypi.local}
TOPIC=${TOPIC:-eeg_voltage}
EPOCH=${EPOCH:-1}
DURATION=${DURATION:-12}
NPZ=${NPZ:-}

if [ -n "$NPZ" ]; then
  python3 bci/scripts/explore/contact_check.py --npz "$NPZ"
else
  python3 bci/scripts/explore/contact_check.py --host "$HOST" --topic "$TOPIC" --epoch "$EPOCH" --duration "$DURATION"
fi

