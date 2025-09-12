#!/bin/bash

# Helpers and environment
SUDO="$(command -v sudo || true)"
USER_HOME="${HOME:-/home/$USER}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
check_systemctl() { command -v systemctl >/dev/null 2>&1; }
service_exists() { check_systemctl && systemctl list-unit-files | awk '{print $1}' | grep -qx "$1"; }
bin_exists() { command -v "$1" >/dev/null 2>&1; }

# temporary
ENABLE_SLEEP=true; SLEEP_TIME=0.5; mysleep() { $ENABLE_SLEEP && sleep "${1:-$SLEEP_TIME}"; }

echo "🛑 Stopping Kiosk Mode..."

# Stop services (no disable by default). Set DISABLE_SERVICES=1 to disable as well.
echo "🔄 Stopping services..."
if service_exists "daemon.service"; then
  $SUDO systemctl stop daemon || true
else
  echo "ℹ️ daemon.service not found; killing manual daemon processes..."
  if [ -f /tmp/eeg_daemon.pid ]; then
    DAEMON_PID=$(cat /tmp/eeg_daemon.pid || true)
    [ -n "${DAEMON_PID:-}" ] && kill "$DAEMON_PID" 2>/dev/null || true
    rm -f /tmp/eeg_daemon.pid
  fi
  pkill -f eeg_daemon || true
fi

if service_exists "kiosk.service"; then
  $SUDO systemctl stop kiosk || true
else
  echo "ℹ️ kiosk.service not found; killing manual kiosk (Next.js) ..."
  if [ -f /tmp/kiosk.pid ]; then
    KPID=$(cat /tmp/kiosk.pid || true)
    [ -n "${KPID:-}" ] && kill "$KPID" 2>/dev/null || true
    rm -f /tmp/kiosk.pid
  fi
  pkill -f "next start" || true
  pkill -f "node .*next" || true
fi

if [ "${DISABLE_SERVICES:-0}" = "1" ]; then
  echo "🔒 DISABLE_SERVICES=1 set; disabling services..."
  service_exists "daemon.service" && $SUDO systemctl disable daemon || true
  service_exists "kiosk.service" && $SUDO systemctl disable kiosk || true
  echo "ℹ️ Services disabled."
fi

echo "✅ Services stopped (or manual processes terminated)"

# Kill Chromium more forcefully (both chromium-browser and chromium)
echo "🔄 Killing Chromium browser..."
pkill -9 -f chromium-browser || true
pkill -9 -f chromium || true
mysleep 1  # Give it time to terminate

# Verify Chromium is not running (check both process names)
if pgrep -f chromium-browser > /dev/null || pgrep -f chromium > /dev/null; then
    echo "⚠️ Warning: Chromium is still running. Trying again..."
    pkill -9 -f chromium-browser || true
    pkill -9 -f chromium || true
    mysleep
fi

# Completely replace the autostart file for development mode
echo "📝 Creating new autostart file for development mode..."

# Create directory if it doesn't exist
mkdir -p "$USER_HOME/.config/labwc"

# Create a development-friendly labwc.yml that shows the cursor
echo "📝 Creating labwc configuration for development mode..."
cat > "$USER_HOME/.config/labwc/labwc.yml" <<EOL
# Development mode configuration - cursor is visible
cursor:
  hide-on-touch: false
  # No default-image setting to use system default cursor
EOL

# Create a clean autostart file (no markers, complete replacement)
cat > "$USER_HOME/.config/labwc/autostart" <<EOL
#!/bin/sh

# Start the default desktop components
/usr/bin/pcmanfm --desktop --profile LXDE-pi &
/usr/bin/wf-panel-pi &
/usr/bin/kanshi &

# Chromium is disabled in development mode
# chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000 &

# Start the XDG autostart applications
/usr/bin/lxsession-xdg-autostart
EOL

# Make the autostart file executable
chmod +x "$USER_HOME/.config/labwc/autostart"

# Make sure all panel instances are killed
echo "🔄 Killing all panel instances..."
pkill -9 -f wf-panel-pi || true  # Force kill all panel instances
mysleep 1  # Give it time to terminate

# Panel will be started by autostart file on next login
echo "🔄 Panel will be started by autostart file on next login..."
# DO NOT manually start panel here to avoid duplicates

# Create a flag file to indicate we're in development mode
touch "$HOME/.kiosk_dev_mode"

# Final verification
echo "🔍 Verifying kiosk mode is stopped..."
if pgrep -f chromium-browser > /dev/null || pgrep -f chromium > /dev/null; then
    echo "⚠️ Warning: Chromium is still running. You may need to kill it manually."
    echo "   Try running: killall chromium"
else
    echo "✅ Chromium is not running."
fi

ACTIVE_MSGS=()
if service_exists "daemon.service" && systemctl is-active --quiet daemon; then ACTIVE_MSGS+=("daemon"); fi
if service_exists "kiosk.service" && systemctl is-active --quiet kiosk; then ACTIVE_MSGS+=("kiosk"); fi
if [ ${#ACTIVE_MSGS[@]} -gt 0 ]; then
    echo "⚠️ Warning: Some services are still active: ${ACTIVE_MSGS[*]}"
else
    echo "✅ Services are stopped."
fi

# Stop the Wayland compositor before restarting LightDM
echo "🔄 Stopping Wayland compositor..."
pkill -9 -f labwc || true
mysleep 2  # Give it time to terminate

# Restart LightDM to properly exit kiosk mode and return to login screen
echo "🔄 Restarting LightDM to exit kiosk mode..."
$SUDO systemctl restart lightdm
LIGHTDM_STATUS=$?
if [ $LIGHTDM_STATUS -eq 0 ]; then
    echo "✅ LightDM restart command succeeded"
else
    echo "⚠️ LightDM restart command failed with status $LIGHTDM_STATUS"
    echo "   You may need to manually restart LightDM with: sudo systemctl restart lightdm"
fi

echo "✅ Kiosk mode stopped. You can now develop!"
echo "ℹ️ Note: Services have been disabled, so they won't start on reboot."