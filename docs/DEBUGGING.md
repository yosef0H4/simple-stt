# Debugging guide

## 1. Fast tests without CUDA

```powershell
.\scripts\python-unit-tests.ps1
cargo test --workspace
```

Python tests cover CUDA rejection with fakes, audio validation, cached download behavior, protocol framing, session cancellation, stable-prefix commits, and an echo-server round trip.

Rust tests cover JSON and PCM framing, config serialization, resampling, PCM assembly, focus gating, and immediate cancellation of a queued text send.

## 2. CUDA and NeMo environment

```powershell
.\scripts\setup-worker.ps1
.\scripts\doctor.ps1
```

The expected production behavior is rejection when `torch.cuda.is_available()` is false. Do not add a CPU fallback.

## 3. Whole-file model test

```powershell
cd worker
uv run --no-sync uvox-worker smoke-test
```

This downloads a known public sample once and calls NeMo's ordinary whole-file `transcribe()` API. If this fails, debug the CUDA or NeMo environment before debugging live streaming.

## 4. Cache-aware streaming file test

```powershell
cd worker
uv run --no-sync uvox-worker stream-file-test --lookahead-ms 80
```

This uses the same stateful `conformer_stream_step` path as live microphone input, but feeds a deterministic WAV file.

## 5. Rust microphone path

```powershell
cargo run -p uvox -- list-inputs
cargo run -p uvox -- record-test --seconds 5 --output recording-test.wav
```

Listen to the generated WAV or inspect it with an audio editor. It should be mono PCM16 at 16 kHz.

## 6. Rust text sender

```powershell
cargo run -p uvox -- type-test "Uvox literal Unicode typing test: héllo world."
```

Focus Notepad during the two-second delay. Some restricted or elevated apps can reject injected input by design.

## 7. Live app

```powershell
$env:RUST_LOG="uvox=debug"
cargo run -p uvox -- run
```

Hold CapsLock, speak, then release. Review logs for:

```text
microphone capture started
starting CUDA worker
CUDA worker is loading Nemotron
CUDA worker is ready
partial transcript
```

## Echo backend

For desktop-flow debugging without CUDA, set `worker_backend` to `echo` in the JSON config or settings GUI. This is a development-only backend; production defaults to `nemotron`.
