"""Compare disposable workers, one reused native process, and two warm workers.

Run after the release build with psutil installed. Results are JSON so memory
trends and median switch latency can be compared without changing production.
"""
import argparse
import ctypes
import json
import os
import statistics
import struct
import subprocess
import time
from pathlib import Path

import psutil

MAGIC = b"UVX1"
WAV = 4
HELLO = 1
WARM = 11
SHUTDOWN = 7


def frame(kind: int, body: bytes = b"") -> bytes:
    return MAGIC + struct.pack("<HHIQ", 1, kind, len(body), 0) + body


def read_frame(stream):
    header = stream.read(20)
    if len(header) != 20 or header[:4] != MAGIC:
        raise RuntimeError("worker closed its protocol stream")
    _, kind, length, _ = struct.unpack("<HHIQ", header[4:])
    body = stream.read(length)
    if len(body) != length:
        raise RuntimeError("short worker response")
    if kind == 6:
        raise RuntimeError(body.decode("utf-8", errors="replace"))
    return kind, body


def private_bytes(pid: int) -> int:
    info = psutil.Process(pid).memory_full_info()
    return getattr(info, "private", info.rss)


def worker_request(worker, kind: int, body: bytes = b""):
    worker.stdin.write(frame(kind, body))
    worker.stdin.flush()
    return read_frame(worker.stdout)


def models_and_audio(root: Path):
    models = root / "external/parakeet-runtime/models"
    return [
        ("english", models / "tdt_ctc-110m-q8_0.gguf", root / "fixtures/parakeet-smoke.wav"),
        ("arabic", models / "lemura-arabic-asr-lite-q8_0.gguf", root / "fixtures/asr/arabic-sa.wav"),
    ]


def process_recycle(root: Path, runtime: Path, cases, count: int, device: str):
    executable = root / "target/release/simple-stt-infer.exe"
    samples = []
    expected = {}
    for index in range(count):
        language, model, audio = cases[index % 2]
        command = [str(executable), "--runtime-dir", str(runtime), "--model-path", str(model),
                   "--log-path", str(root / "artifacts/benchmark-infer.log"), "--language", language,
                   "--inference-device", device]
        started = time.perf_counter()
        worker = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        try:
            worker_request(worker, HELLO)
            _, text = worker_request(worker, WAV, str(audio).encode("utf-8"))
            transcript = text.decode("utf-8")
            if language in expected and transcript != expected[language]:
                raise RuntimeError(f"{language} transcript changed after recycle")
            expected[language] = transcript
            loaded_private = private_bytes(worker.pid)
            worker_request(worker, SHUTDOWN)
            worker.wait(timeout=15)
            samples.append({"language": language, "seconds": time.perf_counter() - started,
                            "loaded_private_bytes": loaded_private})
        finally:
            if worker.poll() is None:
                worker.kill()
                worker.wait()
    return {"samples": samples, "median_seconds": statistics.median(row["seconds"] for row in samples)}


def same_process(runtime: Path, cases, count: int, device: str):
    os.environ["PARAKEET_DEVICE"] = "cpu" if device == "cpu" else "Vulkan0"
    library = ctypes.CDLL(str(runtime / "bin/parakeet.dll"))
    library.parakeet_capi_load.argtypes = [ctypes.c_char_p]
    library.parakeet_capi_load.restype = ctypes.c_void_p
    library.parakeet_capi_free.argtypes = [ctypes.c_void_p]
    library.parakeet_capi_transcribe_path.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    library.parakeet_capi_transcribe_path.restype = ctypes.c_void_p
    library.parakeet_capi_free_string.argtypes = [ctypes.c_void_p]
    samples = []
    expected = {}
    for index in range(count):
        language, model, audio = cases[index % 2]
        started = time.perf_counter()
        context = library.parakeet_capi_load(str(model).encode("utf-8"))
        if not context:
            raise RuntimeError(f"failed to load {model}")
        load_seconds = time.perf_counter() - started
        try:
            result = library.parakeet_capi_transcribe_path(context, str(audio).encode("utf-8"), 0)
            if not result:
                raise RuntimeError(f"failed to transcribe {audio}")
            try:
                transcript = ctypes.string_at(result).decode("utf-8")
            finally:
                library.parakeet_capi_free_string(result)
        finally:
            library.parakeet_capi_free(context)
        if language in expected and transcript != expected[language]:
            raise RuntimeError(f"{language} transcript changed after same-process switch")
        expected[language] = transcript
        samples.append({"language": language, "load_seconds": load_seconds,
                        "seconds": time.perf_counter() - started,
                        "after_free_private_bytes": private_bytes(os.getpid())})
    memory = [row["after_free_private_bytes"] for row in samples]
    baseline = statistics.median(memory[4:10]) if len(memory) >= 10 else memory[0]
    final = statistics.median(memory[-5:])
    return {"samples": samples, "median_seconds": statistics.median(row["seconds"] for row in samples),
            "steady_baseline_bytes": baseline, "final_after_free_bytes": final,
            "final_growth_bytes": final - baseline}


def two_warm_workers(root: Path, runtime: Path, cases, count: int, device: str):
    executable = root / "target/release/simple-stt-infer.exe"
    workers = []
    expected = {}
    try:
        for language, model, _ in cases:
            command = [str(executable), "--runtime-dir", str(runtime), "--model-path", str(model),
                       "--log-path", str(root / "artifacts/benchmark-infer.log"), "--language", language,
                       "--inference-device", device]
            worker = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            workers.append(worker)
            worker_request(worker, HELLO)
            worker_request(worker, WARM)
            read_frame(worker.stdout)
        samples = []
        for index in range(count):
            language, _, audio = cases[index % 2]
            started = time.perf_counter()
            _, text = worker_request(workers[index % 2], WAV, str(audio).encode("utf-8"))
            transcript = text.decode("utf-8")
            if language in expected and transcript != expected[language]:
                raise RuntimeError(f"{language} warm transcript changed")
            expected[language] = transcript
            samples.append({"language": language, "seconds": time.perf_counter() - started,
                            "combined_private_bytes": sum(private_bytes(worker.pid) for worker in workers)})
        return {"samples": samples, "median_seconds": statistics.median(row["seconds"] for row in samples)}
    finally:
        for worker in workers:
            if worker.poll() is None:
                try:
                    worker_request(worker, SHUTDOWN)
                    worker.wait(timeout=15)
                except Exception:
                    worker.kill()
                    worker.wait()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--switches", type=int, default=50)
    parser.add_argument("--device", choices=["cpu", "gpu"], default="gpu")
    parser.add_argument("--output", type=Path, default=Path("artifacts/language-switch-benchmark.json"))
    args = parser.parse_args()
    root = args.root.resolve()
    runtime = root / "external/parakeet-runtime/parakeet-windows-vulkan"
    cases = models_and_audio(root)
    count = args.switches + 1
    results = {
        "device": args.device, "switches": args.switches,
        "process_recycle": process_recycle(root, runtime, cases, count, args.device),
        "same_process": same_process(runtime, cases, count, args.device),
        "two_warm_workers": two_warm_workers(root, runtime, cases, count, args.device),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({strategy: {key: value for key, value in result.items() if key != "samples"}
                      for strategy, result in results.items() if isinstance(result, dict)}, indent=2))
    print(f"Saved {args.output}")


if __name__ == "__main__":
    main()
