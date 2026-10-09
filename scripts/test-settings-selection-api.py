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


def start_settings_server(binary, config, runtime_root, data_home):
    env = dict(os.environ, SIMPLE_STT_CONFIG=str(config),
               SIMPLE_STT_RUNTIME_ROOT=str(runtime_root), XDG_DATA_HOME=str(data_home))
    if os.name != 'nt':
        env.update(XDG_SESSION_TYPE='wayland', WAYLAND_DISPLAY='unavailable-test-display',
                   DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-test-bus')
    process = subprocess.Popen([str(binary), '--no-browser'], cwd=ROOT, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    line = process.stdout.readline().strip()
    if not line:
        stderr = process.stderr.read()
        raise AssertionError(f'Settings server did not start: {stderr}')
    parsed = urlparse(line)
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

    return process, api


def stop_settings_server(process):
    process.terminate()
    process.wait(timeout=5)


def test_setup_choice(binary, temporary, name, choice, *, install_before=False, manual_edit=False,
                      reopen_before_install=False):
    case_root = Path(temporary) / name
    config = case_root / 'config.json'
    runtime = case_root / 'runtime'
    data_home = case_root / 'data'
    process, api = start_settings_server(binary, config, runtime, data_home)
    try:
        initial = api('/api/state')
        setup = initial['model_setup']
        english, arabic = setup['english_file'], setup['arabic_file']
        selected = [filename for filename, use in ((english, choice['english']),
                                                    (arabic, choice['arabic'])) if use]
        model_dir = Path(initial['resolved_model_dir'])

        def install_selected(directory):
            directory.mkdir(parents=True, exist_ok=True)
            for filename in selected:
                (directory / filename).write_bytes(b'isolated fake model')

        if install_before:
            install_selected(model_dir)
        assert api('/api/model-setup', choice)['ok']
        pending = api('/api/state')
        if not install_before:
            assert pending['config']['speech'] == initial['config']['speech'], \
                f'{name}: choice applied before selected model files were installed'
        if selected and not install_before:
            assert pending['model_setup']['pending'] and not pending['model_setup']['show']
            if reopen_before_install:
                stop_settings_server(process)
                process, api = start_settings_server(binary, config, runtime, data_home)
                reopened = api('/api/state')
                assert reopened['config']['speech'] == initial['config']['speech'], \
                    f'{name}: persisted choice applied before downloads completed'
                assert reopened['model_setup']['pending']
                model_dir = Path(reopened['resolved_model_dir'])
            if manual_edit:
                draft = pending['config']
                alternative = next(model['file'] for model in pending['models']
                                   if model['file'] not in selected)
                draft['speech'].update(selection_mode='single_model',
                                       single_model_filename=alternative)
                api('/api/save', {'config': draft, 'expected_hash': pending['config_hash']})
                preserved = api('/api/state')
                assert preserved['config']['speech']['selection_mode'] == 'single_model'
                assert preserved['config']['speech']['single_model_filename'] == alternative
                assert not preserved['model_setup']['pending']
                return
            install_selected(model_dir)

        final = api('/api/state')
        speech = final['config']['speech']
        if choice['english'] and choice['arabic']:
            assert speech['selection_mode'] == 'follow_keyboard'
            assert speech['language_models']['en'] == english
            assert speech['language_models']['ar'] == arabic
        elif selected:
            assert speech['selection_mode'] == 'single_model'
            assert speech['single_model_filename'] == selected[0]
        else:
            assert speech['selection_mode'] == 'single_model'
            assert speech['single_model_filename'] is None
        assert not final['model_setup']['pending'] and not final['model_setup']['show']
    finally:
        if process.poll() is None:
            stop_settings_server(process)


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
            assert state['config']['general']['ui_language'] == 'auto'
            assert state['ui_localization']['system_locale'] in ('en', 'ar')
            assert api('/api/normalize', {'schema_version': 9, 'general': {'ui_language': 'invalid'}})['config']['general']['ui_language'] == 'auto'
            assert state['model_setup']['show']
            assert state['model_setup']['arabic_file'] == 'lemura-arabic-asr-lite-q8_0.gguf'
            for key in ['english_file', 'arabic_file']:
                offered = next(model for model in state['models'] if model['file'] == state['model_setup'][key])
                assert offered['recommended'] and not offered['installed']
            speech = state['config']['speech']
            assert state['config']['schema_version'] == 9
            assert speech['selection_mode'] == 'single_model'
            assert speech['single_model_filename'] is None and speech['language_models'] == {}
            language_draft = copy.deepcopy(state['config'])
            language_draft['general']['ui_language'] = 'ar'
            api('/api/save', {'config': language_draft, 'expected_hash': state['config_hash']})
            localized = api('/api/state')
            assert localized['ui_localization']['locale'] == 'ar' and localized['ui_localization']['direction'] == 'rtl'
            assert localized['config']['speech'] == speech
            language_draft['general']['ui_language'] = 'auto'
            api('/api/save', {'config': language_draft, 'expected_hash': localized['config_hash']})
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
            for version in [7, 8, 10]:
                try:
                    api('/api/normalize', {'schema_version': version})
                    raise AssertionError('unsupported schema accepted')
                except HTTPError as error:
                    assert error.code == 400
                    assert 'unsupported config schema_version' in error.read().decode()
            assert config.read_bytes() == before, 'unsupported schema changed saved configuration'
            normalized_obsolete = api('/api/normalize', {'schema_version': 9, 'speech': {
                'language_mode': 'arabic', 'english_model_filename': 'old-en.gguf',
                'arabic_model_filename': 'old-ar.gguf'}})['config']['speech']
            assert normalized_obsolete['single_model_filename'] is None
            assert normalized_obsolete['language_models'] == {}
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
            try:
                api('/api/model-setup', {}, auth=False)
                raise AssertionError('setup dismissal accepted unauthenticated request')
            except HTTPError as error:
                assert error.code == 400
            assert api('/api/state')['model_setup']['show']
            assert api('/api/model-setup', {'english': False, 'arabic': False})['ok']
            assert not api('/api/state')['model_setup']['show']
            assert api('/api/state')['config']['speech']['selection_mode'] == 'single_model'
            assert api('/api/state')['config']['speech']['single_model_filename'] is None
            print('PASS current-format defaults, explicit None, permissive/missing files, Save and draft-only import/reset')
            test_setup_choice(binary, temporary, 'setup-skip',
                              {'english': False, 'arabic': False})
            test_setup_choice(binary, temporary, 'setup-english',
                              {'english': True, 'arabic': False}, reopen_before_install=True)
            test_setup_choice(binary, temporary, 'setup-arabic',
                              {'english': False, 'arabic': True}, install_before=True)
            test_setup_choice(binary, temporary, 'setup-both',
                              {'english': True, 'arabic': True}, reopen_before_install=True)
            test_setup_choice(binary, temporary, 'setup-manual-edit',
                              {'english': True, 'arabic': True}, manual_edit=True)
            print('PASS onboarding selection outcomes, delayed installs, process reopen, and manual selection preservation')
        finally:
            process.terminate()
            process.wait(timeout=5)


if __name__ == '__main__':
    main()
