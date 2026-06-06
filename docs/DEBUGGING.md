# Debugging

## Fast Checks

```powershell
.\scripts\check-prereqs.ps1
.\scripts\test-audio.ps1
cargo test -p uvox
cargo run -p uvox -- list-inputs
```

## Audio File Test

Run:

```powershell
cargo run -p uvox -- transcribe-file --audio tests\fixtures\parakeet-smoke.wav
```

Expected transcript anchor:

```text
Well, I don't wish to see it any more
```

The Parakeet runtime should log CUDA device selection. If the runtime or model is missing, fix `parakeet_runtime_dir` or `parakeet_model_path` in config.

## Live App Test

Run:

```powershell
.\scripts\run.ps1
```

Focus a normal text box, hold CapsLock while speaking, release CapsLock, and wait for the transcript to type. Use `RUST_LOG=uvox=debug` for detailed recording, loading, transcription, focus, and idle-unload logs.

## Common Failures

- Missing `parakeet.dll`: extract the native runtime bundle to the expected `external` path.
- Missing model: verify `tdt_ctc-110m-f16.gguf` exists under the runtime `models` directory.
- No typing: make sure the target app accepts `SendInput` and is not elevated above Uvox.
- Wrong microphone: run `cargo run -p uvox -- list-inputs`, then set `audio_device_contains`.
