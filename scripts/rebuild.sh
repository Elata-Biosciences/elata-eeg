#!/bin/bash
set -e  # Exit on error

# Check if script is being called from install.sh
FROM_INSTALL=${1:-"false"}

echo "🚀 Starting full rebuild process..."

# Stop services and exit kiosk mode (unless called from install.sh)
if [ "$FROM_INSTALL" != "from-install" ]; then
  echo "🛑 Stopping services and exiting kiosk mode..."
  ./scripts/stop.sh
fi

# Driver is built automatically as a dependency of the daemon
# Rebuild Rust daemon
echo "🔧 Rebuilding Rust daemon..."
# cd daemon
cargo clean  # Ensures a fresh build
cargo build --release --bin eeg_daemon
sudo mv target/release/eeg_daemon /usr/local/bin/
echo "✅ Daemon rebuild complete!"

# Rebuild kiosk
echo "🧹 Cleaning Next.js build cache..."
cd kiosk
rm -rf .next
echo "⚙️ Rebuilding Next.js app..."
npm run build
echo "⚙️ Syncing kiosk build to disk..."
sync          # Ensure all build files are written to disk
cd ..
echo "✅ Kiosk rebuild complete!"

# Start services and kiosk mode (unless called from install.sh)
if [ "$FROM_INSTALL" != "from-install" ]; then
  echo "🚀 Starting services and kiosk mode..."
  ./scripts/start.sh
fi

echo "🎉 Rebuild process complete!"
