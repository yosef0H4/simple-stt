#!/usr/bin/env python3
"""Dependency-free source audit for the split Uvox architecture.

This intentionally does not pretend to be a Rust compiler or an AutoHotkey runtime.
It checks the architectural invariants that are easy to regress during source edits.
"""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[1]
errors: list[str] = []
checks: list[str] = []


def text(path: str) -> str:
    p = root / path
    if not p.exists():
        errors.append(f"missing {path}")
        return ""
    return p.read_text(encoding="utf-8")


def need(path: str, *needles: str) -> str:
    body = text(path)
    for needle in needles:
        if needle not in body:
            errors.append(f"{path} missing {needle!r}")
    return body


def forbid(path: str, *needles: str) -> str:
    body = text(path)
    for needle in needles:
        if needle in body:
            errors.append(f"{path} unexpectedly contains {needle!r}")
    return body


required_files = [
    "ahk/uvox.ahk",
    "ahk/lib/Config.ahk", "ahk/lib/Hotkeys.ahk", "ahk/lib/IpcClient.ahk",
    "ahk/lib/Logging.ahk", "ahk/lib/ProcessSupervisor.ahk", "ahk/lib/SettingsGui.ahk",
    "ahk/lib/TabProtocol.ahk", "ahk/lib/Tray.ahk", "ahk/lib/Typist.ahk", "ahk/lib/Utils.ahk",
    "src/bin/uvox_capture.rs", "src/bin/uvox_infer.rs", "src/bin/uvoxctl.rs", "src/bin/uvox_mock_infer.rs",
    "src/capture/audio.rs", "src/capture/inference_supervisor.rs", "src/capture/ipc_server.rs", "src/capture/process.rs",
    "src/infer/parakeet_native.rs", "src/infer/protocol.rs", "src/common/shell_protocol.rs",
    "docs/ahk-v2-research.md", "docs/current-behavior-inventory.md", "docs/ipc-decision.md",
    "docs/architecture.md", "docs/configuration.md", "docs/packaging.md",
    "docs/memory-cleanup-validation.md", "docs/testing.md", "docs/change-manifest.md",
    "tools/ipc-poc/mock_service.py", "tools/ipc-poc/test_poc.py", "tools/ipc-poc/README.md",
    "scripts/build-release.ps1", "scripts/run-dev.ps1", "scripts/package-release.ps1",
    "scripts/memory-cleanup-validation.ps1",
    "tests/worker_lifecycle.rs",
]
for path in required_files:
    need(path)
for path in ["fixtures/parakeet-smoke.wav"]:
    if not (root / path).is_file():
        errors.append(f"missing {path}")

# Application releases should commit a fresh lockfile after the first Windows build.
gitignore = text(".gitignore")
if any(line.strip() == "Cargo.lock" for line in gitignore.splitlines()):
    errors.append("Cargo.lock must not be ignored; generate and commit a fresh release lockfile on Windows")
checks.append("release lockfile hygiene is explicit: Cargo.lock is not ignored")

# Every executable AHK entry point is v2-only.
directives = "#Requires AutoHotkey v2.0\n#SingleInstance Force"
need("ahk/uvox.ahk", directives, "OnExit(", "OnError(")
for path in sorted((root / "ahk/tests").glob("*.ahk")):
    head = "\n".join(path.read_text(encoding="utf-8").splitlines()[:2])
    if head != directives:
        errors.append(f"{path.relative_to(root)} missing mandatory v2 entry directives")
checks.append("shell entry and AHK smoke scripts are v2-only")

# The active Cargo graph is split; the archived monolith is not compiled.
cargo = forbid("Cargo.toml", "slint", "tray-icon", "global-hotkey")
need("Cargo.toml", '"Win32_System_Threading"')
lib = forbid("src/lib.rs", "pub mod parakeet_native;", "pub mod protocol;")
if (root / "src/main.rs").exists():
    errors.append("legacy src/main.rs is still active")
for binary in ["uvox_capture", "uvox_infer", "uvoxctl"]:
    if f"src/bin/{binary}.rs" not in required_files:
        errors.append(f"required split binary not registered in verifier: {binary}")
checks.append("active Cargo graph has split binaries and no Slint frontend dependency")

# Capture is lightweight and must never load Parakeet.
active_capture = "\n".join(p.read_text(encoding="utf-8") for p in (root / "src/capture").glob("*.rs"))
capture_bin = text("src/bin/uvox_capture.rs")
for needle in ["libloading", "ParakeetNative", "Library::new", "parakeet_capi_"]:
    if needle in active_capture or needle in capture_bin:
        errors.append(f"capture service imports native Parakeet loader token {needle!r}")
all_active_rs = {p.relative_to(root).as_posix(): p.read_text(encoding="utf-8") for p in (root / "src").rglob("*.rs")}
loader_paths = [path for path, body in all_active_rs.items() if "use libloading::Library" in body]
if loader_paths != ["src/infer/parakeet_native.rs"]:
    errors.append(f"libloading import must exist only in src/infer/parakeet_native.rs, got {loader_paths}")
