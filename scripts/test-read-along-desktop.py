#!/usr/bin/python3
"""Explicit KDE desktop E2E: drag a real region, OCR two pages, and play speech.

Uses an isolated profile and a private GTK target. It sends pointer input only
after checking that its own window is active. No clipboard or dictation input.
"""
import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import socket
import shutil
import struct
import subprocess
import tempfile
import threading
import time
import traceback
import uuid

# Terminal agents may lack the graphical session's display variables.
session = subprocess.run(['systemctl', '--user', 'show-environment'], capture_output=True, text=True, check=True)
for line in session.stdout.splitlines():
    key, separator, value = line.partition('=')
    if separator and key in {'WAYLAND_DISPLAY', 'DISPLAY', 'XAUTHORITY', 'XDG_CURRENT_DESKTOP', 'KDE_SESSION_VERSION', 'XDG_SESSION_TYPE'}:
        os.environ.setdefault(key, value)
if os.environ.get('WAYLAND_DISPLAY'):
    os.environ.setdefault('XDG_SESSION_TYPE', 'wayland')
del session

import dbus
import dbus.service
import dbus.mainloop.glib
import gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, GLib, Gdk

ROOT = Path(__file__).resolve().parents[1]
TITLE = 'Simple STT read-along E2E ' + uuid.uuid4().hex[:8]
PAGES = [
    'Speech recognition is a technology that converts spoken language into text.\nIt helps people interact with computers using their voice.',
    'Language models help computers understand words and sentences.\nThis second page verifies continuous reading and speech playback.',
]

def request(path, value):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(str(path))
        payload = b'\0' + json.dumps(value).encode()
        stream.sendall(struct.pack('<I', len(payload)) + payload)
        def exact(count):
            data = b''
            while len(data) < count:
                chunk = stream.recv(count - len(data))
                if not chunk:
                    raise RuntimeError('reader closed the IPC connection')
                data += chunk
            return data
        size, = struct.unpack('<I', exact(4))
        assert 2 <= size <= 65536
        reply = json.loads(exact(size)[1:])
        assert reply['ok'], reply
        return reply


def wait_for(check, seconds, label):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(.1)
    raise AssertionError('timed out: ' + label)


dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
dbus.mainloop.glib.threads_init()
bus = dbus.SessionBus()
window_reply = threading.Event()
window_state = {}

class Callback(dbus.service.Object):
    @dbus.service.method('org.simple_stt.E2E', in_signature='s', out_signature='')
    def Report(self, value):
        window_state.clear()
        window_state.update(json.loads(value))
        window_reply.set()


callback = Callback(bus, '/org/simple_stt/E2E')

def query_target(folder, title=TITLE, activate=False):
    with (Path(os.environ['XDG_RUNTIME_DIR']) / 'simple-stt-kwin-query.lock').open('a') as guard:
        fcntl.flock(guard, fcntl.LOCK_EX)
        return query_target_locked(folder, title, activate)


