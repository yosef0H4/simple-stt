"""Command-line entry point for CUDA checks, integration tests, and the live worker."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .audio import read_pcm16, validate_nemotron_wav
from .constants import EXPECTED_SAMPLE_WORDS, MODEL_NAME
from .cuda import inspect_cuda, require_cuda
from .errors import UvoxError
from .nemotron import NemotronStreamingRecognizer, StreamingConfig, transcribe_file
from .sample import fetch_sample
from .server import ServerConfig, serve


def _print_json(value: object) -> None:
    print(json.dumps(value, indent=2, ensure_ascii=False, default=str))


def command_doctor(args: argparse.Namespace) -> int:
    try:
        info = inspect_cuda()
    except UvoxError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    _print_json(info.to_dict())
    if not info.available:
        print("ERROR: CUDA is unavailable. This worker intentionally rejects CPU execution.", file=sys.stderr)
        return 2
    if args.check_nemo:
        import nemo.collections.asr  # noqa: F401
        print("NeMo ASR import: OK")
    return 0


def command_fetch_sample(args: argparse.Namespace) -> int:
    path = fetch_sample(Path(args.cache_dir) if args.cache_dir else None, force=args.force)
    info = validate_nemotron_wav(path)
    _print_json({"path": str(path), "duration_seconds": info.duration_seconds})
    return 0


def command_smoke_test(args: argparse.Namespace) -> int:
    require_cuda()
    path = Path(args.audio) if args.audio else fetch_sample()
    validate_nemotron_wav(path)
    text = transcribe_file(path, model_name=args.model)
    print("\n=== TRANSCRIPT ===")
    print(text)
    print("==================\n")
    if not args.audio:
        lowercase = text.lower()
        if not any(word in lowercase for word in EXPECTED_SAMPLE_WORDS):
            print(
                "WARNING: sample transcription did not contain the expected anchor words; "
                "inspect the transcript above.",
                file=sys.stderr,
            )
    return 0


def command_stream_file_test(args: argparse.Namespace) -> int:
    require_cuda()
    path = Path(args.audio) if args.audio else fetch_sample()
    validate_nemotron_wav(path)
    recognizer = NemotronStreamingRecognizer(
        StreamingConfig(model_name=args.model, lookahead_ms=args.lookahead_ms)
    )
    recognizer.reset()
    pcm = read_pcm16(path)
    chunk_bytes = recognizer.config.chunk_samples * 2
    latest = ""
    print(f"Streaming {path} in {chunk_bytes // 2} sample chunks...")
    for offset in range(0, len(pcm) - chunk_bytes + 1, chunk_bytes):
        latest = recognizer.transcribe_chunk(pcm[offset : offset + chunk_bytes])
        print(f"PARTIAL: {latest}")
    # File test only: append silence to expose tail output. Live key-up intentionally discards a tail.
    silence = b"\0" * chunk_bytes
    for _ in range(2):
        latest = recognizer.transcribe_chunk(silence)
        print(f"TAIL:    {latest}")
    print("\n=== FINAL STREAMING TRANSCRIPT ===")
    print(latest)
    print("==================================")
    return 0


def command_serve(args: argparse.Namespace) -> int:
    if args.backend != "echo":
        require_cuda()
    serve(
        ServerConfig(
            connect=args.connect,
            token=args.token,
            lookahead_ms=args.lookahead_ms,
            backend=args.backend,
        )
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="uvox-worker")
    sub = parser.add_subparsers(dest="command", required=True)

    doctor = sub.add_parser("doctor", help="Reject non-CUDA environments and print diagnostics")
    doctor.add_argument("--check-nemo", action="store_true")
    doctor.set_defaults(func=command_doctor)

    sample = sub.add_parser("fetch-sample", help="Download and validate the public test WAV once")
    sample.add_argument("--cache-dir")
    sample.add_argument("--force", action="store_true")
    sample.set_defaults(func=command_fetch_sample)

    smoke = sub.add_parser("smoke-test", help="Whole-file Nemotron CUDA transcription test")
    smoke.add_argument("--audio")
    smoke.add_argument("--model", default=MODEL_NAME)
    smoke.set_defaults(func=command_smoke_test)

    stream = sub.add_parser("stream-file-test", help="Exercise the cache-aware live path on a WAV")
    stream.add_argument("--audio")
    stream.add_argument("--model", default=MODEL_NAME)
    stream.add_argument("--lookahead-ms", type=int, default=80)
    stream.set_defaults(func=command_stream_file_test)

    server = sub.add_parser("serve", help="Connect back to the Rust manager and serve live ASR")
    server.add_argument("--connect", required=True)
    server.add_argument("--token", required=True)
    server.add_argument("--lookahead-ms", type=int, default=80)
    server.add_argument("--backend", choices=("nemotron", "echo"), default="nemotron")
    server.set_defaults(func=command_serve)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return int(args.func(args))
    except UvoxError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
