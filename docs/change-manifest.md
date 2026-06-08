# Overhaul change manifest

## Added active shell files

```text
ahk/uvox.ahk
ahk/lib/Config.ahk
ahk/lib/Hotkeys.ahk
ahk/lib/IpcClient.ahk
ahk/lib/Logging.ahk
ahk/lib/ProcessSupervisor.ahk
ahk/lib/SettingsGui.ahk
ahk/lib/TabProtocol.ahk
ahk/lib/Tray.ahk
ahk/lib/Typist.ahk
ahk/lib/Utils.ahk
ahk/tests/hotkeys-manual.ahk
ahk/tests/ipc-smoke.ahk
ahk/tests/typing-smoke.ahk
```

## Added active Rust files

```text
src/bin/uvox_capture.rs
src/bin/uvox_infer.rs
src/bin/uvoxctl.rs
src/bin/uvox_mock_infer.rs              test-only, excluded from release staging
src/capture/audio.rs
src/capture/inference_supervisor.rs
src/capture/ipc_server.rs
src/capture/process.rs
src/capture/mod.rs
src/capture/overlay.rs
src/capture/overlay_windows.rs
src/capture/state.rs
src/common/line_codec.rs
src/common/mod.rs
src/common/shell_protocol.rs
src/config.rs
src/infer/mod.rs
src/infer/parakeet_native.rs
src/infer/protocol.rs
src/lib.rs
src/logging.rs
src/models.rs
tests/worker_lifecycle.rs
fixtures/parakeet-smoke.wav
```

## Added docs, POC, and scripts

```text
docs/ahk-v2-research.md
docs/current-behavior-inventory.md
docs/ipc-decision.md
docs/architecture.md
docs/configuration.md
docs/packaging.md
docs/memory-cleanup-validation.md
docs/testing.md
docs/change-manifest.md
tools/ipc-poc/mock_service.py
tools/ipc-poc/test_poc.py
tools/ipc-poc/README.md
scripts/build-release.ps1
scripts/check-prereqs.ps1
scripts/run-dev.ps1
scripts/package-release.ps1
scripts/test-static.ps1
scripts/ipc-poc.ps1
scripts/memory-cleanup-validation.ps1
```

## Modified top-level guidance

```text
README.md
START_HERE.txt
AGENTS.md
GEMINI.md
CONTRIBUTING.md
OVERLAY_NOTIFICATIONS.md
THIRD_PARTY_NOTICES.md
.gitignore
Cargo.toml
build.rs
```

## Removed from the active graph

The retired monolithic Rust source, Slint UI, Rust tray/hook/text-injection/startup code, screenshot utility, old unload integration test, duplicated `rust/` tree, old root tooltip helper, source zip, old resources, and old uppercase docs were removed from the working tree. Git history remains the reference if any of that material needs to be recovered.

The stale pre-overhaul `Cargo.lock` was removed. A Windows release build should generate a fresh lockfile from the new dependency graph and commit/review it before release.

## Final source-only hardening pass

- Rapid repeated dictations now preserve the newest recording/transcribing overlay state when an older asynchronous transcript completes. Pending transcript sessions are tracked explicitly, so a short recording, empty transcript, successful transcript, or failed transcript cannot hide another in-flight session's visual feedback.
- `Cargo.lock` is no longer ignored. Generate and commit a fresh lockfile during the first Windows release build.

- Every Rust log line now carries an explicit `component` and `pid` prefix, including lines emitted by background worker-supervision threads.
