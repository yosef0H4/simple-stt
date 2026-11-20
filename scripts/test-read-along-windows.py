"""Windows desktop read-along smoke using a private Tk window and local HTTP fixtures."""
import argparse
import base64
import ctypes as C
from ctypes import wintypes as W
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import tkinter as tk

from PIL import Image, ImageGrab

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BIN = ROOT / "target/debug"
PICKER_CLASS = "SimpleSttScreenRegionPicker"
PAGE_COLORS = ("#cc3333", "#33aa33", "#3333cc", "#ac7324")
PAGE_NAMES = ("First page.", "Second page.", "Third page.", "Fourth page.")
DURATION = 5
DELAY_MS = 500

user32 = C.WinDLL("user32", use_last_error=True)
user32.SetProcessDpiAwarenessContext(C.c_void_p(-4))
user32.GetForegroundWindow.restype = W.HWND
user32.GetClassNameW.argtypes = (W.HWND, W.LPWSTR, C.c_int)
user32.GetWindowThreadProcessId.argtypes = (W.HWND, C.POINTER(W.DWORD))
user32.GetAncestor.argtypes = (W.HWND, W.UINT)
user32.GetAncestor.restype = W.HWND
user32.SetForegroundWindow.argtypes = (W.HWND,)
user32.SetPropW.argtypes = (W.HWND, W.LPCWSTR, W.HANDLE)
user32.IsWindowVisible.argtypes = (W.HWND,)
user32.FindWindowW.argtypes = (W.LPCWSTR, W.LPCWSTR)
user32.FindWindowW.restype = W.HWND


