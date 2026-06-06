# Uvox validation report

Date: 2026-06-05

## Environment

- PowerShell: 7.6.0
- Git: 2.51.0.windows.1
- uv: 0.8.13
- Rust: rustc 1.89.0, cargo 1.89.0
- NVIDIA: driver 581.29, CUDA reported by `nvidia-smi` as 13.0
- GPU: NVIDIA GeForce RTX 3050 Ti Laptop GPU

## Results

| Command | Result | Notes |
|---|---:|---|
| `powershell -ExecutionPolicy Bypass -File .\scripts\setup-worker.ps1` | PASS | Installed Python 3.11 worker env, CUDA PyTorch, NeMo, pytest, and ruff. |
| `uv run --no-sync uvox-worker doctor --check-nemo` | PASS | CUDA available; torch `2.11.0+cu128`; torch CUDA `12.8`; NeMo ASR import OK. |
| `powershell -ExecutionPolicy Bypass -File .\scripts\python-unit-tests.ps1` | PASS | 18 Python tests passed. |
| `uv run --no-sync ruff check src tests` | PASS | No lint findings after unused import cleanup. |
| `uv run --no-sync uvox-worker fetch-sample` | PASS | Cached sample at `C:\Users\xxyoc\.cache\uvox\samples\2086-149220-0033.wav`; duration 7.435s. |
| `uv run --no-sync uvox-worker smoke-test` | PASS | Whole-file Nemotron transcription completed on CUDA. |
| `uv run --no-sync uvox-worker stream-file-test --lookahead-ms 80` | PASS | Streaming path emitted partials and final transcript. |
| `cargo fmt --check` | PASS | Formatting clean. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS | No warnings. |
| `cargo test --all-targets --all-features` | PASS | 14 Rust tests passed. |
| `cargo build --release` | PASS | Release executable built. |
| `powershell -ExecutionPolicy Bypass -File .\scripts\test-all.ps1` | PASS | Python tests, Ruff, and Rust workspace tests passed. |
| `cargo run -p uvox -- list-inputs` | PASS | Found `Stereo Mix` and `Microphone Array`. |
| `powershell -ExecutionPolicy Bypass -File .\scripts\doctor.ps1` | PASS | Rust config OK and Python CUDA/NeMo doctor passed. |
| `cargo run -p uvox -- record-test --seconds 5 --output recording-test.wav` | PASS | Wrote `Z:\files\projects\rust\uvox-stt\recording-test.wav`. |

## Audio transcripts

Whole-file STT transcript:

```text
Well I don't wish to see it any more, observed Phoebe, turning away her eyes. It is certainly very like the old portrait.
```

Streaming-file final transcript:

```text
Well I don't wish to see it any more observed three B turning away her eyes if it certainly very likely the old or portrait
```

The streaming command passed and exercised the cache-aware live path, but the final streaming transcript is less accurate than the whole-file transcript.

## Built artifacts

- Release executable: `Z:\files\projects\rust\uvox-stt\target\release\uvox.exe`
- Recorded microphone test: `Z:\files\projects\rust\uvox-stt\recording-test.wav`
- Smoke-test log: `Z:\files\projects\rust\uvox-stt\artifacts-smoke.tmp`
- Streaming-test log: `Z:\files\projects\rust\uvox-stt\artifacts-stream.tmp`

## Fixes made during validation

- Reworked `scripts/setup-worker.ps1` for current uv by installing CUDA PyTorch from the PyTorch CUDA index with `uv pip`, then installing worker dependencies and dev tools.
- Enabled Hatch direct references so the local worker package can build with the direct NeMo Git dependency.
- Fixed Rust API compatibility with current `windows-sys` and CPAL types.
- Removed Rust warning/Clippy failures.
- Removed the `native-windows-gui` runtime dependency because it imported `GetWindowSubclass`, which failed Windows loader startup on this machine.
- Replaced settings UI with a lightweight Notepad-backed settings file opener.
- Replaced UUID token generation with Windows `BCryptGenRandom` and moved `tempfile` to dev-dependencies.
- Updated README setup wording for the working CUDA PyTorch install path.

## Remaining manual check

Run the live app, focus a normal text field such as Notepad, hold CapsLock while speaking, and release CapsLock to verify live insertion/cancellation:

```powershell
.\scripts\run-dev.ps1
```
