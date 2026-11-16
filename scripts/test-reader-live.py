#!/usr/bin/env python3
"""Exercise reader playback on a private PipeWire null sink, never host audio.

Requires PipeWire, WirePlumber, pw-dump, a private D-Bus session, and the same
GPU assets as test-reader-service.py. No microphone or clipboard is used.
"""
import argparse
import importlib.util
import json
import os
import re
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('reader_service_test', ROOT / 'scripts/test-reader-service.py')
service = importlib.util.module_from_spec(spec)
spec.loader.exec_module(service)
BIN = ROOT / 'target/debug'
SUPER = ROOT / 'artifacts/combined-app/downloads/sherpa-onnx-supertonic-3-tts-int8-2026-05-11'
OCR = ROOT / 'artifacts/combined-app/downloads/model-paddle'
RUNTIME = Path.home() / '.local/share/simple-tts/runtime'
KOKORO = Path.home() / '.local/share/simple-tts/models/kokoro-multi-lang-v1_0'

# Explicit module allowlist: no ALSA, UDev, Bluetooth or device monitor.
PW_CONFIG = '''
context.properties = { core.daemon = true core.name = pipewire-0 default.clock.rate = 48000 }
context.spa-libs = { audio.convert.* = audioconvert/libspa-audioconvert audio.adapt = audioconvert/libspa-audioconvert support.* = support/libspa-support }
context.modules = [
 { name = libpipewire-module-protocol-native }
 { name = libpipewire-module-metadata }
 { name = libpipewire-module-spa-node-factory }
 { name = libpipewire-module-client-node }
 { name = libpipewire-module-access }
 { name = libpipewire-module-adapter }
 { name = libpipewire-module-link-factory }
 { name = libpipewire-module-session-manager }
]
context.objects = [
 { factory = spa-node-factory args = { factory.name = support.node.driver node.name = Dummy-Driver priority.driver = 200000 } }
 { factory = adapter args = { factory.name = support.null-audio-sink node.name = private-null node.description = PrivateNull media.class = Audio/Sink audio.position = [ FL FR ] object.linger = true } }
]
'''


def terminate(process):
    if process and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)


def local_request(path, data):
    encoded = json.dumps(data, ensure_ascii=False).encode()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(8)
        client.connect(str(path))
        client.sendall(struct.pack('<I', len(encoded) + 1) + b'\0' + encoded)
        def exact(size):
            result = bytearray()
            while len(result) < size:
                chunk = client.recv(size - len(result))
                if not chunk:
                    raise RuntimeError('controller closed reply')
                result.extend(chunk)
            return bytes(result)
        length = struct.unpack('<I', exact(4))[0]
        assert 2 <= length <= 65536
        payload = exact(length)
        assert payload[:1] == b'\0'
        reply = json.loads(payload[1:])
        assert reply['ok'], reply
        return reply


