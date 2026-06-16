---
name: rust-code-quality
description: Run and interpret the Simple STT Rust quality workflow. Use when working on Rust formatting, Clippy, tests, coverage, cargo-crap/CRAP scores, or safe cleanup/refactoring in this repository.
---

# Rust Code Quality

## Install Tools

Check tools before installing:

```powershell
cargo fmt --version
cargo clippy --version
cargo llvm-cov --version
cargo crap --version
```

Install missing tools:

```powershell
rustup component add clippy
cargo install cargo-llvm-cov
cargo install cargo-crap
```

## Run Workflow

Preferred repo-local command:

```bash
scripts/rust-quality.sh
```

Equivalent commands:

```powershell
cargo fmt --all --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
cargo crap --lcov lcov.info
```

Also run the project-specific Windows validation before release:

```bat
scripts\test-full.cmd
```

## Interpreting Output

Treat Clippy warnings as bugs or maintainability issues until proven otherwise. Prefer changing the code so the warning disappears naturally. Do not add `#[allow(...)]` unless there is a strong reason and a nearby comment explains why the lint is intentionally not applicable.

CRAP scores combine complexity and test coverage. High scores usually mean a function is both branch-heavy and undertested. Reduce CRAP by:

- splitting large functions into named helpers;
- simplifying nested conditionals;
- replacing duplicated branches with common helpers;
- adding meaningful tests for edge cases and failure paths;
- removing dead branches only when you can prove they are unreachable or obsolete.

## Safe Cleanup Rules

- Preserve the documented process boundary: AHK owns desktop behavior, capture owns audio/overlay/IPC/supervision, infer owns Parakeet loading.
- Do not reintroduce active Slint or in-process Parakeet loading.
- Do not delete tests to make checks pass.
- Do not remove public APIs or scripts unless references are audited first.
- Keep behavior changes explicit and covered by tests.
- Prefer small, reviewable edits over broad rewrites.
- Run targeted tests after each risky change, then run `scripts\test-full.cmd`.
