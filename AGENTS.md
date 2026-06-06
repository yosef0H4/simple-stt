# AGENTS.md

This file is the coding-agent entrypoint for Uvox.

## Mission

Keep Uvox a lightweight Windows-first local dictation utility:

```text
CapsLock press
→ Rust starts retaining microphone audio immediately
→ CUDA-only Python Nemotron worker loads or is reused
→ PCM16 frames stream over loopback TCP
→ Python emits partial and conservative stable-prefix commit events
→ Rust inserts committed Unicode text into the original focused app
→ CapsLock release cancels immediately and discards late output
```

Do not add browser-based desktop frameworks. Keep Rust resident and keep the Python ML process disposable.

## Safety and product rule

Do not add random delays, fake mistakes, or anti-detection behavior. Fixed configurable text pacing exists for normal UX and compatibility. Respect applications that reject injected input.

## First debugging commands

Run in this order:

```powershell
.\scripts\python-unit-tests.ps1
.\scripts\first-test.ps1
cargo test --workspace
cargo run -p uvox -- list-inputs
cargo run -p uvox -- record-test --seconds 5 --output recording-test.wav
cargo run -p uvox -- type-test "Uvox typing test."
cargo run -p uvox -- run
```

## Key modules

| Module | Responsibility |
|---|---|
| `worker/src/uvox_worker/cuda.py` | mandatory CUDA rejection |
| `worker/src/uvox_worker/nemotron.py` | whole-file and stateful cache-aware NeMo inference |
| `worker/src/uvox_worker/engine.py` | session cancellation, frame accumulation, stable commits |
| `worker/src/uvox_worker/protocol.py` | IPC framing shared conceptually with Rust |
| `rust/src/audio.rs` | CPAL input stream and frame delivery |
| `rust/src/resample.rs` | stereo downmix, 16 kHz linear resampling, PCM framing |
| `rust/src/hotkey.rs` | global low-level CapsLock down/up hook and suppression |
| `rust/src/input.rs` | Win32 Unicode `SendInput` |
| `rust/src/transcript.rs` | interruptible fixed-rate text queue |
| `rust/src/worker.rs` | worker spawn, authenticated loopback connection, IPC |
| `rust/src/app.rs` | lifecycle and state machine |
| `rust/src/gui.rs` | native Win32 settings form |

## Protocol invariants

- Every frame starts with one byte `kind` and a four-byte little-endian payload length.
- JSON is UTF-8 and uses kind `1`.
- PCM16 uses kind `2`; its payload begins with an eight-byte little-endian session ID followed by signed little-endian 16-bit samples.
- The Rust listener passes a random token to Python. Python echoes it in the first JSON `hello` event.
- Only events carrying the active session ID may affect typing.

## Testing strategy

Python unit tests intentionally avoid CUDA and NeMo imports through lazy loading and fakes. Keep them fast.

CUDA tests are explicit CLI integration commands. Never silently skip the CUDA check in production paths.

Rust tests should focus on platform-neutral logic where possible: config, protocol, resampling, framing, and transcript queue cancellation. Windows-only behavior must also be exercised manually with `record-test`, `type-test`, and `run`.

## Skills

- `.agents/skills/run-smoke-test/SKILL.md`
- `.agents/skills/debug-live-pipeline/SKILL.md`
- `.agents/skills/add-setting/SKILL.md`
