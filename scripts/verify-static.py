#!/usr/bin/env python3
"""Dependency-free source audit for the split SimpleStt architecture.

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
    "ahk/simple-stt.ahk",
    "ahk/lib/Config.ahk", "ahk/lib/Hotkeys.ahk", "ahk/lib/IpcClient.ahk",
    "ahk/lib/Logging.ahk", "ahk/lib/ProcessSupervisor.ahk", "ahk/lib/SettingsGui.ahk",
    "ahk/lib/TabProtocol.ahk", "ahk/lib/Tray.ahk", "ahk/lib/Typist.ahk", "ahk/lib/Utils.ahk",
    "src/bin/simple_stt_capture.rs", "src/bin/simple_stt_infer.rs", "src/bin/simple_stt_ctl.rs", "src/bin/simple_stt_mock_infer.rs",
    "src/capture/audio.rs", "src/capture/inference_supervisor.rs", "src/capture/ipc_server.rs", "src/capture/process.rs",
    "src/infer/parakeet_native.rs", "src/infer/protocol.rs", "src/common/shell_protocol.rs",
    "docs/ahk-v2-research.md", "docs/current-behavior-inventory.md", "docs/ipc-decision.md",
    "docs/architecture.md", "docs/configuration.md", "docs/packaging.md",
    "docs/memory-cleanup-validation.md", "docs/testing.md", "docs/change-manifest.md",
    "tools/ipc-poc/mock_service.py", "tools/ipc-poc/test_poc.py", "tools/ipc-poc/README.md",
    "scripts/build-release.ps1", "scripts/run-dev.ps1", "scripts/package-release.ps1",
    "scripts/memory-cleanup-validation.ps1",
    "build.rs", "resources/simple-stt.exe.manifest", "resources/windows.rc",
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
need("ahk/simple-stt.ahk", directives, "OnExit(", "OnError(")
for path in sorted((root / "ahk/tests").glob("*.ahk")):
    head = "\n".join(path.read_text(encoding="utf-8").lstrip("\ufeff").splitlines()[:2])
    if head != directives:
        errors.append(f"{path.relative_to(root)} missing mandatory v2 entry directives")
checks.append("shell entry and AHK smoke scripts are v2-only")

# The active Cargo graph is split; the archived monolith is not compiled.
cargo = forbid("Cargo.toml", "slint", "tray-icon", "global-hotkey")
need("Cargo.toml", '"Win32_System_Threading"')
lib = forbid("src/lib.rs", "pub mod parakeet_native;", "pub mod protocol;")
if (root / "src/main.rs").exists():
    errors.append("legacy src/main.rs is still active")
if (root / "ui/settings.slint").exists():
    errors.append("inactive Slint settings UI must not be kept in the active tree")
for stale_script in [
    "scripts/apply_perf_patch.py",
    "scripts/apply_remaining_patch.py",
    "scripts/fix_models_cache.py",
    "scripts/patch_refresh_models.py",
    "scripts/patch_settings_gui.py",
]:
    if (root / stale_script).exists():
        errors.append(f"stale one-off patch helper remains in active tree: {stale_script}")
for binary in ["simple_stt_capture", "simple_stt_infer", "simple_stt_ctl"]:
    if f"src/bin/{binary}.rs" not in required_files:
        errors.append(f"required split binary not registered in verifier: {binary}")
checks.append("active Cargo graph has split binaries and no Slint frontend or stale patch helpers")

# Windows visual styles require an embedded Common Controls v6 manifest.
need("Cargo.toml", 'build = "build.rs"', 'embed-resource = "3"')
need("build.rs", 'resources/windows.rc', 'resources/simple-stt.exe.manifest', 'manifest_required()')
need("resources/windows.rc", '1 24 "simple-stt.exe.manifest"')
need("resources/simple-stt.exe.manifest", 'Microsoft.Windows.Common-Controls', 'version="6.0.0.0"', 'PerMonitorV2', 'asInvoker')
checks.append("Windows binaries embed a Common Controls v6 manifest for modern tooltip styling")

# Capture is lightweight and must never load Parakeet.
active_capture = "\n".join(p.read_text(encoding="utf-8") for p in (root / "src/capture").glob("*.rs"))
capture_bin = text("src/bin/simple_stt_capture.rs")
for needle in ["libloading", "ParakeetNative", "Library::new", "parakeet_capi_"]:
    if needle in active_capture or needle in capture_bin:
        errors.append(f"capture service imports native Parakeet loader token {needle!r}")
all_active_rs = {p.relative_to(root).as_posix(): p.read_text(encoding="utf-8") for p in (root / "src").rglob("*.rs")}
loader_paths = [path for path, body in all_active_rs.items() if "use libloading::Library" in body]
if loader_paths != ["src/infer/parakeet_native.rs"]:
    errors.append(f"libloading import must exist only in src/infer/parakeet_native.rs, got {loader_paths}")
need("src/bin/simple_stt_infer.rs", "ParakeetNative", "worker idle timeout reached; exiting process", "MessageType::WarmUp", "MessageType::ModelLoaded", "MessageType::WarmUpAck", "model warm-up begin", "model warm-up end")
need("src/capture/inference_supervisor.rs", "shutdown_now", "force-terminating inference worker", "handshake failed; terminating child", "shutdown_shared", "force_terminate_pid", "pid_tracker", "pub fn warm_up(&mut self, mut on_model_loaded: impl FnMut()) -> Result<()>", "MessageType::WarmUp", "MessageType::ModelLoaded", "MessageType::WarmUpAck")
need("src/capture/process.rs", "OpenProcess", "TerminateProcess", "WaitForSingleObject", "PROCESS_TERMINATE", "exact child PID")
need("src/bin/simple_stt_capture.rs", "shutdown_shared", "nonzero_pid", "next.log_level != config.log_level", "log_level: config.log_level.clone()", "HashSet::<u64>::new()", "restore_overlay_after_success", "restore_overlay_work_state", "newer_overlay_work_survives_older_transcript_completion")
need("src/bin/simple_stt_infer.rs", "log_level: LogLevel", "&args.log_level", "inference_device: InferenceDevice", "PARAKEET_DEVICE", "InferenceDevice::Cpu", "InferenceDevice::NvidiaGpu", "InferenceDevice::Auto")
need("src/config.rs", "pub enum InferenceDevice", "NvidiaGpu", "Auto", "auto_inference_device", "inference_device: InferenceDevice")
need("ahk/lib/SettingsGui.ahk", '["auto", "nvidia_gpu", "cpu"]', 'toggle_delivery_hotkey', 'config.Set("inference_device"')
need("src/logging.rs", "component={component} pid={}", "prefix_lines", "component_prefix_survives_split_writes_and_multiline_events")
need("src/capture/inference_supervisor.rs", '.arg("--log-level")', '.arg("--inference-device")')
forbid("src/bin/simple_stt_capture.rs", "worker.lock().unwrap().worker_pid()", "worker.lock().unwrap().replace_config")
checks.append("Parakeet DLL/model loading is isolated to disposable simple-stt-infer with exact-PID forced-exit fallback")

# AHK shell owns desktop behavior and supports safe typed or clipboard-backed delivery.
tray_ahk = need("ahk/lib/Tray.ahk", "A_TrayMenu", "Open Settings", "Restart Audio Service", "Unload Speech Model")
for needle in ["TraySetIcon", "SetColor", "SetPreferredAppMode", "FlushMenuThemes"]:
    if needle in tray_ahk:
        errors.append(f"tray menu must use the default Windows/AHK drawing path, but found {needle!r}")
for path in ["ahk/lib/SettingsGui.ahk", "ahk/simple-stt.ahk"]:
    body = text(path)
    for needle in ["GetProcAddress", "SetPreferredAppMode"]:
        if needle in body:
            errors.append(f"{path} must not set process-wide app theme state; found {needle!r}")
need("ahk/lib/SettingsGui.ahk", "Gui(", "Record shortcut", "Refresh devices", "Download model", "Runtime locations", "text_delivery_mode")
hotkeys_ahk = need("ahk/lib/Hotkeys.ahk", "Hotkey(", "InputHook(", "*CapsLock", "CapsLock & ", "AltGr", "SetCapsLockState")
if '"*CapsLock & "' in hotkeys_ahk:
    errors.append("CapsLock custom combination must not prepend wildcard; combinations already wildcard-match")
need("ahk/lib/TabProtocol.ahk", 'StrReplace(value . "", "\\", "\\\\")', 'case "\\": out .= "\\"', "Loop 20", "unable to read helper response after retry")
need("ahk/lib/Utils.ahk", 'DllCall("advapi32\\SystemFunction036"')
typist = need("ahk/lib/Typist.ahk", "SendText(", "ClipboardAll()", 'A_Clipboard := this.text', 'Send("^+v")', 'Send("^v")', "RestoreClipboardIfOwned", "GetClipboardSequenceNumber", "WinActive(\"A\") != this.targetWindow", "AnyPhysicalModifierDown")
checks.append("AHK owns tray, GUI, hotkeys, full-format clipboard-preserving paste modes, and foreground-safe Unicode typing")

# The shell stays non-blocking for service calls and reconnects after a new capture PID.
ipc_ahk = need("ahk/lib/IpcClient.ahk", "Run(command", "SetTimer", "poll-events --after-seq ", "--wait-ms 900", "ResetServiceSession", "this.latestSeq := 0", "RetryPing", "simple-stt-ctl helper timed out", 'responseReady := FileExist(job["path"])', '"missing_since"', "A_TickCount - job[\"missing_since\"] < 250")
supervisor_ahk = need("ahk/lib/ProcessSupervisor.ahk", "Run(command", "ProcessWaitClose", "ProcessClose", "SimpleSttRandomToken", "ResetServiceSession", "readyProbeInFlight", "this.startTimer")
need("ahk/simple-stt.ahk", "pendingStarts", "pendingStops", "recording stop deferred until start acknowledgement", "ToggleDeliveryModeHotkey", "deliveryToggleHotkey")
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
need("scripts/build-distribution.ps1", "fixtures\\parakeet-smoke.wav")
need("scripts/build-release.ps1", "--bin simple-stt-capture --bin simple-stt-infer --bin simple-stt-ctl")
need("scripts/memory-cleanup-validation.ps1", "Get-Process", "nvidia-smi", "unload-model")
checks.append("schema-v2 migration, install-relative paths, atomic writes, HTTPS partial downloads, and diagnostics are present")

# Deterministic worker integration coverage exists but is intentionally not shipped.
need("src/bin/simple_stt_mock_infer.rs", "Test-only disposable inference worker", "hang-handshake", "mock مرحبا 世界 🙂", "MessageType::WarmUp", "MessageType::ModelLoaded", "MessageType::WarmUpAck")
need("tests/worker_lifecycle.rs", "worker_launches_lazily_and_reuses_warm_process", "warm_up_loads_and_primes_worker_before_first_transcript", "worker_exits_after_idle_timeout", "model_switch_recycles_worker_before_next_request", "device_switch_recycles_worker_before_next_request", "crashed_worker_is_discarded_and_recoverable", "blocked_inference_is_force_terminated_by_exact_pid")
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
