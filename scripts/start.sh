#!/bin/bash

# temporary
ENABLE_SLEEP=true; SLEEP_TIME=0.5; mysleep() { $ENABLE_SLEEP && sleep "${1:-$SLEEP_TIME}"; }
set -euo pipefail

# Parse flags (default to development mode; use --prod for production)
PROD=0
KIOSK_OS=0
CONFIG=""
# Enhanced flag parsing (supports --config PATH)
while [ $# -gt 0 ]; do
  case "$1" in
    --prod|-p) PROD=1; shift ;;
    --kiosk-os) KIOSK_OS=1; shift ;;
    --config|-c) CONFIG="$2"; shift 2 ;;
    *) shift ;;
  esac
done


# Build daemon args from flags
DAEMON_ARGS=()
if [ -n "$CONFIG" ]; then
  DAEMON_ARGS+=(--config "$CONFIG")
fi

# Resolve current user home and sudo
USER_HOME="${HOME:-/home/$USER}"
SUDO="$(command -v sudo || true)"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

check_systemctl() { command -v systemctl >/dev/null 2>&1; }
service_exists() { check_systemctl && systemctl list-unit-files | awk '{print $1}' | grep -qx "$1"; }
bin_exists() { command -v "$1" >/dev/null 2>&1; }


echo "🚀 Starting Kiosk Mode..."

# Add diagnostic information
echo "🔍 Diagnostic Information:"
echo "- Display Environment:"
echo "  DISPLAY=${DISPLAY:-}"
echo "  WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
echo "  XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-}"
echo "- Running Display Servers:"
echo "  X11 processes: $(pgrep -c Xorg || echo 0)"
echo "  Wayland processes: $(pgrep -c labwc || echo 0)"
echo "- Current User Session:"
echo "  User: $(whoami)"
echo "  TTY: $(tty 2>/dev/null || echo N/A)"
echo "- LightDM Configuration:"
echo "  $(grep -E 'greeter-session|user-session|autologin-session' /etc/lightdm/lightdm.conf 2>/dev/null || echo "  Could not read LightDM config")"

# Enable and start services with proper delays
echo "🔄 Enabling and starting daemon and kiosk services (if installed)..."
STARTED_VIA="unknown"
if service_exists "daemon.service"; then
  $SUDO systemctl enable daemon
  mysleep
  $SUDO systemctl start daemon
  STARTED_VIA="systemd"
else
  echo "ℹ️ daemon.service not found; attempting manual start..."
  if pgrep -f "eeg_daemon" >/dev/null 2>&1; then
    echo "✅ eeg_daemon process already running"
    STARTED_VIA="manual"
  else
    if [ -x "/usr/local/bin/eeg_daemon" ]; then
      echo "▶️ Starting /usr/local/bin/eeg_daemon"
      nohup /usr/local/bin/eeg_daemon "${DAEMON_ARGS[@]}" > /tmp/eeg_daemon.log 2>&1 &
      DAEMON_PID=$!
      STARTED_VIA="manual"
    elif [ -x "$REPO_ROOT/target/release/eeg_daemon" ]; then
      echo "▶️ Starting $REPO_ROOT/target/release/eeg_daemon"
      nohup "$REPO_ROOT/target/release/eeg_daemon" "${DAEMON_ARGS[@]}" > /tmp/eeg_daemon.log 2>&1 &
      DAEMON_PID=$!
      STARTED_VIA="manual"
    elif [ -x "$REPO_ROOT/target/debug/eeg_daemon" ]; then
      echo "▶️ Starting $REPO_ROOT/target/debug/eeg_daemon"
      nohup "$REPO_ROOT/target/debug/eeg_daemon" "${DAEMON_ARGS[@]}" > /tmp/eeg_daemon.log 2>&1 &
      DAEMON_PID=$!
      STARTED_VIA="manual"
    elif bin_exists cargo; then
      echo "▶️ Building and starting daemon via cargo (dev)"
      (cd "$REPO_ROOT" && nohup cargo run --bin eeg_daemon -- "${DAEMON_ARGS[@]}" > /tmp/eeg_daemon.log 2>&1 & echo $! > /tmp/eeg_daemon.pid)
      DAEMON_PID=$(cat /tmp/eeg_daemon.pid 2>/dev/null || true)
      STARTED_VIA="manual"
    else
      echo "❌ Could not find eeg_daemon binary and cargo is not installed."
    fi
  fi
