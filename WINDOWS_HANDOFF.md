# Windows review and end-to-end validation handoff

## Mission

Take the current `master` branch of Simple STT on a real Windows machine, review the complete application, run the automated and manual Windows end-to-end suites, fix every reproducible bug found, rerun the relevant regression coverage, and commit and push the verified fixes.

Do not treat a successful compilation as completion. Acceptance requires exercising the installed application with a physical microphone, the real Parakeet model/runtime, AutoHotkey v2, the tray, browser settings, hotkeys, delivery modes, model lifecycle, and privacy-safe diagnostics.

Linux/KDE Fedora has received extensive real-world testing and is currently stable. Windows must retain feature and behavior parity where the implementation is shared, while using the native Windows shell and automation behavior where appropriate.

## Important current state

- The optional S1-mini transcript-cleanup experiment was intentionally discarded. Do not restore its model, llama.cpp runtime, cleanup UI, schema, worker, or notifications.
- The supported pipeline is `microphone -> Parakeet -> existing text transforms -> delivery`.
- Cleanup/model-generation assets are not part of the repository or release.
- Logs are private and bounded by default. Transcript text must remain absent unless the user explicitly enables transcript logging.
- Preserve existing configuration through migrations and normalization. Never silently reset valid user choices.
- If a bug exists in shared Windows/Linux logic, fix the shared implementation and run Linux-safe regression tests as well. Do not add a Windows-only workaround unless the behavior is genuinely platform-specific.

## Prepare the Windows machine

1. Clone the repository or fetch the latest `origin/master` and verify the worktree is clean before editing.
2. Record the starting commit with `git rev-parse HEAD` in the final validation report.
3. Install or verify:
   - stable Rust and Cargo;
   - Python 3;
   - AutoHotkey v2 under `%ProgramFiles%\AutoHotkey\v2`;
   - the expected Parakeet Windows CUDA runtime and model;
   - a working microphone;
   - an NVIDIA GPU/driver for the real CUDA pass, if the test machine supports it.
4. Do not reuse stale release binaries. Every runtime test must use binaries built from the checked-out commit.
5. Preserve the user's real configuration and clipboard. Automated tests should use their isolated temporary config/state paths. Back up any real configuration before a manual migration test.

## Review the complete implementation

Review all Rust, AutoHotkey, web settings, build/package scripts, configuration migrations, IPC protocols, tests, and documentation—not only files recently changed. Pay particular attention to:

- capture state transitions and cancellation at recording, warming, transcription, and delivery boundaries;
- tray and hotkey paths sharing the same toggle state, so either can start and the other can stop;
- duplicate activation/debouncing and rapid repeated dictation;
- exact-child PID ownership, crash recovery, bounded shutdown, and idle model unloading;
- microphone selection, stable device IDs, preferred-device disappearance, default fallback, and automatic return when the preferred microphone reappears;
- foreground identity checks and prevention of stale delivery into a later window/session;
- Unicode, multiline text, punctuation removal, lowercase transforms, and typing speed/randomization;
- clipboard preservation, clipboard-only mode, Ctrl+V, Ctrl+Shift+V, Shift+Insert where exposed, and app-specific delivery overrides;
- delivery-mode cycling, live settings synchronization, and readable notifications;
- tray startup, menu state, settings launch, reload, shutdown, and startup shortcut behavior;
- browser settings authentication, external-edit conflict handling, save/reset/import flows, model/device controls, responsive layout, and accessibility;
- schema-v5 migration/default behavior and retention of existing user settings;
- minimal release logging, bounded retention, and transcript privacy defaults;
- release packaging containing only production binaries and required assets.

Remove dead code, stale documentation, and obsolete test assumptions only when the removal is clearly safe and covered by tests. Do not broaden the product or reintroduce experimental transcript cleanup during this handoff.

## Mandatory automated validation

Run from a normal Windows checkout:

```bat
scripts\test-full.cmd
```

This is the authoritative suite and must pass without skipping stages. It covers Rust tests, static architecture checks, the IPC proof of concept, AutoHotkey validation, and runtime smoke tests.

Also build the release explicitly:

```powershell
.\scripts\build-release.ps1
```

Confirm these fresh production binaries exist and launch:

```text
target\release\simple-stt-capture.exe
target\release\simple-stt-infer.exe
target\release\simple-stt-ctl.exe
target\release\simple-stt-settings.exe
```

Run individual commands while diagnosing failures rather than weakening the full suite:

```powershell
cargo test --all-targets
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
scripts\test-ahk-full.cmd
```