def run_private(folder, runtime, kokoro, supertonic, ocr, stt_runtime=None, stt_model=None):
    root = Path(folder)
    assert os.environ.get('SIMPLE_STT_PRIVATE_AUDIO') == '1'
    env = os.environ.copy()
    config_dir = root / 'pipewire-conf'
    config_dir.mkdir()
    (config_dir / 'private.conf').write_text(PW_CONFIG)
    (root / 'asound.conf').write_text('pcm.!default { type null }\nctl.!default { type hw card -1 }\n')
    env.pop('PIPEWIRE_CONFIG_DIR', None)
    env.update(PIPEWIRE_REMOTE='pipewire-0',
               ALSA_CONFIG_PATH=str(root / 'asound.conf'), SIMPLE_TTS_NO_DESKTOP='1',
               SIMPLE_STT_READER_DATA=str(root / 'reader'), SIMPLE_TTS_CONFIG=str(root / 'reader/config.json'),
               SIMPLE_STT_RUNTIME_ROOT=str(root / 'runtime'), XDG_DATA_HOME=str(root / 'data'),
               XDG_CONFIG_HOME=str(root / 'config'), XDG_CACHE_HOME=str(root / 'cache'),
               XDG_STATE_HOME=str(root / 'state'), SIMPLE_STT_NO_DESKTOP='1',
               SIMPLE_STT_CONFIG=str(root / 'stt-config.json'))
    stt_config = {} if stt_runtime is None else {'diagnostics': {'log_level': 'normal'}, 'speech': {
        'runtime_dir': str(stt_runtime), 'model_dir': str(stt_model.parent),
        'inference_device': 'gpu', 'idle_worker_timeout_secs': 3,
    }}
    (root / 'stt-config.json').write_text(json.dumps(stt_config))
    if stt_runtime is not None:
        # The test overrides the program root to a private tree. Supply its
        # public test input there; installed-package fixture presence is checked
        # independently by test-linux-package.py.
        fixture = root / 'runtime/fixtures/parakeet-smoke.wav'
        fixture.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / 'fixtures/parakeet-smoke.wav', fixture)
    for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'PULSE_SERVER', 'PULSE_RUNTIME_PATH'):
        env.pop(key, None)
    logs = ROOT / 'artifacts/combined-app/live-reader'
    logs.mkdir(parents=True, exist_ok=True)
    handles = {name: (logs / f'{name}.log').open('w') for name in ('pipewire','policy','readerd','capture')}
    pipewire = policy = daemon = capture = conversion = None
    observed = set()
    capture_workers = {}
    monitor_stop = threading.Event()
    monitor = None
    try:
        pipewire = subprocess.Popen(['pipewire', '-c', 'private.conf'],
                                    env={**env, 'PIPEWIRE_CONFIG_DIR': str(config_dir)},
                                    stdin=subprocess.DEVNULL, stdout=handles['pipewire'], stderr=handles['pipewire'])
        service.wait_for(lambda: (Path(env['XDG_RUNTIME_DIR']) / 'pipewire-0').exists() or pipewire.poll() is not None,
                         8, 'private PipeWire socket')
        assert pipewire.poll() is None, 'private PipeWire failed'
        # The policy profile links streams and chooses defaults but enables no hardware monitors.
        policy = subprocess.Popen(['wireplumber', '-p', 'policy'], env=env, stdin=subprocess.DEVNULL,
                                  stdout=handles['policy'], stderr=handles['policy'])
        def graph():
            result = subprocess.run(['pw-dump'], env=env, capture_output=True, text=True, timeout=4)
            return json.loads(result.stdout) if result.returncode == 0 else []
        def sink_ready():
            return any(o.get('info', {}).get('props', {}).get('node.name') == 'private-null' for o in graph())
        service.wait_for(sink_ready, 10, 'private null sink')
        assert policy.poll() is None, 'private session policy failed'

        # Exercise the shared production Abort command. Audio sees only this
        # private PipeWire graph and ALSA null device; no recording is started.
        capture_state = root / 'capture-state.json'
        capture_token = uuid.uuid4().hex
        capture = subprocess.Popen([str(BIN / 'simple-stt-capture'), '--token', capture_token,
                                    '--state-file', str(capture_state), '--config', env['SIMPLE_STT_CONFIG']],
                                   env=env, stdin=subprocess.DEVNULL,
                                   stdout=handles['capture'], stderr=handles['capture'])
        service.wait_for(lambda: capture_state.exists() or capture.poll() is not None, 10, 'private capture control')
        assert capture.poll() is None, 'capture control exited'
        def global_abort():
            result = subprocess.run([str(BIN / 'simple-stt-ctl'), '--state-file', str(capture_state),
                                     '--token', capture_token, 'cancel'], env=env,
                                    capture_output=True, text=True, timeout=8)
            assert result.returncode == 0 and 'status\tok' in result.stdout, result.stderr + result.stdout
        global_abort()
        time.sleep(.15)
        assert not (root / 'reader/simple-tts.sock').exists(), 'Abort must not start the optional reader'
        if stt_runtime is not None:
            control = [BIN / 'simple-stt-ctl', '--state-file', capture_state, '--token', capture_token]
            def model_test(expected_count):
                queued = service.run([*control, 'test-model', '--filename', stt_model.name], env)
                assert 'status\tok' in queued, queued
                def completed():
                    for pid in service.children(capture.pid):
                        if service.executable(pid) == 'simple-stt-infer':
                            capture_workers[pid] = service.start_tick(pid)
                    events = service.run([*control, 'poll-events'], env)
                    return sum('\tmodel_test_complete\t' in line for line in events.splitlines()) >= expected_count
                service.wait_for(completed, 30, f'GPU capture model test {expected_count}')
            model_test(1)
            capture_log = (logs / 'capture.log').read_text(errors='replace')
            assert re.search(r'backend using device:\s*(vulkan|cuda|gpu)', capture_log, re.IGNORECASE), \
                f'GPU backend was not confirmed in capture log: {capture_log[-4000:]}'
            assert len(capture_workers) == 1, f'expected one STT worker: {capture_workers}'
            pid, tick = next(iter(capture_workers.items()))
            assert service.alive(pid) and service.start_tick(pid) == tick, 'STT worker did not remain warm'
            model_test(2)
            assert capture_workers == {pid: tick}, f'STT worker changed during warm reuse: {capture_workers}'
            assert service.alive(pid) and service.start_tick(pid) == tick, 'reused STT worker exited early'
            service.wait_for(lambda: not service.alive(pid) or service.start_tick(pid) != tick,
                             8, 'STT worker idle expiration after reuse')
            print(f'PASS capture GPU STT exact-PID {pid}/{tick} reused then expired at idle timeout', flush=True)

        assets = root / 'assets'
        catalog = {a['id']: a for a in json.loads((ROOT / 'assets/catalog/downloads.json').read_text())}
        service.stage_asset(assets, 'sherpa-cuda', runtime, ['lib'], catalog['sherpa-cuda']['sha256'])
        for dep in catalog['sherpa-cuda'].get('dependencies', []):
            asset = catalog[dep]
            dest = assets / dep
            dest.mkdir(parents=True)
            for item in asset['required']:
                link = dest / item
                link.parent.mkdir(parents=True, exist_ok=True)
                target = runtime / 'lib' / Path(item).name
                assert target.exists(), target
                link.symlink_to(target.resolve())
            (dest / '.verified').write_text(asset['sha256'])
        for asset_id, path in [('kokoro', kokoro), ('supertonic3', supertonic), ('paddle-ar-en', ocr)]:
            service.stage_asset(assets, asset_id, path, catalog[asset_id]['required'], catalog[asset_id]['sha256'])
        reader = root / 'reader'
        reader.mkdir()
        bind = lambda model: {'runtime': 'sherpa-cuda', 'model': model, 'device': 'gpu'}
        (reader / 'config.json').write_text(json.dumps({
            'speech': {'selection': {'mode': 'auto_detect', 'languages': {'en': bind('kokoro'), 'ar': bind('supertonic3')}}},
            'ocr': {'selection': {'mode': 'single_model', 'single': {'runtime': 'paddle-cuda',
                                    'model': 'paddle-ar-en', 'device': 'gpu'}}},
            'playback': {'keep_warm_seconds': 2, 'max_buffer_seconds': 8},
        }))
        daemon = subprocess.Popen([str(BIN / 'readerd'), 'daemon', '--foreground', '--no-desktop'], env=env,
                                  stdin=subprocess.DEVNULL, stdout=handles['readerd'], stderr=handles['readerd'])
        path = reader / 'simple-tts.sock'
        service.wait_for(lambda: path.exists() or daemon.poll() is not None, 10, 'reader socket')
        assert daemon.poll() is None
        def monitor_children():
            while not monitor_stop.wait(.02):
                observed.update(service.children(daemon.pid))
        monitor = threading.Thread(target=monitor_children)
        monitor.start()
        def request(value):
            return local_request(path, value)
        def status():
            state = request({'op': 'status'})['state']
            (logs / 'last-status.json').write_text(json.dumps(state, indent=2))
            return state
        def action(name):
            return request({'op': 'action', 'action': name})
        def read(text):
            uid = uuid.uuid4().hex
            request({'op': 'read_start', 'id': uid, 'source': 'private-test'})
            request({'op': 'read_chunk', 'id': uid, 'text': text})
            request({'op': 'read_finish', 'id': uid})
        def phase(state):
            return state.get('activity', {}).get('phase')
        def await_state(predicate, label, timeout=25):
            return service.wait_for(lambda: (lambda s: s if predicate(s) else None)(status()), timeout, label)
        # Ordinary playback completion must retain the exact worker, even for
        # a GPU voice whose allocation exceeds the old estimate thresholds.
        read('Hello world.')
        first_read = await_state(lambda s: s['state'] == 'reading' and bool(s.get('chunk_text')), 'first spoken passage')
        assert first_read['chunk_text'].strip(), first_read
        voice_pid = service.wait_for(lambda: service.worker(daemon.pid, 'reader-tts'), 5, 'local voice PID')
        voice_tick = service.start_tick(voice_pid)
        await_state(lambda s: s['state'] == 'idle', 'ordinary local playback completion')
        time.sleep(.4)
        assert service.alive(voice_pid) and service.start_tick(voice_pid) == voice_tick, 'voice unloaded immediately after reading'
        read('Hello again.')
        await_state(lambda s: s['state'] == 'reading' and bool(s.get('chunk_text')), 'reused voice reading')
        assert service.worker(daemon.pid, 'reader-tts') == voice_pid and service.start_tick(voice_pid) == voice_tick
        await_state(lambda s: s['state'] == 'idle', 'second ordinary completion')
        service.wait_for(lambda: not service.alive(voice_pid), 8, 'local voice idle expiry')
        print(f'PASS local voice exact-PID {voice_pid}/{voice_tick} warm reuse and configured expiry', flush=True)
        long_text = ('This is a private audio test with several sentences for playback. '
                     'The voice should continue while another request is prepared. ') * 15
        read(long_text)
        first = await_state(lambda s: s['audio_callback_frames'] > 0 and s['audio_queued_samples'] > 0,
                            'real output callback and queued speech')
        assert not first.get('audio_error') and first['audio_callback_errors'] == 0, first
        assert 'CUDA verified' in first['device'], first
        phases = {event['phase'] for event in first['activity']['events']} | {phase(first)}
        assert {'detecting_language', 'loading_voice', 'synthesizing', 'reading'} <= phases, phases
        def linked_to_sink():
            objects = graph()
            sinks = {o['id'] for o in objects if o.get('info', {}).get('props', {}).get('node.name') == 'private-null'}
            return any(o.get('type') == 'PipeWire:Interface:Link' and
                       o.get('info', {}).get('output-node-id') is not None and
                       o.get('info', {}).get('input-node-id') in sinks for o in objects)
        service.wait_for(linked_to_sink, 5, 'stream linked to private null sink (not ALSA fallback)')
        action('pause')
        paused = await_state(lambda s: s['state'] == 'paused', 'dictation pause')
        assert paused['audio_queued_samples'] > 0
        action('resume')
        await_state(lambda s: s['state'] == 'reading' and s['audio_callback_frames'] > paused['audio_callback_frames'], 'resume')
        generation = status()['generation']
        read('مرحبا بالعالم هذا اختبار باللغة العربية. ' * 15)
        newer = await_state(lambda s: s['generation'] and s['generation'] != generation and
                            s.get('activity', {}).get('model') == 'supertonic3' and
                            s['audio_queued_samples'] > 0 and s['state'] == 'reading' and
                            s['audio_callback_frames'] > first['audio_callback_frames'], 'superseding Arabic read')
        new_generation = newer['generation']
        assert newer['activity']['generation'] == new_generation, newer
        time.sleep(.2)
        latest = status()
        assert latest['generation'] == new_generation and latest['activity']['generation'] == new_generation, latest
        assert 'CUDA verified' in newer['device'], newer
        # File OCR must not interrupt the active reading generation.
        cli = subprocess.run([str(BIN / 'simple-stt-linux'), 'media', 'ocr', str(service.FIXTURE)],
                             env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=35)
        assert cli.returncode == 0 and cli.stdout.strip() == service.EXPECTED, cli.stderr
        assert status()['generation'] == new_generation, 'OCR replaced active read'
        global_abort()
        await_state(lambda s: s['state'] == 'idle' and s['audio_queued_samples'] == 0, 'stop and empty buffer')
        time.sleep(.3)
        assert status()['state'] == 'idle' and not status().get('prepared_text'), 'late speech survived Abort'
        output = root / 'cancelled-render.wav'
        conversion = subprocess.Popen([str(BIN / 'simple-stt-linux'), 'media', 'speak', long_text * 3,
                                       '--output', str(output)], env=env, stdin=subprocess.DEVNULL,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        service.wait_for(lambda: service.workers(daemon.pid, 'reader-tts'), 5, 'file-render worker')
        time.sleep(.15)
        global_abort()
        stdout, stderr = conversion.communicate(timeout=8)
        assert conversion.returncode != 0 and not output.exists(), (stdout, stderr)
        snapshot = status()
        assert not snapshot.get('audio_error') and snapshot['audio_callback_errors'] == 0, snapshot
        idle_before = service.footprint(daemon.pid)
        time.sleep(1)
        idle_after = service.footprint(daemon.pid)
        assert idle_after['cpu_ticks'] - idle_before['cpu_ticks'] <= 2, (idle_before, idle_after)
        print(json.dumps({'result': 'PASS', 'callback_frames': snapshot['audio_callback_frames'],
                          'initial_phase': phase(first), 'initial_phases': sorted(phases), 'pause_phase': phase(paused),
                          'idle_cpu_ticks': idle_after['cpu_ticks'] - idle_before['cpu_ticks'],
                          'controller': idle_after, 'observed_children': sorted(observed)}), flush=True)
        request({'op': 'shutdown'})
        daemon.wait(timeout=8)
        service.wait_for(lambda: all(not service.alive(pid) for pid in observed), 8, 'reader children exit')
    finally:
        monitor_stop.set()
        if monitor:
            monitor.join(timeout=2)
        if daemon:
            observed.update(service.children(daemon.pid))
        terminate(daemon)
        terminate(capture)
        terminate(conversion)
        for pid, tick in capture_workers.items():
            if service.start_tick(pid) == tick and service.alive(pid):
                os.kill(pid, 9)
        for pid in observed:
            if service.alive(pid):
                try: os.kill(pid, 9)
                except ProcessLookupError: pass
        terminate(policy)
        terminate(pipewire)
        for handle in handles.values(): handle.close()


def main():
    global BIN
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin-dir', type=Path, default=BIN)
    parser.add_argument('--session', help=argparse.SUPPRESS)
    parser.add_argument('--runtime', type=Path, default=RUNTIME)
    parser.add_argument('--kokoro', type=Path, default=KOKORO)
    parser.add_argument('--supertonic', type=Path, default=SUPER)
    parser.add_argument('--ocr', type=Path, default=OCR)
    parser.add_argument('--stt-runtime', type=Path)
    parser.add_argument('--stt-model', type=Path)
    args = parser.parse_args()
    BIN = args.bin_dir.resolve()
    if bool(args.stt_runtime) != bool(args.stt_model):
        parser.error('--stt-runtime and --stt-model must be supplied together')
    if args.session:
        run_private(args.session, args.runtime.resolve(), args.kokoro.resolve(), args.supertonic.resolve(), args.ocr.resolve(),
                    args.stt_runtime.resolve() if args.stt_runtime else None,
                    args.stt_model.resolve() if args.stt_model else None)
        return
    tools = ('pipewire', 'wireplumber', 'pw-dump', 'dbus-run-session', 'nvidia-smi')
    missing = [name for name in tools if not shutil.which(name)]
    missing += [str(BIN / name) for name in ('readerd', 'reader-tts', 'reader-ocr', 'readerctl', 'simple-stt-linux') if not (BIN / name).is_file()]
    catalog = {a['id']: a for a in json.loads((ROOT / 'assets/catalog/downloads.json').read_text())}
    for name, source in [('kokoro', args.kokoro), ('supertonic3', args.supertonic), ('paddle-ar-en', args.ocr)]:
        missing += [str(source / entry) for entry in catalog[name]['required'] if not (source / entry).exists()]
    if not (args.runtime / 'lib/libsherpa-onnx-c-api.so').exists():
        missing.append(str(args.runtime / 'lib/libsherpa-onnx-c-api.so'))
    if missing:
        print('SKIP private audio integration: missing ' + ', '.join(missing))
        return
    gpu = subprocess.run(['nvidia-smi', '-L'], capture_output=True, timeout=5)
    if gpu.returncode:
        print('SKIP private audio integration: NVIDIA GPU unavailable')
        return
    with tempfile.TemporaryDirectory(prefix='simple-stt-private-audio-') as tmp:
        runtime_dir = Path(tmp) / 'xdg-runtime'
        runtime_dir.mkdir(mode=0o700)
        env = os.environ.copy()
        for key in ('DBUS_SESSION_BUS_ADDRESS', 'DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET',
                    'PULSE_SERVER', 'PULSE_RUNTIME_PATH', 'PIPEWIRE_REMOTE'):
            env.pop(key, None)
        env.update(XDG_RUNTIME_DIR=str(runtime_dir), SIMPLE_STT_PRIVATE_AUDIO='1')
        command = ['dbus-run-session', '--', sys.executable, str(Path(__file__).resolve()), '--session', tmp,
                   '--bin-dir', str(BIN),
                   '--runtime', str(args.runtime.resolve()), '--kokoro', str(args.kokoro.resolve()),
                   '--supertonic', str(args.supertonic.resolve()), '--ocr', str(args.ocr.resolve())]
        if args.stt_runtime:
            command += ['--stt-runtime', str(args.stt_runtime.resolve()), '--stt-model', str(args.stt_model.resolve())]
        subprocess.run(command, env=env, check=True, timeout=140)


if __name__ == '__main__':
    main()