fi

# Wait for daemon to respond on port 9000 (/api/state)
if ! curl -sf http://127.0.0.1:9000/api/state >/dev/null; then
  echo "⏳ Waiting for daemon to become ready on :9000 (/api/state)"
  READY=0
  for i in {1..40}; do
    if curl -sf http://127.0.0.1:9000/api/state >/dev/null; then
      echo "✅ Daemon is responding (attempt $i)"
      READY=1
      break
    fi
    mysleep 0.5
  done
  if [ "$READY" -ne 1 ] && [ "$STARTED_VIA" = "systemd" ] && bin_exists cargo; then
    echo "⚠️ Daemon not responding via systemd. Falling back to dev mode: cargo run --bin eeg_daemon"
    $SUDO systemctl stop daemon || true
    (cd "$REPO_ROOT" && nohup cargo run --bin eeg_daemon -- "${DAEMON_ARGS[@]}" > /tmp/eeg_daemon.log 2>&1 & echo $! > /tmp/eeg_daemon.pid)
    for i in {1..40}; do
      if curl -sf http://127.0.0.1:9000/api/state >/dev/null; then
        echo "✅ Daemon is responding after cargo fallback (attempt $i)"
        break
      fi
      mysleep 0.5
    done
  fi
fi

if service_exists "kiosk.service"; then
  $SUDO systemctl enable kiosk
  mysleep
  $SUDO systemctl start kiosk
else
  echo "ℹ️ kiosk.service not found; starting kiosk manually..."
  KIOSK_DIR="$REPO_ROOT/kiosk"
  if ! bin_exists npm; then
    echo "❌ npm not found. Please install Node.js and npm."
  else
    if [ ! -d "$KIOSK_DIR/node_modules" ]; then
      echo "📦 Installing kiosk dependencies (node_modules missing)..."
      (cd "$KIOSK_DIR" && (npm ci || npm install))
    fi
    if [ "$PROD" -eq 1 ]; then
      echo "⚙️ Building kiosk (production)..."
      if ! (cd "$KIOSK_DIR" && npm run build); then
        echo "❌ Kiosk build failed. Aborting."
        exit 1
      fi
      echo "▶️ Starting kiosk (next start) in background..."
      (cd "$KIOSK_DIR" && nohup npm start > /tmp/kiosk.log 2>&1 & echo $! > /tmp/kiosk.pid)
    else
      # Always use port 3000 for dev; kill any existing process occupying it
      KIOSK_PORT=3000
      if curl -sS --max-time 1 http://127.0.0.1:${KIOSK_PORT} >/dev/null; then
        echo "⚠️ Port ${KIOSK_PORT} is busy; attempting to free it..."
        if bin_exists fuser; then
          $SUDO fuser -k -n tcp ${KIOSK_PORT} || true
        elif bin_exists lsof; then
          PIDS=$(lsof -ti tcp:${KIOSK_PORT} || true)
          if [ -n "$PIDS" ]; then $SUDO kill -9 $PIDS || true; fi
        elif bin_exists ss; then
          PIDS=$(ss -lptn 'sport = :${KIOSK_PORT}' 2>/dev/null | awk -F',' '/pid=/ {for(i=1;i<=NF;i++){if($i ~ /pid=/){gsub(/pid=/,"",$i); gsub(/\).*/,"",$i); print $i}}}' | tr '\n' ' ')
          if [ -n "$PIDS" ]; then $SUDO kill -9 $PIDS || true; fi
        else
          echo "⚠️ Could not detect fuser/lsof/ss; skipping auto-kill."
        fi
        mysleep 1
      fi
      echo "⚙️ Starting kiosk (development) via custom server (server.js with proxy) on :$KIOSK_PORT..."
      (cd "$KIOSK_DIR" && PORT=$KIOSK_PORT nohup node server.js > /tmp/kiosk.log 2>&1 & echo $! > /tmp/kiosk.pid)
    fi
    KIOSK_PID=$(cat /tmp/kiosk.pid 2>/dev/null || true)
    echo "ℹ️ kiosk PID: ${KIOSK_PID:-unknown} (logs: /tmp/kiosk.log)"
  fi
