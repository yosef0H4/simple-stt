#!/usr/bin/env python3
"""Real local inference through OpenAI's SDK and Simple STT's own clients.

Requires the openai Python package and installed local model/runtime assets.
Creates private headless host/client profiles; never contacts OpenAI's cloud.
"""
import base64
import copy
from concurrent.futures import ThreadPoolExecutor
import io
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import wave
import cairo
from openai import OpenAI, AuthenticationError, BadRequestError, APITimeoutError

ROOT = Path(__file__).resolve().parents[1]
BINS = ROOT / 'target/debug'
INSTALLED = Path.home() / '.local/share/simple-stt/instances/installed'
folder = Path(tempfile.mkdtemp(prefix='simple-stt-openai-e2e-'))
folder.chmod(0o700)
token = secrets.token_urlsafe(32)
processes = []
logs = []
log_paths = []
PRIVATE_CONTENT_PREFIXES = (
    'server-', 'transcription-', 'read-along-', 'clipboard-', 'cli-', 'region-', 'capture-',
)


def wait_for(check, seconds, label):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError('Timed out: ' + label)


def free_port():
    with socket.socket() as stream:
        stream.bind(('127.0.0.1', 0))
        return stream.getsockname()[1]


def private_json(path, value):
    path.write_text(json.dumps(value))
    path.chmod(0o600)


def profile(name, reader, app):
    directory = folder / name
    data = directory / 'reader'
    data.mkdir(parents=True)
    (directory / 'assets').symlink_to(INSTALLED / 'assets', target_is_directory=True)
    private_json(data / 'config.json', reader)
    private_json(directory / 'app.json', app)
    env = dict(os.environ, SIMPLE_STT_CONFIG=str(directory / 'app.json'),
        SIMPLE_STT_READER_DATA=str(data), SIMPLE_TTS_CONFIG=str(data / 'config.json'),
        SIMPLE_STT_RUNTIME_ROOT=str(directory), SIMPLE_STT_EMBEDDED_READER='1',
        SIMPLE_TTS_SERVER_TOKEN=token, SIMPLE_TTS_SPEECH_API_KEY=token,
        SIMPLE_TTS_OCR_API_KEY=token, SIMPLE_STT_STT_API_KEY=token)
    # A real server must work without a graphical desktop session.
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY']:
        env.pop(key, None)
    return directory, data, env


def launch(name, args, env):
    log_path = folder / f'{name}.log'
    log = log_path.open('w')
    logs.append(log)
    log_paths.append(log_path)
    child = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    processes.append(child)
    return child


def descendants(parent):
    found = {}
    queue = [parent]
    while queue:
        pid = queue.pop()
        path = Path(f'/proc/{pid}/task/{pid}/children')
        try:
            children = [int(value) for value in path.read_text().split()]
        except OSError:
            continue
        queue.extend(children)
        for child in children:
            try:
                binary = Path(Path(f'/proc/{child}/cmdline').read_bytes().split(b'\0')[0].decode()).name
                found[child] = binary
            except (OSError, UnicodeError):
                pass
    return found


def model_workers(parent):
    return {pid: name for pid, name in descendants(parent).items()
            if name in {'reader-tts', 'reader-ocr', 'simple-stt-infer'}}


def cli(env, *args, success=True):
    result = subprocess.run([str(BINS / 'simple-stt-linux'), 'media', *args], env=env,
        capture_output=True, text=True, timeout=180)
    if success:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode != 0, 'Request silently succeeded with fallback disabled'
    return result


def valid_wav(data):
    with wave.open(io.BytesIO(data)) as audio:
        assert audio.getnframes() > 1000 and audio.getframerate() >= 8000
        samples = audio.readframes(audio.getnframes())
        assert any(samples), 'silent audio response'


def assert_no_internal_content(data_dir):
    for directory_name in ('capture', 'requests', 'cli'):
        directory = data_dir / directory_name
        if not directory.exists():
            continue
        leftovers = [
            path.name for path in directory.iterdir()
            if path.is_file() and path.name.startswith(PRIVATE_CONTENT_PREFIXES)
        ]
        assert not leftovers, f'internal content remains in {directory}: {leftovers}'