def query_target_locked(folder, title=TITLE, activate=False):
    window_reply.clear()
    script = folder / ('window-' + uuid.uuid4().hex + '.js')
    script.write_text('var w=workspace.windowList().find(w=>w.caption===' + json.dumps(title) + ');'
        + ('if(w)workspace.activeWindow=w;' if activate else '') +
        'var g=w?w.frameGeometry:null;callDBus(' + json.dumps(bus.get_unique_name()) +
        ',"/org/simple_stt/E2E","org.simple_stt.E2E","Report",JSON.stringify({'
        'found:!!w,active:!!w&&!!workspace.activeWindow&&String(workspace.activeWindow.internalId)===String(w.internalId),'
        'activeClass:workspace.activeWindow?String(workspace.activeWindow.resourceClass):null,'
        'targetId:w?String(w.internalId):null,activeId:workspace.activeWindow?String(workspace.activeWindow.internalId):null,'
        'testTitles:workspace.windowList().filter(w=>Number(w.pid)===' + str(os.getpid()) + ').map(w=>({caption:String(w.caption),class:String(w.resourceClass),minimized:w.minimized})),'
        'x:g?g.x:0,y:g?g.y:0,width:g?g.width:0,height:g?g.height:0}));')
    scripting = dbus.Interface(bus.get_object('org.kde.KWin', '/Scripting'), 'org.kde.kwin.Scripting')
    name = 'simple-stt-e2e-' + uuid.uuid4().hex
    identifier = scripting.loadScript(str(script), name, signature='ss')
    try:
        dbus.Interface(bus.get_object('org.kde.KWin', '/Scripting/Script' + str(identifier)), 'org.kde.kwin.Script').run()
        assert window_reply.wait(3), 'KWin test callback did not arrive'
        return dict(window_state)
    finally:
        scripting.unloadScript(name)
        for _ in range(20):
            if not scripting.isScriptLoaded(name):
                break
            time.sleep(.005)
        script.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--timeout', type=int, default=180)
    parser.add_argument('--no-prefetch', action='store_true')
    parser.add_argument('--overlap', action='store_true')
    parser.add_argument('--focus-change', action='store_true')
    parser.add_argument('--transient-empty', action='store_true')
    parser.add_argument('--unchanged-once', action='store_true')
    parser.add_argument('--game-dialogue', action='store_true')
    parser.add_argument('--fullscreen', action='store_true')
    parser.add_argument('--hud-progress', action='store_true', help='Verify current text and independent next-page work during playback')
    parser.add_argument('--repeat-region', action='store_true', help='Manually change dialogue, then Repeat the saved area without reselecting or advancing')
    parser.add_argument('--repeat-read-along', action='store_true', help='Repeat a saved read-along area while its initial audio is still playing')
    args = parser.parse_args()
    installed = Path.home() / '.local/share/simple-stt/instances/installed'
    folder = Path(tempfile.mkdtemp(prefix='simple-stt-readalong-e2e-'))
    data = folder / 'reader'
    data.mkdir()
    permission_token = Path(os.environ['XDG_RUNTIME_DIR']) / 'simple-stt-readalong-e2e-portal-token'
    if permission_token.exists():
        shutil.copyfile(permission_token, data / 'remote-desktop-keyboard-token')
        (data / 'remote-desktop-keyboard-token').chmod(0o600)
    (folder / 'assets').symlink_to(installed / 'assets', target_is_directory=True)
    config = json.loads((installed / 'reader/config.json').read_text())
    config['server'] = {'enabled': False}
    config['read_along'] = {'prefetch': not (args.no_prefetch or args.repeat_read_along), 'advance_key': 'Space', 'action': 'key', 'delay_ms': 600, 'retry_count': 2 if args.unchanged_once else 0}
    config['playback']['keep_warm_seconds'] = 2
    config['ocr']['idle_timeout_secs'] = 2
    (data / 'config.json').write_text(json.dumps(config))
    env = dict(os.environ, SIMPLE_STT_READER_DATA=str(data), SIMPLE_TTS_CONFIG=str(data / 'config.json'), SIMPLE_STT_EMBEDDED_READER='1', RUST_LOG='info')
    window = Gtk.Window(title=TITLE)
    window.set_default_size(940, 400)
    window.set_keep_above(True)
    if args.game_dialogue:
        fixture_spec = importlib.util.spec_from_file_location('game_fixture', ROOT / 'scripts/fixtures/read-along-game.py')
        fixture = importlib.util.module_from_spec(fixture_spec)
        fixture_spec.loader.exec_module(fixture)
        label = fixture.GameDialogue()
    else:
        label = Gtk.Label()
    label.set_markup('<span size="24000" foreground="#111111">' + PAGES[0] + '</span>')
    if not args.game_dialogue:
        label.set_margin_start(36)
        label.set_margin_end(36)
    window.add(label)
    css = Gtk.CssProvider()
    css.load_from_data(b'window {background: #ffffff;}')
    window.get_style_context().add_provider(css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
    page = [0]
    presses = [0]
    first_change_presses = [None]
    def key(_, event):
        if event.keyval == Gdk.KEY_space:
            presses[0] += 1
            if args.unchanged_once and presses[0] == 1:
                return True
            page[0] += 1
            if first_change_presses[0] is None:
                first_change_presses[0] = presses[0]
            def show_page():
                label.set_markup('<span size="24000" foreground="#111111">' + PAGES[min(page[0], 1)] + '</span>')
                return False
            if args.transient_empty:
                label.set_text('')
                GLib.timeout_add(1400, show_page)
            else:
                show_page()
            return True
        return False
    window.connect('key-press-event', key)
    window.connect('destroy', Gtk.main_quit)
    window.show_all()
    if args.fullscreen:
        window.fullscreen()
    decoy = None
    decoy_keys = []
    if args.overlap or args.focus_change:
        # Created later but visibly below the reading window: creation order
        # must never be mistaken for KWin stacking order.
        decoy = Gtk.Window(title=TITLE + ' overlap')
        decoy.set_default_size(940, 400)
        decoy.add(Gtk.Label(label='WRONG WINDOW — this must never be captured or advanced'))
        decoy.connect('key-press-event', lambda _window, event:
            (decoy_keys.append('Space') if event.keyval == Gdk.KEY_space else None) or True)
        decoy.show_all()
    window.present()
    outcome = {'passed': False}
    def exercise():
        daemon = None
        log = (folder / 'reader.log').open('w')
        try:
            wait_for(lambda: query_target(folder).get('found'), 10, 'test target mapped by KWin')
            query_target(folder, activate=True)
            target = wait_for(lambda: (v if (v := query_target(folder)).get('found') and v.get('active') else None), 10, 'test target foreground')
            (folder / 'target.json').write_text(json.dumps(target))
            daemon = subprocess.Popen([str(args.bin_dir.resolve() / 'readerd'), 'daemon', '--no-desktop'], env=env, stdout=log, stderr=log)
            ipc = data / 'simple-tts.sock'
            wait_for(ipc.exists, 10, 'isolated reader socket')
            def preserve_captures():
                while daemon.poll() is None:
                    for image in (data / 'capture').glob('*.png'):
                        try:
                            shutil.copyfile(image, folder / image.name)
                        except OSError:
                            pass
                    time.sleep(.02)
            threading.Thread(target=preserve_captures, daemon=True).start()
            request(ipc, {'op': 'action', 'action': 'read-region' if args.repeat_region else 'read-along'})
            wait_for(lambda: subprocess.run(['pgrep', '-P', str(daemon.pid), 'slurp'], capture_output=True).returncode == 0, 10, 'real region picker')
            time.sleep(.6)  # Process creation precedes mapping the input surface.
            x, y = int(target['x']), int(target['y'])
            w, h = int(target['width']), int(target['height'])
            yd = dict(env, YDOTOOL_SOCKET=str(Path(os.environ['XDG_RUNTIME_DIR']) / '.ydotool_socket'))
            def mouse(*arguments):
                subprocess.run(['ydotool', *map(str, arguments)], env=yd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            def position(px, py):
                # XTest moves the compositor's real pointer in screen pixels;
                # ydotool's absolute device uses a different coordinate range.
                subprocess.run(['xdotool', 'mousemove', str(px), str(py)], env=env, check=True, timeout=3)
                time.sleep(.15)
            position(x+40, y+h-200) if args.game_dialogue else position(x+20, y+50)
            mouse('click', '0x40')
            position(x+w-20, y+h-20)
            mouse('click', '0x80')
            if args.focus_change:
                query_target(folder, title=TITLE + ' overlap', activate=True)
            observed = set()
            observed_hud_current = False
            observed_hud_next = False
            waited_for_focus = False
            returned_focus = False
            first_audio = False
            second_audio_at = None
            repeated_region = False
            initial_repeat_at = None
            deadline = time.monotonic() + args.timeout
            while time.monotonic() < deadline:
                state = request(ipc, {'op': 'status'})['state']
                phase = state.get('activity', {}).get('phase')
                if phase not in observed:
                    observed.add(phase)
                    print('phase:', phase, flush=True)
                if phase == 'error':
                    raise AssertionError(state.get('notice_text') or 'reader entered error state')
                if args.hud_progress:
                    current_text = state.get('chunk_text') or state.get('sentence_text')
                    if state.get('state') == 'reading' and current_text:
                        assert state.get('read_along_active') is True, 'read-along status missing active flag'
                        observed_hud_current = True
                        next_phase = (state.get('next_page_activity') or {}).get('phase')
                        observed_hud_next |= next_phase in {'capturing','ocr','tts','ready'}
                if args.repeat_region or args.repeat_read_along:
                    spoken = (state.get('prepared_text') or '') + ' ' + (state.get('sentence_text') or '')
                    frames = state.get('audio_callback_frames', 0)
                    assert presses[0] == 0, 'manual Repeat unexpectedly sent Space'
                    if not repeated_region and frames > 1000 and 'speech recognition' in spoken.lower():
                        if args.repeat_read_along:
                            assert request(ipc, {'op':'action','action':'read-along-stop'}).get('ok')
                        initial_repeat_at = state.get('last_action_at_ms', 0)
                        assert initial_repeat_at > 0, 'screenshot action did not register Repeat metadata'
                        drawn = threading.Event()
                        def change_dialogue():
                            label.set_markup('<span size="24000" foreground="#111111">' + PAGES[1] + '</span>')
                            page[0] = 1
                            drawn.set()
                            return False
                        GLib.idle_add(change_dialogue)
                        assert drawn.wait(2), 'dialogue fixture did not update'
                        time.sleep(.3)
                        reply = request(ipc, {'op':'action','action':'repeat'})
                        assert reply.get('ok'), reply.get('message')
                        repeated_region = True
                        second_audio_at = frames
                    elif repeated_region:
                        assert subprocess.run(['pgrep','-P',str(daemon.pid),'slurp'],capture_output=True).returncode != 0, 'Repeat opened a new region picker'
                        if 'language models' in spoken.lower() and frames > second_audio_at + 1000:
                            assert state.get('last_action_at_ms', 0) > initial_repeat_at
                            outcome['passed'] = True
                            print('PASS: initial region selection -> real OCR/speech -> manual dialogue change -> Repeat recaptures same area; no reselection or page key', flush=True)
                            break
                    time.sleep(.1)
                    continue
                if args.focus_change and not returned_focus and state.get('notice_text') == 'Read-along waiting for selected window':
                    waited_for_focus = True
                    time.sleep(1)
                    assert presses[0] == 0 and not decoy_keys, f'input escaped into the wrong window: target Space={presses[0]}, decoy Space={len(decoy_keys)}'
                    query_target(folder, activate=True)
                    returned_focus = True
                first_audio |= state.get('audio_callback_frames', 0) > 0
                spoken = (state.get('prepared_text') or '') + ' ' + (state.get('sentence_text') or '')
                if first_audio and page[0] >= 1 and 'language models' in spoken.lower():
                    frames = state.get('audio_callback_frames', 0)
                    if second_audio_at is None:
                        second_audio_at = frames
                    elif frames > second_audio_at + 1000:
                        assert not args.focus_change or waited_for_focus, 'no focus wait was exercised'
                        if args.hud_progress:
                            assert observed_hud_current, 'HUD never had current reading text'
                            assert args.no_prefetch or observed_hud_next, 'HUD never exposed independent next-page work during playback'
                            print('PASS: read-along HUD current text and next-page work exposed independently', flush=True)
                        assert first_change_presses[0] == (2 if args.unchanged_once else 1), 'unexpected extra or missing Space presses'
                        assert not decoy_keys, 'background window received input'
                        outcome['passed'] = True
                        print('PASS: real region selection -> KWin capture -> OCR -> speech output -> next-page reading; prefetch=' + str(not args.no_prefetch), flush=True)
                        break
                time.sleep(.1)
            assert outcome['passed'], 'no second-page speech after real capture/OCR; see ' + str(folder / 'reader.log')
        except Exception:
            traceback.print_exc()
            print('E2E artifacts:', folder, flush=True)
        finally:
            rotated_token = data / 'remote-desktop-keyboard-token'
            if rotated_token.exists():
                descriptor = os.open(permission_token, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
                with os.fdopen(descriptor, 'wb') as destination:
                    destination.write(rotated_token.read_bytes())
            if daemon:
                try:
                    request(data / 'simple-tts.sock', {'op': 'shutdown'})
                    daemon.wait(timeout=5)
                except Exception:
                    daemon.kill()
                    daemon.wait()
            log.close()
            GLib.idle_add(Gtk.main_quit)
    GLib.timeout_add(600, lambda: (threading.Thread(target=exercise, daemon=True).start(), False)[1])
    Gtk.main()
    raise SystemExit(0 if outcome['passed'] else 1)


if __name__ == '__main__':
    main()
