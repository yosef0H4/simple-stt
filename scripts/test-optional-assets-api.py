#!/usr/bin/env python3
"""Actual authenticated Settings API, isolated profile, no desktop or audio."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.error import HTTPError
import importlib.util
spec = importlib.util.spec_from_file_location("settings_api", Path(__file__).with_name("test-settings-selection-api.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
ROOT = Path(__file__).resolve().parents[1]

with tempfile.TemporaryDirectory(prefix="simple-stt-assets-api-") as temporary:
    base = Path(temporary)
    process, api = module.start_settings_server(ROOT / "target/debug/simple-stt-settings", base / "config.json", base / "runtime", base / "data")
    try:
        assets = api("/api/assets")
        assert assets["progress"]["phase"] == ""
        assert not any(asset["installed"] for asset in assets["assets"])
        cuda = next(asset for asset in assets["assets"] if asset["id"] == "parakeet-cuda")
        assert cuda["bytes"] > 500_000_000
        reader = api("/api/reader")
        assert reader["status"] is None
        assert reader["config"]["speech"]["provider"]["kind"] == "local_kokoro"
        assert not (Path(reader["config"]["speech"]["runtime_dir"]).parent.parent / "reader/config.json").exists(), "Viewing Settings must not enable the reader"
        config = reader["config"]
        config["speech"]["speed"] = 1.2
        saved = api("/api/reader-save", {"config":config, "expected_revision":config["revision"]})
        assert saved["config"]["revision"] == 2
        assert abs(api("/api/reader")["config"]["speech"]["speed"] - 1.2) < 0.00001
        try:
            api("/api/reader-save", {"config":config, "expected_revision":1})
            raise AssertionError("Stale reading revision accepted")
        except HTTPError as error:
            assert b"revision conflict" in error.read()
        try:
            api("/api/asset-action", {"action":"install", "id":"../escape"})
            raise AssertionError("Unknown asset accepted")
        except HTTPError:
            pass
        try:
            api("/api/assets", auth=False)
            raise AssertionError("Unauthenticated catalog access accepted")
        except HTTPError as error:
            assert error.code in (400,401,403)
            assert b"unauthorized" in error.read()
        assert api("/api/reader-status")["status"] is None
        # Small real upstream asset, installed only under this temporary profile.
        api("/api/asset-action", {"action":"install", "id":"paddle-ar-en"})
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            value = api("/api/assets")
            progress = value["progress"]
            if progress["phase"] in ["ready", "failed", "cancelled"]:
                break
            time.sleep(0.1)
        assert progress["phase"] == "ready", progress
        arabic = next(asset for asset in value["assets"] if asset["id"] == "paddle-ar-en")
        assert arabic["installed"]
        assert (Path(arabic["directory"]) / "arabic_pp-ocrv5_mobile_rec.onnx").stat().st_size == 8026538
        api("/api/asset-action", {"action":"remove", "id":"paddle-ar-en"})
        assert not Path(arabic["directory"]).exists()
        api("/api/asset-action", {"action":"install", "id":"sherpa-cpu"})
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            value = api("/api/assets")
            if value["progress"]["phase"] in ["ready", "failed", "cancelled"]:
                break
            time.sleep(0.1)
        assert value["progress"]["phase"] == "ready", value["progress"]
        runtime = next(asset for asset in value["assets"] if asset["id"] == "sherpa-cpu")
        assert runtime["installed"]
        library = Path(runtime["directory"]) / "lib/libsherpa-onnx-c-api.so"
        assert library.is_file()
        result = subprocess.run(["ldd",str(library)],capture_output=True,text=True,check=True)
        assert "not found" not in result.stdout, result.stdout
        api("/api/asset-action", {"action":"remove", "id":"sherpa-cpu"})
        assert not Path(runtime["directory"]).exists()
        print("PASS optional assets API: auth, lazy reader, Save/conflicts, catalog validation, real verified Arabic OCR and Sherpa runtime downloads/removal")
    finally:
        module.stop_settings_server(process)
