#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
npm --prefix web/settings ci
npm --prefix web/settings run build
python scripts/verify-settings-frontend.py
cargo build --release --bin simple-stt-linux --bin simple-stt-capture --bin simple-stt-infer --bin simple-stt-ctl --bin simple-stt-settings
