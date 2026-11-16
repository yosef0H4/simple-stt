# Testing matrix

## Pre-commit validation

Run validation for the current platform before committing code changes. On both
Linux and Windows, run formatting, Clippy, Rust tests, Settings API tests, cleanup
E2E, static verification, and IPC tests:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --bin simple-stt-settings
python scripts/test-settings-single-instance.py
python scripts/test-settings-upgrade.py
python scripts/test-settings-selection-api.py
python scripts/test-cleanup-settings-e2e.py
python scripts/verify-static.py
python tools/ipc-poc/test_poc.py
```

Build the Settings frontend before any Cargo build. Release packaging and the
full-suite scripts run this automatically. Linux uses `scripts/test-full.sh`
(with Playwright available). Desktop builds use
`scripts/package-linux-release.sh fedora` or `arch`; the legacy
`scripts/build-linux-release.sh` now forwards to that packaging workflow.
Install with DNF/Pacman before any real desktop run or app restart. Checkout
binaries are allowed only in isolated tests. During targeted
development, run `cd web/settings && npm ci && npm run build`. The build writes
`dist/index.html`, `dist/app.js`, `dist/styles.css`, and a deterministic
`dist/manifest.json`. `python scripts/verify-settings-frontend.py` checks source
and output hashes plus the 100 KiB gzipped JS/CSS and 300 KiB total asset budgets.
The Rust build script independently rejects missing or stale frontend output.

On Linux, also run `python scripts/test-linux-static.py`. Run the browser model
selection regression for Settings changes and the relevant real-device tests
below for keyboard routing or inference changes. Cross-compile and run Clippy
for Windows-specific changes when the Windows target toolchain is available.

On Windows, the authoritative native validation command is:

```bat
scripts\test-full.cmd
```

The command runs:

```text
cargo test --all-targets
cargo build --bin simple-stt-settings
python scripts\test-settings-selection-api.py
python scripts\test-cleanup-settings-e2e.py
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
node scripts\test-settings-model-selection.cjs
node scripts\test-settings-overhaul.cjs
node scripts\test-settings-contracts.cjs
scripts\test-ahk-full.cmd
```

The AutoHotkey portion rebuilds current release binaries first, so runtime smoke tests cannot accidentally validate stale executables.

If a Windows host is unavailable, report native Windows runtime checks as
unvalidated. They do not block committing changes validated on Linux.

The cleanup E2E launches the real `simple-stt-settings` process against a
deterministic OpenAI-compatible HTTP server, then exercises bootstrap and
**Test cleanup** across the authenticated browser API. Debug-only provider
values can be supplied through the ignored `.env` file documented by
`.env.example`.

## Full-suite coverage

The combined validation suite covers:

```text
Rust unit tests
real-child-process worker lifecycle integration tests
nested schema-v9 normalization and malformed-file preservation
AI cleanup provider response parsing, Unicode preservation, and raw-text fallback
AI credential separation from portable configuration
install-relative runtime path behavior
Windows Common Controls v6 manifest embedding
loopback authenticated IPC
Unicode transport and malformed protocol rejection
state-file reconnect after simulated service restart
AHK v2 load validation for all shell and test entry points
authenticated browser Settings load/save and external-edit conflict detection
typed delivery and foreground mismatch cancellation
punctuation removal and lowercase transcript transforms
Windows response-file sharing-violation retry handling
real Parakeet model-test transcription
worker unload and PID disappearance
recording-start model prewarm while recording remains active
capture-service restart and reconnect
Ctrl+V paste delivery
Ctrl+Shift+V paste delivery
restoration of a custom non-text clipboard format
```

The end-to-end smoke uses isolated temporary config and state files. It does not overwrite `%APPDATA%\simple-stt\config.json` or reuse the live shell discovery file.

The Windows AHK full smoke also uses a controlled edit window to switch the foreground thread between installed English and Arabic keyboard layouts. It checks the recording-start language event, runs the Arabic model test, and restores the original layout. The native inference check runs both English and Arabic models on CPU and Vulkan:

```powershell
python scripts/test-inference-devices.py --mode both --language both
```

The saved Arabic fixtures include four synthetic prompts and three human FLEURS clips. Conversion parity was checked locally before publishing the finished Q8 GGUF. Checkpoints, conversion environments, and NeMo/F16 comparison scripts are not part of this repository or required for installation.

For experimental memory and latency comparisons across 50 alternating switches:

```powershell
python scripts/benchmark-language-switch.py --switches 50
```

On Linux, run `python scripts/test-linux-static.py` alongside the normal Rust tests. Follow-keyboard selection supports KDE Plasma Wayland and X11. Fixed English and Arabic modes work on all supported Linux desktops. Install the Linux Vulkan runtime and both models with `bash scripts/bootstrap-linux-vulkan.sh`, then run `python scripts/test-inference-devices.py --mode both --language both`. To check a live layout, run `SIMPLE_STT_TEST_KEYBOARD_LANGUAGE=english cargo test --lib live_keyboard_language` (or `arabic` after switching layouts).

## Run Rust tests only

```powershell
cargo test --all-targets
```

Included Rust unit coverage:

```text
audio mono/downmix/resampling/frame behavior
preferred microphone fallback/return, Windows endpoint-event retry policy, and default tracking
shell JSON Unicode and malformed JSON
escaped helper protocol Unicode/control-character round trip
worker framed protocol PCM and Unicode transcript framing
protocol-version and malformed-size rejection
schema-v9 normalization, unknown-field removal, and malformed-file preservation
AI cleanup defaults, nested invalid-value recovery, OAuth PKCE, and Codex SSE parsing
approved model-name restriction
lazy launch / warm reuse / model replacement / idle policy
install-relative runtime root behavior
worker logging-level propagation
stationary compact Unicode waveform behavior
new-recording supersession and delivery-completion acknowledgement
```

`src/bin/simple_stt_mock_infer.rs` is a deterministic test-only worker. `tests/worker_lifecycle.rs` launches it as a real child process and covers:

```text
lazy worker launch
warm worker reuse before timeout
idle process exit
worker recycle after model switch
worker crash discard and recovery
blocked inference exact-PID termination fallback
Unicode transcript transport across child pipes
```

The release build script names `simple-stt-capture`, `simple-stt-infer`, `simple-stt-ctl`, and `simple-stt-settings`, so the mock binary is not staged.

## Run source and IPC checks only

```powershell
.\scripts\test-static.ps1
```

If local PowerShell execution policy blocks unsigned scripts, invoke the underlying checks directly:

```bat
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
```

The static verifier checks architectural invariants that are easy to regress during refactors:

```text
AHK v2 directives on every executable entry point
split Rust binary structure
Parakeet DLL isolation in simple-stt-infer
Common Controls v6 manifest embedding for modern tooltips
loopback-only versioned control IPC
framed Rust-to-Rust PCM protocol
clipboard-preserving paste implementation
response-file sharing-violation retry
recording-start worker warm-up
schema normalization, atomic writes, authenticated Settings, and download hygiene
mock-worker exclusion from release packaging
```

The IPC proof of concept validates:

```text
authenticated loopback PING/PONG
START_RECORDING / STOP_RECORDING
asynchronous polled Unicode TRANSCRIPT
state-file reconnect after simulated service restart
```

## Browser Settings coverage

`node scripts/test-settings-modules.cjs` checks the empty first install, keyboard routing default, compatible installed choices, download queue and filters, and a narrow Arabic layout using an isolated fixture.

`node scripts/test-settings-shortcut-portal.cjs` checks the visible English and
Arabic shortcut setup action, pending/error behavior, and 360/1280px screenshots
in both themes using an isolated HTTP fixture. Installed-package smoke also
checks the portal desktop identity and real Settings-to-shell request/result
transport with a fake responder and an unavailable session bus. Native portal
dialog appearance remains a manual installed-app check.


`python scripts/test-settings-single-instance.py` launches the real Settings
binary concurrently and checks one shared server, authenticated Close from a
reopened session, immediate reopen during shutdown, crash/stale-session recovery,
and independent installation isolation. On Linux it also suspends the server to
verify a slow/unresponsive instance never starts a duplicate. The OS lock remains
held throughout the server lifetime; its persistent lock file is intentionally
not deleted. Closing a browser tab alone does not close the server: use the
Settings Close action to shut it down for all tabs.

Browser automation covers Settings pages, including AI Cleanup, narrow and wide layouts, accessible labels, explicit Save, Reset/Import previews, security headers, offline editing, device/model controls, provider model search/manual IDs, credential-state controls, and download progress. Release validation additionally starts the capture service, downloads a real catalog model through the UI, waits for completion, selects and saves it, and runs the real smoke-audio model test. The routine CI download uses a deterministic local fixture server; the release pass uses the production catalog and model URL.

Provider tests use deterministic loopback fixtures and must never use a developer's saved API key. A real provider test is manual, requires a freshly issued credential supplied through the OS vault or `SIMPLE_STT_AI_API_KEY`, and must verify that a failed or timed-out request delivers the original transcript. Screenshot testing must also verify the visible capture notice, denylist behavior, active-window targeting, and that no image or history survives capture-process exit. Wayland's compositor-owned picker remains a manual desktop check.

For an opt-in quality check against the provider configured in the ignored local `.env`, run:

```powershell
python scripts\benchmark-cleanup-live.py
```

The benchmark uses the real debug Settings process and scores several unrelated ASR-error patterns plus a preservation control. It prints provider output for review and is intentionally excluded from deterministic validation because model output and external service availability can vary.

## Linux X11 shortcut E2E

Clipboard delivery regression: `python scripts/test-linux-paste.py` creates a
disposable Xvfb server and controlled Tk editor, runs the production Linux paste
path with immediate, 800 ms and 2500 ms clipboard reads, and checks exactly one
Unicode insertion and retained clipboard content. It requires Xvfb, xclip,
xdotool and Python Tkinter, and never uses the live desktop clipboard. The Rust
clipboard tests also cover delayed publication, failure, cancellation, timeout,
stalled readers and user-copy changes. Windows full smoke covers delayed reads,
full-format restoration after verified insertion, new user copies and unknown
targets retaining the transcript; these checks require native Windows.

The native X11 shortcut listener has an opt-in end-to-end test. It needs `Xvfb`, `xdpyinfo`, and `xdotool`:

```bash
Xvfb :99 -screen 0 1024x768x24 -nolisten tcp
DISPLAY=:99 SIMPLE_STT_X11_E2E=1 cargo test --bin simple-stt-linux x11_global_shortcut_end_to_end -- --nocapture
```

The test registers `Meta+Z` through the production X11 parser and passive-grab path, synthesizes the chord with `xdotool`, and requires the corresponding X11 key event. The ordinary test suite skips external X-server interaction unless `SIMPLE_STT_X11_E2E` is set.

## Run AutoHotkey validation and runtime smoke only

```bat
scripts\test-ahk-full.cmd
```

The runner resolves AutoHotkey v2 from:

```text
%ProgramFiles%\AutoHotkey\v2\AutoHotkey64.exe
%ProgramFiles%\AutoHotkey\v2\AutoHotkey.exe
```

It first runs load-time validation with:

```text
/ErrorStdOut=UTF-8 /Validate
```

That sends AHK load errors to stderr instead of opening modal GUI error dialogs.

Validated entry points:

```text
ahk\simple-stt.ahk
ahk\tests\hotkeys-manual.ahk
ahk\tests\ipc-smoke.ahk
ahk\tests\typing-smoke.ahk
ahk\tests\text-transform-smoke.ahk
ahk\tests\tabprotocol-retry-smoke.ahk
ahk\tests\full-smoke.ahk
```

Runtime smoke scripts:

```text
hotkeys-manual.ahk
  parser and runtime binding smoke