fi
echo "✅ Service enable/start step complete"

# Wait for web service to be ready
echo "⏳ Waiting for network and kiosk service..."
READY=0
for i in {1..120}; do
    if curl -sf http://127.0.0.1:${KIOSK_PORT:-3000} >/dev/null; then
        echo "✅ Kiosk web service is responding on :${KIOSK_PORT:-3000} (attempt $i)"
        READY=1
        break
    fi
    if [ $((i % 10)) -eq 0 ]; then
        echo "⏳ Still waiting... ($i/120). If this persists, check /tmp/kiosk.log"
    fi
    mysleep 1
done
if [ "$READY" -ne 1 ]; then
  echo "❌ Kiosk did not respond on :3000 within timeout. Tail of /tmp/kiosk.log (if exists):"
  tail -n 40 /tmp/kiosk.log 2>/dev/null || true
fi

# Verify kiosk can reach daemon via proxy (/api/state)
if [ "$READY" -eq 1 ]; then
  echo "⏳ Verifying kiosk->daemon proxy (/api/state)"
  PROXY_OK=0
  for i in {1..30}; do
    if curl -sf http://127.0.0.1:${KIOSK_PORT:-3000}/api/state >/dev/null; then
      echo "✅ Kiosk proxy to daemon is responding on :${KIOSK_PORT:-3000} (attempt $i)"
      PROXY_OK=1
      break
    fi
    mysleep 1
  done
  if [ "$PROXY_OK" -ne 1 ]; then
    echo "⚠️ Kiosk responded, but /api/state via kiosk proxy did not respond within timeout"
    echo "   Check that the daemon is running and listening on :9000, and kiosk proxy config is correct."
  fi
fi


# Optional kiosk-OS (autostart/browser) changes
if [ "$KIOSK_OS" -eq 1 ]; then
  echo "Enabling kiosk-OS browser/autostart changes (--kiosk-os)"



# Remove the development mode flag if it exists
if [ -f "$HOME/.kiosk_dev_mode" ]; then
    echo "🗑️ Removing kiosk dev mode flag..."
    rm "$HOME/.kiosk_dev_mode"
fi

# Create labwc configuration directory
echo "📝 Setting up labwc configuration for kiosk mode..."
mkdir -p "$USER_HOME/.config/labwc"

# Create labwc.yml for cursor hiding in kiosk mode
cat > "$USER_HOME/.config/labwc/labwc.yml" <<EOL
cursor:
  hide-on-touch: true
  default-image: none
EOL

# Create a clean autostart file for labwc (complete replacement)
echo "📝 Creating labwc autostart file for kiosk mode..."

# Create a clean autostart file (no markers, complete replacement)
cat > "$USER_HOME/.config/labwc/autostart" <<EOL
#!/bin/sh

# Start the Wayland desktop components
/usr/bin/kanshi &

# Start Chromium in kiosk mode with Wayland
chromium-browser --ozone-platform=wayland --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
EOL

# Make the autostart file executable
chmod +x "$USER_HOME/.config/labwc/autostart"

# Make sure all panel instances are killed
echo "🔄 Checking for duplicate panels..."
if bin_exists "wf-panel-pi"; then
  PANEL_COUNT=$(pgrep -f wf-panel-pi | wc -l)
  if [ "$PANEL_COUNT" -gt 1 ]; then
    echo "Detected $PANEL_COUNT panel instances. Fixing..."
    pkill -9 -f wf-panel-pi || true
    mysleep 1
    /usr/bin/wf-panel-pi &
    echo "Panel fixed. Now running a single instance."
  else
    echo "Panel check OK: $PANEL_COUNT instance running."
  fi
else
  echo "wf-panel-pi not found; skipping panel check."
fi

