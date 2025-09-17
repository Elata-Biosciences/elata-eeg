#!/bin/bash
set -euo pipefail

# Simple test runner for the workspace
# Usage: ./run test
# What it does (fast, safe-by-default):
# - Rust: cargo test (workspace), clippy (deny warnings)
# - Frontend: lint (only if node_modules is present; does not install deps)

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

pass() { echo -e "\033[32m$1\033[0m"; }
info() { echo -e "\033[36m$1\033[0m"; }
warn() { echo -e "\033[33m$1\033[0m"; }

info "Rust: running unit/integration tests (workspace)"
cargo test --workspace
pass "Rust tests passed"

info "Rust: clippy (deny warnings)"
cargo clippy --workspace -- -D warnings
pass "Clippy passed"

if [ -d kiosk ]; then
  if command -v npm >/dev/null 2>&1; then
    if [ -d kiosk/node_modules ]; then
      info "Frontend: lint (kiosk)"
      npm run --prefix kiosk lint
      pass "Kiosk lint passed"
    else
      warn "Skipping kiosk lint: node_modules missing (not installing deps in test script)"
    fi
  else
    warn "Skipping kiosk lint: npm not found"
  fi
fi

pass "All checks complete"

