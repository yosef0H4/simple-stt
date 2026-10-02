#!/usr/bin/env python3
"""Opt-in KDE layout switching through the real capture service; no text delivery.

Requires configured English and Arabic layouts. Restores the original layout.
Uses an isolated config/state and never changes the user's app settings.
"""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def layout_call(method, *args):
    return subprocess.check_output([
        "gdbus", "call", "--session", "--dest", "org.kde.keyboard",
        "--object-path", "/Layouts", "--method", "org.kde.KeyboardLayouts." + method,
        *map(str, args),
    ], text=True)


def command(state, token, name, expect_ok=True, **fields):
    host, port = state["address"].rsplit(":", 1)
    with socket.create_connection((host, int(port)), timeout=10) as connection:
        stream = connection.makefile("rwb")
        stream.write((json.dumps({"type": "hello", "protocol": 3, "token": token}) + "\n").encode())
        stream.flush()
        assert json.loads(stream.readline())["type"] == "hello_ack"
        stream.write((json.dumps({"type": "command", "request_id": 1,
                                 "command": {"name": name, **fields}}) + "\n").encode())
        stream.flush()
        response = json.loads(stream.readline())["response"]
        assert response["ok"] == expect_ok, response
        return response


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--switch-layouts", action="store_true", required=True,
                        help="Briefly switch KDE keyboard layouts, restoring the original afterwards")
    args = parser.parse_args()
    original = int(re.search(r"uint32 (\d+)", layout_call("getLayout"))[1])
    # Obtain layout IDs without relying on localized labels.
    layouts = re.findall(r"\('([^']+)'", layout_call("getLayoutsList"))
    english = next(i for i, name in enumerate(layouts) if name in ("us", "gb", "au"))
    arabic = layouts.index("ara")
    with tempfile.TemporaryDirectory(prefix="simple-stt-language-") as directory:
        directory = Path(directory)
        config = directory / "config.json"
        config.write_text(json.dumps({"schema_version": 9, "speech": {
            "selection_mode": "follow_keyboard", "inference_device": "gpu",
            "single_model_filename": None,
            "language_models": {"en": "tdt_ctc-110m-q8_0.gguf", "ar": "lemura-arabic-asr-lite-q8_0.gguf"},
            "runtime_dir": str(ROOT / "external/parakeet-runtime/parakeet-linux-vulkan"),
            "model_dir": str(ROOT / "external/parakeet-runtime/models"),
        }}))
        token = secrets.token_hex(32)
        env = os.environ.copy()
        env["XDG_DATA_HOME"] = str(directory / "data")
        env["SIMPLE_STT_CONFIG"] = str(config)
        shown = subprocess.check_output([str(ROOT / "target/release/simple-stt-ctl"), "config-show"], env=env, text=True)
        values = {fields[1]: fields[2] for line in shown.splitlines()
                  if len(fields := line.split("\t", 2)) == 3 and fields[0] == "value"}
        state_path = Path(values["service_state_path"]).with_name("linux-capture-state.json")
        state_path.parent.mkdir(parents=True, exist_ok=True)
        data_path = state_path.parent.parent
        (data_path / "linux-token").write_text(token)
        with (directory / "capture.log").open("w") as output:
            process = subprocess.Popen([
                str(ROOT / "target/release/simple-stt-capture"), "--token", token,
                "--state-file", str(state_path), "--config", str(config),
            ], env=env, stdout=output, stderr=output)
            try:
                deadline = time.monotonic() + 20
                while not state_path.exists():
                    if process.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError((directory / "capture.log").read_text())
                    time.sleep(0.1)
                state = json.loads(state_path.read_text())
                sequence = 0
                def poll():
                    nonlocal sequence
                    events = command(state, token, "poll_events", after_seq=sequence)["events"]
                    sequence = max([sequence] + [event["seq"] for event in events])
                    return events

                def wait_event(kind):
                    deadline = time.monotonic() + 90
                    while time.monotonic() < deadline:
                        events = poll()
                        finished = next((e for e in events if e["kind"] == kind), None)
                        if finished:
                            return finished
                        assert not any(e["level"] == "error" for e in events), events
                        time.sleep(0.05)
                    raise RuntimeError(f"{kind} timed out")

                def save_selection(mode, single, assignments):
                    draft = json.loads(config.read_text())
                    draft["speech"].update(selection_mode=mode, single_model_filename=single,
                                            language_models=assignments)
                    config.write_text(json.dumps(draft))
                    command(state, token, "reload_config")
                    time.sleep(0.1)
                    poll()

                def skipped(session):
                    response = command(state, token, "start_recording", session_id=session, target_window=None)
                    assert response["values"]["recording"] == "skipped", response
                    time.sleep(0.2)
                    events = poll()
                    assert not any(e["kind"] in ("recording_started", "transcribing", "transcript", "model_loading", "model_loaded", "model_ready") for e in events), events
                    assert "worker_pid" not in command(state, token, "ping")["values"]
                    response = command(state, token, "stop_recording", expect_ok=False, session_id=session)
                    assert response["message"] == "no recording is active", response

                save_selection("single_model", None, {})
                skipped(101)
                # Exercise the real shell, which must not wait for a transcript or touch clipboard.
                clipboard_before = subprocess.run(["wl-paste", "--no-newline"], capture_output=True, timeout=3)
                for action in ["toggle", "stop", "toggle", "stop"]:
                    subprocess.run([str(ROOT / "target/release/simple-stt-linux"), action], env=env,
                                   capture_output=True, check=True, timeout=5)
                    shell_state = json.loads((data_path / "linux-recording-session.json").read_text())
                    assert shell_state["recording"] is False, shell_state
                clipboard_after = subprocess.run(["wl-paste", "--no-newline"], capture_output=True, timeout=3)
                assert (clipboard_before.returncode, clipboard_before.stdout) == (clipboard_after.returncode, clipboard_after.stdout), "skipped shell changed clipboard"
                print("PASS Linux shell skipped toggle/stop: inactive state, no timeout, clipboard unchanged")
                save_selection("single_model", "unavailable.gguf", {})
                skipped(102)
                save_selection("follow_keyboard", None, {"en": None, "ar": None})
                skipped(103)
                # Even with another assigned language, an explicit None remains inactive.
                assert "true" in layout_call("setLayout", arabic)
                save_selection("follow_keyboard", None, {"en": "tdt_ctc-110m-q8_0.gguf", "ar": None})
                skipped(104)
                print("PASS None, missing model, and inactive language: no recording, model events, worker or transcript")
                save_selection("follow_keyboard", None, {"en": "tdt_ctc-110m-q8_0.gguf", "ar": "lemura-arabic-asr-lite-q8_0.gguf"})
                for session, (language, index) in enumerate(
                    [("en", english), ("ar", arabic), ("en", english)], 1
                ):
                    assert "true" in layout_call("setLayout", index)
                    command(state, token, "start_recording", session_id=session, target_window=None)
                    response = command(state, token, "poll_events", after_seq=sequence, wait_ms=0)
                    events = response["events"]
                    sequence = max([sequence] + [event["seq"] for event in events])
                    started = next(event for event in events if event["kind"] == "recording_started"
                                   and event["session_id"] == session)
                    assert started["values"]["language"] == language, started
                    if not any(event["kind"] == "model_ready" for event in events):
                        wait_event("model_ready")
                    command(state, token, "cancel")
                    poll()
                    print(f"PASS recording-start keyboard selection: {language}")
                save_selection("follow_keyboard", None, {"en": "tdt_ctc-110m-q8_0.gguf", "ar": "tdt_ctc-110m-q8_0.gguf"})
                reused_pid = None
                for session, index in enumerate([english, arabic, english], 201):
                    assert "true" in layout_call("setLayout", index)
                    command(state, token, "start_recording", session_id=session, target_window=None)
                    wait_event("model_ready")
                    pid = command(state, token, "ping")["values"]["worker_pid"]
                    if reused_pid is not None:
                        assert pid == reused_pid, (pid, reused_pid)
                    reused_pid = pid
                    command(state, token, "cancel")
                    poll()
                print(f"PASS English/Arabic shared-model worker reuse: PID {reused_pid}")
                for language, filename in [("english", "tdt_ctc-110m-q8_0.gguf"), ("arabic", "lemura-arabic-asr-lite-q8_0.gguf")]:
                    # Explicit file is independent of both current assignments and fixture.
                    command(state, token, "test_model", language=language, filename=filename)
                    finished = wait_event("model_test_complete")
                    assert finished["text"].strip(), finished
                    print(f"PASS capture-service {language} model: {finished['text']}")
                pid = int(command(state, token, "ping")["values"]["worker_pid"])
                command(state, token, "unload_model")
                deadline = time.monotonic() + 10
                while Path(f"/proc/{pid}").exists() and time.monotonic() < deadline:
                    time.sleep(0.1)
                assert not Path(f"/proc/{pid}").exists(), "worker survived unload"
                print("PASS worker unload: exact PID disappeared")
                command(state, token, "shutdown")
                process.wait(timeout=10)
            finally:
                layout_call("setLayout", original)
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=10)


if __name__ == "__main__":
    main()
