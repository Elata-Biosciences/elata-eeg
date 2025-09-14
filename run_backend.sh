#!/bin/bash
# Start the EEG daemon backend
cd "$(dirname "$0")"
RUST_BACKTRACE=1 RUST_LOG=debug cargo run --bin eeg_daemon