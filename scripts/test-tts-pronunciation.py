#!/usr/bin/env python3
"""Render public pronunciation probes on GPU; transcribe files without playing audio.

ASR is supporting evidence, not a substitute for human pronunciation review.
All profiles and services are private. WAVs and observations are retained in artifacts.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import wave

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('workers', ROOT / 'scripts/test-reader-workers.py')
workers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workers)
BIN = ROOT / 'target/debug'
RUNTIME = Path.home() / '.local/share/simple-tts/runtime'
OUT = ROOT / 'artifacts/combined-app/pronunciation'
CASES = [
    ('kokoro', 'en', 'slash-original', 'Shared in one process: code, heap/data, open files.'),
    ('kokoro', 'en', 'slash-prepared', 'Shared in one process: code, heap, data, open files.'),
    ('kokoro', 'en', 'dollar-original', 'The price is 5$.'),
    ('kokoro', 'en', 'dollar-prepared', 'The price is 5 dollars.'),
    ('supertonic3', 'ar', 'dollar-original', 'السعر ٥$.'),
    ('supertonic3', 'ar', 'dollar-prepared', 'السعر ٥ دولارات.'),
    ('supertonic3', 'ar', 'dollar-ascii-prepared', 'السعر 5 دولارات.'),
    ('supertonic3', 'ar', 'dollar-words-prepared', 'السعر خمسة دولارات.'),
]

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    observations = []
    with tempfile.TemporaryDirectory(prefix='simple-stt-pronunciation-') as tmp:
        private = Path(tmp)
        env = dict(os.environ, XDG_DATA_HOME=tmp, XDG_CONFIG_HOME=tmp, XDG_CACHE_HOME=tmp,
                   XDG_RUNTIME_DIR=tmp, SIMPLE_STT_RUNTIME_ROOT=str(private / 'runtime'),
                   DISPLAY='', WAYLAND_DISPLAY='', DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-pronunciation-bus',
                   LD_LIBRARY_PATH=str(RUNTIME / 'lib') + os.pathsep + os.environ.get('LD_LIBRARY_PATH', ''))
        models = {
            'kokoro': Path.home() / '.local/share/simple-tts/models/kokoro-multi-lang-v1_0',
            'supertonic3': ROOT / 'artifacts/combined-app/downloads/sherpa-onnx-supertonic-3-tts-int8-2026-05-11',
        }
        for model, path in models.items():
            with (OUT / f'{model}.log').open('w') as log:
                child, events = workers.launch(BIN / 'reader-tts', ['--provider', 'local_supertonic' if model == 'supertonic3' else 'local_kokoro',
                    '--model-dir', str(path), '--runtime-dir', str(RUNTIME), '--device', 'cuda'], env, log)
                try:
                    for engine, language, name, text in CASES:
                        if engine != model:
                            continue
                        start = time.monotonic()
                        workers.send(child, {'type':'synthesize', 'job_id':name, 'generation':1, 'sentence':0, 'unit':0,
                            'text':text, 'voice':'sid:0' if model == 'supertonic3' else 'af_heart', 'steps':5,
                            'language':language, 'speed':1.0, 'device':'cuda'})
                        pcm = bytearray()
                        device = ''
                        rate = None
                        while True:
                            kind, event, chunk = events.get(timeout=50)
                            assert kind != 'error', event
                            if kind == 'pcm':
                                assert event['sample_offset'] == len(pcm)//2
                                rate = rate or event['sample_rate']
                                assert event['sample_rate'] == rate and 8000 <= rate <= 192000
                                pcm.extend(chunk)
                            else:
                                assert event['type'] != 'error', event
                                if event['type'] == 'ready': device = event['device']
                                if event['type'] == 'finished': break
                        assert pcm and any(pcm) and 'CUDA verified' in device, device
                        output = OUT / f'{model}-{name}.wav'
                        with wave.open(str(output), 'wb') as audio:
                            audio.setnchannels(1); audio.setsampwidth(2); audio.setframerate(rate); audio.writeframes(pcm)
                        observations.append({'model':model,'language':language,'case':name,'input':text,'wav':str(output),
                                             'generation_ms':round((time.monotonic()-start)*1000,1)})
                finally:
                    workers.finish(child)
        for observation in observations:
            language = observation['language']
            model = 'lemura-arabic-asr-lite-q8_0.gguf' if language == 'ar' else 'realtime_eou_120m-v1-q8_0.gguf'
            config = private / 'stt-config.json'
            config.write_text(json.dumps({'speech':{'selection_mode':'single_model','single_model_filename':model,
                'model_dir':str(Path.home()/'.local/share/simple-stt/models'),
                'runtime_dir':str(ROOT/'external/parakeet-runtime/parakeet-linux-vulkan'), 'inference_device':'gpu'}}))
            result = subprocess.run([str(BIN/'simple-stt-linux'),'media','transcribe',observation['wav']],
                env={**env,'SIMPLE_STT_CONFIG':str(config)},capture_output=True,text=True,timeout=90)
            assert result.returncode == 0, result.stderr
            observation['asr_observation'] = result.stdout.strip()
        (OUT/'report.json').write_text(json.dumps(observations,ensure_ascii=False,indent=2))
        print(json.dumps(observations,ensure_ascii=False,indent=2))
    print('PASS non-silent CUDA probes and file-only GPU ASR; pronunciation needs human judgment')

if __name__ == '__main__':
    main()