def reader_tooltip(executable):
    """Find a visible tooltip from the isolated reader executable."""
    kernel = C.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = (W.DWORD, W.BOOL, W.DWORD)
    kernel.OpenProcess.restype = W.HANDLE
    kernel.QueryFullProcessImageNameW.argtypes = (W.HANDLE, W.DWORD, W.LPWSTR, C.POINTER(W.DWORD))
    kernel.CloseHandle.argtypes = (W.HANDLE,)
    callback = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
    found = []

    @callback
    def visit(hwnd, _):
        name = C.create_unicode_buffer(64)
        if not user32.IsWindowVisible(hwnd) or not user32.GetClassNameW(hwnd, name, len(name)) or name.value != "tooltips_class32":
            return True
        pid = W.DWORD()
        user32.GetWindowThreadProcessId(hwnd, C.byref(pid))
        process = kernel.OpenProcess(0x1000, False, pid)
        if process:
            try:
                path = C.create_unicode_buffer(1024)
                size = W.DWORD(len(path))
                if kernel.QueryFullProcessImageNameW(process, 0, path, C.byref(size)) and Path(path.value) == executable:
                    found.append(hwnd)
                    return False
            finally:
                kernel.CloseHandle(process)
        return True

    user32.EnumWindows(visit, 0)
    return next(iter(found), None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=DEFAULT_BIN)
    parser.add_argument("--recovery", choices=("resume", "next"), default="resume")
    parser.add_argument("--read-screen", action="store_true")
    parser.add_argument("--feedback", choices=("copy-region", "read-region", "read-selection"),
                        help="Require a visible reader tooltip while OCR or speech is still blocked")
    parser.add_argument("--fullscreen", action="store_true", help="Use the Windows borderless fullscreen target")
    parser.add_argument("--require-fullscreen-graphics", action="store_true",
                        help="Require native fullscreen selection and window capture beneath a covering window")
    parser.add_argument("--prewarm-assets", type=Path, help="Use installed reader assets for a warm-worker test")
    parser.add_argument("--local-read-along", action="store_true", help="Verify local OCR and speech overlapped in Read Along")
    parser.add_argument("--ocr-device", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--advance", choices=("sequence", "space"), default="sequence")
    parser.add_argument("--playback-device", choices=("preferred", "missing"),
                        help="Check endpoint selection or fallback during model-free speech feedback")
    args = parser.parse_args()
    if args.require_fullscreen_graphics and (
        not args.fullscreen or args.feedback != "read-region" or args.prewarm_assets
    ):
        parser.error("--require-fullscreen-graphics needs --fullscreen --feedback read-region with mock providers")
    assets = args.prewarm_assets.resolve() if args.prewarm_assets else None
    if args.local_read_along and not assets:
        parser.error("--local-read-along requires --prewarm-assets")
    if assets:
        assert assets.name == "assets" and assets.is_dir(), assets
    binaries = args.bin_dir.resolve()
    output = ROOT / "artifacts/windows-validation"
    output.mkdir(parents=True, exist_ok=True)
    captures, failures, key_releases, spoken = [], [], [], []
    ocr_response, speech_response = threading.Event(), threading.Event()
    rejected_pages = {2}
    page = 0

    def page_at(color):
        palette = [tuple(bytes.fromhex(value[1:])) for value in PAGE_COLORS]
        if color in palette:
            return palette.index(color)
        if args.fullscreen:
            # HDR conversion can change saturation and brightness. An unrelated
            # screen, black frame, or different page must still fail the check.
            if max(color) - min(color) < 64:
                return None
            if color[0] > color[1] > color[2] + 15:
                return 3
            high = color.index(max(color))
            if abs(color[(high + 1) % 3] - color[(high + 2) % 3]) <= 4:
                return high
        return None

    class Fixture(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if self.path.endswith("/chat/completions"):
                if self.headers.get("Authorization") != "Bearer desktop-ocr-fixture":
                    self.send_error(401)
                    return
                image_url = next(
                    item["image_url"]["url"]
                    for message in body["messages"]
                    if isinstance(message["content"], list)
                    for item in message["content"]
                    if item["type"] == "image_url"
                )
                with Image.open(io.BytesIO(base64.b64decode(image_url.split(",", 1)[1]))) as image:
                    image = image.convert("RGB")
                    color = image.getpixel((image.width // 2, image.height // 2))
                captured_page = page_at(color)
                if captured_page is None:
                    failures.append(f"unexpected crop pixel {color}")
                    self.send_error(400)
                    return
                captures.append((captured_page, time.monotonic()))
                if captured_page in rejected_pages:
                    self.send_error(400, "simulated unreadable page")
                    return
                response = json.dumps(
                    {"choices": [{"message": {"content": PAGE_NAMES[captured_page]}}]}
                ).encode()
                content_type = "application/json"
                if args.feedback:
                    ocr_response.wait(timeout=15)
            elif self.path.endswith("/audio/speech"):
                if self.headers.get("Authorization") != "Bearer desktop-tts-fixture":
                    self.send_error(401)
                    return
                spoken.append(body["input"])
                if args.feedback:
                    speech_response.wait(timeout=15)
                response = bytes(24000 * 2 * DURATION)
                content_type = "audio/pcm"
            else:
                self.send_error(404)
                return
            try:
                self.send_response(200)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)
            except (ConnectionResetError, BrokenPipeError):
                if args.feedback != "copy-region":
                    raise

    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    app = tk.Tk()
    app.title("Simple STT private read-along test")
    app.geometry("600x310+360+270")
    if args.fullscreen:
        app.attributes("-fullscreen", True)
    app.configure(background=PAGE_COLORS[0])
    if assets:
        page_text = (
            "This article describes what the visitors saw.",
            "Another chapter follows the train to London.",
            "The last chapter is about the journey home.",
            "The final notes describe the garden at dusk.",
        )
        introduction = "The screen is ready."
        if args.local_read_along:
            introduction = page_text[0]
        label = tk.Label(app, text=introduction, wraplength=430, font=("Arial", 23), bg="white", fg="black")
        label.place(x=48, y=48, width=490, height=185)

    def advance(_event):
        nonlocal page
        page = min(page + 1, len(PAGE_COLORS) - 1)
        if args.local_read_along:
            label.configure(text=page_text[page])
            return
        app.configure(background="white")
        animation_ms = 700 if args.advance == "sequence" else 250
        app.after(animation_ms, lambda: app.configure(background=PAGE_COLORS[page]))

    press, release = {
        "sequence": ("<Control-space>", "<KeyRelease-Control_L>"),
        "space": ("<KeyPress-space>", "<KeyRelease-space>"),
    }[args.advance]
    app.bind(press, advance)
    app.bind(release, lambda _: key_releases.append(time.monotonic()))

    def pump(seconds, condition=lambda: False):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            app.update()
            value = condition()
            if value:
                return value
            if failures:
                raise AssertionError(failures)
            time.sleep(0.015)
        return None

    def picker_ready():
        window = user32.FindWindowW(PICKER_CLASS, None)
        return window if window and user32.IsWindowVisible(window) else None

    def focus_target():
        app.lift()
        app.focus_force()
        user32.SetForegroundWindow(user32.GetAncestor(app.winfo_id(), 2))
        app.update()

    try:
        with tempfile.TemporaryDirectory(prefix="simple-stt-read-along-win-", dir=assets.parent if assets else None) as folder:
            profile = Path(folder) if assets else Path(folder) / "reader"
            profile.mkdir(exist_ok=True)
            config = profile / "config.json"
            env = dict(os.environ, SIMPLE_STT_READER_DATA=str(profile), SIMPLE_TTS_CONFIG=str(config),
                       SIMPLE_STT_EMBEDDED_READER="1", SIMPLE_TTS_NO_DESKTOP="1",
                       SIMPLE_TTS_OCR_API_KEY="desktop-ocr-fixture", SIMPLE_TTS_SPEECH_API_KEY="desktop-tts-fixture", RUST_LOG="info")
            options = dict(env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            defaults = subprocess.run(
                [binaries / "simple-stt-ctl.exe", "config", "--scope", "reader", "defaults"],
                capture_output=True, check=True, timeout=10, **options,
            )
            settings = json.loads(defaults.stdout)
            if args.playback_device == "preferred":
                devices = subprocess.run([binaries / "readerd.exe", "list-output-devices"],
                                         capture_output=True, check=True, timeout=5, **options)
                settings["playback"]["output_device"] = json.loads(devices.stdout)[0]["id"]
            elif args.playback_device == "missing":
                settings["playback"]["output_device"] = "wasapi:simple-stt-missing-endpoint-fixture"
            if args.playback_device or args.require_fullscreen_graphics:
                settings["privacy"]["log_content"] = True
            base = f"http://127.0.0.1:{server.server_port}/v1"
            settings["speech"].update(provider=dict(kind="open_ai_compatible", base_url=base,
                model="desktop-tts", voice="fixture", format="pcm"), fallback_to_local=False)
            settings["ocr"].update(provider=dict(kind="open_ai_compatible", base_url=base,
                model="desktop-ocr", api="chat_completions"), fallback_to_local=False)
            settings["read_along"].update(delay_ms=DELAY_MS, prefetch=True, retry_count=1,
                sequence={"events": [
                    {"at_ms": 0, "key": "ControlLeft", "down": True},
                    {"at_ms": 450, "key": "Space", "down": True},
                    {"at_ms": 490, "key": "Space", "down": False},
                    {"at_ms": 900, "key": "ControlLeft", "down": False},
                ]})
            if args.advance != "sequence":
                settings["read_along"].update(
                    sequence=None, advance_key="Space",
                )
            if assets:
                settings["speech"].update(provider={"kind":"local_kokoro"}, language="en", selection={
                    "mode":"single_model","single":{"runtime":"sherpa-cpu","model":"kokoro","voice":"af_heart","device":"cpu"},
                })
                settings["ocr"].update(provider={"kind":"local_paddle"}, selection={
                    "mode":"single_model","single":{
                        "runtime":f"paddle-{args.ocr_device}","model":"paddle-ar-en",
                        "device":"gpu" if args.ocr_device == "cuda" else "cpu",
                    },
                })
            config.write_text(json.dumps(settings), encoding="utf-8")

            def cli(*args, check=True):
                return subprocess.run(
                    [binaries / "readerctl.exe", *args], capture_output=True,
                    timeout=8, check=check, **options,
                )

            def status():
                return json.loads(cli("status", "--json").stdout)

            def check_fullscreen_recapture():
                root = user32.GetAncestor(app.winfo_id(), 2)
                cover = tk.Toplevel(app, background="magenta")
                cover.overrideredirect(True)
                cover.attributes("-topmost", True)
                cover.geometry(f"220x150+{app.winfo_rootx()+20}+{app.winfo_rooty()+20}")
                cover.lift()
                pump(0.12)
                try:
                    crop = profile / "occluded-fullscreen-region.png"
                    subprocess.run(
                        [binaries / "readerd.exe", "capture-window", str(root), str(os.getpid()),
                         "40", "40", "120", "90", crop],
                        capture_output=True, check=True, timeout=5, **options,
                    )
                    with Image.open(crop) as screenshot:
                        assert screenshot.size == (120, 90)
                        assert page_at(screenshot.convert("RGB").getpixel((60, 45))) == 0
                finally:
                    cover.destroy()
                    app.update()

            def next_page():
                # The Windows reader HUD sends this action through the profile pipe.
                fingerprint = hashlib.sha256(str(profile.resolve()).lower().encode()).hexdigest()
                address = r"\\.\pipe\simple-stt-reader-" + fingerprint
                frame = b"\0" + json.dumps({"op": "action", "action": "read-along-next"}).encode()
                with open(address, "r+b", buffering=0) as pipe:
                    pipe.write(len(frame).to_bytes(4, "little") + frame)
                    length = int.from_bytes(pipe.read(4), "little")
                    response = json.loads(pipe.read(length)[1:])
                assert response["ok"], response

            with (output / "desktop-read-along.log").open("wb") as log:
                daemon = subprocess.Popen(
                    [binaries / "readerd.exe", "daemon", *([] if args.feedback else ["--no-desktop"])],
                    stdout=log, stderr=log, **options,
                )
                try:
                    pump(0.5)
                    focus_target()
                    if args.feedback:
                        frames = status()["audio_callback_frames"]
                        if args.feedback == "read-selection":
                            selection = tk.Entry(app)
                            selection.insert(0, "Selected test text.")
                            selection.pack()
                            selection.focus_force()
                            selection.selection_range(0, tk.END)
                            app.update()
                        cli(args.feedback)
                        if args.feedback != "read-selection":
                            assert pump(5, picker_ready), "screen picker did not appear"
                            x, y = app.winfo_rootx(), app.winfo_rooty()
                            user32.SetCursorPos(x + 48, y + 48)
                            user32.mouse_event(2, 0, 0, 0, 0)
                            pump(0.12)
                            user32.SetCursorPos(x + 530, y + 250)
                            released = time.monotonic()
                            user32.mouse_event(4, 0, 0, 0, 0)
                            phase = "ocr_activity" if args.feedback == "copy-region" else "activity"
                            assert pump(3, lambda: not picker_ready() and reader_tooltip(binaries / "reader-hud.exe")
                                        and status()[phase]["phase"] == "recognizing"), status()
                            print(f"PASS {args.feedback} showed OCR progress {1000*(time.monotonic()-released):.0f} ms after release, before OCR returned", flush=True)
                            if args.feedback == "copy-region":
                                # Cancel the private test before it can replace the user's clipboard.
                                cli("stop")
                                ocr_response.set()
                                assert pump(3, lambda: status()["ocr_activity"]["phase"] == "idle"), status()
                                assert status()["notice_text"] != "Text copied"
                                return
                        ocr_response.set()
                        assert pump(5, lambda: spoken and reader_tooltip(binaries / "reader-hud.exe")
                                    and status()["activity"]["phase"] in ("loading_voice", "synthesizing")), status()
                        assert status()["audio_callback_frames"] == frames
                        speech_response.set()
                        assert pump(5, lambda: status()["audio_callback_frames"] > frames), status()
                        if args.playback_device:
                            expected = ("using preferred playback device" if args.playback_device == "preferred"
                                        else "preferred playback device unavailable; using system default")
                            assert pump(3, lambda: expected in (output / "desktop-read-along.log").read_text(encoding="utf-8"))
                            assert json.loads(config.read_text(encoding="utf-8"))["playback"]["output_device"] == settings["playback"]["output_device"]
                            print(f"PASS playback device: {expected}, and the saved preference remains", flush=True)
                        print(f"PASS {args.feedback} showed speech progress before audio arrived", flush=True)
                        cli("stop")
                        if args.require_fullscreen_graphics:
                            assert captures[0][0] == 0
                            logs = (output / "desktop-read-along.log").read_text(encoding="utf-8")
                            assert "wgc_picker_capture" in logs, logs
                            check_fullscreen_recapture()
                            print("PASS native fullscreen selection and window-specific capture beneath an occluding window", flush=True)
                        return
                    if assets:
                        def loaded():
                            return [event for event in status()["model_events"] if event["event"] == "loaded"]

                        def select_region():
                            x, y = app.winfo_rootx(), app.winfo_rooty()
                            user32.SetCursorPos(x + 36, y + 36)
                            user32.mouse_event(2, 0, 0, 0, 0)
                            pump(0.12)
                            user32.SetCursorPos(x + 550, y + 250)
                            started = time.monotonic()
                            user32.mouse_event(4, 0, 0, 0, 0)
                            return started

                        if args.local_read_along:
                            played = status()["audio_callback_frames"]
                            cli("read-along")
                            assert pump(5, picker_ready), "the read-along picker did not appear"
                            assert pump(35, lambda: {event["feature"] for event in loaded()} == {"ocr", "tts"}), status()["model_events"]
                            first_loads = loaded()
                            assert picker_ready(), "workers loaded after selection"
                            started = select_region()
                            assert pump(8, lambda: not picker_ready())
                            focus_target()
                            assert pump(20, lambda: status()["audio_callback_frames"] > played), status()
                            first_ms = (time.monotonic() - started) * 1000

                            def ready():
                                state = status()
                                next_activity = state.get("next_page_activity") or {}
                                return state["state"] == "reading" and next_activity.get("phase") == "ready"

                            assert pump(20, ready), status()
                            assert page == 1, "reader advanced beyond one page before promotion"
                            assert loaded() == first_loads, loaded()
                            print(f"PASS prewarmed {args.ocr_device.upper()} OCR and Kokoro; first audio in {first_ms:.0f} ms, with one next page ready during playback", flush=True)
                            next_page()
                            assert pump(12, lambda: "Another chapter" in (status().get("chunk_text") or "")), status()
                            print("PASS Next played the already prepared second page", flush=True)
                            cli("stop")
                            return

                        first_loads = None
                        for attempt in range(2):
                            focus_target()
                            played = status()["audio_callback_frames"]
                            cli("read-region")
                            assert pump(5, picker_ready), "the prewarm picker did not appear"
                            if attempt == 0:
                                assert pump(30, lambda: {event["feature"] for event in loaded()} == {"ocr", "tts"}), status()["model_events"]
                                first_loads = loaded()
                                assert picker_ready(), "the models loaded after the picker closed"
                                print("PASS both local models loaded while the picker was open", flush=True)
                            select_region()
                            assert pump(8, lambda: not picker_ready())
                            assert pump(20, lambda: status()["audio_callback_frames"] > played), status()
                        assert loaded() == first_loads, loaded()
                        print("PASS two screen reads played audio using the same prewarmed OCR and speech workers", flush=True)
                        cli("stop")
                        return
                    action = "read-region" if args.read_screen else "read-along"
                    cli(action)
                    assert pump(10, picker_ready), f"{action} picker did not open"
                    cli("stop")
                    assert pump(5, lambda: not picker_ready()), "stop did not close the picker"
                    print("PASS Windows Stop closes an active region picker", flush=True)
                    tooltip = None
                    if args.read_screen:
                        tooltip = tk.Toplevel(app)
                        tooltip.overrideredirect(True)
                        tooltip.attributes("-topmost", True)
                        tooltip.configure(background="#ffff00")
                        tooltip.geometry(f"130x80+{app.winfo_rootx()+225}+{app.winfo_rooty()+115}")
                        app.update()
                        tooltip_hwnd = user32.GetAncestor(tooltip.winfo_id(), 2)
                        assert user32.SetPropW(tooltip_hwnd, "SimpleSttOverlay", 1)
                        assert user32.IsWindowVisible(tooltip_hwnd)
                    focus_target()
                    started = time.monotonic()
                    cli(action)
                    picker = pump(10, picker_ready)
                    assert picker, "read-along picker did not reopen"
                    if args.read_screen:
                        elapsed = (time.monotonic() - started) * 1000
                        assert elapsed < 750, f"picker took {elapsed:.0f} ms to open"
                        assert not user32.IsWindowVisible(tooltip_hwnd), "tooltip visible during selection"
                        print(f"PASS native picker visible in {elapsed:.0f} ms with tooltip hidden", flush=True)
                    pump(0.1)
                    if args.read_screen:
                        app.configure(background=PAGE_COLORS[1])
                        app.update()
                    x, y = app.winfo_rootx(), app.winfo_rooty()
                    user32.SetCursorPos(x + 48, y + 48)
                    user32.mouse_event(2, 0, 0, 0, 0)
                    pump(0.12)
                    if args.read_screen:
                        for index in range(12):
                            user32.SetCursorPos(x + 420 + index * 10, y + 230 + index)
                            pump(0.025)
                            with ImageGrab.grab(bbox=(x+32, y+32, x+546, y+266)) as frame:
                                assert frame.getpixel((200, 110)) == (204, 51, 51), "selection flashed or captured the tooltip"
                                assert frame.getpixel((4, 4))[0] > 80, "darkened background flashed black"
                    user32.SetCursorPos(x + 530, y + 250)
                    if args.read_screen:
                        pump(0.15)
                        with ImageGrab.grab(bbox=(x+32, y+32, x+546, y+266)) as preview:
                            inside = preview.getpixel((200, 110))
                            outside = preview.getpixel((4, 4))
                            assert inside == (204, 51, 51) and outside[0] < inside[0], (inside, outside)
                            mode = "fullscreen" if args.fullscreen else "window"
                            preview.save(output / f"read-screen-picker-{mode}.png")
                    user32.mouse_event(4, 0, 0, 0, 0)
                    assert pump(8, lambda: not picker_ready())
                    focus_target()
                    if args.read_screen:
                        tooltip.destroy()
                        assert pump(12, lambda: spoken), captures
                        assert [item[0] for item in captures] == [0], captures
                        assert spoken == ["First page."], spoken
                        print("PASS Read Screen rectangle captured the frozen pixels and reached speech", flush=True)
                        cli("stop")
                        return
                    assert pump(16, lambda: len(captures) >= 2), (captures, key_releases)
                    assert [value for value, _ in captures[:2]] == [0, 1], captures
                    assert key_releases, "the page-advance input was not released"
                    elapsed_ms = (captures[1][1] - key_releases[0]) * 1000
                    assert elapsed_ms >= DELAY_MS - 10, elapsed_ms
                    print(f"PASS off-origin capture and {args.advance} advance. Next OCR arrived {elapsed_ms:.0f} ms after input release.", flush=True)
                    pump(0.3)
                    assert page == 1, "reader advanced beyond one page while the first page was playing"
                    print("PASS one-page lookahead during first-page playback", flush=True)

                    def reached_failure():
                        state = status()
                        notice = state.get("notice_text") or ""
                        return not state["read_along_active"] and "Retry last action" in notice

                    assert pump(24, reached_failure), captures
                    assert page == 2, "OCR failure triggered another page turn"
                    assert pump(7, lambda: not status().get("notice_text")), "failure notice did not dismiss"
                    rejected_pages.clear()
                    next_capture = len(captures)
                    if args.recovery == "resume":
                        cli("resume")
                    else:
                        next_page()
                    assert pump(12, lambda: len(captures) > next_capture), captures
                    assert captures[next_capture][0] == 2 and not picker_ready(), captures
                    print(f"PASS {args.recovery.title()} recaptures the saved page after the failure notice dismisses", flush=True)
                    cli("stop")
                finally:
                    cli("shutdown", check=False)
                    ocr_response.set()
                    speech_response.set()
                    try:
                        daemon.wait(timeout=6)
                    except subprocess.TimeoutExpired:
                        daemon.kill()
                        daemon.wait(timeout=4)
    finally:
        app.destroy()
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