need("src/bin/uvox_infer.rs", "ParakeetNative", "worker idle timeout reached; exiting process")
need("src/capture/inference_supervisor.rs", "shutdown_now", "force-terminating inference worker", "handshake failed; terminating child", "shutdown_shared", "force_terminate_pid", "pid_tracker")
need("src/capture/process.rs", "OpenProcess", "TerminateProcess", "WaitForSingleObject", "PROCESS_TERMINATE", "exact child PID")
need("src/bin/uvox_capture.rs", "shutdown_shared", "nonzero_pid", "next.log_level != config.log_level", "log_level: config.log_level.clone()", "HashSet::<u64>::new()", "restore_overlay_after_success", "restore_overlay_work_state", "newer_overlay_work_survives_older_transcript_completion")
need("src/bin/uvox_infer.rs", "log_level: LogLevel", "&args.log_level")
need("src/logging.rs", "component={component} pid={}", "prefix_lines", "component_prefix_survives_split_writes_and_multiline_events")
need("src/capture/inference_supervisor.rs", '.arg("--log-level")')
forbid("src/bin/uvox_capture.rs", "worker.lock().unwrap().worker_pid()", "worker.lock().unwrap().replace_config")
checks.append("Parakeet DLL/model loading is isolated to disposable uvox-infer with exact-PID forced-exit fallback")

# AHK shell owns desktop behavior and does not use the clipboard as typing transport.
need("ahk/lib/Tray.ahk", "A_TrayMenu", "TraySetIcon", "Open Settings", "Restart Audio Service", "Unload Speech Model")
need("ahk/lib/SettingsGui.ahk", "Gui(", "Record chord", "List microphones", "Download model", "Advanced runtime directory")
hotkeys_ahk = need("ahk/lib/Hotkeys.ahk", "Hotkey(", "InputHook(", "*CapsLock", "CapsLock & ", "AltGr", "SetCapsLockState")
if '"*CapsLock & "' in hotkeys_ahk:
    errors.append("CapsLock custom combination must not prepend wildcard; combinations already wildcard-match")
need("ahk/lib/TabProtocol.ahk", 'StrReplace(value . "", "\\", "\\\\")', 'case "\\": out .= "\\"')
need("ahk/lib/Utils.ahk", 'DllCall("advapi32\\SystemFunction036"')
typist = need("ahk/lib/Typist.ahk", "SendText(", "WinActive(\"A\") != this.targetWindow", "AnyPhysicalModifierDown")
if "Clipboard" in typist or "A_Clipboard" in typist:
    errors.append("Typist.ahk must not use the clipboard by default")
checks.append("AHK owns tray, GUI, hotkeys, Caps Lock behavior, and foreground-safe Unicode typing")

# The shell stays non-blocking for service calls and reconnects after a new capture PID.
ipc_ahk = need("ahk/lib/IpcClient.ahk", "Run(command", "SetTimer", "poll-events", "ResetServiceSession", "this.latestSeq := 0", "RetryPing", "uvoxctl helper timed out", 'responseReady := FileExist(job["path"])')
supervisor_ahk = need("ahk/lib/ProcessSupervisor.ahk", "Run(command", "ProcessWaitClose", "ProcessClose", "UvoxRandomToken", "ResetServiceSession", "readyProbeInFlight", "this.startTimer")
need("ahk/uvox.ahk", "pendingStarts", "pendingStops", "recording stop deferred until start acknowledgement")
if "RunWait(command" in ipc_ahk:
    errors.append("IpcClient.ahk service calls must be asynchronous")
checks.append("AHK helper IPC is asynchronous, token-rotated, sequenced, and reconnectable")

# Public control IPC is loopback-only and versioned; Rust-to-Rust PCM stays framed.
need("src/capture/ipc_server.rs", 'TcpListener::bind("127.0.0.1:0")', "SHELL_PROTOCOL_VERSION", "protocol_mismatch", "unauthorized")
need("src/common/shell_protocol.rs", "SHELL_PROTOCOL_VERSION", "StartRecording", "PollEvents")
need("src/infer/protocol.rs", 'pub const MAGIC: [u8; 4] = *b"UVX1"', "VERSION", "TranscribePcm", "sample_rate")
checks.append("control IPC is loopback-only and versioned; raw PCM stays on framed child pipes")

# Canonical schema-v2 config, partial downloads, and process-exit diagnostics are present.
need("src/config.rs", "CONFIG_SCHEMA_VERSION: u32 = 2", "MoveFileExW", "MOVEFILE_REPLACE_EXISTING", "schema1_is_migrated_and_backed_up", "runtime_root", "current_exe")
need("src/models.rs", 'https://', 'gguf.partial.', "replace_file_atomic", "validate_model_filename", 'join("fixtures")')
need("scripts/package-release.ps1", "fixtures\\parakeet-smoke.wav")
need("scripts/build-release.ps1", "--bin uvox-capture --bin uvox-infer --bin uvoxctl")
need("scripts/memory-cleanup-validation.ps1", "Get-Process", "nvidia-smi", "unload-model")
checks.append("schema-v2 migration, install-relative paths, atomic writes, HTTPS partial downloads, and diagnostics are present")

# Deterministic worker integration coverage exists but is intentionally not shipped.
need("src/bin/uvox_mock_infer.rs", "Test-only disposable inference worker", "hang-handshake", "mock مرحبا 世界 🙂")
need("tests/worker_lifecycle.rs", "worker_launches_lazily_and_reuses_warm_process", "worker_exits_after_idle_timeout", "model_switch_recycles_worker_before_next_request", "crashed_worker_is_discarded_and_recoverable", "blocked_inference_is_force_terminated_by_exact_pid")
if "--bins" in text("scripts/build-release.ps1"):
    errors.append("release build must not ship the mock inference binary via --bins")
checks.append("mock-worker lifecycle integration sources cover reuse, idle exit, model switch, crash recovery, and forced shutdown")

# Legacy sources remain available without contaminating the active graph.
checks.append("retired monolith trees are removed from the working tree; git history remains the archive")

if errors:
    print("STATIC VERIFY FAILED")
    print("\n".join(" - " + error for error in errors))
    sys.exit(1)
print("STATIC VERIFY PASSED")
for check in checks:
    print(" - " + check)
