#!/usr/bin/env python3
"""Exercise the real media CLI and private reader controller without desktop input/output."""
import argparse
import base64
import io
import json
import os
from pathlib import Path
import signal
import socket
import shutil
import subprocess
import tempfile
import threading
import time
import wave
from concurrent.futures import ThreadPoolExecutor
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'scripts/fixtures/reader/ocr-light.png'
EXPECTED = 'Simple TTS reads this screenshot.\nThe local voice stays on this device.'


def wait_for(predicate, timeout, description):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.04)
    raise AssertionError(f'timed out waiting for {description}')


def children(pid):
    """Read all Linux task child lists: worker processes may be spawned by a thread."""
    result = set()
    tasks = Path(f'/proc/{pid}/task')
    try:
        for task in tasks.iterdir():
            try:
                result.update(int(child) for child in (task / 'children').read_text().split())
            except (FileNotFoundError, ProcessLookupError):
                pass
    except FileNotFoundError:
        pass
    return result


def executable(pid):
    try:
        return Path(os.readlink(f'/proc/{pid}/exe')).name
    except (FileNotFoundError, ProcessLookupError):
        return ''


def workers(pid, name):
    return {child for child in children(pid) if executable(child) == name}


def worker(pid, name):
    matches = workers(pid, name)
    assert len(matches) <= 1, (name, matches)
    return next(iter(matches), None)


def alive(pid):
    try:
        # A zombie has exited even if /proc has not yet been reaped.
        return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z'
    except (FileNotFoundError, ProcessLookupError):
        return False


def start_tick(pid):
    try:
        return int(Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[19])
    except (OSError, ValueError, IndexError):
        return None


def footprint(pid):
    """Return Linux CPU ticks and resident bytes for an exact live PID."""
    stat = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return {'pid': pid, 'cpu_ticks': int(stat[11]) + int(stat[12]),
            'threads': int(stat[17]), 'rss_bytes': int(stat[21]) * os.sysconf('SC_PAGE_SIZE')}


class WorkerMonitor:
    """Observe only exact children of our private controller while work runs."""
    def __init__(self, pid):
        self.pid = pid
        self.lock = threading.Lock()
        self.samples = []
        self.identities = {}
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while not self.stop.wait(.01):
            sample = {'reader-tts': set(), 'reader-ocr': set()}
            for pid in children(self.pid):
                name = executable(pid)
                if name in sample:
                    tick = start_tick(pid)
                    if tick is not None:
                        sample[name].add(pid)
                        self.identities.setdefault(pid, tick)
            with self.lock:
                self.samples.append((time.monotonic(), sample))

    def mark(self):
        return time.monotonic()

    def since(self, mark, name):
        with self.lock:
            return set().union(*(sample[name] for when, sample in self.samples if when >= mark))

    def close(self):
        self.stop.set()
        self.thread.join(timeout=3)

    def owned_alive(self, pid):
        expected = self.identities.get(pid)
        return expected is not None and start_tick(pid) == expected and alive(pid)


