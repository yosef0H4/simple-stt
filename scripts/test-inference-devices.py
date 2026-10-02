#!/usr/bin/env python3
import argparse
import os
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

MAGIC = b"UVX1"
VERSION = 1
HELLO = 1
HELLO_ACK = 2
TRANSCRIBE_WAV = 4
TRANSCRIPT = 5
ERROR = 6
SHUTDOWN = 7
SHUTDOWN_ACK = 8


def frame(kind: int, body: bytes = b"", session_id: int = 0) -> bytes:
    return MAGIC + struct.pack("<HHIQ", VERSION, kind, len(body), session_id) + body


def read_exact(stream, count: int) -> bytes:
    data = bytearray()
    while len(data) < count:
        chunk = stream.read(count - len(data))
        if not chunk:
            raise RuntimeError(f"worker closed pipe after {len(data)} of {count} bytes")
        data.extend(chunk)
    return bytes(data)


def read_frame(stream):
    header = read_exact(stream, 20)
    if header[:4] != MAGIC:
        raise RuntimeError(f"bad worker magic: {header[:4]!r}")
    version, kind, body_len, session_id = struct.unpack("<HHIQ", header[4:])
    if version != VERSION:
        raise RuntimeError(f"unexpected worker version {version}")
    body = read_exact(stream, body_len)
    return kind, session_id, body


def request(process, kind: int, body: bytes = b"", session_id: int = 0):
    assert process.stdin is not None
    assert process.stdout is not None
    process.stdin.write(frame(kind, body, session_id))
    process.stdin.flush()
    return read_frame(process.stdout)


def run_mode(root: Path, mode: str, language: str, expected_gpu: str | None = None) -> str:
    windows = sys.platform == "win32"
    runtime = root / "external" / "parakeet-runtime" / ("parakeet-windows-vulkan" if windows else "parakeet-linux-vulkan")
    library = "parakeet.dll" if windows else "libparakeet.so"
    model_name = "lemura-arabic-asr-lite-q8_0.gguf" if language == "arabic" else "tdt_ctc-110m-q8_0.gguf"
    model = root / "external" / "parakeet-runtime" / "models" / model_name
    audio = root / "fixtures" / "asr" / "arabic-sa.wav" if language == "arabic" else root / "fixtures" / "parakeet-smoke.wav"
    executable = "simple-stt-infer.exe" if windows else "simple-stt-infer"
    exe = root / "target" / "release" / executable
    if not exe.exists():
        exe = root / executable
    log = root / "artifacts" / f"simple-stt-infer-{language}-{mode}.log"
    for required in [runtime / "bin" / library, model, audio, exe]:
        if not required.exists():
            raise RuntimeError(f"missing required file: {required}")
    command = [
        str(exe), "--runtime-dir", str(runtime), "--model-path", str(model),
        "--log-path", str(log), "--log-level", "debug",
        "--inference-device", mode, "--language", language, "--idle-timeout-secs", "60",
    ]
    started = time.perf_counter()
    env = os.environ.copy()
    if mode == "cpu":
        env["PARAKEET_DEVICE"] = "cpu"
    else:
        env.pop("PARAKEET_DEVICE", None)
    native_output = tempfile.TemporaryFile()
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=native_output,
        env=env,
    )
    try:
        kind, _, body = request(process, HELLO)
        if kind != HELLO_ACK:
            raise RuntimeError(f"{mode}: expected HELLO_ACK, got {kind}: {body!r}")
        kind, _, body = request(process, TRANSCRIBE_WAV, str(audio).encode("utf-8"), 42)
        if kind == ERROR:
            raise RuntimeError(f"{mode}: worker error: {body.decode('utf-8', errors='replace')}")
        if kind != TRANSCRIPT:
            raise RuntimeError(f"{mode}: expected TRANSCRIPT, got {kind}: {body!r}")
        transcript = body.decode("utf-8")
        if not transcript.strip():
            raise RuntimeError(f"{mode}: transcript was empty")
        if mode == "gpu" and expected_gpu and sys.platform == "linux":
            evidence = subprocess.check_output([
                "nvidia-smi", "--query-compute-apps=pid,process_name,used_gpu_memory",
                "--format=csv,noheader",
            ], text=True)
            assert any(row.strip().startswith(f"{process.pid},") for row in evidence.splitlines()), evidence
            print(f"PASS NVIDIA VRAM allocation: worker PID {process.pid}\n{evidence.strip()}")
        shutdown_kind, _, shutdown_body = request(process, SHUTDOWN)
        if shutdown_kind != SHUTDOWN_ACK:
            raise RuntimeError(f"{mode}: expected SHUTDOWN_ACK, got {shutdown_kind}: {shutdown_body!r}")
        process.wait(timeout=10)
        native_output.seek(0)
        native_log = native_output.read().decode("utf-8", errors="replace")
        log.with_suffix(".native.log").write_text(native_log, encoding="utf-8")
        if mode == "cpu" and "using device: Vulkan" in native_log:
            raise RuntimeError(f"{mode}: native runtime selected a GPU despite CPU mode: {native_log}")
        if mode == "gpu" and "using device: Vulkan" not in native_log:
            raise RuntimeError(f"{mode}: native runtime did not report Vulkan selection: {native_log}")
        if mode == "gpu" and expected_gpu:
            selected = native_log.split("using device: ")[-1].splitlines()[0]
            index = selected.removeprefix("Vulkan")
            device_line = next((line for line in native_log.splitlines()
                                if line.startswith(f"ggml_vulkan: {index} = ")), "")
            if expected_gpu not in device_line:
                raise RuntimeError(f"expected GPU {expected_gpu!r}, got {device_line!r}")
            print(f"PASS physical GPU: {device_line}")
        elapsed = time.perf_counter() - started
        backend = "cpu" if mode == "cpu" else "Vulkan"
        print(f"PASS {language}/{mode}: backend={backend} {elapsed:.2f}s transcript={transcript!r}")
        return transcript
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
        native_output.close()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["cpu", "gpu", "both"], default="both")
    parser.add_argument("--language", choices=["english", "arabic", "both"], default="both")
    parser.add_argument("--expect-gpu", help="Require this physical GPU name; NVIDIA VRAM checked on Linux")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    root = args.root.resolve()
    modes = ["cpu", "gpu"] if args.mode == "both" else [args.mode]
    languages = ["english", "arabic"] if args.language == "both" else [args.language]
    for language in languages:
        transcripts = [run_mode(root, mode, language, args.expect_gpu) for mode in modes]
        if len(transcripts) == 2 and transcripts[0] != transcripts[1]:
            raise RuntimeError(f"{language} CPU/GPU transcript mismatch: {transcripts!r}")
        if len(transcripts) == 2:
            print(f"PASS {language} cpu_vs_gpu: transcripts match")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
