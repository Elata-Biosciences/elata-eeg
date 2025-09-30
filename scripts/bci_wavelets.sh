#!/usr/bin/env bash
# Live/Offline wavelet viewer wrapper for cwt_live.py
# Usage:
#   ./run bci_wavelets
#
# Environment variables (optional):
#   HOST=raspberrypi.local   # Pi hostname or IP (no ws://)
#   TOPIC=eeg_voltage        # WebSocket topic
#   EPOCH=1                  # subscription epoch
#   CHANNELS="0,1"            # 0-based channel indices to show (overrides NCH/ALL)
#   NCH=2                    # show first N channels if CHANNELS not set and ALL=0
#   ALL=0                    # set to 1 to show all channels
#   GRID_COLS=2              # subplot columns
#   WINDOW=12                # rolling window seconds
#   FMIN=1                   # min frequency (Hz)
#   FMAX=45                  # max frequency (Hz)
#   ZSCORE=1                 # 1 to enable robust per-frequency z-score, 0 to disable
#   HP=1                     # 1 to enable median-demean (reduce DC drift), 0 to disable
#   NPZ=                     # if set, replay this .npz offline via stdin (no live WS)
#   REPLAY_SPEED=1.0         # offline: 1.0=real-time, 2.0=2x, 0=as fast as possible
#   REPLAY_BATCH=64          # offline: samples pushed per update
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

HOST=${HOST:-raspberrypi.local}
TOPIC=${TOPIC:-eeg_voltage}
EPOCH=${EPOCH:-1}
CHANNELS=${CHANNELS:-}
NCH=${NCH:-2}
ALL=${ALL:-0}
GRID_COLS=${GRID_COLS:-2}
WINDOW=${WINDOW:-12}
FMIN=${FMIN:-1}
FMAX=${FMAX:-45}
ZSCORE=${ZSCORE:-1}
HP=${HP:-1}
NPZ=${NPZ:-}
REPLAY_SPEED=${REPLAY_SPEED:-1.0}
REPLAY_BATCH=${REPLAY_BATCH:-64}

ARGS=(
  --host "$HOST"
  --topic "$TOPIC"
  --epoch "$EPOCH"
  --grid-cols "$GRID_COLS"
  --window "$WINDOW"
  --fmin "$FMIN"
  --fmax "$FMAX"
  --replay-speed "$REPLAY_SPEED"
  --replay-batch "$REPLAY_BATCH"
)

if [ -n "$CHANNELS" ]; then
  ARGS+=(--channels "$CHANNELS")
else
  if [ "${ALL}" = "1" ]; then
    ARGS+=(--all)
  else
    ARGS+=(--nch "$NCH")
  fi
fi

if [ "$ZSCORE" = "1" ]; then
  ARGS+=(--zscore)
else
  ARGS+=(--no-zscore)
fi

if [ "$HP" = "1" ]; then
  ARGS+=(--hp)
else
  ARGS+=(--no-hp)
fi

echo "==> Wavelet viewer"
echo "    host=${HOST} topic=${TOPIC} epoch=${EPOCH}"
echo "    channels=${CHANNELS:-'(first '"$NCH"')'} all=${ALL} grid=${GRID_COLS} window=${WINDOW}s"
echo "    f=[${FMIN}, ${FMAX}]Hz zscore=${ZSCORE} hp=${HP}"

if [ -n "$NPZ" ]; then
  echo "    offline npz=${NPZ} replay_speed=${REPLAY_SPEED} batch=${REPLAY_BATCH}"
  if [ ! -f "$NPZ" ]; then
    echo "Error: NPZ file not found: $NPZ" >&2
    exit 2
  fi
  # Offline replay via stdin
  cat "$NPZ" | python3 bci/scripts/explore/cwt_live.py --stdin "${ARGS[@]}"
else
  # Live WS mode
  python3 bci/scripts/explore/cwt_live.py "${ARGS[@]}"
fi