def run(args, env, timeout=45):
    result = subprocess.run([str(x) for x in args], env=env, stdin=subprocess.DEVNULL,
                            capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise AssertionError(f'{args} failed ({result.returncode}):\n{result.stderr}\n{result.stdout}')
    return result.stdout.strip()


def stage_asset(assets, name, source, entries, digest):
    destination = assets / name
    destination.mkdir(parents=True)
    for entry in entries:
        target = source / entry
        if not target.exists():
            raise FileNotFoundError(target)
        (destination / entry).symlink_to(target, target_is_directory=target.is_dir())
    # The receipt belongs to our private directory; host assets remain untouched.
    (destination / '.verified').write_text(digest)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--tts-runtime', type=Path,
                        default=Path.home() / '.local/share/simple-tts/runtime')
    parser.add_argument('--tts-model', type=Path,
                        default=Path.home() / '.local/share/simple-tts/models/kokoro-multi-lang-v1_0')
    parser.add_argument('--ocr-model', type=Path, default=ROOT / 'artifacts/combined-app/downloads/model-paddle')
    parser.add_argument('--ocr-runtime', type=Path, default=Path.home() / '.local/share/simple-tts/runtime')
    parser.add_argument('--cpu-ocr-runtime', type=Path, default=ROOT / 'artifacts/combined-app/memory/cpu-runtime',
                        help='CPU ONNX Runtime used to verify low-memory auto OCR')
    parser.add_argument('--device', choices=['cpu', 'gpu'], default='gpu')
    parser.add_argument('--tts-runtime-id', choices=['sherpa-cpu', 'sherpa-cuda'], default='sherpa-cuda')
    parser.add_argument('--stt-runtime', type=Path, help='installed Parakeet runtime directory')
    parser.add_argument('--stt-model', type=Path, help='installed GGUF file; enables mixed GPU STT test')
    parser.add_argument('--stt-input', type=Path, default=ROOT / 'fixtures/parakeet-smoke.wav')
    args = parser.parse_args()
    if bool(args.stt_runtime) != bool(args.stt_model):
        parser.error('--stt-runtime and --stt-model must be supplied together')
    if args.device == 'gpu':
        nvidia = shutil.which('nvidia-smi')
        if not nvidia or subprocess.run([nvidia, '-L'], capture_output=True, timeout=5).returncode != 0:
            print('SKIP GPU reader integration: no usable NVIDIA GPU; use --device cpu --tts-runtime-id sherpa-cpu for CPU coverage', flush=True)
            return
    binaries = {name: (args.bin_dir / name).resolve() for name in
                ('simple-stt-linux', 'readerd', 'readerctl', 'reader-tts', 'reader-ocr')}
    for path in binaries.values():
        if not path.is_file():
            raise SystemExit(f'missing executable: {path}')
    catalog = {entry['id']: entry for entry in json.loads((ROOT / 'assets/catalog/downloads.json').read_text())}
    tts_missing = [args.tts_runtime / 'lib/libsherpa-onnx-c-api.so'] + [
        args.tts_model / name for name in catalog['kokoro']['required']]
    tts_available = all(path.exists() for path in tts_missing)
    if not tts_available:
        print('SKIP TTS and cross-worker lease checks: missing ' +
              ', '.join(str(path) for path in tts_missing if not path.exists()), flush=True)
    ocr_missing = [args.ocr_model / name for name in catalog['paddle-ar-en']['required']]
    if not all(path.exists() for path in ocr_missing):
        print('SKIP reader integration: missing PaddleOCR assets: ' + ', '.join(str(p) for p in ocr_missing if not p.exists()), flush=True)
        return
    if not args.stt_runtime:
        print('SKIP mixed GPU STT: provide --stt-runtime and --stt-model', flush=True)

    with tempfile.TemporaryDirectory(prefix='simple-stt-reader-service-') as tmp:
        private = Path(tmp)
        data = private / 'data'
        reader = data / 'simple-stt' / 'reader'
        reader.mkdir(parents=True)
        assets = reader.parent / 'assets'
        ocr_model = catalog['paddle-ar-en']
        stage_asset(assets, 'paddle-ar-en', args.ocr_model.resolve(),
                    ocr_model['required'], ocr_model['sha256'])
        staged = set()
        def stage_runtime(asset_id, source):
            if asset_id in staged:
                return
            stage_asset(assets, asset_id, source.resolve(), ['lib'], catalog[asset_id]['sha256'])
            staged.add(asset_id)
            for dependency in catalog[asset_id].get('dependencies', []):
                if dependency in staged:
                    continue
                asset = catalog[dependency]
                destination = assets / dependency
                destination.mkdir(parents=True)
                for entry in asset['required']:
                    target = source / 'lib' / Path(entry).name
                    if not target.exists():
                        raise FileNotFoundError(target)
                    link = destination / entry
                    link.parent.mkdir(parents=True, exist_ok=True)
                    link.symlink_to(target.resolve())
                (destination / '.verified').write_text(asset['sha256'])
                staged.add(dependency)
        if not (args.cpu_ocr_runtime / 'lib/libonnxruntime.so').exists():
            raise SystemExit(f'missing CPU OCR runtime: {args.cpu_ocr_runtime}')
        stage_runtime('sherpa-cpu', args.cpu_ocr_runtime)
        if args.device == 'gpu':
            stage_runtime('sherpa-cuda', args.ocr_runtime)
        if tts_available:
            stage_runtime(args.tts_runtime_id, args.tts_runtime)
            stage_asset(assets, 'kokoro', args.tts_model.resolve(),
                        [entry.name for entry in args.tts_model.iterdir() if entry.name != '.verified'], catalog['kokoro']['sha256'])
        env = os.environ.copy()
        env.update(XDG_DATA_HOME=str(data), XDG_CONFIG_HOME=str(private / 'config'),
                   XDG_CACHE_HOME=str(private / 'cache'), SIMPLE_STT_RUNTIME_ROOT=str(private / 'runtime'),
                   SIMPLE_STT_READER_DATA=str(reader), SIMPLE_TTS_CONFIG=str(reader / 'config.json'),
                   SIMPLE_TTS_NO_DESKTOP='1', DISPLAY='', WAYLAND_DISPLAY='',
                   DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-simple-stt-test-bus')
        env['SIMPLE_TTS_SERVER_TOKEN'] = 'private-reader-acceptance-token'
        if args.stt_runtime:
            if not args.stt_runtime.is_dir() or not args.stt_model.is_file() or not args.stt_input.is_file():
                raise SystemExit('STT runtime, model and input must exist')
            stt_config = private / 'stt-config.json'
            stt_config.write_text(json.dumps({'speech': {
                'selection_mode': 'single_model', 'runtime_dir': str(args.stt_runtime.resolve()),
                'model_dir': str(args.stt_model.resolve().parent),
                'single_model_filename': args.stt_model.name, 'inference_device': 'gpu',
                'idle_worker_timeout_secs': 1}}))
            env['SIMPLE_STT_CONFIG'] = str(stt_config)
        binding = lambda runtime, model: {'runtime': runtime, 'model': model, 'device': args.device}
        config = {
            'speech': {'selection': {'mode': 'auto_detect', 'languages': {'en': binding(args.tts_runtime_id, 'kokoro')}}},
            'ocr': {'selection': {'mode': 'single_model', 'single': binding('paddle-cuda' if args.device == 'gpu' else 'paddle-cpu', 'paddle-ar-en')},
                    'idle_timeout_secs': 4},
            'playback': {'keep_warm_seconds': 12},
        }
        config_path = reader / 'config.json'
        config_path.write_text(json.dumps(config))
        observed = set()
        with (private / 'readerd.log').open('w+') as log:
            daemon = subprocess.Popen([str(binaries['readerd']), 'daemon', '--foreground', '--no-desktop'],
                                      env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            monitor = WorkerMonitor(daemon.pid)
            try:
                def socket_ready():
                    if daemon.poll() is not None:
                        log.flush()
                        raise AssertionError(f'controller exited {daemon.returncode}: {(private / "readerd.log").read_text()}')
                    return (reader / 'simple-tts.sock').exists()
                wait_for(socket_ready, 10, 'private controller socket')
                cli = binaries['simple-stt-linux']
                def ocr():
                    start = time.monotonic()
                    text = run([cli, 'media', 'ocr', FIXTURE], env)
                    assert text == EXPECTED, f'unexpected OCR text: {text!r}'
                    print(f'OCR latency_ms={(time.monotonic()-start)*1000:.1f}', flush=True)
                def speak(path, text='Hello world, this is an English sentence.'):
                    start = time.monotonic()
                    run([cli, 'media', 'speak', text, '--output', path], env, timeout=120)
                    with wave.open(str(path), 'rb') as audio:
                        assert audio.getnframes() > 0 and audio.getnchannels() > 0 and audio.getframerate() > 0
                        assert any(audio.readframes(audio.getnframes())), 'WAV audio was entirely silent'
                    print(f'TTS {path.name} latency_ms={(time.monotonic()-start)*1000:.1f}', flush=True)
                def observed_operation(name, fn):
                    mark = monitor.mark()
                    fn()
                    pids = monitor.since(mark, name)
                    assert pids, f'{name} was not observed while operation ran'
                    observed.update(pids)
                    return pids

                ocr_pids = observed_operation('reader-ocr', ocr)
                ocr_pid = wait_for(lambda: worker(daemon.pid, 'reader-ocr'), 3, 'retained OCR')
                assert ocr_pid in ocr_pids and monitor.owned_alive(ocr_pid)
                ocr_tick = monitor.identities[ocr_pid]
                observed_operation('reader-ocr', ocr)
                assert worker(daemon.pid, 'reader-ocr') == ocr_pid and start_tick(ocr_pid) == ocr_tick, 'OCR was not reused'
                wait_for(lambda: not monitor.owned_alive(ocr_pid), 9, 'OCR idle timeout')
                print(f'PASS OCR exact-PID {ocr_pid}/{ocr_tick} reuse and idle expiry', flush=True)
                if tts_available:
                    file_pids = observed_operation('reader-tts', lambda: speak(private / 'first.wav'))
                    file_pid = wait_for(lambda: worker(daemon.pid, 'reader-tts'), 3, 'retained file voice')
                    assert file_pid in file_pids and monitor.owned_alive(file_pid)
                    file_tick = monitor.identities[file_pid]
                    observed_operation('reader-tts', lambda: speak(private / 'second.wav'))
                    assert worker(daemon.pid, 'reader-tts') == file_pid and start_tick(file_pid) == file_tick, 'file voice was not reused'
                    # An unassigned language cannot borrow the English model,
                    # even though that runtime and model are installed and warm.
                    unassigned_output = private / 'unassigned-arabic.wav'
                    missing = subprocess.run([str(cli), 'media', 'speak', 'مرحبا بالعالم.', '--output', str(unassigned_output)], env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
                    assert missing.returncode != 0 and not unassigned_output.exists(), 'unassigned Arabic inherited an English assignment'
                    assert worker(daemon.pid, 'reader-tts') == file_pid and monitor.owned_alive(file_pid), 'missing language retired the configured voice'
                    print('PASS unassigned Arabic is rejected without inheriting or unloading English', flush=True)
                    # OCR's shorter timeout must not retire the independent file voice.
                    ocr_pids = observed_operation('reader-ocr', ocr)
                    ocr_pid = wait_for(lambda: worker(daemon.pid, 'reader-ocr'), 3, 'second retained OCR')
                    assert ocr_pid in ocr_pids
                    wait_for(lambda: not monitor.owned_alive(ocr_pid), 9, 'independent OCR idle timeout')
                    assert monitor.owned_alive(file_pid), 'OCR timeout retired the voice worker'
                    wait_for(lambda: not monitor.owned_alive(file_pid), 17, 'file voice idle timeout')
                    print(f'PASS file voice exact-PID {file_pid}/{file_tick} reuse and independent idle expiry', flush=True)
                    if args.stt_runtime:
                        # Start all three tasks together, sampling the private CLI's exact
                        # STT child and the controller's reader children while active.
                        transcript_path = private / 'mixed-transcript.txt'
                        transcript_file = transcript_path.open('w+', encoding='utf-8')
                        stt = subprocess.Popen([str(cli), 'media', 'transcribe', str(args.stt_input)],
                                               env=env, stdin=subprocess.DEVNULL,
                                               stdout=transcript_file, stderr=subprocess.PIPE, text=True)
                        stt_pids = set()
                        triple = threading.Event()
                        stop_monitor = threading.Event()
                        def monitor_stt():
                            while not stop_monitor.wait(0.01):
                                active = {pid for pid in children(stt.pid) if executable(pid) == 'simple-stt-infer'}
                                stt_pids.update(active)
                                if active and workers(daemon.pid, 'reader-tts') and workers(daemon.pid, 'reader-ocr'):
                                    triple.set()
                        monitor_thread = threading.Thread(target=monitor_stt)
                        monitor_thread.start()
                        mark = monitor.mark()
                        try:
                            with ThreadPoolExecutor(max_workers=2) as pool:
                                ocr_result = pool.submit(ocr)
                                speech_result = pool.submit(speak, private / 'mixed.wav')
                                ocr_result.result(timeout=120)
                                speech_result.result(timeout=120)
                            _, stderr = stt.communicate(timeout=120)
                            transcript_file.flush()
                            transcript = transcript_path.read_text(encoding='utf-8').strip()
                            assert stt.returncode == 0, f'GPU STT failed: {stderr}\n{transcript}'
                            assert transcript, 'private STT wrote an empty transcript file'
                            assert stt_pids, 'did not observe actual STT worker while active'
                            assert triple.is_set(), 'did not observe STT, OCR and TTS workers concurrently'
                            wait_for(lambda: all(not alive(pid) for pid in stt_pids), 5, 'STT worker exit')
                            for name in ('reader-tts', 'reader-ocr'):
                                pids = monitor.since(mark, name)
                                observed.update(pids)
                                assert pids, f'{name} was not observed during mixed work'
                            print(f'PASS overlapping OCR/TTS/GPU STT; exact STT PIDs {sorted(stt_pids)}', flush=True)
                        finally:
                            stop_monitor.set()
                            monitor_thread.join()
                            if stt.poll() is None:
                                stt.kill()
                                stt.wait()
                            transcript_file.close()
                            for pid in stt_pids:
                                if alive(pid):
                                    try:
                                        os.kill(pid, signal.SIGKILL)
                                    except ProcessLookupError:
                                        pass

                    # Start only a private loopback HTTP listener. Its model worker is
                    # independent from desktop playback, and unload keeps the listener.
                    with socket.socket() as port_probe:
                        port_probe.bind(('127.0.0.1', 0))
                        port = port_probe.getsockname()[1]
                    config['server'] = {'enabled': True, 'listen': f'127.0.0.1:{port}'}
                    config_path.write_text(json.dumps(config))
                    run([binaries['readerctl'], 'server-start'], env)
                    base_url = f'http://127.0.0.1:{port}'
                    def http(path, value=None, authorized=True):
                        headers = {'Content-Type':'application/json'}
                        if authorized: headers['Authorization'] = 'Bearer ' + env['SIMPLE_TTS_SERVER_TOKEN']
                        request = Request(base_url + path, data=json.dumps(value,ensure_ascii=False).encode() if value else None, headers=headers)
                        with urlopen(request, timeout=45) as response:
                            return response.read()
                    try:
                        http('/v1/models', authorized=False)
                        raise AssertionError('unauthenticated inference API accepted')
                    except HTTPError as error:
                        assert error.code == 401
                    assert json.loads(http('/v1/models'))['data']
                    api_pid = None
                    for text in ['Hello world.', 'The price is five dollars.']:
                        result = []
                        def request_speech():
                            result.append(http('/v1/audio/speech', {'input':text, 'response_format':'wav'}))
                        pids = observed_operation('reader-tts', request_speech)
                        live = {pid for pid in pids if monitor.owned_alive(pid)}
                        assert live, 'API voice exited immediately after completion'
                        if api_pid is None:
                            api_pid = next(iter(live))
                            api_tick = monitor.identities[api_pid]
                        else:
                            assert api_pid in live and start_tick(api_pid) == api_tick, 'API voice was not reused'
                        with wave.open(io.BytesIO(result[0]), 'rb') as audio:
                            assert audio.getframerate() == 24000 and any(audio.readframes(audio.getnframes()))
                    time.sleep(1)
                    assert monitor.owned_alive(api_pid), 'API voice did not stay warm'
                    wait_for(lambda: not monitor.owned_alive(api_pid), 17, 'API voice idle timeout')
                    print(f'PASS API voice exact-PID {api_pid}/{api_tick} reuse and idle expiry', flush=True)
                    image = 'data:image/png;base64,' + base64.b64encode(FIXTURE.read_bytes()).decode()
                    result = []
                    def request_ocr():
                        result.append(json.loads(http('/v1/chat/completions', {'model':'simple-tts-ocr', 'messages':[
                            {'role':'user','content':[{'type':'image_url','image_url':{'url':image}}]}]})))
                    http_ocr_pids = observed_operation('reader-ocr', request_ocr)
                    recognized = result[0]
                    assert recognized['choices'][0]['message']['content'] == EXPECTED, recognized
                    run([binaries['readerctl'], 'unload-now'], env)
                    wait_for(lambda: not workers(daemon.pid,'reader-tts') and not workers(daemon.pid,'reader-ocr'), 5, 'Unload all native models')
                    assert json.loads(http('/v1/models'))['data'], 'Unload stopped the API listener'
                    fresh_pids = observed_operation('reader-ocr', ocr)
                    assert fresh_pids.isdisjoint(http_ocr_pids), f'OCR reused an unloaded PID: {http_ocr_pids} / {fresh_pids}'
                    run([binaries['readerctl'], 'unload-now'], env)
                    wait_for(lambda: not workers(daemon.pid, 'reader-ocr'), 5, 'fresh OCR unload')
                    print('PASS authenticated HTTP WAV and unload-all with listener retained', flush=True)
                else:
                    print('PASS CLI OCR and private controller', flush=True)
                # Switch only this temporary profile to auto OCR with the CPU
                # runtime. Cheap Paddle should be retained until its own timeout.
                config['ocr']['selection']['single'] = binding('paddle-cpu', 'paddle-ar-en')
                config['ocr']['selection']['single']['device'] = 'auto'
                config['ocr']['idle_timeout_secs'] = 4
                config['playback']['keep_warm_seconds'] = 30
                config_path.write_text(json.dumps(config))
                cpu_pids = observed_operation('reader-ocr', ocr)
                cpu_pid = wait_for(lambda: worker(daemon.pid, 'reader-ocr'), 5, 'cheap CPU OCR retained')
                assert cpu_pid in cpu_pids
                for _ in range(2):
                    observed_operation('reader-ocr', ocr)
                    assert worker(daemon.pid, 'reader-ocr') == cpu_pid, 'CPU OCR was not reused'
                maps = Path(f'/proc/{cpu_pid}/maps').read_text()
                assert 'libonnxruntime_providers_cuda' not in maps, 'CPU auto OCR loaded CUDA provider'
                if shutil.which('nvidia-smi'):
                    gpu_list = run(['nvidia-smi', '--query-compute-apps=pid,used_gpu_memory',
                                    '--format=csv,noheader,nounits'], env, timeout=5)
                    assert not any(line.split(',')[0].strip() == str(cpu_pid) for line in gpu_list.splitlines()), 'CPU OCR owns GPU allocation'
                print(f'CPU OCR exact-PID diagnostic: {footprint(cpu_pid)}', flush=True)
                before_idle = footprint(cpu_pid)
                time.sleep(1)
                after_idle = footprint(cpu_pid)
                assert after_idle['cpu_ticks'] - before_idle['cpu_ticks'] <= 2, (before_idle, after_idle)
                print(f'PASS retained CPU OCR idle ticks: {after_idle["cpu_ticks"] - before_idle["cpu_ticks"]}', flush=True)
                wait_for(lambda: not monitor.owned_alive(cpu_pid), 8, 'independent CPU OCR idle timeout')
                assert not workers(daemon.pid, 'reader-ocr')
                print('PASS CPU auto OCR reuse and independent idle expiry', flush=True)
                if args.device == 'gpu':
                    # CPU execution must stay cheap even when the user has only
                    # downloaded the CUDA runtime shared by their other models.
                    config['ocr']['selection']['single'] = binding('paddle-cuda', 'paddle-ar-en')
                    config['ocr']['selection']['single']['device'] = 'auto'
                    config_path.write_text(json.dumps(config))
                    observed_operation('reader-ocr', ocr)
                    cpu_pid = wait_for(lambda: worker(daemon.pid, 'reader-ocr'), 5, 'CPU OCR with CUDA runtime retained')
                    assert 'libonnxruntime_providers_cuda' not in Path(f'/proc/{cpu_pid}/maps').read_text()
                    gpu_list = run(['nvidia-smi', '--query-compute-apps=pid,used_gpu_memory',
                                    '--format=csv,noheader,nounits'], env, timeout=5)
                    assert not any(line.split(',')[0].strip() == str(cpu_pid) for line in gpu_list.splitlines()), 'CPU OCR with CUDA runtime owns GPU allocation'
                    before_idle = footprint(cpu_pid)
                    time.sleep(1)
                    after_idle = footprint(cpu_pid)
                    assert after_idle['cpu_ticks'] - before_idle['cpu_ticks'] <= 2, (before_idle, after_idle)
                    print(f'PASS CPU OCR with CUDA runtime idle ticks: {after_idle["cpu_ticks"] - before_idle["cpu_ticks"]}', flush=True)
                    run([binaries['readerctl'], 'unload-now'], env)
                    wait_for(lambda: not monitor.owned_alive(cpu_pid), 5, 'CPU OCR manual unload')
                    print('PASS automatic CPU OCR uses CUDA runtime without loading CUDA provider or allocating VRAM', flush=True)
                assert daemon.poll() is None, 'controller exited during CLI requests'
            finally:
                monitor.close()
                observed.update(monitor.since(0, 'reader-tts'))
                observed.update(monitor.since(0, 'reader-ocr'))
                shutdown_error = None
                if daemon.poll() is None:
                    try:
                        run([binaries['readerctl'], 'shutdown'], env, timeout=8)
                        daemon.wait(timeout=8)
                    except Exception as error:
                        shutdown_error = error
                        daemon.terminate()
                        try:
                            daemon.wait(timeout=3)
                        except subprocess.TimeoutExpired:
                            daemon.kill()
                            daemon.wait(timeout=3)
                try:
                    wait_for(lambda: all(not monitor.owned_alive(pid) for pid in observed), 5,
                             'private child processes to exit after graceful shutdown')
                finally:
                    for pid in observed:
                        if monitor.owned_alive(pid):
                            # Only terminate children we observed under our private controller.
                            try:
                                os.kill(pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                if shutdown_error:
                    raise AssertionError(f'private controller did not shut down cleanly: {shutdown_error}') from shutdown_error


if __name__ == '__main__':
    main()
