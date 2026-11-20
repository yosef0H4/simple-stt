# Current behavior inventory before the overhaul

This inventory was recorded from the archived monolithic source. The working tree now contains only the split architecture; use git history if you need to recover the removed monolith files.

## Old executable shape

The root package built one resident Rust application. `src/main.rs` owned CLI dispatch, the tray utility, Slint settings, low-level keyboard hook, foreground-window checks, typed-text injection, CPAL microphone capture, overlay rendering, native Parakeet FFI, and idle DLL unload. A duplicate Rust tree also existed under `rust/`; both trees are now removed from the working tree to keep the active build unambiguous.

Old CLI commands:

```text
run
settings
config-show
config-reset
list-inputs
transcribe-file --audio <wav>
ui-screenshot --surface settings|overlay|overlay-desktop --output <png>
tooltip-bench --iterations <n>     (hidden)
```

## Old tray commands

The Rust tray menu exposed:

```text
Open Settings
Disable / Enable Hotkey
Reload Config
Open Log
Unload Model
Test Model
Exit
```

The new shell preserves these behaviors and adds `Restart Audio Service`; labels are adjusted to the requested wording.

## Old persisted config and defaults

The old JSON was stored at the platform config directory as `simple-stt/config.json`, overrideable through `SIMPLE_STT_CONFIG`. It had no explicit schema version.

| Old field | Old default | Notes |
| --- | --- | --- |
| `idle_timeout_secs` | `180` | In-process Parakeet-context unload timeout. |
| `typing_interval_ms` | `20` | Delay between typed chunks. |
| `typing_chunk_chars` | `3` | Unicode chunk size. |
| `audio_gain` | `1.0` | Applied before PCM conversion. |
| `audio_device_contains` | `""` | Empty chooses default microphone; otherwise case-insensitive substring matching. |
| `parakeet_runtime_dir` | `external\\parakeet-runtime\\parakeet-windows-cuda` | Runtime root. |
| `parakeet_model_path` | `...\\models\\tdt_ctc-110m-f16.gguf` | Full selected model path. |
| `start_with_windows` | `false` | Old Rust startup registration. |
| `hotkey_enabled` | `true` | Runtime enable/disable. |
| `record_hotkey` | `capslock+s` | Configurable hold chord. |
| `capslock_always_off` | `false` | Optional forced-off behavior. |
| `log_level` | `minimal` | `minimal`, `normal`, `debug`, or `extreme`. |

Schema-v2 migration maps this to `idle_worker_timeout_secs`, `model_dir`, `selected_model_filename`, and `capslock_behavior`. A backup is written before migration.

## Old hotkey semantics

The old Rust low-level keyboard hook parsed one non-modifier key plus modifiers. Supported modifier classes included CapsLock, Ctrl, Shift, Win, left Alt, and right Alt. It filtered injected low-level events, recognized key-down and key-up, and had special Caps Lock handling. Default behavior was hold `CapsLock+S` to record and release the final key to stop. The old README text had drifted and sometimes described holding CapsLock alone; source behavior is authoritative.

The overhaul moves all user-facing binding and tap behavior into AHK v2. Manual scripts cover CapsLock tap, CapsLock+S hold/release, generic and left/right modifiers, AltGr, reassignment, enable/disable, and generated typing.

## Old audio behavior

The useful audio path is preserved in `src/capture/audio.rs`:

- CPAL default host and default microphone when no match string is configured.
- Optional case-insensitive device-name substring selection.
- Default input stream config from CPAL.
- Accepted device formats: `f32`, `i16`, and `u16`.
- Downmix all configured channels to mono.
- Apply configured gain.
- Linear resampling to `16_000` Hz.
- Emit `320` PCM16 samples per frame, which is `20 ms` at 16 kHz.
- Compute a smoothed/expanded RMS-derived visualizer level from each emitted frame.
- Buffer frames only during active recording in the new service.

The old audio channel capacity was `4096` frames; the new capture service keeps that value.

## Old overlay behavior

The old Rust overlay rendered a rapidly updating Win32 tooltip/overlay driven by an atomic latest-level value. This is intentionally kept in Rust because it updates far more frequently than the AHK shell should. The new wrapper introduces explicit primary states and transient notices instead of arbitrary UI strings.

## Old typing safety behavior

The old Rust typist remembered the foreground HWND at recording start. Before each typed chunk it checked that the current foreground HWND still matched. On mismatch, it rejected typing. It inserted text through a normal Windows text sink at configurable chunk size and delay.

The shell now owns this behavior and sends literal Unicode with AHK `SendText()` without clipboard use. It queues rapid transcripts by session and repeats the foreground check before every chunk.

## Old model behavior

The historical behavior below is not the initial setup of the current app:
new installations start with no selected dictation model or native runtime.

The monolith loaded `parakeet.dll` and the selected GGUF model directly in its own resident process. The idle watcher requested `UnloadParakeetContext` every five seconds once the configured timeout had elapsed. This could free native context and VRAM while leaving allocator/process RAM resident.

