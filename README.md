# Uvox STT

A lightweight, Windows-first, open-source push-to-talk dictation prototype.

Hold **CapsLock** to record. The Rust manager starts recording immediately, launches a CUDA-only Python worker when needed, buffers early microphone frames during cold startup, receives stable transcript commits, and inserts literal Unicode text into the focused application at a fixed configurable cadence. Releasing CapsLock cancels the current session immediately and discards any late transcript tail.

The project intentionally does **not** attempt to disguise synthetic input or bypass application restrictions. Text pacing is fixed and configurable for usability and compatibility with normal text fields.

## Status

This repository is a complete first implementation intended for testing on Windows with an NVIDIA CUDA GPU. The Python worker unit tests are runnable without CUDA. The real Nemotron integration commands deliberately fail if `torch.cuda.is_available()` is false.

The Rust source is included in full, but the generated archive was assembled in an environment without a Rust toolchain or NVIDIA GPU. Run the provided PowerShell tests on a Windows CUDA machine before treating it as production-ready.

## Architecture

```text
CapsLock hook ───────┐
                     ▼
Rust manager ── microphone capture ── 16 kHz mono ring buffer
     │                                      │
     │ loopback TCP binary IPC              │ PCM16 frames
     ▼                                      ▼
Python worker ── CUDA-only NeMo ── Nemotron cache-aware streaming RNNT
     │
     ├── partial hypothesis ── logs / future overlay
     └── conservative stable-prefix commit
                         │
                         ▼
Rust fixed-rate Unicode sender ── focused normal text field
```

The model worker is disposable: Rust unloads it after an idle timeout to free RAM and VRAM. The Rust process remains resident and lightweight.

## Prerequisites

Use Windows 10 or newer with:

- an NVIDIA CUDA-capable GPU and a current NVIDIA driver;
- Git, because the current NeMo install is sourced from NVIDIA's repository;
- Rust stable with Cargo;
- `uv` from Astral;
- PowerShell.

The Python environment uses Python 3.11 through `uv` for a conservative NeMo-compatible setup.

## First command to run

From PowerShell at the repository root:

```powershell
.\scripts\first-test.ps1
```

That command:

1. installs Python 3.11 through `uv` if needed;
2. creates the worker environment and installs CUDA PyTorch from the PyTorch CUDA wheel index;
3. rejects the device if CUDA is unavailable;
4. downloads NVIDIA's small public 16 kHz mono sample WAV only if it is not already cached;
5. runs a whole-file Nemotron STT smoke test;
6. runs the stateful cache-aware streaming path on the same file.

The cached sample is stored below `%USERPROFILE%\.cache\uvox\samples`.

## Run the desktop prototype

```powershell
.\scripts\run-dev.ps1
```

Then hold CapsLock while speaking into a normal text box. Release CapsLock to stop immediately and discard the unfinished tail.

Open the native Win32 settings window with:

```powershell
.\scripts\settings.ps1
```

## Useful commands

```powershell
# Validate Rust config, CUDA, and NeMo imports
.\scripts\doctor.ps1

# List microphone input device names
cargo run -p uvox -- list-inputs

# Capture five seconds through the Rust mic/resampler path
.\scripts\record-test.ps1

# Send a literal Unicode typing test after a two-second focus delay
.\scripts\type-test.ps1

# Run Python unit tests, Ruff, and Rust tests
.\scripts\test-all.ps1

# Build release executable
.\scripts\build-release.ps1
```

## Worker-only CLI

Run these from `worker/` after setup:

```powershell
uv run --no-sync uvox-worker doctor --check-nemo
uv run --no-sync uvox-worker fetch-sample
uv run --no-sync uvox-worker smoke-test
uv run --no-sync uvox-worker stream-file-test --lookahead-ms 80
```

The whole-file `smoke-test` is deliberately separate from `stream-file-test`. This makes it easier to identify whether a failure is caused by CUDA/model loading or by the stateful streaming path.

## Latency settings

Nemotron supports four lookahead values in this implementation:

| Lookahead setting | Effective audio chunk | Trade-off |
|---:|---:|---|
| `0` ms | `80` ms | Lowest delay, lower accuracy |
| `80` ms | `160` ms | Recommended starting point |
| `480` ms | `560` ms | More context |
| `1040` ms | `1120` ms | Highest latency, strongest context |

The default is `80` ms lookahead, meaning 160 ms model chunks.

## Important behavior

- Rust captures the microphone immediately on CapsLock down, even while the Python worker is loading.
- A bounded ring buffer retains recent audio during cold startup.
- Every dictation run receives a monotonically increasing session ID.
- Late model events from cancelled sessions are ignored.
- Only conservative stable prefixes are typed. Revisable partial text is not injected.
- Typing aborts if CapsLock is released or the foreground window changes.
- The worker has no CPU fallback. CUDA rejection is intentional.
- Text insertion uses Win32 `SendInput` with Unicode events. It cannot type into every restricted or elevated application.

## Repository map

```text
rust/                         Rust desktop manager
worker/                       CUDA-only Python NeMo worker
scripts/                      one-command PowerShell workflows
docs/                         architecture, protocol, research, debugging
.agents/skills/               coding-agent runbooks
AGENTS.md                     coding-agent entrypoint
THIRD_PARTY_NOTICES.md         attribution and model terms notes
```

## Development notes

Start with `AGENTS.md`. The narrowest debugging sequence is:

```text
Python unit tests
→ worker doctor
→ whole-file smoke test
→ streaming file test
→ Rust record-test
→ Rust type-test
→ live CapsLock run
```

See `docs/DEBUGGING.md` for failure isolation.

## License

The repository is released under GPL-2.0-only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.
