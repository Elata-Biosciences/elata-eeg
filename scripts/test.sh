#!/usr/bin/env bash
set -euo pipefail

echo "==> Python unit tests (bci/scripts/tests)"
if command -v python3 >/dev/null 2>&1; then
  PY=python3
elif command -v python >/dev/null 2>&1; then
  PY=python
else
  echo "Python not found; skipping Python tests" >&2
  exit 0
fi

# Ensure repo root on sys.path for tests
export PYTHONPATH="${PYTHONPATH:-}:${PWD}"

$PY -m unittest discover -s bci/scripts/tests -p 'test_*.py' -v

echo "✅ Python tests passed"

