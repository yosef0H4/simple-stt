#!/usr/bin/env python3
"""Exercise Settings -> cleanup client through a real local HTTP process boundary."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]


class Provider(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        if self.path != "/chat/completions":
            self.send_error(404)
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert self.headers.get("Authorization") == "Bearer settings-e2e-key"
        assert body["model"] == "settings-e2e-model"
        assert body["stream"] is False
        payload = json.dumps(
            {
                "choices": [
                    {
                        "message": {"content": "Hello Jayson, this is an end-to-end test."},
                        "finish_reason": "stop",
                    }
                ]
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def request_json(url, token, method="GET", body=None):
    encoded = None if body is None else json.dumps(body).encode()
    request = Request(url, data=encoded, method=method)
    request.add_header("X-Simple-STT-Token", token)
    request.add_header("Origin", urlparse(url).scheme + "://" + urlparse(url).netloc)
    if encoded is not None:
        request.add_header("Content-Type", "application/json")
    with urlopen(request, timeout=20) as response:
        return json.load(response)


def main():
    provider = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    binary = ROOT / "target" / "debug" / (
        "simple-stt-settings.exe" if os.name == "nt" else "simple-stt-settings"
    )
    if not binary.is_file():
        raise SystemExit(f"settings binary missing: {binary}")

    with tempfile.TemporaryDirectory(prefix="simple-stt-cleanup-e2e-") as temp:
        env = os.environ.copy()
        env.update(
            {
                "SIMPLE_STT_CONFIG": str(Path(temp) / "config.json"),
                "SIMPLE_STT_AI_API_KEY": "settings-e2e-key",
                "SIMPLE_STT_AI_BASE_URL": f"http://127.0.0.1:{provider.server_port}",
                "SIMPLE_STT_AI_MODEL": "settings-e2e-model",
            }
        )
        process = subprocess.Popen(
            [str(binary), "--no-browser"],
            cwd=ROOT,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            launch_url = process.stdout.readline().strip()
            parsed = urlparse(launch_url)
            token = parse_qs(parsed.fragment)["token"][0]
            origin = f"{parsed.scheme}://{parsed.netloc}"
            state = request_json(origin + "/api/state", token)
            compatible = state["config"]["cleanup"]["openai_compatible"]
            assert compatible["base_url"] == env["SIMPLE_STT_AI_BASE_URL"]
            assert compatible["model"] == env["SIMPLE_STT_AI_MODEL"]
            result = request_json(
                origin + "/api/cleanup-action",
                token,
                "POST",
                {
                    "action": "test",
                    "config": state["config"],
                    "transcript": "uh hello jason no sorry Jayson",
                },
            )
            assert result["message"] == "Cleanup test passed"
            assert result["result"]["text"] == "Hello Jayson, this is an end-to-end test."
            print("PASS: Settings cleanup end-to-end process and HTTP test")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
            provider.shutdown()


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"FAIL: Settings cleanup E2E: {error}", file=sys.stderr)
        raise
