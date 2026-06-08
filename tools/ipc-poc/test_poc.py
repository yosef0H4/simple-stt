#!/usr/bin/env python3
"""Exercises the dependency-free loopback IPC proof of concept."""
from __future__ import annotations
import json
import pathlib
import socket
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parent
SERVICE = ROOT / "mock_service.py"


def wait_for_state(path: pathlib.Path, timeout: float = 3.0) -> dict:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                pass
        time.sleep(0.025)
    raise RuntimeError(f"state file was not published: {path}")


def request(state: dict, token: str, command: dict, request_id: int = 1) -> dict:
    host, port = state["address"].rsplit(":", 1)
    with socket.create_connection((host, int(port)), timeout=2.0) as conn:
        stream = conn.makefile("rwb")
        send(stream, {"type": "hello", "protocol": 1, "token": token})
        hello_ack = recv(stream)
        assert hello_ack == {"type": "hello_ack", "protocol": 1}, hello_ack
        send(stream, {"type": "command", "request_id": request_id, "command": command})
        response = recv(stream)
        assert response["type"] == "response", response
        assert response["request_id"] == request_id, response
        return response["response"]


def send(stream, value: dict) -> None:
    stream.write((json.dumps(value, ensure_ascii=False) + "\n").encode("utf-8"))
    stream.flush()


def recv(stream) -> dict:
    line = stream.readline()
    if not line:
        raise RuntimeError("service closed connection")
    return json.loads(line.decode("utf-8"))


def launch(state_path: pathlib.Path, token: str) -> subprocess.Popen:
    return subprocess.Popen([sys.executable, str(SERVICE), "--state", str(state_path), "--token", token])


def stop(process: subprocess.Popen) -> None:
    process.terminate()
    try:
        process.wait(timeout=2.0)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=2.0)


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="uvox-ipc-poc-") as raw:
        temp = pathlib.Path(raw)
        state_path = temp / "capture-state.json"
        token = uuid.uuid4().hex
        service = launch(state_path, token)
        try:
            state = wait_for_state(state_path)
            assert request(state, token, {"name": "ping"})["message"] == "pong"
            assert request(state, token, {"name": "start_recording", "session_id": 42})["ok"]
            assert request(state, token, {"name": "stop_recording", "session_id": 42})["ok"]
            events = request(state, token, {"name": "poll_events", "after_seq": 0})["events"]
            assert [event["kind"] for event in events] == ["recording_started", "transcript"], events
            assert events[-1]["text"] == "مرحبا 世界 🙂", events[-1]
        finally:
            stop(service)

        # Simulate capture-service crash/restart. The shell helper reconnects by
        # reopening the state file and using the new launch token.
        try:
            state_path.unlink()
        except FileNotFoundError:
            pass
        next_token = uuid.uuid4().hex
        service = launch(state_path, next_token)
        try:
            next_state = wait_for_state(state_path)
            assert next_state["pid"] != state["pid"], (state, next_state)
            assert request(next_state, next_token, {"name": "ping"})["message"] == "pong"
        finally:
            stop(service)
    print("IPC POC PASSED")
    print(" - authenticated loopback PING/PONG")
    print(" - START_RECORDING / STOP_RECORDING")
    print(" - asynchronous polled Unicode TRANSCRIPT")
    print(" - state-file reconnect after simulated service restart")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
