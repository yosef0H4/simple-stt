from __future__ import annotations

import os
import json
import shutil
import subprocess
import sys
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parents[2]
REPO_ROOT = PROJECT_ROOT.parents[2]
BUILD_DIR = PROJECT_ROOT / ".build" / "windows-live-captions-stt-helper"
HELPER_EXE = BUILD_DIR / "windows_live_captions_stt_helper.exe"
HELPER_SOURCE = PROJECT_ROOT / "src" / "live_captions_stt" / "windows_live_captions_stt_helper.cs"
LOCAL_SDK_CACHE = PROJECT_ROOT / ".cache" / "windows-live-captions-sdk"
WINDOWS_NATURAL_BUILD = REPO_ROOT / "services" / "tts" / "windows_natural" / ".build" / "windows-natural-helper"
WINDOWS_NATURAL_CACHE = REPO_ROOT / "services" / "tts" / "windows_natural" / ".cache" / "windows-natural-sdk"
KNOWN_SDK_CACHES = (
    LOCAL_SDK_CACHE,
    WINDOWS_NATURAL_CACHE,
    Path(r"Z:\files\projects\js\tts-electron\services\tts\windows_natural\.cache\windows-natural-sdk"),
)
KNOWN_BUILD_DIRS = (
    WINDOWS_NATURAL_BUILD,
    Path(r"Z:\files\projects\js\tts-electron\services\tts\windows_natural\.build\windows-natural-helper"),
    Path(r"C:\Program Files\WindowsApps\Microsoft.MicrosoftOfficeHub_19.2605.59121.0_x64__8wekyb3d8bbwe"),
)
SYSTEM_WEB_EXTENSIONS = Path(r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\System.Web.Extensions.dll")
LIVE_CAPTIONS_RUNTIME = Path(r"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\LiveCaptions")
CLIENT_CORE_ROOT = Path(r"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy")
CSC_CANDIDATES = (
    Path(r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe"),
    Path(r"C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe"),
    Path(r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\Roslyn\csc.exe"),
)

SPEECH_DLLS = (
    "Microsoft.CognitiveServices.Speech.core.dll",
    "Microsoft.CognitiveServices.Speech.csharp.dll",
    "Microsoft.CognitiveServices.Speech.extension.audio.sys.dll",
    "Microsoft.CognitiveServices.Speech.extension.codec.dll",
    "Microsoft.CognitiveServices.Speech.extension.embedded.sr.dll",
    "Microsoft.CognitiveServices.Speech.extension.embedded.sr.runtime.dll",
    "Microsoft.CognitiveServices.Speech.extension.kws.dll",
    "Microsoft.CognitiveServices.Speech.extension.kws.ort.dll",
    "Microsoft.CognitiveServices.Speech.extension.lu.dll",
    "Microsoft.CognitiveServices.Speech.extension.onnxruntime.dll",
    "Microsoft.CognitiveServices.Speech.extension.telemetry.dll",
    "msvcp140_app.dll",
    "msvcp140_codecvt_ids_app.dll",
    "vcruntime140_app.dll",
    "vcruntime140_1_app.dll",
)


def find_csc() -> Path:
    for candidate in CSC_CANDIDATES:
        if candidate.exists():
            return candidate
    raise RuntimeError("Could not find csc.exe for compiling the STT helper.")


def _copy_from_cache(name: str) -> bool:
    candidates = [LIVE_CAPTIONS_RUNTIME / name, CLIENT_CORE_ROOT / name]
    candidates.extend(build_dir / name for build_dir in KNOWN_BUILD_DIRS)
    for cache in KNOWN_SDK_CACHES:
        candidates.extend(
            [
                cache / "speech" / "runtimes" / "win-x64" / "native" / name,
                cache / "speech" / "lib" / "net462" / name,
                cache / "onnx" / "runtimes" / "win-x64" / "native" / name,
                cache / "telemetry" / "runtimes" / "win-x64" / "native" / name,
            ]
        )
    for candidate in candidates:
        if candidate.exists():
            shutil.copy2(candidate, BUILD_DIR / name)
            return True
    return False


def stage_speech_sdk() -> None:
    BUILD_DIR.mkdir(parents=True, exist_ok=True)
    missing: list[str] = []
    for name in SPEECH_DLLS:
        source = LIVE_CAPTIONS_RUNTIME / name
        if not source.exists():
            source = WINDOWS_NATURAL_BUILD / name
        if source.exists():
            shutil.copy2(source, BUILD_DIR / name)
        elif not _copy_from_cache(name):
            missing.append(name)
    if missing:
        raise RuntimeError("Missing Speech SDK helper DLLs: " + ", ".join(missing))


def helper_is_stale() -> bool:
    if not HELPER_EXE.exists():
        return True
    return HELPER_EXE.stat().st_mtime < HELPER_SOURCE.stat().st_mtime


def compile_helper() -> Path:
    stage_speech_sdk()
    if not helper_is_stale():
        return HELPER_EXE
    csc = find_csc()
    cmd = [
        str(csc),
        "/nologo",
        "/optimize+",
        "/platform:x64",
        f"/out:{HELPER_EXE}",
        f"/reference:{BUILD_DIR / 'Microsoft.CognitiveServices.Speech.csharp.dll'}",
        f"/reference:{SYSTEM_WEB_EXTENSIONS}",
        str(HELPER_SOURCE),
    ]
    subprocess.run(cmd, cwd=PROJECT_ROOT, check=True)
    return HELPER_EXE


def run_helper(args: list[str]) -> int:
    proc = _run_helper_process(args, capture=False)
    return int(proc.returncode)


def run_helper_json(args: list[str]) -> dict[str, object]:
    proc = _run_helper_process(args, capture=True)
    if proc.stdout:
        print(proc.stdout.strip())
    if proc.returncode != 0:
        if proc.stderr:
            print(proc.stderr.strip(), file=sys.stderr)
        raise RuntimeError(f"helper failed with exit code {proc.returncode}")
    return json.loads(proc.stdout)


def _run_helper_process(args: list[str], capture: bool) -> subprocess.CompletedProcess[str]:
    exe = compile_helper()
    env = os.environ.copy()
    env["PATH"] = os.pathsep.join(
        [
            str(BUILD_DIR),
            str(LIVE_CAPTIONS_RUNTIME),
            str(CLIENT_CORE_ROOT),
            env.get("PATH", ""),
        ]
    )
    return subprocess.run(
        [str(exe), *args],
        cwd=BUILD_DIR,
        env=env,
        text=True,
        capture_output=capture,
    )


def find_default_model_root() -> Path:
    proc = subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-Command",
            "(Get-AppxPackage MicrosoftWindows.Speech.en-US.1).InstallLocation",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    path = proc.stdout.strip()
    if path:
        root = Path(path)
        if root.exists():
            return root
    candidates = sorted(Path(r"C:\Program Files\WindowsApps").glob("MicrosoftWindows.Speech.en-US.1_*"))
    if candidates:
        return candidates[-1]
    raise RuntimeError("Could not find the installed MicrosoftWindows.Speech.en-US.1 model package.")
