#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
npm --prefix web/settings ci
npm --prefix web/settings run build
python scripts/verify-settings-frontend.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --bin simple-stt-settings
python scripts/test-settings-single-instance.py
python scripts/test-settings-selection-api.py
python scripts/test-cleanup-settings-e2e.py
python scripts/verify-static.py
python tools/ipc-poc/test_poc.py
python scripts/test-linux-static.py
node scripts/test-settings-model-selection.cjs
node scripts/test-settings-overhaul.cjs
node scripts/test-settings-contracts.cjs
