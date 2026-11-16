#!/usr/bin/env python3
"""Exercise a first-use STT download and inference in a private Linux session.

Requires network, a Vulkan GPU, PipeWire, WirePlumber, and built debug binaries.
A fresh run archives any previous test profile; --reuse keeps its downloads.
The private audio graph has only a null sink, and never uses host devices.
"""
import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
PROFILE = ROOT / 'artifacts/runtime-download-fix/stt-profile'
MODEL = 'realtime_eou_120m-v1-q8_0.gguf'
RUNTIME = 'parakeet-vulkan'


def load_script(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


live = load_script('private_reader_live', 'test-reader-live.py')
service = live.service
protocol = load_script('private_stt_protocol', 'test-linux-language.py')


def run_private(profile, binaries, reuse):
    assert os.environ.get('SIMPLE_STT_PRIVATE_AUDIO') == '1'
    env = os.environ.copy()
    for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'PULSE_SERVER',
                'PULSE_RUNTIME_PATH', 'PIPEWIRE_CONFIG_DIR'):
        env.pop(key, None)
    conf = profile / 'pipewire-conf'
    conf.mkdir(parents=True, exist_ok=True)
    (conf / 'private.conf').write_text(live.PW_CONFIG)
    alsa = profile / 'asound.conf'
    alsa.write_text('pcm.!default { type null }\nctl.!default { type hw card -1 }\n')
    runtime_root = profile / 'runtime'
    fixture = runtime_root / 'fixtures/parakeet-smoke.wav'
    fixture.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / 'fixtures/parakeet-smoke.wav', fixture)
    env.update(PIPEWIRE_REMOTE='pipewire-0', ALSA_CONFIG_PATH=str(alsa),
               SIMPLE_STT_RUNTIME_ROOT=str(runtime_root), SIMPLE_STT_NO_DESKTOP='1',
               SIMPLE_TTS_NO_DESKTOP='1', SIMPLE_STT_CONFIG=str(profile / 'config.json'),
               SIMPLE_STT_READER_DATA=str(profile / 'reader'), SIMPLE_TTS_CONFIG=str(profile / 'reader/config.json'),
               XDG_DATA_HOME=str(profile / 'data'), XDG_CONFIG_HOME=str(profile / 'config'),
               XDG_CACHE_HOME=str(profile / 'cache'), XDG_STATE_HOME=str(profile / 'state'))
    runtime_dir = Path(env['XDG_RUNTIME_DIR'])
    logs = profile / 'logs'
    logs.mkdir(parents=True, exist_ok=True)
    handles = {name: (logs / f'{name}.log').open('w') for name in ('pipewire', 'policy', 'capture', 'settings')}
    pipewire = policy = capture = settings = None
    state_path = profile / 'capture-state.json'
    state_path.unlink(missing_ok=True)
    capture_token = uuid.uuid4().hex
    workers = {}
    try:
        pipewire = subprocess.Popen(['pipewire', '-c', 'private.conf'],
                                   env={**env, 'PIPEWIRE_CONFIG_DIR': str(conf)},
                                   stdin=subprocess.DEVNULL, stdout=handles['pipewire'], stderr=handles['pipewire'])
        service.wait_for(lambda: (runtime_dir / 'pipewire-0').exists() or pipewire.poll() is not None,
                         10, 'private PipeWire socket')
        assert pipewire.poll() is None, 'private PipeWire failed; see pipewire.log'
        policy = subprocess.Popen(['wireplumber', '-p', 'policy'], env=env, stdin=subprocess.DEVNULL,
                                  stdout=handles['policy'], stderr=handles['policy'])
        def null_sink():
            result = subprocess.run(['pw-dump'], env=env, capture_output=True, text=True, timeout=4)
            return result.returncode == 0 and any(
                item.get('info', {}).get('props', {}).get('node.name') == 'private-null'
                for item in json.loads(result.stdout))
        service.wait_for(null_sink, 10, 'private null sink')
        assert policy.poll() is None, 'private audio policy exited'
        capture = subprocess.Popen([str(binaries / 'simple-stt-capture'), '--token', capture_token,
                                    '--state-file', str(state_path), '--config', env['SIMPLE_STT_CONFIG']],
                                   env=env, stdin=subprocess.DEVNULL,
                                   stdout=handles['capture'], stderr=handles['capture'])
        service.wait_for(lambda: state_path.exists() or capture.poll() is not None, 10, 'private capture')
        assert capture.poll() is None, 'capture exited; see capture.log'
        capture_state = json.loads(state_path.read_text())
        settings = subprocess.Popen([str(binaries / 'simple-stt-settings'), '--no-browser',
                                     '--state-file', str(state_path), '--service-token', capture_token],
                                    env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=handles['settings'], text=True)
        assert select.select([settings.stdout], [], [], 15)[0], 'Settings URL timeout'
        parsed = urlparse(settings.stdout.readline().strip())
        assert parsed.scheme == 'http' and parsed.hostname == '127.0.0.1' and parsed.port
        web_token = parse_qs(parsed.fragment)['token'][0]
        origin = f'http://127.0.0.1:{parsed.port}'
        def api(path, body=None):
            request = Request(origin + path, data=None if body is None else json.dumps(body).encode())
            request.add_header('Origin', origin)
            request.add_header('X-Simple-STT-Token', web_token)
            if body is not None:
                request.add_header('Content-Type', 'application/json')
            with urlopen(request, timeout=15) as response:
                return json.load(response)
        def command(name, **fields):
            return protocol.command(capture_state, capture_token, name, **fields)
        state = api('/api/state')
        assert state['service_online'], 'Settings did not connect to the private capture service'
        speech = state['config']['speech']
        if not reuse:
            assert not speech['runtime_dir'] and not speech['single_model_filename'], speech
            assert not any((profile / 'data').rglob('libparakeet.so')), 'fresh profile already has a runtime'
            assert not (profile / 'models' / MODEL).exists(), 'fresh profile already has a model'
            draft = copy.deepcopy(state['config'])
            draft['speech'].update(selection_mode='single_model', runtime_dir='', single_model_filename=None,
                                   model_dir=str(profile / 'models'), inference_device='gpu', idle_worker_timeout_secs=15)
            api('/api/save', {'config': draft, 'expected_hash': state['config_hash']})
            speech = api('/api/state')['config']['speech']
            assert speech['selection_mode'] == 'single_model' and not speech['runtime_dir'] and speech['single_model_filename'] is None
        else:
            assert speech['selection_mode'] == 'single_model', 'reused profile is not in single-model mode'
        assets = api('/api/assets')['assets']
        runtime = next(a for a in assets if a['id'] == RUNTIME)
        if not reuse:
            assert not runtime['installed'], 'runtime was installed before the first-use download'
        if not runtime['installed']:
            api('/api/asset-action', {'action': 'install', 'id': RUNTIME})
            def runtime_ready():
                snapshot = api('/api/assets')
                job = next((j for j in snapshot['jobs'] if j['job_id'] == RUNTIME), None)
                if job and job['phase'] in ('failed', 'cancelled'):
                    raise AssertionError(f'runtime download {job["phase"]}: {job.get("error", "")}')
                return next(a for a in snapshot['assets'] if a['id'] == RUNTIME)['installed'] and job and job['phase'] == 'ready'
            service.wait_for(runtime_ready, 600, 'runtime download and verification')
        asset_dir = Path(next(a for a in api('/api/assets')['assets'] if a['id'] == RUNTIME)['directory'])
        assert asset_dir.resolve().is_relative_to((profile / 'data').resolve()), 'asset escaped private data directory'
        assert not asset_dir.is_symlink(), 'runtime directory must be a real download'
        library = asset_dir / 'libparakeet.so'
        assert library.is_file() and not library.is_symlink(), f'expected real downloaded runtime at {library}'
        assert (asset_dir / '.verified').is_file(), 'asset manager did not verify downloaded runtime'
        print('PASS verified Vulkan runtime download', flush=True)
        model_path = profile / 'models' / MODEL
        sequence = 0
        def poll_events():
            nonlocal sequence
            response = command('poll_events', after_seq=sequence)
            events = response['events']
            sequence = max([sequence] + [event['seq'] for event in events])
            for pid in service.children(capture.pid):
                if service.executable(pid) == 'simple-stt-infer':
                    workers[pid] = service.start_tick(pid)
            return events
        if not model_path.exists():
            api('/api/action', {'action': 'download_model', 'filename': MODEL})
            def model_complete():
                for event in poll_events():
                    if event['kind'] == 'model_download_failed':
                        raise AssertionError(f'model download failed: {event}')
                    if event['kind'] == 'model_download_complete' and event['values'].get('filename') == MODEL:
                        return True
                return False
            service.wait_for(model_complete, 1200, 'actual speech model download')
        elif not reuse:
            raise AssertionError('fresh model unexpectedly existed')
        assert model_path.is_file() and not model_path.is_symlink() and model_path.stat().st_size > 1024 * 1024
        speech = api('/api/state')['config']['speech']
        assert speech['single_model_filename'] == MODEL, f'model was not auto-selected: {speech}'
        assert Path(speech['runtime_dir']).resolve() == asset_dir.resolve(), 'model download changed the runtime'
        saved = json.loads((profile / 'config.json').read_text())['speech']
        assert saved['single_model_filename'] == MODEL and Path(saved['runtime_dir']).resolve() == asset_dir.resolve()
        print('PASS downloaded model and persisted automatic selections', flush=True)
        def model_test():
            api('/api/action', {'action': 'test_model', 'filename': MODEL, 'language': 'english'})
            def finished():
                for event in poll_events():
                    if event['kind'] == 'model_test_failed':
                        raise AssertionError(f'model test failed: {event}')
                    if event['kind'] == 'model_test_complete':
                        return event
                return None
            event = service.wait_for(finished, 120, 'real speech model test')
            assert re.search(r'\([1-9]\d* characters\)', event['text']), f'empty transcription: {event}'
        model_test()
        assert len(workers) == 1, f'expected one inference worker: {workers}'
        pid, tick = next(iter(workers.items()))
        assert service.alive(pid) and service.start_tick(pid) == tick, 'first inference worker not retained'
        model_test()
        assert workers == {pid: tick} and service.alive(pid) and service.start_tick(pid) == tick, f'worker was not reused: {workers}'
        capture_log = (logs / 'capture.log').read_text(errors='replace')
        assert re.search(r'backend using device:\s*(vulkan|cuda|gpu)', capture_log, re.IGNORECASE), 'GPU backend not confirmed; see capture.log'
        print(f'PASS public WAV transcription on GPU and exact worker reuse ({pid}/{tick})', flush=True)
    finally:
        for process in (settings, capture, policy, pipewire):
            live.terminate(process)
        for pid, tick in workers.items():
            if service.start_tick(pid) == tick and service.alive(pid):
                os.kill(pid, 9)
        for handle in handles.values():
            handle.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--reuse', action='store_true', help='reuse the previous private downloaded profile')
    parser.add_argument('--private-session', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    if args.private_session:
        run_private(PROFILE, binaries, args.reuse)
        return
    missing = [name for name in ('pipewire', 'wireplumber', 'pw-dump', 'dbus-run-session') if not shutil.which(name)]
    missing += [str(binaries / name) for name in ('simple-stt-settings', 'simple-stt-capture', 'simple-stt-infer') if not (binaries / name).is_file()]
    if missing:
        raise SystemExit('Missing prerequisites: ' + ', '.join(missing))
    PROFILE.parent.mkdir(parents=True, exist_ok=True)
    if not args.reuse and PROFILE.exists():
        archive = PROFILE.with_name(f'stt-profile-previous-{time.time_ns()}')
        PROFILE.rename(archive)
        print(f'Archived previous test profile to {archive}', flush=True)
    PROFILE.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='stt-private-runtime-') as runtime:
        os.chmod(runtime, 0o700)
        env = os.environ.copy()
        for key in ('DBUS_SESSION_BUS_ADDRESS', 'DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET',
                    'PULSE_SERVER', 'PULSE_RUNTIME_PATH', 'PIPEWIRE_REMOTE'):
            env.pop(key, None)
        env.update(XDG_RUNTIME_DIR=runtime, SIMPLE_STT_PRIVATE_AUDIO='1')
        command = ['dbus-run-session', '--', sys.executable, str(Path(__file__).resolve()),
                   '--private-session', '--bin-dir', str(binaries)]
        if args.reuse:
            command.append('--reuse')
        subprocess.run(command, env=env, check=True, timeout=2100)


if __name__ == '__main__':
    main()
