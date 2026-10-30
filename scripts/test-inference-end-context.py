#!/usr/bin/env python3
"""Real PCM/ASR regression: no microphone, playback, clipboard or desktop access."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import struct
import wave

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('inference_devices', ROOT / 'scripts/test-inference-devices.py')
wire = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wire)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--infer', type=Path, default=ROOT / 'target/debug/simple-stt-infer')
    parser.add_argument('--runtime-dir', type=Path, default=Path('/usr/lib/simple-stt/runtime'))
    parser.add_argument('--model-path', type=Path, default=Path.home() / '.local/share/simple-stt/models/realtime_eou_120m-v1-q8_0.gguf')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    fixtures = ROOT / 'fixtures/asr/end-context'
    results = []
    with tempfile.TemporaryDirectory(prefix='simple-stt-end-context-') as temporary, tempfile.TemporaryFile() as errors:
        process = subprocess.Popen([str(args.infer), '--runtime-dir', str(args.runtime_dir),
            '--model-path', str(args.model_path), '--inference-device', 'gpu',
            '--log-path', str(Path(temporary) / 'infer.log')], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=errors)
        try:
            assert wire.request(process, 1)[0] == 2
            for fixture in json.loads((fixtures / 'manifest.json').read_text()):
                with wave.open(str(fixtures / fixture['file'])) as audio:
                    assert audio.getparams()[:3] == (1, 2, 16000)
                    raw = audio.readframes(audio.getnframes())
                # The old worker truncated the final token for these exact bytes.
                kind, _, body = wire.request(process, 3, struct.pack('<II', 16000, len(raw)//2) + raw, 42)
                assert kind == 5, body.decode(errors='replace')
                text = body.decode().strip()
                assert text.lower().rstrip('.!?').endswith(fixture['expected_last_word']), (fixture, text)
                results.append(text)
                print('PASS complete final word:', text, flush=True)
            assert wire.request(process, 7)[0] == 8
            process.wait(timeout=10)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
    if args.output:
        args.output.write_text('\n'.join(results), encoding='utf-8')


if __name__ == '__main__':
    main()
