#!/bin/bash
echo "🔄 Restarting Kiosk Mode..."
SCRIPT_DIR="$(dirname "$0")"
$SCRIPT_DIR/stop.sh
$SCRIPT_DIR/start.sh