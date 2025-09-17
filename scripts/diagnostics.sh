#!/bin/bash
set -euo pipefail

# Device/Daemon/Kiosk quick diagnostics (non-destructive)
# Usage: ./run diagnostics

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

CURL="curl -sS --max-time 3"      # no -f, so 404 still counts as reachable
CURLF="curl -sfS --max-time 3"     # -f for endpoints where we require 2xx
PASS() { echo -e "\033[32m✓ $*\033[0m"; }
FAIL() { echo -e "\033[31m✗ $*\033[0m"; }
WARN() { echo -e "\033[33m! $*\033[0m"; }
INFO() { echo -e "\033[36m- $*\033[0m"; }

have() { command -v "$1" >/dev/null 2>&1; }
service_exists() { systemctl list-unit-files | grep -q "^$1" || systemctl status "$1" >/dev/null 2>&1; }

SUMMARY=()
add_summary() { SUMMARY+=("$1"); }

section() {
  echo
  echo "==== $* ===="
}

section "Environment"
INFO "Repo: $ROOT_DIR"
INFO "Kernel: $(uname -a | cut -d' ' -f1-3)"
INFO "User: $(whoami)"
INFO "Tools: curl=$(if have curl; then echo yes; else echo no; fi), jq=$(if have jq; then echo yes; else echo no; fi), cargo=$(if have cargo; then echo yes; else echo no; fi)"

section "Services"
if service_exists daemon.service; then
  if systemctl is-active --quiet daemon; then PASS "daemon.service is active"; add_summary "daemon:active"; else FAIL "daemon.service is NOT active"; add_summary "daemon:inactive"; fi
else
  WARN "daemon.service not installed"
fi
if service_exists kiosk.service; then
  if systemctl is-active --quiet kiosk; then PASS "kiosk.service is active"; add_summary "kiosk:active"; else WARN "kiosk.service is not active"; add_summary "kiosk:inactive"; fi
else
  INFO "kiosk.service not installed (dev mode likely)"
fi

section "Ports"
if $CURL http://127.0.0.1:9000/ >/dev/null; then PASS "daemon http :9000 reachable"; add_summary "port9000:ok"; else FAIL "daemon http :9000 not reachable"; add_summary "port9000:fail"; fi
if $CURL http://127.0.0.1:3000/ >/dev/null; then PASS "kiosk http :3000 reachable"; add_summary "port3000:ok"; else WARN "kiosk http :3000 not reachable"; add_summary "port3000:fail"; fi

section "Daemon API"
STATE_JSON=""
if STATE_JSON=$($CURLF http://127.0.0.1:9000/api/state 2>/dev/null); then
  PASS "GET /api/state ok"
  if have jq; then echo "$STATE_JSON" | jq 'del(.pipeline)' | sed -n '1,20p'; fi
  add_summary "state:ok"
else
  FAIL "GET /api/state failed"
  add_summary "state:fail"
fi

section "SSE Events"
EVENTS_OUT=""
if EVENTS_OUT=$(timeout 4s bash -c "$CURL http://127.0.0.1:9000/api/events | head -n 20" 2>/dev/null); then
  echo "$EVENTS_OUT" | sed -n '1,12p'
  add_summary "sse:ok"
  if echo "$EVENTS_OUT" | grep -q "PipelineStarted"; then PASS "PipelineStarted seen"; else WARN "No PipelineStarted event seen"; fi
  if echo "$EVENTS_OUT" | grep -q "SourceReady"; then PASS "SourceReady seen (hardware init)"; add_summary "source:ready"; else FAIL "No SourceReady event (check wiring/power)"; add_summary "source:not_ready"; fi
else
  FAIL "SSE /api/events not reachable"
  add_summary "sse:fail"
fi

section "WebSocket broker (subscribe to eeg_voltage)"
if have cargo; then
  INFO "Running cargo run --bin data_test (5s timeout)"
  if timeout 5s bash -lc 'RUST_LOG=warn cargo run --quiet --bin data_test' 2>&1 | tee /tmp/eeg_ws_diag.log | sed -n '1,12p'; then
    if grep -q "Subscribed" /tmp/eeg_ws_diag.log; then PASS "WS subscribe ACK received"; add_summary "ws:ack"; else FAIL "No WS ACK received"; add_summary "ws:no_ack"; fi
  else
    FAIL "data_test failed to run/connect"
    add_summary "ws:fail"
  fi
else
  WARN "cargo not found; skipping WS smoke"
fi

section "SPI / Hardware presence"
if ls /dev/spidev* >/dev/null 2>&1; then PASS "spidev present: $(ls /dev/spidev* | xargs echo)"; add_summary "spidev:present"; else WARN "No /dev/spidev* devices found"; add_summary "spidev:missing"; fi
if lsmod | grep -q spi; then INFO "SPI kernel modules: $(lsmod | awk '/spi/ {print $1}' | xargs echo)"; fi
if grep -qi 'raspberry pi' /proc/cpuinfo 2>/dev/null; then INFO "Raspberry Pi detected"; fi

section "Summary"
printf '%s\n' "${SUMMARY[@]}"

section "Checklist"
ck() {
  local label="$1"; local ok_tag="$2"; local fail_tag="${3:-}"
  if printf '%s\n' "${SUMMARY[@]}" | grep -q "$ok_tag"; then
    PASS "$label"
  elif [ -n "$fail_tag" ] && printf '%s\n' "${SUMMARY[@]}" | grep -q "$fail_tag"; then
    FAIL "$label"
  else
    WARN "$label (not checked)"
  fi
}
ck "daemon.service active" "daemon:active" "daemon:inactive"
ck "kiosk.service active" "kiosk:active" "kiosk:inactive"
ck "Daemon HTTP :9000 reachable" "port9000:ok" "port9000:fail"
ck "Kiosk HTTP :3000 reachable" "port3000:ok" "port3000:fail"
ck "Daemon /api/state OK" "state:ok" "state:fail"
ck "SSE reachable" "sse:ok" "sse:fail"
ck "SourceReady event seen" "source:ready" "source:not_ready"
ck "WebSocket subscribe ACK received" "ws:ack" "ws:no_ack"
ck "SPI devices present" "spidev:present" "spidev:missing"

# Heuristic diagnosis
echo
if printf '%s\n' "${SUMMARY[@]}" | grep -q 'state:ok' && printf '%s\n' "${SUMMARY[@]}" | grep -q 'source:not_ready'; then
  echo "Likely cause: hardware/source not initializing (wiring/power/board)."
  echo "Action: verify power, GND, DRDY/CS/SCLK/MOSI/MISO; re-seat cables, then restart: ./run stop && ./run start"
fi
if printf '%s\n' "${SUMMARY[@]}" | grep -q 'port9000:fail'; then
  echo "Daemon not reachable on :9000. Start it (dev): RUST_LOG=debug cargo run --bin eeg_daemon"
fi
if printf '%s\n' "${SUMMARY[@]}" | grep -q 'ws:no_ack\|ws:fail'; then
  echo "WS broker not healthy; check daemon logs for errors in websocket_broker."
fi

exit 0

