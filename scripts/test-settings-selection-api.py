#!/usr/bin/env python3
"""Real Settings HTTP regression for schema 9, discovery, and draft-only refresh."""
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]


def main():
    binary = ROOT / 'target/debug' / ('simple-stt-settings.exe' if os.name == 'nt' else 'simple-stt-settings')
    with tempfile.TemporaryDirectory(prefix='simple-stt-selection-api-') as temporary:
        config = Path(temporary) / 'config.json'
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(config),
                   SIMPLE_STT_RUNTIME_ROOT=str(Path(temporary) / "runtime"),
                   XDG_DATA_HOME=str(Path(temporary) / "data"))
        if os.name != 'nt':
            # Unsupported Wayland cannot inherit the real session's XWayland language.
            env.update(XDG_SESSION_TYPE='wayland', WAYLAND_DISPLAY='unavailable-test-display',
                       DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-test-bus')
        process = subprocess.Popen([str(binary), '--no-browser'], cwd=ROOT, env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            parsed = urlparse(process.stdout.readline().strip())
            origin = f'{parsed.scheme}://{parsed.netloc}'
            token = parse_qs(parsed.fragment)['token'][0]

            def api(path, body=None, auth=True):
                request = Request(origin + path, data=None if body is None else json.dumps(body).encode())
                request.add_header('Origin', origin)
                if auth:
                    request.add_header('X-Simple-STT-Token', token)
                if body is not None:
                    request.add_header('Content-Type', 'application/json')
                with urlopen(request, timeout=10) as response:
                    return json.load(response)

            state = api('/api/state')
            speech = state['config']['speech']
            assert state['config']['schema_version'] == 9
            assert speech['selection_mode'] == 'single_model'
            assert speech['single_model_filename'] is None and speech['language_models'] == {}
            before = config.read_bytes()
            try:
                api('/api/keyboard-languages', auth=False)
                raise AssertionError('discovery accepted unauthenticated request')
            except HTTPError as error:
                assert error.code == 400
            languages = api('/api/keyboard-languages')
            assert config.read_bytes() == before, 'refresh wrote configuration'
            if os.name != 'nt':
                assert languages['available'] is False and languages['languages'] == [], languages
                assert 'unavailable' in languages['message'].lower()
            print('PASS authenticated read-only discovery; unsupported Wayland does not use XWayland')
            for mode, expected in [('english', 'old-en.gguf'), ('arabic', 'old-ar.gguf'), ('follow_keyboard', 'old-en.gguf')]:
                migrated = api('/api/normalize', {'schema_version': 8, 'speech': {
                    'language_mode': mode, 'english_model_filename': 'old-en.gguf',
                    'arabic_model_filename': 'old-ar.gguf'}})['config']['speech']
                assert migrated['single_model_filename'] == expected
                assert migrated['language_models'] == {'en': 'old-en.gguf', 'ar': 'old-ar.gguf'}
                assert migrated['selection_mode'] == ('follow_keyboard' if mode == 'follow_keyboard' else 'single_model')
            draft = copy.deepcopy(state['config'])
            draft['speech'].update(selection_mode='follow_keyboard', single_model_filename='missing.gguf',
                                   language_models={'en': None, 'ar': 'tdt_ctc-110m-q8_0.gguf',
                                                    'xkb:unknown:variant': 'missing.gguf'})
            normalized = api('/api/normalize', draft)['config']
            assert normalized['speech'] == draft['speech'], 'metadata or missing files changed choice'
            assert config.read_bytes() == before, 'import preview wrote configuration'
            saved = api('/api/save', {'config': normalized, 'expected_hash': state['config_hash']})
            assert saved['config']['speech'] == draft['speech']
            assert json.loads(config.read_text())['speech'] == draft['speech']
            before = config.read_bytes()
            api('/api/keyboard-languages')
            assert config.read_bytes() == before
            defaults = api('/api/defaults')['config']['speech']
            assert defaults['single_model_filename'] is None and defaults['language_models'] == {}
            assert config.read_bytes() == before, 'reset preview wrote configuration'
            print('PASS legacy migrations, explicit None, permissive/missing files, Save and draft-only import/reset')
        finally:
            process.terminate()
            process.wait(timeout=5)


if __name__ == '__main__':
    main()
