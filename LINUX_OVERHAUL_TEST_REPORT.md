# Linux overhaul test report

Date: 2026-06-27

## Passed in this environment

```bash
python3 scripts/verify-static.py
python3 scripts/test-linux-static.py
python3 -m py_compile linux/simple-stt-linux.py scripts/build-linux-fast-paste.py
python3 tools/ipc-poc/test_poc.py
python3 scripts/build-linux-fast-paste.py
```

Results:

- Static architecture verifier passed.
- Linux-specific static checks passed.
- Python Linux shell and paste-helper build script compile.
- IPC proof-of-concept passed.
- Linux fast-paste build script ran; it skipped native helper compilation because this container is missing the `libxtst` development package. That is a dependency/environment limitation, not a source syntax failure.

## Could not run here

```bash
cargo test --all-targets
cargo build --release
```

Reason: this execution environment does not have `cargo`/Rust installed (`cargo: command not found`). The Rust source was still updated for Linux compile support and the dependency graph was changed, but final Rust compilation must be run on a machine with Rust installed.

## Recommended local validation on Linux

```bash
rustup default stable
cargo test --all-targets
cargo build --release
python3 scripts/build-linux-fast-paste.py
python3 scripts/verify-static.py
python3 scripts/test-linux-static.py
python3 tools/ipc-poc/test_poc.py
```