The new guarantee is stronger: only `simple-stt-infer.exe` loads Parakeet. Cleanup is verified by worker process exit rather than trusting DLL unload.

The old app had:

- an approved model catalog;
- HTTPS downloads from a Hugging Face repository;
- `.partial` writes followed by rename;
- WAV smoke-test transcription;
- model selection in Slint settings.

The new service preserves those capabilities. Checksums remain a known limitation because authoritative per-file checksums were not bundled in the repository.

## Old startup and logs

`src/startup.rs` modified a per-user Windows Run registry value. The new shell uses an `A_Startup` shortcut that launches `simple-stt.ahk` through AutoHotkey v2 in development and launches the packaged `simple-stt.cmd` in distribution.

The old logger wrote one `latest.log`. The new architecture writes append-style component logs:

```text
simple-stt-shell.log
simple-stt-capture.log
simple-stt-infer.log
```

Transcripts are not logged by default.

## Optional AI cleanup

The current split architecture can optionally clean a completed transcript before the shell applies text transforms and delivery. This work belongs to `simple-stt-capture`, runs off its control thread, and does not change the Parakeet isolation boundary. OpenAI-compatible credentials and ChatGPT subscription tokens are stored outside portable `config.json` in the operating-system credential vault. Cleanup is disabled by default, falls back to the raw transcript on every error or timeout, and keeps only five comparison pairs in process memory.

Screen context is a separate privacy opt-in. Windows captures the HWND remembered at recording start, X11 uses the active window, and Wayland requests a compositor-owned portal selection for each dictation. The image is resized, JPEG-encoded, sent with one request, and never written by Simple STT. Password-manager names seed the local denylist.

## Old utilities and tests

Archived utilities:

- deterministic settings/overlay screenshot rendering;
- hidden tooltip benchmark;
- WAV transcription smoke test;
- `tests/idle_unload_integration.rs`, which observed in-process unload behavior;
- `tests/fixtures/parakeet-smoke.wav`.

The original fixture was preserved and copied to active install-relative `fixtures/parakeet-smoke.wav` so packaged model tests do not depend on a source checkout. The obsolete idle-unload integration is archived and replaced by process-exit validation scripts.

## Missing runtime files and environmental fixtures

The repository intentionally does not bundle the large native Parakeet runtime or GGUF models. The following are expected externally:

```text
external\\parakeet-runtime\\parakeet-windows-vulkan\\bin\\parakeet.dll
external\\parakeet-runtime\\models\\tdt_ctc-110m-q8_0.gguf
external\\parakeet-runtime\\models\\lemura-arabic-asr-lite-q8_0.gguf
```

A Windows target, AutoHotkey v2 for development, Cargo/Rust, CPAL-compatible microphone device, and the Vulkan Parakeet runtime are required for Windows end-to-end validation. Linux dictation requires an installed CUDA or Vulkan runtime and an assigned model.

## Shortcut reminders

Assigned shortcuts appear as a compact localized list beneath active overlay
states and notices. General → Shortcuts → Shortcut reminders disables this list
(default: on). The Show shortcuts button displays the list for eight seconds.
On Windows, hold the assigned shortcut to see the hints, even when reminders
are disabled. Caps Lock is the default and a quick tap just toggles capitalization.
A shortcut used while Caps Lock is held preserves the previous capitalization
state. Show Shortcuts is the only action that accepts Caps Lock alone. KDE/Wayland
assignments remain desktop-owned. Unassigned actions are omitted. Linux uses
registered portal/X11 trigger descriptions, never suggested config defaults;
compositor command bindings are not discoverable and are not invented.

Retry registers a delivery session before waiting for shortcut release. The
overlay keeps “Release keys” visible while the shortcut is held, then shows the
effective Typing/Pasting state. Completion and failure clear the active state;
late progress or completion cannot replace newer work.

Overlay shortcuts use separate keycaps, such as [Meta][Ctrl][.], with standard
Unicode emoji for recording, cancel, delivery, cleanup, retry and shortcut help.
Linux uses the desktop's emoji font and draws only keycap outlines. Native
Windows tooltips use the same compact emoji shortcut rows and bracketed
keycaps. Localized status text stays readable in Common Controls. The Windows tray and
installer use the same microphone artwork as the Linux launcher. Windows
tray menus display color emoji as native item icons.

The Linux base package leaves speech/OCR runtime and model installation to Settings → Downloads; it bundles the compressed fastText language-identification model. CUDA/Vulkan dictation and the integrated Kokoro/Supertonic/PaddleOCR reading routes use separate optional assets. Dictation defaults to keyboard-language routing, reading defaults to automatic text-language routing, and OCR defaults to a single Paddle selection. Reading settings use the existing explicit Save bar; the main shell owns reading shortcuts and tray actions. Reading workers, API providers, OCR, clipboard listening, chunk navigation and cancellation are adapted from Simple TTS under `reader/crates/`, without depending on that checkout.

