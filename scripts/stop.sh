#!/bin/bash

# temporary
ENABLE_SLEEP=true; SLEEP_TIME=0.5; mysleep() { $ENABLE_SLEEP && sleep "${1:-$SLEEP_TIME}"; }

echo "🛑 Stopping Kiosk Mode..."

# Stop and disable services to prevent auto-restart on boot
echo "🔄 Stopping and disabling services..."
sudo systemctl stop daemon
sudo systemctl stop kiosk
sudo systemctl disable daemon
sudo systemctl disable kiosk
echo "✅ Services stopped and disabled"

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
mkdir -p "/home/elata/.config/labwc"

# Create a development-friendly labwc.yml that shows the cursor
echo "📝 Creating labwc configuration for development mode..."
cat > "/home/elata/.config/labwc/labwc.yml" <<EOL
# Development mode configuration - cursor is visible
cursor:
  hide-on-touch: false
  # No default-image setting to use system default cursor
EOL

# Create a clean autostart file (no markers, complete replacement)
cat > "/home/elata/.config/labwc/autostart" <<EOL
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
chmod +x "/home/elata/.config/labwc/autostart"

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

if systemctl is-active --quiet daemon || systemctl is-active --quiet kiosk; then
    echo "⚠️ Warning: Some services are still active. You may need to stop them manually."
else
    echo "✅ Services are stopped."
fi

# Stop the Wayland compositor before restarting LightDM
echo "🔄 Stopping Wayland compositor..."
pkill -9 -f labwc || true
mysleep 2  # Give it time to terminate

# Restart LightDM to properly exit kiosk mode and return to login screen
echo "🔄 Restarting LightDM to exit kiosk mode..."
sudo systemctl restart lightdm
LIGHTDM_STATUS=$?
if [ $LIGHTDM_STATUS -eq 0 ]; then
    echo "✅ LightDM restart command succeeded"
else
    echo "⚠️ LightDM restart command failed with status $LIGHTDM_STATUS"
    echo "   You may need to manually restart LightDM with: sudo systemctl restart lightdm"
fi

echo "✅ Kiosk mode stopped. You can now develop!"
echo "ℹ️ Note: Services have been disabled, so they won't start on reboot."