#!/bin/bash

# temporary
ENABLE_SLEEP=true; SLEEP_TIME=0.5; mysleep() { $ENABLE_SLEEP && sleep "${1:-$SLEEP_TIME}"; }

echo "🚀 Starting Kiosk Mode..."

# Add diagnostic information
echo "🔍 Diagnostic Information:"
echo "- Display Environment:"
echo "  DISPLAY=$DISPLAY"
echo "  WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
echo "  XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
echo "- Running Display Servers:"
echo "  X11 processes: $(pgrep -c Xorg || echo "0")"
echo "  Wayland processes: $(pgrep -c labwc || echo "0")"
echo "- Current User Session:"
echo "  User: $(whoami)"
echo "  TTY: $(tty)"
echo "- LightDM Configuration:"
echo "  $(grep -E 'greeter-session|user-session|autologin-session' /etc/lightdm/lightdm.conf 2>/dev/null || echo "  Could not read LightDM config")"

# Enable and start services with proper delays
echo "🔄 Enabling and starting daemon and kiosk services..."
sudo systemctl enable daemon
sudo systemctl enable kiosk

# Add delay before starting services
echo "⏳ Waiting for system to stabilize..."
mysleep

# Start services with delay between them
sudo systemctl start daemon
mysleep  # Add delay between service starts
sudo systemctl start kiosk
echo "✅ Services enabled and started"

# Wait for web service to be ready
echo "⏳ Waiting for network and kiosk service..."
for i in {1..10}; do
    if curl -s http://localhost:3000 >/dev/null; then
        echo "✅ Kiosk web service is responding"
        break
    fi
    echo "⏳ Waiting for kiosk web service to respond ($i/10)..."
    mysleep
done

# Remove the development mode flag if it exists
if [ -f "$HOME/.kiosk_dev_mode" ]; then
    echo "🗑️ Removing kiosk dev mode flag..."
    rm "$HOME/.kiosk_dev_mode"
fi

# Create labwc configuration directory
echo "📝 Setting up labwc configuration for kiosk mode..."
mkdir -p "/home/elata/.config/labwc"

# Create labwc.yml for cursor hiding in kiosk mode
cat > "/home/elata/.config/labwc/labwc.yml" <<EOL
cursor:
  hide-on-touch: true
  default-image: none
EOL

# Create a clean autostart file for labwc (complete replacement)
echo "📝 Creating labwc autostart file for kiosk mode..."

# Create a clean autostart file (no markers, complete replacement)
cat > "/home/elata/.config/labwc/autostart" <<EOL
#!/bin/sh

# Start the Wayland desktop components
/usr/bin/kanshi &

# Start Chromium in kiosk mode with Wayland
chromium-browser --ozone-platform=wayland --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
EOL

# Make the autostart file executable
chmod +x "/home/elata/.config/labwc/autostart"

# Make sure all panel instances are killed
echo "🔄 Checking for duplicate panels..."
PANEL_COUNT=$(pgrep -f wf-panel-pi | wc -l)
if [ "$PANEL_COUNT" -gt 1 ]; then
    echo "Detected $PANEL_COUNT panel instances. Fixing..."
    
    # Kill all panel instances
    pkill -9 -f wf-panel-pi || true
    mysleep 1
    
    # Start a single panel instance
    /usr/bin/wf-panel-pi &
    
    echo "Panel fixed. Now running a single instance."
else
    echo "Panel check OK: $PANEL_COUNT instance running."
fi

# Kill Chromium if it's running
echo "🔄 Restarting Chromium..."
pkill -f chromium-browser || true
mysleep

# Stop any existing Wayland compositor before restarting LightDM
echo "🔄 Stopping any existing Wayland compositor..."
pkill -9 -f labwc || true
mysleep 2

# Restart LightDM to apply the new configuration
echo "🔄 Restarting LightDM to apply Wayland configuration..."
sudo systemctl restart lightdm
LIGHTDM_STATUS=$?
if [ $LIGHTDM_STATUS -eq 0 ]; then
    echo "✅ LightDM restart command succeeded"
else
    echo "⚠️ LightDM restart command failed with status $LIGHTDM_STATUS"
fi

# Wait for Wayland to start
echo "⏳ Waiting for Wayland session to start..."
mysleep 5

# Check if Wayland is running after wait
if [ -n "$WAYLAND_DISPLAY" ]; then
    echo "✅ Wayland display detected: $WAYLAND_DISPLAY"
elif pgrep -c labwc > /dev/null; then
    echo "✅ labwc process detected, but WAYLAND_DISPLAY not set"
else
    echo "⚠️ Warning: No Wayland session detected after waiting"
fi

# Start Chromium directly
echo "Starting Chromium in kiosk mode..."
# Try with Wayland flags if we're in a Wayland session
if [ "$XDG_SESSION_TYPE" = "wayland" ] || [ -n "$WAYLAND_DISPLAY" ]; then
    echo "Detected Wayland session, using Wayland flags"
    echo "Command: chromium-browser --ozone-platform=wayland --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000"
    chromium-browser --ozone-platform=wayland --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
    CHROMIUM_PID=$!
    echo "Chromium started with PID: $CHROMIUM_PID"
else
    # Check if we need to set DISPLAY manually
    if [ -z "$DISPLAY" ]; then
        echo "DISPLAY not set, trying with DISPLAY=:0"
        echo "Command: DISPLAY=:0 chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000"
        DISPLAY=:0 chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
        CHROMIUM_PID=$!
        echo "Chromium started with PID: $CHROMIUM_PID"
    else
        echo "Using standard X11 mode with DISPLAY=$DISPLAY"
        echo "Command: chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000"
        chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito --disable-features=MediaDevices http://localhost:3000 &
        CHROMIUM_PID=$!
        echo "Chromium started with PID: $CHROMIUM_PID"
    fi
fi

# Check if Chromium is actually running after a short delay
mysleep 2
if ps -p $CHROMIUM_PID > /dev/null; then
    echo "✅ Chromium process is running"
else
    echo "⚠️ Warning: Chromium process is not running"
    # Check for error messages in the journal
    echo "Recent Chromium errors from journal:"
    journalctl -n 10 | grep -i chromium || echo "No recent Chromium errors found in journal"
fi

# Verify services are running
echo "🔍 Verifying kiosk mode is started..."
if ! systemctl is-active --quiet daemon || ! systemctl is-active --quiet kiosk; then
    echo "⚠️ Warning: Some services are not active. Kiosk mode may not function properly."
else
    echo "✅ Services are running."
fi

echo "✅ Kiosk mode started!"
echo "ℹ️ Services have been enabled and will start automatically on boot."
