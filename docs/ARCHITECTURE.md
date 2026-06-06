# Architecture

## Design Goals

- Record immediately on CapsLock down.
- Transcribe only after CapsLock release.
- Reuse a warm native Parakeet engine when available.
- Free model memory after an idle timeout.
- Reject missing CUDA/runtime files clearly.
- Keep the app native Rust without Python or C# helpers.

## Runtime Flow

Rust owns the full product path:

- low-level CapsLock hook;
- foreground-window tracking;
- microphone capture through CPAL;
- stereo downmix, gain, and 16 kHz mono PCM16 resampling;
- completed recording buffer;
- native `parakeet.dll` loading through `libloading`;
- fixed-rate Unicode `SendInput` queue.

On CapsLock down, Rust starts a new recording and loads Parakeet if needed. Audio frames continue to accumulate while the model loads. On CapsLock release, Rust sends the complete clip to the native C API, then types the transcript only if the foreground window still matches the original target.

## Runtime Artifact

The CUDA runtime and model are local ignored artifacts:

```text
external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

Config may override these paths. Missing files are treated as configuration errors.

## Cancellation and Focus

CapsLock release ends recording and starts transcription. The resulting text is typed only for that recording's session and only into the original focused window. If focus changes, the queued text is cancelled.
