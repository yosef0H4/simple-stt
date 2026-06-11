#!/usr/bin/env python3
import argparse
import os
import struct
import subprocess
import sys
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


def run_mode(root: Path, mode: str) -> str:
    runtime = root / "external" / "parakeet-runtime" / "parakeet-windows-cuda"
    model = runtime / "models" / "tdt_ctc-110m-f16.gguf"
    audio = root / "fixtures" / "parakeet-smoke.wav"
    exe = root / "target" / "release" / "simple-stt-infer.exe"
    log = root / "artifacts" / f"simple-stt-infer-{mode}.log"
    for required in [runtime / "bin" / "parakeet.dll", model, audio, exe]:
        if not required.exists():
            raise RuntimeError(f"missing required file: {required}")
    command = [
        str(exe), "--runtime-dir", str(runtime), "--model-path", str(model),
        "--log-path", str(log), "--log-level", "debug",
        "--inference-device", mode, "--idle-timeout-secs", "60",
    ]
    started = time.perf_counter()
    env = os.environ.copy()
    if mode == "cpu":
        env["PARAKEET_DEVICE"] = "cpu"
    else:
        env.pop("PARAKEET_DEVICE", None)
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
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
        shutdown_kind, _, shutdown_body = request(process, SHUTDOWN)
        if shutdown_kind != SHUTDOWN_ACK:
            raise RuntimeError(f"{mode}: expected SHUTDOWN_ACK, got {shutdown_kind}: {shutdown_body!r}")
        process.wait(timeout=10)
        assert process.stderr is not None
        native_log = process.stderr.read().decode("utf-8", errors="replace")
        if mode == "cpu" and "using GPU device" in native_log:
            raise RuntimeError(f"{mode}: native runtime selected a GPU despite CPU mode: {native_log}")
        if mode == "nvidia_gpu" and "using GPU device: CUDA" not in native_log:
            raise RuntimeError(f"{mode}: native runtime did not report CUDA selection: {native_log}")
        elapsed = time.perf_counter() - started
        backend = "cpu" if mode == "cpu" else "CUDA"
        print(f"PASS {mode}: backend={backend} {elapsed:.2f}s transcript={transcript!r}")
        return transcript
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["cpu", "nvidia_gpu", "both"], default="both")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    modes = ["cpu", "nvidia_gpu"] if args.mode == "both" else [args.mode]
    transcripts = [run_mode(root, mode) for mode in modes]
    if len(transcripts) == 2 and transcripts[0] != transcripts[1]:
        raise RuntimeError(f"CPU/GPU transcript mismatch: {transcripts!r}")
    if len(transcripts) == 2:
        print("PASS cpu_vs_nvidia_gpu: transcripts match")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