After every fix, add or strengthen a regression test at the lowest useful layer. At the end, rerun `scripts\test-full.cmd` from a cleanly rebuilt state.

## Mandatory real Windows E2E pass

Use a physical microphone and the actual UI. Verify all of the following:

1. Start the application normally and confirm one responsive tray icon appears.
2. Open Settings from the tray, navigate every page, save a setting, close/reopen Settings, and confirm persistence.
3. Verify the configured record hotkey, cancel hotkey, and delivery-cycle hotkey. Test press/release behavior, rapid repeats, extra modifiers, left/right Ctrl and Alt, AltGr, and runtime reassignment.
4. Start recording from the hotkey and stop from the tray; then start from the tray and stop from the hotkey.
5. Record short, long, silent, Unicode, punctuation-heavy, and multiline dictations.
6. Exercise typing, normal paste, terminal paste, and clipboard-only delivery. Confirm clipboard-only never injects input.
7. Confirm app overrides choose the expected delivery mode/tool. Exercise a terminal, browser, basic text editor, and an application that rejects simulated paste if available.
8. Change modes with the cycle hotkey while Settings is open. Confirm the current selection updates live and a readable notification appears.
9. Confirm notifications are not clipped or replaced too quickly and never reveal transcript text when private logging is disabled.
10. Change typing speed and verify the slider midpoint/default behavior and humanization/randomization remain intact.
11. Test preferred microphone selection. If only one physical microphone exists, use a virtual/USB device or disable/re-enable an endpoint to validate fallback to the current default and automatic return to the preferred endpoint.
12. Restart Windows Audio while the app is running and confirm capture recovers without restarting the entire application.
13. Kill the capture worker and inference worker separately. Confirm supervised recovery, tray/hotkey reconnection, and no orphan process.
14. Confirm Parakeet begins warming only at recording start, remains loaded during recording, never unloads because a recording is long, and exits after the configured idle timeout.
15. Exercise cancel/focus-loss/shutdown during recording, warming, transcription, and delivery. Confirm no stale text is typed or pasted and the clipboard remains unchanged.
16. Test startup shortcut creation/removal, sign out or reboot if practical, and confirm the configured startup behavior.
17. Leave the app idle and confirm negligible recurring CPU usage, no microphone stream, and no inference worker after timeout.
18. Inspect logs after the pass. Confirm bounded retention, minimal release noise, character counts instead of transcript contents by default, and full text only after explicitly opting in.

## Performance and resource validation

Measure and report:

- idle shell/capture CPU and memory with no recording;
- cold and warm recording-to-delivery latency;
- Parakeet model load time and inference time;
- worker RAM/VRAM while loaded and after idle unload;
- process count before recording, while recording, after delivery, and after timeout;
- whether any worker, hotkey hook, audio stream, or web settings process remains unexpectedly active.

Investigate material regressions rather than accepting them because functional tests pass. The application goal is lightweight, fast, private speech-to-text.

## Bug-fix rules

- Reproduce each issue and identify the responsible layer before editing.
- Preserve user data, clipboard contents, and configuration.
- Prefer shared fixes for shared behavior and platform-specific fixes for native Windows behavior.
- Keep IPC loopback-only, authenticated, versioned, and asynchronous.
- Never log PCM or transcripts by default.
- Never kill processes by name or broad pattern; terminate only the exact owned PID.
- Do not add Python, a resident model server, or another background runtime to the product.
- Do not commit downloaded models, runtimes, logs, credentials, temporary configs, generated distributions, or build output.
- Do not reduce or skip tests to make the suite pass.

## Completion and handback

Before pushing:

1. Review `git diff` for scope, secrets, generated files, and accidental user-data changes.
2. Run `git diff --check`.
3. Run the complete Windows suite again.
4. Verify the release binaries were freshly rebuilt after the final source change.
5. Commit all intended source, test, documentation, and packaging fixes with a clear message.
6. Push the verified branch to `origin`.

Provide a final report containing:

- starting and ending commit IDs;
- Windows edition/build and hardware used;
- every automated command and its result;
- the manual E2E matrix with pass/fail notes;
- bugs found, root causes, fixes, and regression tests;
- cold/warm latency and resource measurements;
- any remaining limitation with exact reproduction steps;
- confirmation that transcript privacy, bounded logs, cancellation safety, worker unloading, and clipboard preservation passed.

Do not declare the handoff complete while a reproducible in-scope defect remains unfixed or an acceptance item remains untested without a clearly documented hardware/environment blocker.
