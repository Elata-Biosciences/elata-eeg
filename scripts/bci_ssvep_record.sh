#!/usr/bin/env bash
set -euo pipefail

# Single-command SSVEP stimulus + recording helper
# - Starts the kiosk Next.js server (if not already running)
# - Opens the 4-section SSVEP page in your browser
# - Runs the cued recorder with matching labels
# - Stops the kiosk server (unless KEEP_KIOSK=1)
#
# Usage (defaults shown):
#   HOST=172.20.10.12 LABELS="left,right,up,down" MINUTES=5 BLOCK=5 \
#   LABEL_SHIFT=0.2 CHS="2,3,4" FREQS="15,12,10,7.5" INVERT=0 \
#   ./run bci_ssvep_record
#
# Keep kiosk running after recording:
#   KEEP_KIOSK=1 ./run bci_ssvep_record

HOST=${HOST:-raspberrypi.local}
LABELS=${LABELS:-left,right,up,down}
MINUTES=${MINUTES:-5}
BLOCK=${BLOCK:-5}
LABEL_SHIFT=${LABEL_SHIFT:-0.2}
CHS=${CHS:-2,3,4}
FREQS=${FREQS:-15,12,10,7.5}
INVERT=${INVERT:-0}
PORT=${PORT:-3000}
KEEP_KIOSK=${KEEP_KIOSK:-0}

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
KIOSK_DIR="$ROOT_DIR/kiosk"
LOG_FILE="${TMPDIR:-/tmp}/kiosk_ssvep_$$.log"
KIOSK_PID=""

cleanup() {
  if [[ -n "${KIOSK_PID}" ]] && ps -p "${KIOSK_PID}" >/dev/null 2>&1; then
    if [[ "${KEEP_KIOSK}" != "1" ]]; then
      echo "[ssvep] Stopping kiosk (pid ${KIOSK_PID})"
      kill "${KIOSK_PID}" >/dev/null 2>&1 || true
      sleep 0.5 || true
      if ps -p "${KIOSK_PID}" >/dev/null 2>&1; then
        kill -9 "${KIOSK_PID}" >/dev/null 2>&1 || true
      fi
    else
      echo "[ssvep] KEEP_KIOSK=1 set — leaving kiosk running (pid ${KIOSK_PID})"
    fi
  fi
}
trap cleanup EXIT INT TERM

# 1) Ensure kiosk dependencies are installed (best-effort check)
if [[ ! -d "${KIOSK_DIR}/node_modules" ]]; then
  echo "[ssvep] kiosk/node_modules not found. Please run:"
  echo "  cd kiosk && npm install"
  exit 1
fi

# 2) Start kiosk server
echo "[ssvep] Starting kiosk on http://localhost:${PORT} ... (logs: ${LOG_FILE})"
(
  cd "${KIOSK_DIR}"
  NODE_ENV=development node server.js >>"${LOG_FILE}" 2>&1 &
  echo $! >"${LOG_FILE}.pid"
)
KIOSK_PID=$(cat "${LOG_FILE}.pid" 2>/dev/null || true)
if [[ -z "${KIOSK_PID}" ]]; then
  echo "[ssvep] Failed to start kiosk (no PID). See logs: ${LOG_FILE}"
  exit 1
fi

# Wait briefly for server to boot
sleep 2

# 3) Open SSVEP page
SSVEP_URL="http://localhost:${PORT}/ssvep?freqs=${FREQS}&labels=${LABELS}&invert=${INVERT}"
echo "[ssvep] Opening stimulus: ${SSVEP_URL}"
if command -v open >/dev/null 2>&1; then
  open "${SSVEP_URL}" || true
elif command -v xdg-open >/dev/null 2>&1; then
  xdg-open "${SSVEP_URL}" || true
else
  echo "[ssvep] Please open the URL above in your browser."
fi

# 4) Run cued recording with matching labels
echo "[ssvep] Recording: HOST=${HOST} LABELS=\"${LABELS}\" MINUTES=${MINUTES} BLOCK=${BLOCK} LABEL_SHIFT=${LABEL_SHIFT} CHS=\"${CHS}\""
HOST="${HOST}" LABELS="${LABELS}" MINUTES="${MINUTES}" BLOCK="${BLOCK}" \
LABEL_SHIFT="${LABEL_SHIFT}" CHS="${CHS}" ./run bci_record_cued

# 5) Done. Cleanup handler will stop kiosk unless KEEP_KIOSK=1
echo "[ssvep] Recording complete. Output saved by recorder script."

