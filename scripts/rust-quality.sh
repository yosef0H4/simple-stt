#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo was not found on PATH" >&2
  exit 127
fi

cargo fmt --all --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

if cargo llvm-cov --version >/dev/null 2>&1; then
  cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
else
  echo "cargo-llvm-cov is not installed; skipping coverage" >&2
  echo "install with: cargo install cargo-llvm-cov" >&2
  exit 127
fi

if cargo crap --version >/dev/null 2>&1; then
  cargo crap --lcov lcov.info
else
  echo "cargo-crap is not installed; skipping CRAP analysis" >&2
  echo "install with: cargo install cargo-crap" >&2
  exit 127
fi