# Kill Chromium if it's running
echo "🔄 Restarting Chromium..."
pkill -f chromium-browser || true
mysleep

# Stop any existing Wayland compositor before restarting LightDM
echo "🔄 Stopping any existing Wayland compositor..."
pkill -9 -f labwc || true
mysleep 2

# Restart display manager to apply the new configuration (if available)
echo "🔄 Restarting display manager (if available)..."
if service_exists "lightdm.service"; then
  $SUDO systemctl restart lightdm
  LIGHTDM_STATUS=$?
  if [ $LIGHTDM_STATUS -eq 0 ]; then
    echo "✅ LightDM restart command succeeded"
  else
    echo "⚠️ LightDM restart command failed with status $LIGHTDM_STATUS"
  fi
else
  echo "ℹ️ LightDM not installed; skipping restart."
fi

# Wait for Wayland to start
echo "⏳ Waiting for Wayland session to start..."
mysleep 5

# Check if Wayland is running after wait
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    echo "✅ Wayland display detected: ${WAYLAND_DISPLAY:-}"
elif pgrep -c labwc > /dev/null; then
    echo "✅ labwc process detected, but WAYLAND_DISPLAY not set"
else
    echo "⚠️ Warning: No Wayland session detected after waiting"
fi

# Start Chromium directly
echo "Starting Chromium in kiosk mode..."

BROWSER=""
if bin_exists "chromium-browser"; then BROWSER="chromium-browser";
elif bin_exists "chromium"; then BROWSER="chromium";
elif bin_exists "google-chrome"; then BROWSER="google-chrome"; fi

if [ -n "$BROWSER" ]; then
  # Try with Wayland flags if we're in a Wayland session
  if [ "${XDG_SESSION_TYPE:-}" = "wayland" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
    echo "Detected Wayland session, using Wayland flags"
    echo "Command: $BROWSER --ozone-platform=wayland --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000"
    $BROWSER --ozone-platform=wayland --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
    CHROMIUM_PID=$!
  else
    # Check if we need to set DISPLAY manually
    if [ -z "${DISPLAY:-}" ]; then
      echo "DISPLAY not set, trying with DISPLAY=:0"
      echo "Command: DISPLAY=:0 $BROWSER --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000"
      DISPLAY=:0 $BROWSER --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
      CHROMIUM_PID=$!
    else
      echo "Using standard X11 mode with DISPLAY=$DISPLAY"
      echo "Command: $BROWSER --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000"
      $BROWSER --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
      CHROMIUM_PID=$!
    fi
  fi
  echo "Chromium started with PID: ${CHROMIUM_PID:-unknown}"
else
  echo "ℹ️ Chromium/Chrome not found; skipping browser launch."
fi

else
  echo "Skipping kiosk-OS browser/autostart changes (--kiosk-os not set)"
fi

# Check if Chromium is actually running after a short delay
mysleep 2
if [ -n "${CHROMIUM_PID:-}" ] && ps -p "$CHROMIUM_PID" > /dev/null; then
  echo "✅ Chromium process is running"
elif [ -n "${CHROMIUM_PID:-}" ]; then
  echo "⚠️ Warning: Chromium process is not running"
  # Check for error messages in the journal
  echo "Recent Chromium errors from journal:"
  journalctl -n 10 | grep -i chromium || echo "No recent Chromium errors found in journal"
else
  echo "ℹ️ Browser was not started; skipping process check."
fi

# Verify services are running
echo "🔍 Verifying kiosk mode is started..."
if service_exists "daemon.service"; then
  if $SUDO systemctl is-active --quiet daemon; then echo "✅ daemon.service is active"; else echo "⚠️ daemon.service is not active"; fi
else
  echo "ℹ️ daemon.service not installed; skipping status check."
fi
if service_exists "kiosk.service"; then
  if $SUDO systemctl is-active --quiet kiosk; then echo "✅ kiosk.service is active"; else echo "⚠️ kiosk.service is not active"; fi
else
  echo "ℹ️ kiosk.service not installed; skipping status check."
fi

echo "✅ Kiosk mode started!"
echo "ℹ️ Services have been enabled and will start automatically on boot."
