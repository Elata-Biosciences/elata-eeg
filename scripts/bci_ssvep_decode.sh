#!/usr/bin/env bash
# Offline, training-free SSVEP decoder using CCA
# Examples:
#   NPZ=$(ls -t bci/scripts/data/session_*.npz | head -n1) ./run bci_ssvep_decode
#   NPZ=path/to/session.npz FREQS="15,12,10,7.5" LABELS="left,right,up,down" ./run bci_ssvep_decode
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

NPZ=${NPZ:-}
if [ -z "${NPZ}" ]; then
  # Pick the most recent session if present
  if ls bci/scripts/data/session_*.npz >/dev/null 2>&1; then
    NPZ=$(ls -t bci/scripts/data/session_*.npz | head -n1)
  else
    echo "Set NPZ=/path/to/session.npz" >&2
    exit 2
  fi
fi

FREQS=${FREQS:-15,12,10,7.5}
LABELS=${LABELS:-left,right,up,down}
CHS=${CHS:-2,3,4}
WINDOW_S=${WINDOW_S:-1.0}
HOP_S=${HOP_S:-0.25}
HARMONICS=${HARMONICS:-3}
SAVE_CSV=${SAVE_CSV:-}

ARGS=(--npz "$NPZ" --freqs "$FREQS" --labels "$LABELS" --chs "$CHS" \
      --window_s "$WINDOW_S" --hop_s "$HOP_S" --harmonics "$HARMONICS")
if [ -n "$SAVE_CSV" ]; then
  ARGS+=(--save_csv "$SAVE_CSV")
fi

echo "> Decoding CCA: NPZ=$NPZ FREQS=$FREQS CHS=$CHS WINDOW=$WINDOW_S HOP=$HOP_S H=$HARMONICS"
python3 bci/scripts/ssvep/decode_cca.py "${ARGS[@]}"

