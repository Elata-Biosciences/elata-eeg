#!/bin/bash
set -euo pipefail

# Usage:
#   scripts/rebuild.sh [--prod|-p] [--daemon-only] [--kiosk-only] [--clean] [--no-install]
#                      [--release|-r] [from-install]
# Notes:
# - Defaults: incremental builds (no clean), build daemon in release, build kiosk only in --prod.
# - from-install is a sentinel used by install.sh to avoid stop/start during install.

FROM_INSTALL=${1:-""}
# Flag defaults
PROD=0
DAEMON_ONLY=0
KIOSK_ONLY=0
DO_CLEAN=0
NO_INSTALL=0
BUILD_RELEASE=1
WITH_TESTS=0

# Parse flags (skip legacy from-install positional if present)
ARGS=()
for arg in "$@"; do
  case "$arg" in
    from-install) : ;; # ignore, handled by caller
    --prod|-p) PROD=1 ;;
    --daemon-only) DAEMON_ONLY=1 ;;
    --kiosk-only) KIOSK_ONLY=1 ;;
    --clean) DO_CLEAN=1 ;;
    --no-install) NO_INSTALL=1 ;;
    --release|-r) BUILD_RELEASE=1 ;;
    --dev) BUILD_RELEASE=0 ;;
    --with-tests) WITH_TESTS=1 ;;
    *) ARGS+=("$arg") ;;
  esac
done

if [ $DAEMON_ONLY -eq 1 ] && [ $KIOSK_ONLY -eq 1 ]; then
  echo "❌ Cannot use --daemon-only and --kiosk-only together" >&2
  exit 1
fi

PROFILE=release
[ $BUILD_RELEASE -eq 0 ] && PROFILE=debug

echo "🚀 Starting rebuild (prod=$PROD, profile=$PROFILE, clean=$DO_CLEAN)"

# Stop services unless called from install.sh
if [[ ! " ${ARGS[*]} " =~ " from-install " ]]; then
  echo "🛑 Stopping services (soft)..."
  ./scripts/stop.sh || true
fi

# Rebuild Rust daemon (incremental by default)
echo "🔧 Building Rust daemon ($PROFILE)..."
[ $DO_CLEAN -eq 1 ] && cargo clean || true
if [ "$PROFILE" = "release" ]; then
  cargo build --release --bin eeg_daemon
else
  cargo build --bin eeg_daemon
fi

# Optional: run tests and clippy before installing
if [ $WITH_TESTS -eq 1 ]; then
  echo "🧪 Running tests and clippy"
  ./scripts/test.sh
fi


if [ $NO_INSTALL -eq 0 ]; then
  echo "📦 Installing eeg_daemon to /usr/local/bin (requires sudo if not writable)"
  if install -m 0755 "target/$PROFILE/eeg_daemon" /usr/local/bin/ 2>/dev/null; then
    echo "✅ Installed to /usr/local/bin"
  else
    echo "ℹ️ Using sudo to install binary"
    sudo install -m 0755 "target/$PROFILE/eeg_daemon" /usr/local/bin/
  fi
else
  echo "ℹ️ Skipping binary install (--no-install)"
fi

# Rebuild kiosk only in prod, unless explicitly requested via --kiosk-only
if [ $KIOSK_ONLY -eq 1 ] || [ $PROD -eq 1 ]; then
  echo "⚙️ Rebuilding kiosk (Next.js)"
  pushd kiosk > /dev/null
  if [ $DO_CLEAN -eq 1 ]; then
    echo "🧹 Cleaning .next"
    rm -rf .next
  fi
  if [ -d node_modules ]; then
    npm run build
  else
    echo "ℹ️ node_modules missing; installing deps..."
    (npm ci || npm install)
    npm run build
  fi
  sync || true
  popd > /dev/null
  echo "✅ Kiosk rebuild complete"
else
  echo "ℹ️ Skipping kiosk build (use --prod or --kiosk-only to build)"
fi

# Start services unless called from install.sh
if [[ ! " ${ARGS[*]} " =~ " from-install " ]]; then
  echo "🚀 Starting services"
  ./scripts/start.sh $([ $PROD -eq 1 ] && echo "--prod")
fi

echo "🎉 Rebuild complete"