def seed_crash_leftovers(data_dir):
    for directory_name, filename in (
        ('capture', 'capture-crash.png'),
        ('requests', 'transcription-crash.wav'),
        ('cli', 'cli-crash.png'),
    ):
        directory = data_dir / directory_name
        directory.mkdir(exist_ok=True)
        (directory / filename).write_bytes(b'SSTT_PRIVATE_CRASH_LEFTOVER')


def assert_default_logs_redact(*sentinels):
    for log in logs:
        log.flush()
    contents = '\n'.join(path.read_text(errors='replace') for path in log_paths)
    for sentinel in sentinels:
        assert sentinel not in contents, f'default logs retained content sentinel {sentinel!r}'


def main():
    reader = json.loads((INSTALLED / 'reader/config.json').read_text())
    app = json.loads((Path.home() / '.config/simple-stt/instances/installed/config.json').read_text())
    # Select known installed local engines, independent of desktop UI choices.
    reader['speech']['provider'] = {'kind': 'local_kokoro'}
    reader['speech']['selection'] = {'mode': 'single_model', 'single': {'device': 'cpu', 'runtime': 'sherpa-cuda', 'model': 'kokoro'}, 'languages': {}}
    reader['ocr']['provider'] = {'kind': 'local_paddle'}
    reader['ocr']['selection']['single']['device'] = 'cpu'
    reader['playback']['keep_warm_seconds'] = 2
    reader['ocr']['idle_timeout_secs'] = 2
    reader['speech']['selection']['single']['clean_text'] = False
    reader['speech']['selection']['single']['lowercase'] = False
    reader.setdefault('privacy', {})['log_content'] = False
    app['speech'].update(provider='local', selection_mode='single_model',
        single_model_filename=app['speech']['language_models']['en'], inference_device='cpu',
        runtime_dir=str(INSTALLED / 'assets/parakeet-cuda'), idle_worker_timeout_secs=2)
    app['cleanup']['enabled'] = False
    port = free_port()
    url = f'http://127.0.0.1:{port}/v1'
    reader['server'] = {'enabled': True, 'listen': f'127.0.0.1:{port}', 'allow_remote': False, 'max_clients': 4}
    host_dir, host_data, host_env = profile('host', reader, app)
    seed_crash_leftovers(host_data)
    server = launch('server', [str(BINS / 'readerd'), '--config', str(host_data / 'config.json'), 'server'], host_env)
    sdk = OpenAI(api_key=token, base_url=url, max_retries=0, timeout=180)
    def ready():
        if server.poll() is not None:
            raise AssertionError('headless server exited: see server.log')
        try:
            return sdk.models.list()
        except Exception:
            return None
    models = wait_for(ready, 15, 'authenticated server discovery')
    assert_no_internal_content(host_data)
    assert {'kokoro-v1', 'simple-tts-ocr'} <= {model.id for model in models.data}
    try:
        OpenAI(api_key='invalid-test-token', base_url=url, max_retries=0, timeout=3).models.list()
        raise AssertionError('server accepted invalid bearer token')
    except AuthenticationError:
        pass
    print('PASS official OpenAI SDK discovery and bearer authentication', flush=True)
    try:
        sdk.audio.speech.create(model='unavailable-test-model', voice='af_heart', input='Invalid model test.')
        raise AssertionError('server accepted an unsupported speech model')
    except BadRequestError:
        pass
    image = folder / 'dialogue.png'
    surface = cairo.ImageSurface(cairo.FORMAT_RGB24, 1000, 180)
    painter = cairo.Context(surface)
    painter.set_source_rgb(1, 1, 1); painter.paint()
    ocr_sentinel = 'SSTT_PRIVATE_OCR_314159'
    painter.set_source_rgb(0, 0, 0); painter.select_font_face('DejaVu Sans'); painter.set_font_size(42)
    painter.move_to(30, 75); painter.show_text('Courtroom dialogue 7')
    painter.set_font_size(30)
    painter.move_to(30, 140); painter.show_text(ocr_sentinel)
    surface.write_to_png(str(image))
    encoded = 'data:image/png;base64,' + base64.b64encode(image.read_bytes()).decode()
    tts_sentinel = 'SSTT_PRIVATE_TTS_271828'
    error_sentinel = 'SSTT_PRIVATE_ERROR_161803'
    text = f'This is an end to end speech synthesis test. {tts_sentinel}.'
    response = sdk.audio.speech.create(model='kokoro-v1', voice='af_heart', input=text, response_format='wav')
    valid_wav(response.content)
    first_tts = {pid for pid, name in model_workers(server.pid).items() if name == 'reader-tts'}
    valid_wav(sdk.audio.speech.create(model='kokoro-v1', voice='af_heart', input='A second warm request.', response_format='wav').content)
    second_tts = {pid for pid, name in model_workers(server.pid).items() if name == 'reader-tts'}
    assert first_tts and first_tts == second_tts, 'TTS did not reuse its warm worker'
    with ThreadPoolExecutor(max_workers=2) as pool:
        for data in pool.map(lambda number: sdk.audio.speech.create(model='kokoro-v1', voice='af_heart', input=f'Concurrent request {number}.', response_format='wav').content, [1, 2]):
            valid_wav(data)
    assert first_tts == {pid for pid, name in model_workers(server.pid).items() if name == 'reader-tts'}, 'concurrent speech requests duplicated the worker'
    print('PASS SDK speech: real non-silent WAV and warm worker reuse', flush=True)
    chat = sdk.chat.completions.create(model='simple-tts-ocr', messages=[{'role': 'user', 'content': [
        {'type': 'text', 'text': 'Transcribe the image without rewriting.'}, {'type': 'image_url', 'image_url': {'url': encoded}}]}])
    recognized = chat.choices[0].message.content
    assert 'courtroom dialogue' in recognized.lower(), recognized
    assert ocr_sentinel.lower() in recognized.lower(), recognized
    responses = sdk.responses.create(model='simple-tts-ocr', input=[{'role': 'user', 'content': [
        {'type': 'input_text', 'text': 'Transcribe the image.'}, {'type': 'input_image', 'image_url': encoded}]}])
    assert 'courtroom dialogue' in responses.output_text.lower(), responses.output_text
    print('PASS SDK OCR: chat completions and Responses API with real Paddle OCR', flush=True)
    with (ROOT / 'fixtures/parakeet-smoke.wav').open('rb') as audio:
        transcript = sdk.audio.transcriptions.create(model='configured-stt', file=audio, language='en').text
    assert len(transcript.strip()) > 4, transcript
    first_stt = {pid for pid, name in model_workers(server.pid).items() if name == 'simple-stt-infer'}
    with (ROOT / 'fixtures/parakeet-smoke.wav').open('rb') as audio:
        repeated = sdk.audio.transcriptions.create(model='configured-stt', file=audio, language='en').text
    assert transcript == repeated
    assert first_stt and first_stt == {pid for pid, name in model_workers(server.pid).items() if name == 'simple-stt-infer'}, 'STT worker was duplicated/reloaded'
    print('PASS SDK multipart STT: real transcription and warm worker reuse', flush=True)
    assert_no_internal_content(host_data)
    remote_reader = copy.deepcopy(reader)
    remote_reader['server'] = {'enabled': False}
    remote_reader['speech']['provider'] = {'kind': 'open_ai_compatible', 'base_url': url, 'model': 'kokoro-v1', 'voice': 'af_heart', 'format': 'wav'}
    remote_reader['ocr']['provider'] = {'kind': 'open_ai_compatible', 'base_url': url, 'model': 'simple-tts-ocr', 'api': 'chat_completions'}
    remote_reader['speech']['fallback_to_local'] = False
    remote_reader['ocr']['fallback_to_local'] = False
    # Missing local assets make an accidental client-side local fallback fail.
    remote_reader['speech']['model_dir'] = str(folder / 'missing-tts-model')
    remote_reader['ocr']['model_dir'] = str(folder / 'missing-ocr-model')
    remote_app = copy.deepcopy(app)
    remote_app['speech']['provider'] = 'remote_openai_compatible'
    remote_app['speech']['remote_openai_compatible'] = {'base_url': url, 'model': 'configured-stt', 'fallback_to_local': False, 'timeout_ms': 180000}
    client_dir, client_data, client_env = profile('client', remote_reader, remote_app)
    seed_crash_leftovers(client_data)
    client = launch('client', [str(BINS / 'readerd'), 'daemon', '--no-desktop'], client_env)
    wait_for((client_data / 'simple-tts.sock').exists, 10, 'headless remote client')
    assert_no_internal_content(client_data)
    output = folder / 'client-speech.wav'
    cli(client_env, 'speak', text, '--output', str(output))
    valid_wav(output.read_bytes())
    recognized_client = json.loads(cli(client_env, 'ocr', str(image), '--json').stdout)['text']
    assert 'courtroom dialogue' in recognized_client.lower(), recognized_client
    assert ocr_sentinel.lower() in recognized_client.lower(), recognized_client
    client_transcript = json.loads(cli(client_env, 'transcribe', str(ROOT / 'fixtures/parakeet-smoke.wav'), '--language', 'english', '--json').stdout)['text']
    assert client_transcript == transcript
    assert_no_internal_content(client_data)
    invalid_image = folder / 'invalid-image.png'
    invalid_image.write_bytes(error_sentinel.encode())
    cli(client_env, 'ocr', str(invalid_image), '--json', success=False)
    assert_no_internal_content(client_data)
    assert_no_internal_content(host_data)
    print('PASS our clients → our server: real TTS, OCR, STT; local fallback disabled', flush=True)
    remote_app['speech']['remote_openai_compatible']['base_url'] = f'http://127.0.0.1:{free_port()}/v1'
    private_json(client_dir / 'app.json', remote_app)
    cli(client_env, 'transcribe', str(ROOT / 'fixtures/parakeet-smoke.wav'), '--language', 'english', '--json', success=False)
    remote_app['speech']['remote_openai_compatible']['fallback_to_local'] = True
    private_json(client_dir / 'app.json', remote_app)
    fallback = json.loads(cli(client_env, 'transcribe', str(ROOT / 'fixtures/parakeet-smoke.wav'), '--language', 'english', '--json').stdout)['text']
    assert fallback == transcript
    print('PASS disconnected STT provider: fail with fallback off; real local transcription with fallback on', flush=True)
    try:
        wait_for(lambda: not model_workers(server.pid), 15, 'server native model workers idle-unload')
    except AssertionError:
        print('Workers remaining after idle:', model_workers(server.pid), flush=True)
        raise
    print('PASS headless server model workers exit after idle timeout', flush=True)
    with (ROOT / 'fixtures/parakeet-smoke.wav').open('rb') as audio:
        reloaded = sdk.audio.transcriptions.create(model='configured-stt', file=audio, language='en').text
    assert reloaded == transcript
    reloaded_workers = {pid for pid, name in model_workers(server.pid).items() if name == 'simple-stt-infer'}
    assert reloaded_workers and first_stt != reloaded_workers, 'STT did not reload after idle exit'
    print('PASS next STT request lazily reloads after idle exit', flush=True)
    assert_no_internal_content(host_data)
    try:
        sdk.with_options(timeout=.02).audio.speech.create(model='kokoro-v1', voice='af_heart', input='Client timeout recovery test.', response_format='wav')
    except APITimeoutError:
        wait_for(lambda: not model_workers(server.pid), 15, 'model cleanup after client timeout')
        assert sdk.models.list().data
        print('PASS timed-out SDK caller: server remains responsive and workers retire', flush=True)
    assert_no_internal_content(host_data)
    assert_no_internal_content(client_data)
    assert_default_logs_redact(tts_sentinel, ocr_sentinel, error_sentinel)
    print('PASS internal screenshots/audio are removed after success and OCR error; default logs redact payload sentinels', flush=True)


try:
    main()
finally:
    for process in reversed(processes):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGINT)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
    for log in logs:
        log.close()
    print('E2E artifacts:', folder, flush=True)
