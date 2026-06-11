# Third-party notices

## AutoHotkey v2 and Ahk2Exe

The development shell requires AutoHotkey v2. Distribution may compile `ahk\simple-stt.ahk` into `simple-stt.exe` with the official Ahk2Exe tool. AutoHotkey and Ahk2Exe are not vendored in this source tree. Preserve their applicable notices when redistributing a compiled shell.

Official projects:

```text
https://www.autohotkey.com/
https://github.com/AutoHotkey/Ahk2Exe
```

## Rust dependencies

The Rust crate uses third-party packages declared in `Cargo.toml`, including `anyhow`, `clap`, `crossbeam-channel`, `dirs`, `rand`, `reqwest`, `serde`, `serde_json`, `tracing`, `tracing-subscriber`, and Windows-only `cpal`, `libloading`, and `windows-sys`. Generate a fresh Cargo lockfile and review dependency notices during the Windows release build.

## Native Parakeet runtime

Simple STT dynamically loads a local native Parakeet CUDA runtime only inside `simple-stt-infer.exe` from:

```text
external\parakeet-runtime\parakeet-windows-cuda
```

That runtime bundle is a local artifact and is intentionally not committed. Preserve the runtime's license and notice files when redistributing it.

## NVIDIA Parakeet model

The default model path is:

```text
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

Model usage is governed by the publisher's terms. GGUF files are not committed.

## Public sample audio

The retained smoke-test fixture is based on a public LibriSpeech sample:

```text
fixtures\parakeet-smoke.wav
```
