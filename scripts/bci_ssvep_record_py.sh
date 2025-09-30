#!/usr/bin/env bash
set -euo pipefail

# Single-command SSVEP stimulus (tkinter) + cued recording (no kiosk)
# Usage (defaults shown):
#   HOST=172.20.10.12 LABELS="left,right,up,down" MINUTES=5 BLOCK=5 \
#   LABEL_SHIFT=0.2 CHS="2,3,4" FREQS="15,12,10,7.5" INVERT=0 FULLSCREEN=1 \
#   ./run bci_ssvep_record_py

HOST=${HOST:-raspberrypi.local}
LABELS=${LABELS:-left,right,up,down}
MINUTES=${MINUTES:-5}
BLOCK=${BLOCK:-5}
LABEL_SHIFT=${LABEL_SHIFT:-0.2}
CHS=${CHS:-2,3,4}
FREQS=${FREQS:-15,12,10,7.5}
INVERT=${INVERT:-0}
FULLSCREEN=${FULLSCREEN:-1}
COUNTDOWN=${COUNTDOWN:-3}
MODE=${MODE:-sine}
CONTRAST=${CONTRAST:-0.35}

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
STIM_PY="$ROOT_DIR/bci/scripts/ssvep/stimulus.py"

if [[ ! -f "$STIM_PY" ]]; then
  echo "[ssvep] Stimulus script not found: $STIM_PY" >&2
  exit 1
fi

SECONDS_TOTAL=$(( MINUTES * 60 ))
# Give a small buffer so stimulus stays up until the recorder saves
STIM_SECONDS=$(( SECONDS_TOTAL + 3 ))

# 1) Start stimulus window in background
CMD=(python3 "$STIM_PY" --freqs "$FREQS" --labels "$LABELS" --seconds "$STIM_SECONDS" --block "$BLOCK" --countdown "$COUNTDOWN" --mode "$MODE" --contrast "$CONTRAST")
if [[ "$INVERT" = "1" || "$INVERT" = "true" ]]; then
  CMD+=(--invert 1)
fi
if [[ "$FULLSCREEN" = "1" || "$FULLSCREEN" = "true" ]]; then
  CMD+=(--fullscreen)
fi

echo "[ssvep] Launching stimulus: ${CMD[*]}"
"${CMD[@]}" &
STIM_PID=$!
trap 'kill "$STIM_PID" >/dev/null 2>&1 || true' EXIT INT TERM

# Wait for countdown so cues align with recorder start
if [[ "$COUNTDOWN" -gt 0 ]]; then
  echo "[ssvep] Countdown ${COUNTDOWN}s before recording..."
  sleep "$COUNTDOWN"
fi

# 2) Run cued recording with matching labels
echo "[ssvep] Recording: HOST=${HOST} LABELS=\"${LABELS}\" MINUTES=${MINUTES} BLOCK=${BLOCK} LABEL_SHIFT=${LABEL_SHIFT} CHS=\"${CHS}\""
HOST="${HOST}" LABELS="${LABELS}" MINUTES="${MINUTES}" BLOCK="${BLOCK}" \
LABEL_SHIFT="${LABEL_SHIFT}" CHS="${CHS}" ./run bci_record_cued

# 3) Done. Stimulus will auto-close after STIM_SECONDS or be killed on exit.
echo "[ssvep] Recording complete."

