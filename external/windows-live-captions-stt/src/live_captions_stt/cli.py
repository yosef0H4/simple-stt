from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

from live_captions_stt.helper_manager import find_default_model_root, run_helper, run_helper_json
from live_captions_stt.text_normalize import contains_expected, normalize_text


def _convert_to_wav(source: Path) -> Path:
    if source.suffix.lower() == ".wav":
        return source
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise RuntimeError("ffmpeg is required to convert non-WAV audio for direct recognition.")
    target = source.with_suffix(".direct-16k.wav")
    subprocess.run(
        [
            ffmpeg,
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(source),
            "-ac",
            "1",
            "-ar",
            "16000",
            "-sample_fmt",
            "s16",
            str(target),
        ],
        check=True,
    )
    return target


def cmd_direct_models(args: argparse.Namespace) -> int:
    helper_args = ["list-models"]
    helper_args.extend(["--model-root", args.model_root or str(find_default_model_root())])
    return run_helper(helper_args)


def cmd_direct_recognize(args: argparse.Namespace) -> int:
    audio = Path(args.audio)
    if not audio.exists():
        print(f"Audio file not found: {audio}", file=sys.stderr)
        return 2
    wav = _convert_to_wav(audio)
    helper_args = ["recognize-file", "--audio", str(wav.resolve())]
    helper_args.extend(["--model-root", args.model_root or str(find_default_model_root())])
    if args.model:
        helper_args.extend(["--model", args.model])
    if args.license_mode:
        helper_args.extend(["--license-mode", args.license_mode])
    if args.profanity:
        helper_args.extend(["--profanity", args.profanity])
    return run_helper(helper_args)


def _direct_helper_args(args: argparse.Namespace, wav: Path) -> list[str]:
    helper_args = ["recognize-file", "--audio", str(wav.resolve())]
    helper_args.extend(["--model-root", args.model_root or str(find_default_model_root())])
    if getattr(args, "model", None):
        helper_args.extend(["--model", args.model])
    if getattr(args, "license_mode", None):
        helper_args.extend(["--license-mode", args.license_mode])
    if getattr(args, "profanity", None):
        helper_args.extend(["--profanity", args.profanity])
    return helper_args


def cmd_direct_verify_file(args: argparse.Namespace) -> int:
    audio = Path(args.audio)
    if not audio.exists():
        print(f"Audio file not found: {audio}", file=sys.stderr)
        return 2
    wav = _convert_to_wav(audio)
    deadline = time.monotonic() + args.timeout
    attempt = 0
    last_text = ""
    while time.monotonic() < deadline:
        attempt += 1
        print(f"Attempt {attempt}: recognizing {audio}")
        result = run_helper_json(_direct_helper_args(args, wav))
        last_text = str(result.get("text") or "")
        if contains_expected(last_text, args.expected):
            print("Matched expected text.")
            return 0
        if args.once:
            break
    print("Expected text was not found in direct recognizer output.", file=sys.stderr)
    print("Expected:", normalize_text(args.expected), file=sys.stderr)
    print("Observed:", normalize_text(last_text), file=sys.stderr)
    return 1


def cmd_direct_mic(args: argparse.Namespace) -> int:
    helper_args = ["recognize-mic"]
    if args.seconds is not None:
        helper_args.extend(["--seconds", str(args.seconds)])
    helper_args.extend(["--model-root", args.model_root or str(find_default_model_root())])
    if args.model:
        helper_args.extend(["--model", args.model])
    if args.license_mode:
        helper_args.extend(["--license-mode", args.license_mode])
    if args.profanity:
        helper_args.extend(["--profanity", args.profanity])
    if args.json:
        helper_args.append("--json")
    if args.final_only:
        helper_args.append("--final-only")
    return run_helper(helper_args)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Windows embedded speech recognition CLI")
    sub = parser.add_subparsers(dest="command", required=True)

    direct_models = sub.add_parser("direct-models", help="List embedded Windows speech recognition models")
    direct_models.add_argument("--model-root", default=None)
    direct_models.set_defaults(func=cmd_direct_models)

    direct = sub.add_parser("direct-recognize", help="Recognize an audio file using the Windows embedded speech model")
    direct.add_argument("--audio", required=True)
    direct.add_argument("--model-root", default=None)
    direct.add_argument("--model", default=None)
    direct.add_argument("--license-mode", choices=("key", "legal"), default=None)
    direct.add_argument("--profanity", choices=("raw", "masked", "removed"), default="raw")
    direct.set_defaults(func=cmd_direct_recognize)

    direct_verify = sub.add_parser("direct-verify-file", help="Recognize an audio file until expected text appears")
    direct_verify.add_argument("--audio", required=True)
    direct_verify.add_argument("--expected", required=True)
    direct_verify.add_argument("--model-root", default=None)
    direct_verify.add_argument("--model", default=None)
    direct_verify.add_argument("--license-mode", choices=("key", "legal"), default=None)
    direct_verify.add_argument("--profanity", choices=("raw", "masked", "removed"), default="raw")
    direct_verify.add_argument("--timeout", type=float, default=90.0)
    direct_verify.add_argument("--once", action="store_true")
    direct_verify.set_defaults(func=cmd_direct_verify_file)

    direct_mic = sub.add_parser("direct-mic", help="Print default microphone speech until Ctrl+C")
    direct_mic.add_argument("--seconds", type=int, default=None, help="Optional duration limit; omit to run until Ctrl+C")
    direct_mic.add_argument("--model-root", default=None)
    direct_mic.add_argument("--model", default=None)
    direct_mic.add_argument("--license-mode", choices=("key", "legal"), default=None)
    direct_mic.add_argument("--profanity", choices=("raw", "masked", "removed"), default="raw")
    direct_mic.add_argument("--json", action="store_true", help="Print JSON events instead of plain text")
    direct_mic.add_argument("--final-only", action="store_true", help="Only print finalized recognition segments")
    direct_mic.set_defaults(func=cmd_direct_mic)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
