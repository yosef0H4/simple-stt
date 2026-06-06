# Architecture

## Design goals

- Record immediately on CapsLock down.
- Reuse a warm model when available.
- Free model RAM and VRAM after an idle timeout.
- Reject CPU-only execution instead of silently becoming slow.
- Keep the desktop manager native and lightweight.
- Make failures easy to isolate through CLI commands.

## Rust process

Rust owns all latency-sensitive desktop integration:

- low-level CapsLock hook;
- foreground-window tracking;
- microphone capture through CPAL;
- stereo downmix, gain, linear conversion to 16 kHz mono PCM16;
- bounded cold-start audio ring buffer;
- Python worker process lifecycle;
- authenticated loopback TCP connection;
- fixed-rate Unicode `SendInput` queue;
- native Win32 settings UI.

The Rust process does not perform ML inference.

## Python process

The Python worker is disposable and CUDA-only. It owns:

- CUDA validation through `torch.cuda.is_available()`;
- NeMo and Nemotron loading;
- encoder cache state;
- pre-encode feature cache;
- chunk accumulation;
- partial hypotheses;
- conservative stable-prefix commits.

The worker is spawned on first dictation and killed after the configured idle timeout.

## Why a ring buffer exists

Loading PyTorch, NeMo, CUDA libraries, and a 600M-parameter model cannot be made instantaneous. The Rust audio callback begins producing 20 ms PCM frames immediately and retains a bounded recent history until the worker reports `ready`. Rust then starts the session and drains the saved frames to Python.

## Cancellation rule

CapsLock release is authoritative:

1. clear Rust session state;
2. cancel the Rust text sender through shared cancellation state;
3. send `cancel` to Python if connected;
4. ignore late events because their session ID no longer matches.

The live path deliberately does not flush a final tail after release.

## Future work

- Add a small partial-transcript overlay.
- Add a tray icon using native Win32 controls.
- Persist latency metrics.
- Add a release installer.
- Consider named pipes after the loopback TCP implementation is proven stable.