typing-smoke.ahk
  typed queue behavior, foreground mismatch cancellation, and completion acknowledgement

text-transform-smoke.ahk
  punctuation removal, lowercase conversion, and combined transform

tabprotocol-retry-smoke.ahk
  reproduces an exclusive Windows file lock and verifies bounded read retry

ipc-smoke.ahk
  authenticated capture-service ping and graceful shutdown

full-smoke.ahk
  isolated capture service, microphone/model listing, real model test,
  unload verification, recording-start prewarm, restart/reconnect,
  one hello world typed check, Ctrl+V paste, Ctrl+Shift+V paste,
  and custom non-text clipboard-format restoration
```

The typing and paste checks intentionally use only `hello world` in controlled temporary edit boxes. Do not turn them into large keyboard simulations.

## Manual release checklist

`python scripts/test-recording-worker-reuse.py` uses the real capture service and
microphone with an isolated copy of the test-only worker (never delivered through
the desktop shell). Build `simple-stt-capture` and `simple_stt_mock_infer` in debug
first. It checks interruption of a blocked transcription, replacement warm-up
during the new recording, repeated ready-worker reuse without model notices,
and rapid model switches with no obsolete transcript. The Rust lifecycle tests
also verify readiness resets after idle exit and warm-up stays skipped after a
successful real request. The KDE routing test checks real English/Arabic models
and shared-model reuse through `model_reused` events.

Automation does not replace a final desktop pass. Before publishing a release, manually verify:

```text
physical microphone capture and transcription
CapsLock+S hold and release
plain Caps Lock tap
extra modifiers, left/right Ctrl, left/right Alt, and AltGr layouts
rapid repeated dictations
runtime hotkey reassignment
disable and re-enable hotkey
foreground-window mismatch prevents delivery
Unicode transcript delivery
capture-service restart and reconnect
AHK GUI responsiveness during model test/download/transcription
tray Reload App action
tray commands and stateful enable/disable label
startup shortcut creation and removal
microphone selection followed by audio-service restart
compact stationary waveform placement near the cursor on each monitor
model loading, loaded, and unloaded tooltip notices
RAM and VRAM cleanup after explicit unload and idle timeout
release packaging with build-distribution.cmd
```

For memory-specific measurements, see `docs/memory-cleanup-validation.md`.

Linux desktop regression: `python scripts/test-linux-language.py --switch-layouts` briefly switches configured KDE English/Arabic layouts, verifies recording-start selection through authenticated capture IPC, tests both real models, unloads the worker, and restores the layout. For NVIDIA verification use `python scripts/test-inference-devices.py --mode both --language both --expect-gpu "NVIDIA GeForce RTX 3050 Ti"`; it requires the native selected-device log and an exact worker PID with NVIDIA VRAM allocation.

Model selection regressions: `python scripts/test-settings-selection-api.py` exercises the real Settings HTTP server's current-format normalization, nullable selections, authenticated read-only discovery, unavailable Wayland, Save, and draft-only reset/import. Build the debug Settings binary first with `cargo build --bin simple-stt-settings`. `node scripts/test-settings-model-selection.cjs` exercises the browser UI using Playwright; install Playwright or set `NODE_PATH` to the bundled Node packages. It tests searchable mouse/keyboard selection, deterministic matches, preserved None, unavailable files, saved removal guards, download events, Save/Reset/import, and narrow layouts.

The Linux language regression also checks silent skipped starts, real shell toggle/stop state and clipboard preservation, and worker PID reuse when English and Arabic share a model. The Windows full smoke checks skipped starts and explicitly selects its installed fixture models; run `scripts\test-full.cmd` when validating on Windows. Windows cross-compilation verifies build compatibility but does not replace native keyboard, AHK, or Vulkan desktop tests; report those checks as unvalidated when no Windows host is available.


Settings overhaul browser validation: `node scripts/test-settings-overhaul.cjs`
covers every page, cross-page drafts, Save/Reset, shortcut capture, delivery
cycles and app overrides, cleanup testing, credential/OAuth actions against
local fixtures, metadata search, JSON editing/export, conflicts, offline editing,
and Linux-specific controls. `node scripts/test-settings-contracts.cjs` checks
TypeScript contracts against the real Rust API and loads the compiled UI under
its CSP. These scripts require Node with TypeScript type stripping support
(Node 24 is tested) and Playwright through `NODE_PATH` or an installation.

For release-asset performance comparisons run `node scripts/benchmark-settings.cjs`
with `SETTINGS_WEB_ROOT` pointing to baseline assets, then current assets. Use
`SETTINGS_BENCHMARK_OUTPUT` to retain measurements and
`SETTINGS_BENCHMARK_BASELINE` on the current pass to enforce startup/interaction
and heap budgets. Linux Settings-server RSS comparison uses
`python scripts/benchmark-settings-server.py --baseline PATH --current PATH`.
These tests use isolated configurations and never modify the running app's
settings. Browser heap measures only the tab's JavaScript; shared browser engine
RAM is separate. Background shell/capture code is outside this UI overhaul.

### Retry and clean clipboard delivery

Historical X11 direct-insertion coverage (retired with portal-only injection): `scripts/test-linux-paste.py`. Use `test-portal-input-e2e.py` for current input. The original suite ran after building `simple-stt-linux` and running `python scripts/build-linux-fast-paste.py`. The wrapper invokes `test-linux-clean-clipboard.py` under an isolated X server and accessibility bus, never the user's desktop; the helper must not be run standalone. In addition to delayed paste consumption, these check real Unicode direct insertion while preserving a custom binary clipboard format; large X11 INCR transfers and history-exclusion hints; yielding to user copies; and the real retry CLI against an authenticated capture fixture. Retry must use exact cached final text in the current editor, never retransform it, never start recording/inference, never persist the cache, and leave ongoing work alone when no cache exists. Option-off must leave an ordinary copy with no exclusion hint.

The retry fixture holds Meta/Ctrl on that private display and requires the CLI to wait, then insert exactly once after release. It asserts progress, submission, empty-cache and failure notices. Rust tests also cover arming on press, quick release, duplicate press/release events, and accepting the next deliberate retry. The wrapper removes inherited Wayland/accessibility connections and uses a private runtime directory and D-Bus session. Render the English/Arabic retry notices with `overlay-e2e --offscreen`; live KDE portal timing still requires a user desktop pass.

Rust tests exercise bounded cache replacement, stale/cancelled/empty writes, and schema-9 defaults/explicit option-off. Settings browser regressions exercise shortcut capture and Save/reload of the clean option. Windows full smoke checks delayed Edit consumption, full-format restoration, single-character Unicode publication, marker DWORD values and option-off retention. Native Windows AHK checks require Windows; cross-builds do not replace them.

### UI localization

Run `node scripts/test-settings-localization.cjs` with Playwright available. Set `SETTINGS_VISUAL_REVIEW=1` to retain English/Arabic light/dark captures at 360/768/1280px, including RTL, draft language changes, dropdowns, search, and mixed identifiers. The suite also checks 200% zoom. Run `cargo run --bin overlay-e2e -- --offscreen` for isolated PNG rendering with the production Pango/Cairo font, bidi, measurement and drawing paths under `artifacts/localization/offscreen`. For an explicitly requested desktop check, run `cargo run --bin overlay-e2e` inside KDE Wayland with Spectacle to capture the production overlay's Arabic loading/ready/unloaded, multiline, mixed-direction, and recording states under `artifacts/localization/native`. These are screenshots of a real compositor surface, not browser approximations. Inspect joining, clipping, alignment, waveform order, and font fallback. Native Windows/AHK validation still requires `scripts\test-full.cmd` on Windows.

## Isolated Wayland clipboard restoration

Build `simple-stt-linux` and the native paste helper, then run:

```sh
python scripts/test-linux-unicode-input.py
```

The harness creates a private D-Bus session, XDG home/runtime and virtual KWin compositor. It never sends input or clipboard requests to the user's desktop. It exercises the production clipboard transaction and paste helper against real GTK and Kitty targets, compares original text, custom binary and PNG formats after restoration, and covers Retry routing and selection replacement. The earlier rejected native keysym experiment's Kitty wire-order evidence is retained under ignored `artifacts/linux-unicode-failure/`.

The Wayland harness also offers 37 image MIME types and three serialized slow
image conversions. It verifies paste completion and byte-exact restoration of
every format, plus failure before selection replacement for a stalled source.
For an isolated old/new regression comparison, supply the previous installed
Linux binary via `SIMPLE_STT_OLD_SHELL=/usr/lib/simple-stt/simple-stt-linux`;
that binary is launched only inside the private compositor.

### Resident current clipboard buffer

Run `python scripts/test-linux-unicode-input.py --cache-only` after building
`simple-stt-linux` and the production paste helper. It starts the resident
buffer on a private KWin compositor, warms a slow multi-format image once, and
requires paste readiness within 1.5 seconds without rereading the original
image provider. It checks repeated warm reuse, exact full-format restoration,
cancellation of a stalled backup by newer text copies, replacement by a new
image with different bytes, sharing a transfer already in progress, rejection
of incomplete backups, copy races during paste, and an empty clipboard.
Disabling prefetch must retain the currently restored selection until the next
external copy, then exit without leaving its socket behind.
The fixture records MIME request names only, so a warm hit can be distinguished
from another full backup. The original cold-owner suite remains separate.

The GTK fixtures in the Wayland clipboard suite disable their own AT-SPI bridge:
Fedora GTK3 can crash in its key-snooper callback list with the private registry,
and the deliberately stalled image provider must not block unrelated foreground
accessibility queries. The real paste helper stays unchanged. Accessible-text
insertion is tested separately by the private X11 `test-linux-paste.py` suite.

## Clipboard buffer diagnostics

Run `node scripts/test-settings-clipboard-notices.cjs` with Playwright available. It checks the default-off flag, Linux visibility, Windows hiding, English/Arabic search, and Save/reopen persistence. Screenshots are retained under `artifacts/linux-release/clipboard-buffer-notices/`.

The private Wayland cache suite also checks ordered buffering/ready phase events; Rust tests check bounded event retention and fast-transcript delivery waiting for shortcut release. `overlay-e2e --offscreen` renders the buffer notices in English and Arabic without opening a host window.

### Shortcut reminders

`node scripts/test-settings-shortcut-reminders.cjs` checks default-on reminders,
draft/Save/reopen behavior, independent Show shortcuts, shortcut recording, the
KDE-assigned shortcut row, Arabic search and narrow/wide screenshots. The
production offscreen renderer (`target/debug/overlay-e2e --offscreen`) also
captures reminders during recording, transcription, typing, paste and warnings
in both languages, plus disabled reminders and manual display. Neither test
opens a desktop window, captures audio or touches the host clipboard.

`python scripts/test-shortcut-reminders.py` exercises the real Linux and control
CLIs and Settings HTTP action against an authenticated loopback capture fixture.
It asserts that showing shortcuts sends no recording, cancellation or delivery
commands. Native Windows smoke also covers shortcut registration and commands.

Retry delivery regressions hold the modifiers beyond the old two-second notice
timeout, require delivery registration before release, require each progress
update to name a registered session, and require completion after delivery.
Rust checks that registration rejects active/newer work. Windows native smoke
also exercises BeginDelivery and waiting/typing progress commands.

Keycap rendering checks cover standard emoji glyph availability, punctuation
and plus/bracket keys, exclusion of ordinary bracketed notices, intact keycaps
when wrapping, and bounding boxes in both English and Arabic. Offscreen renders
also capture the four assigned shortcut example at native tooltip dimensions.

## Integrated reading and optional assets

`cargo test --workspace --all-targets` includes the reader contracts, framed worker protocol, generation cancellation, provider/OCR validation and downloader fixture-server tests. The downloader tests exercise successful atomic activation, checksum rejection, cancellation and archive-path validation without public network or desktop access.

`node scripts/test-settings-reading.cjs` runs headless Playwright against an isolated HTTP fixture. It covers download/cancel controls, reader draft persistence across page changes, Save/reopen, unchanged dictation models and English/Arabic layouts at 360/768/1280px in both themes. Screenshots go to ignored `artifacts/combined-app/settings/`.

`python scripts/test-optional-assets-api.py` exercises the actual authenticated Settings endpoints in a temporary profile. `python scripts/test-reader-workers.py` communicates only through worker stdin/stdout: speech PCM is captured in memory without playback, and OCR uses committed fixture images. It does not use the microphone, clipboard, portals or host input. Optional real speech checks require explicit model/runtime arguments; no assets are installed into the host profile by these tests.

`node scripts/test-settings-auto-selection.cjs` verifies immediate selection updates after a completed install, preservation of unsaved choices, and a Save racing an install. Merely refreshing the catalog does not assign anything.

`node scripts/test-settings-paddle-catalog.cjs` checks the full supported OCR catalog, shared runtime gating/names, pagination, language/version searches, installed-only selection and the preserved Arabic/English recommendation. Set `SETTINGS_VISUAL_REVIEW=1` for English/Arabic screenshots in both themes at 360/768/1280px. Long language lists show three names and a count; the help control exposes the complete supported list.

`python scripts/test-paddle-catalog.py --runtime-cache PATH_TO_VERIFIED_TEST_ASSETS` downloads every supported Paddle model into a private profile and runs each through the real GPU OCR worker. The optional cache must contain genuine verified NVIDIA runtime downloads from another isolated test profile; it does not fabricate receipts. Images are rendered offscreen with Pango, including script-specific text and a numeral round trip. No host clipboard, screenshot portal, input or audio device is used. The harness verifies GPU allocation for each worker and that model changes never leave multiple OCR workers resident. It requires about 1 GB of model/detector downloads and suitable local fonts. Results are written under ignored `artifacts/paddle-catalog/profile`; `--reuse` repeats execution without redownloading already verified assets.

`python scripts/test-nvidia-fresh-install.py --profile artifacts/runtime-download-fix/fresh-gpu-profile` downloads the pinned NVIDIA runtime dependencies and Supertonic/Paddle models through the real Settings API into an empty isolated profile, verifies automatic assignments, then runs English/Arabic WAV rendering and OCR on the GPU. It checks that native libraries came from those downloads. Allow about 1.8 GB of downloads; no host clipboard, microphone or audio output is used. `python scripts/test-stt-fresh-download.py` independently downloads a Vulkan runtime and recognition model and transcribes the public WAV in a private null audio session, checking worker reuse. Both support `--bin-dir` for packaged binaries and `--reuse` for a repeat run; a reused run is not fresh-install evidence.

`python scripts/test-reader-service.py` exercises the real media CLI and a private reader controller; it requires built binaries and local model/runtime assets. `python scripts/test-reader-live.py` additionally requires PipeWire, WirePlumber, a private D-Bus session and GPU assets, and uses a private null sink rather than host playback. Consult each script's `--help` for asset paths and prerequisites. `python scripts/test-linux-selection-copy.py` uses a private Xvfb and D-Bus session to exercise selection reads; its source lists its desktop test dependencies and required built helper. These harnesses do not replace a manual installed-desktop pass. Native Windows runtime checks require a Windows host; cross-compilation alone does not validate them.

### Speech preparation and shared Abort

Run `python3 scripts/test-tts-pronunciation.py` for file-only GPU pronunciation probes when the documented local models are available. It never plays audio; recognizer output is diagnostic, especially for Arabic. The private live-reader harness also exercises capture/control Cancel against active reading and verifies Cancel does not start an absent reader. Rust coverage includes one shared tray Abort action and rejection of late OCR clipboard commits.

Settings single-instance reuse is scoped to the executable build, not just the installation. `python scripts/test-settings-upgrade.py` atomically replaces an isolated running executable, verifies that the new build opens a new server, that repeated launches reuse that new server, and that closing it preserves the older page and saved configuration. An older open page may contain unsaved work and is never killed automatically by an upgrade.

## RemoteDesktop default delivery

On the KDE Wayland desktop, after compiling `simple-stt-linux`, `simple-stt-ctl` and the native helper, run `/usr/bin/python3 scripts/test-linux-portal-input.py`. It creates a temporary editor and isolated settings, and may show the keyboard-control permission dialog. It exercises real default Unicode paste twice, including portal restoration; cached transcription is a fixture. Logs stay in its printed temporary artifacts directory.

Add `--typing` to the same live harness to exercise selectable Type mode instead. This checks exact repeated multilingual insertion and compares clipboard formats and bytes before/after. Current Type uses the shared EIS text broker. Historical AT-SPI results do not validate this path; use the private portal-input E2E above for unattended checks.

Read-along regression desktop pass: `/usr/bin/python3 scripts/test-read-along-desktop.py --overlap --focus-change --transient-empty --unchanged-once`, then repeat with `--no-prefetch`. The target window is created before a competing window, focus is moved away and back, the first Space is ignored, and the next page initially renders blank. Require exact bounded key behavior, no Space in the competitor, and actual second-page OCR/audio. A private test permission token is retained in the user runtime directory to exercise restoration across fresh test processes.

For a rendered dialogue box, run the desktop harness with `--game-dialogue --fullscreen` using Python with GTK3, Cairo and PyOpenGL installed. Its text is uploaded as an OpenGL texture and must survive window capture/OCR/page turning. It records the GL renderer. No commercial game assets are used.

For actual OpenAI interoperability, run `python scripts/test-openai-server-e2e.py` in an environment with the official `openai` SDK and Cairo. It uses installed local model assets with private headless host/client profiles, never OpenAI cloud. It checks SDK authentication/discovery, real WAV TTS, image OCR through chat completions and Responses, multipart STT, warm reuse, app-client TTS/OCR/STT with local fallback disabled, disconnected STT fallback, idle process exit and reload. Server/client logs stay in its printed private temporary directory.

### Portal-only Linux input and privacy regression

```sh
python scripts/test-portal-input-e2e.py
python scripts/test-openai-server-e2e.py
python scripts/test-read-along-desktop.py --overlap --focus-change --transient-empty --unchanged-once
python scripts/test-read-along-desktop.py --overlap --focus-change --transient-empty --unchanged-once --no-prefetch
python scripts/test-read-along-desktop.py --game-dialogue --fullscreen
```

The portal-input harness provisions a private KWin compositor and D-Bus session;
it relays the private compositor's EIS FD and does not change user permissions.
The host read-along harness requires an already authorized test grant. The SDK
harness requires Python `openai` and verifies headless local server/client
interoperability, lazy worker reuse/unload, fallback, request deadlines,
temporary-file cleanup and content-free default logs. It makes no cloud calls.
The game fixture is an actual OpenGL-rendered fullscreen dialogue surface; it
does not establish compatibility with a specific commercial game or Proton.

### Repeat saved reading area

`python scripts/test-read-along-desktop.py --repeat-region` selects a region once in an isolated test profile, performs real OCR and speech, changes the dialogue manually, and repeats the saved area. It asserts new OCR output and audio frames without a second selection or a page-advance key. Reader unit tests cover stale completion protection; root tests cover choosing the newest reader or dictation action without transferring payloads.

`python scripts/test-read-along-desktop.py --hud-progress` verifies that real playback retains current text while independent next-page capture/OCR/TTS progress is available. HUD formatter tests cover stale activity metadata and suppressing shortcut hints only for read-along. `overlay-e2e --offscreen` includes English and Arabic current-sentence/next-page examples.
