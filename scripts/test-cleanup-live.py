#!/usr/bin/env python3
"""Run the real debug Settings cleanup path using the ignored local .env."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen
from urllib.error import HTTPError


ROOT = Path(__file__).resolve().parents[1]


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
    with tempfile.TemporaryDirectory(prefix="simple-stt-cleanup-live-") as temp:
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
            result = request_json(
                origin + "/api/cleanup-action",
                token,
                "POST",
                {
                    "action": "test",
                    "config": state["config"],
                    "transcript": "uh hello jason no sorry Jayson comma this is a live test",
                },
            )
            print(
                "PASS: live Settings cleanup E2E "
                f"model={result['result']['model']} latency_ms={result['result']['latency_ms']}"
            )
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()


if __name__ == "__main__":
    main()
