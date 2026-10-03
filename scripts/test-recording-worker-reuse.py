#!/usr/bin/env python3
"""Real capture/microphone with an isolated mock worker; no shell or text delivery."""
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import tempfile
import time

from importlib.util import module_from_spec, spec_from_file_location

ROOT = Path(__file__).resolve().parents[1]
spec = spec_from_file_location('language_test', ROOT / 'scripts/test-linux-language.py')
language_test = module_from_spec(spec)
spec.loader.exec_module(language_test)
command = language_test.command


def main():
    suffix = '.exe' if os.name == 'nt' else ''
    with tempfile.TemporaryDirectory(prefix='simple-stt-recording-reuse-') as temporary:
        directory = Path(temporary)
        for source, target in [('simple-stt-capture', 'simple-stt-capture'),
                               ('simple_stt_mock_infer', 'simple-stt-infer')]:
            shutil.copy2(ROOT / 'target/debug' / (source + suffix), directory / (target + suffix))
        for filename in ['hang.gguf', 'normal.gguf', 'other.gguf']:
            (directory / filename).touch()
        config = directory / 'config.json'
        draft = {'schema_version': 9, 'speech': {
            'selection_mode': 'single_model', 'single_model_filename': 'hang.gguf',
            'model_dir': str(directory), 'runtime_dir': str(directory),
            'inference_device': 'cpu', 'worker_shutdown_grace_ms': 250,
            'idle_worker_timeout_secs': 10,
        }}
        config.write_text(json.dumps(draft))
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(config),
                   SIMPLE_STT_RUNTIME_ROOT=str(directory), XDG_DATA_HOME=str(directory / 'data'))
        state_path = directory / 'state.json'
        token = secrets.token_hex(32)
        with (directory / 'capture.log').open('w') as output:
            process = subprocess.Popen([str(directory / ('simple-stt-capture' + suffix)),
                '--config', str(config), '--state-file', str(state_path), '--token', token],
                env=env, stdout=output, stderr=output)
            try:
                deadline = time.monotonic() + 15
                while not state_path.exists():
                    assert process.poll() is None, (directory / 'capture.log').read_text()
                    assert time.monotonic() < deadline, 'capture startup timed out'
                    time.sleep(.05)
                state = json.loads(state_path.read_text())
                sequence = 0
                observed = []

                def poll():
                    nonlocal sequence
                    events = command(state, token, 'poll_events', after_seq=sequence)['events']
                    sequence = max([sequence] + [event['seq'] for event in events])
                    observed.extend(events)
                    return events

                def wait_event(kind, session=None):
                    deadline = time.monotonic() + 10
                    while time.monotonic() < deadline:
                        for event in poll():
                            if event['kind'] == kind and (session is None or event['session_id'] == session):
                                return event
                        time.sleep(.02)
                    raise AssertionError(f'{kind} timed out: {(directory / "capture.log").read_text()}')

                def select(filename):
                    draft['speech']['single_model_filename'] = filename
                    config.write_text(json.dumps(draft))
                    command(state, token, 'reload_config')

                command(state, token, 'start_recording', session_id=1, target_window=None)
                wait_event('model_ready')
                old_pid = command(state, token, 'ping')['values']['worker_pid']
                time.sleep(.3)
                command(state, token, 'stop_recording', session_id=1)
                wait_event('transcribing', 1)
                # The first mock deliberately never finishes transcription.
                select('normal.gguf')
                command(state, token, 'start_recording', session_id=2, target_window=None)
                wait_event('model_ready')
                new_pid = command(state, token, 'ping')['values']['worker_pid']
                assert new_pid != old_pid
                assert not any(event['kind'] == 'transcript' and event['session_id'] == 1 for event in observed)
                print('PASS: new start aborts blocked inference and primes replacement during recording')
                time.sleep(.3)
                command(state, token, 'stop_recording', session_id=2)
                wait_event('transcript', 2)
                command(state, token, 'delivery_complete', session_id=2)

                for session in [3, 4, 5]:
                    command(state, token, 'start_recording', session_id=session, target_window=None)
                    wait_event('model_reused', session)
                    assert command(state, token, 'ping')['values']['worker_pid'] == new_pid
                    command(state, token, 'cancel')
                repeated = [event for event in observed if event['session_id'] in [3, 4, 5]]
                assert not any(event['kind'] in ['model_loading', 'model_loaded', 'model_ready'] for event in repeated)
                print('PASS: repeated starts reuse one ready worker without loading/warm-up notices')

                # Rapid successive starts while the worker is being replaced.
                select('other.gguf')
                command(state, token, 'start_recording', session_id=6, target_window=None)
                select('normal.gguf')
                command(state, token, 'start_recording', session_id=7, target_window=None)
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if any(event['kind'] in ['model_reused', 'model_ready'] and event['session_id'] == 7 for event in poll()):
                        break
                    time.sleep(.02)
                else:
                    raise AssertionError('latest rapid start did not prepare its model')
                time.sleep(.3)
                command(state, token, 'stop_recording', session_id=7)
                wait_event('transcript', 7)
                assert not any(event['kind'] == 'transcript' and event['session_id'] in [1, 6] for event in observed)
                print('PASS: rapid model switches deliver only the latest session')
                command(state, token, 'shutdown')
                assert process.wait(timeout=10) == 0
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=10)


if __name__ == '__main__':
    main()