Mode-based assignments show one editor for the selected language, with explicit assignments only and no general fallback. Voice speed is local to the assignment; background timeouts remain shared per module. The default recording finish delay is 500 ms. Normal completed work stays warm until its configured independent idle timeout; memory estimates do not evict a just-used worker. The local reading tooltip shows the current prepared spoken passage, while notification privacy remains separate.

## Recorded actions and preparation overlap

Read-along can record physical key presses and releases in a focused Settings
panel. Esc or Stop finishes after release; leaving the panel cancels. The first
event starts at zero and no idle time follows the final release. Existing saved
keys remain valid, and reset restores a single Space press. Actions are bounded
to 128 events and 30 seconds. Linux replays through its authorized RemoteDesktop
EIS session; Windows replays through the AHK v2 helper with target identity and
focus checks, cancellation, and held-key cleanup.

With prefetch enabled, an initial captured image is advanced while its OCR and
speech start. The settling timer starts after the final recorded key release and backend
completion. Exactly one next page may be captured and prepared while the
current page plays. A ready page waits for current playback to finish; only then may another
advance begin. Processing failures retry within the configured limit, without
repeating an advance action whose delivery may already have occurred. An
exhausted attempt clears next-page activity, finishes current audio, and pauses;
Resume or Next retries that pending page, retaining OCR text if only speech
failed. Disabling prefetch uses sequential preparation.
OCR and speech keep their independent local/remote routes. Captures and actions
always remain local to the client. New server configurations listen on all IPv4
interfaces when enabled and still require a valid bearer token.

Client language detection stays local. TTS language assignments and dictation
keyboard-language assignments can independently select a remote URL, model,
voice, credential slot, alternate server, and local fallback assignment. A local
route preserves dormant connection fields. Credentials remain in the secure
store; configuration files contain only slot references. Replacing or clearing
credentials invalidates remote voice workers before their next request.

The server exposes installed Supertonic 3 as `supertonic3` in the TTS model
catalog, with `sid:0` through `sid:9` voices. Explicit speech model requests select
the matching installed engine rather than reusing an unrelated default model.

Read-along Repeat always captures the current contents of its last saved area,
including during active playback. It cancels the previous pipeline and reads
the full read-along pipeline there without reselecting the area. Its initial
capture precedes its first advance, just like a fresh run. Pressing the
read-along shortcut again stops active reading or pending region selection.
Pending-page speech becomes playable on its first completed chunk; subsequent
chunks use a one-slot cancellable queue, retaining sentence/unit tags and gaps.
Current playback has synthesis priority. Producer completion and audio drain
both gate the next promotion. Failed uncommitted chunks retry within the limit,
then wait for Resume/Next without replaying spoken chunks or discarding the
pending page. Long pauses resume the existing stream rather than starting a
second speech job. Chunks and total page audio retain explicit duration bounds.
Reading settings include a saved case-insensitive literal word/phrase exclusion
list. Matches respect Unicode word boundaries, apply before model cleanup for
both local and remote speech, and leave the original OCR text unchanged. Empty
spoken chunks are skipped; an entirely excluded pending page pauses explicitly.

The shared reading HUD shows `current/total` beside the current chunk for
normal reading and read-along, including paused playback. Background next-page
OCR updates its own status without replacing current playback activity. Failed
new captures do not publish a superseded session as live. Resume, Next or Repeat
can retry an initial failed read-along capture at its saved area. Structured
provider rejection messages are bounded and redact credentials and image data.

Read-along separates action replay, the remaining settling countdown, capture,
OCR, and speech preparation in the HUD. Short ambiguous speech chunks inherit
the whole page route, using the same unit routing as ordinary reading. A selected
monolingual server model fixes its default language instead of redetecting every
client chunk; an explicit request language still undergoes capability validation.

The HUD shows the playing chunk (`1/6`) and a simple pending-page phase, with a
remaining countdown during settling. Generation/queue metrics remain available
in status diagnostics and are omitted from the compact HUD.

Linux desktop input retains separate keyboard and pointer connections for their
authorized capabilities. Each owns an explicit, bounded Session.Close lease: a
failed setup, canceled action, broken EIS connection or replacement closes its
portal session. CreateSession obtains its handle before honoring cancellation
so cleanup can close a session created during that race. Healthy sessions remain
reusable, and saved restore tokens remain separate from live sessions.

Read-along reports numbered retries for speech preparation as well as OCR. Empty
provider audio is a retryable speech failure, while OCR text removed entirely by
cleanup is recaptured within the bounded OCR retry loop without another advance.
After retry exhaustion, current audio drains and the run ends; the final notice
expires after five seconds and Repeat retains the saved area.

The shared retry shortcut is labeled “Retry last action” in Settings, desktop
shortcut registration and CLI guidance. Its existing `retry` ID and saved chord
remain unchanged.
