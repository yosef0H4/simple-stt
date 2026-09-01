#!/usr/bin/env python3
"""Score general ASR cleanup behavior through the real debug Settings process."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
CASES = (
    {
        "name": "semantic near-sound",
        "input": "I need to sea whether the backup finished",
        "required": ("see whether",),
        "forbidden": ("sea whether",),
    },
    {
        "name": "verb near-sound",
        "input": "Please right the changes to the configuration file",
        "required": ("write the changes",),
        "forbidden": ("right the changes",),
    },
    {
        "name": "grammar and homophone",
        "input": "There planning to update they're application tomorrow",
        "required": ("they're planning", "their application"),
        "forbidden": ("there planning", "they're application"),
    },
    {
        "name": "compound recognition",
        "input": "Open the source cold repository and review the latest pool request",
        "required": ("source code repository", "pull request"),
        "forbidden": ("source cold", "pool request"),
    },
    {
        "name": "preserve plausible wording",
        "input": "The blue folder is beside the monitor",
        "required": ("blue folder", "beside the monitor"),
        "forbidden": (),
    },
)


def request_json(url, token, method="GET", body=None):
    encoded = None if body is None else json.dumps(body).encode()
    request = Request(url, data=encoded, method=method)
    request.add_header("X-Simple-STT-Token", token)
    request.add_header("Origin", urlparse(url).scheme + "://" + urlparse(url).netloc)
    if encoded is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urlopen(request, timeout=150) as response:
            return json.load(response)
    except HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"Settings returned HTTP {error.code}: {detail}") from error


def main():
    binary = ROOT / "target" / "debug" / (
        "simple-stt-settings.exe" if os.name == "nt" else "simple-stt-settings"
    )
    if not (ROOT / ".env").is_file():
        raise SystemExit(".env is missing; copy .env.example and fill it first")

    with tempfile.TemporaryDirectory(prefix="simple-stt-cleanup-benchmark-") as temp:
        env = os.environ.copy()
        env["SIMPLE_STT_CONFIG"] = str(Path(temp) / "config.json")
        for name in ("SIMPLE_STT_AI_API_KEY", "SIMPLE_STT_AI_BASE_URL", "SIMPLE_STT_AI_MODEL"):
            env.pop(name, None)
        process = subprocess.Popen(
            [str(binary), "--no-browser"],
            cwd=ROOT,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            launch = urlparse(process.stdout.readline().strip())
            origin = f"{launch.scheme}://{launch.netloc}"
            token = parse_qs(launch.fragment)["token"][0]
            state = request_json(origin + "/api/state", token)
            compatible = state["config"]["cleanup"]["openai_compatible"]
            if not compatible["base_url"] or not compatible["model"]:
                raise RuntimeError("debug Settings did not load the URL and model from .env")

            passed = 0
            total_latency = 0
            for case in CASES:
                response = request_json(
                    origin + "/api/cleanup-action",
                    token,
                    "POST",
                    {
                        "action": "test",
                        "config": state["config"],
                        "transcript": case["input"],
                    },
                )["result"]
                output = response["text"]
                folded = output.casefold()
                ok = all(value in folded for value in case["required"]) and not any(
                    value in folded for value in case["forbidden"]
                )
                passed += int(ok)
                total_latency += response["latency_ms"]
                print(f"{'PASS' if ok else 'FAIL'}: {case['name']} ({response['latency_ms']} ms)")
                print(f"  input:  {case['input']}")
                print(f"  output: {output}")

            print(
                f"SCORE: {passed}/{len(CASES)} "
                f"model={compatible['model']} average_latency_ms={total_latency // len(CASES)}"
            )
            raise SystemExit(0 if passed == len(CASES) else 1)
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()


if __name__ == "__main__":
    main()
