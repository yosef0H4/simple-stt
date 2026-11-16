#!/usr/bin/env python3
"""Serial, warm, direct-worker CPU/GPU benchmarks; no desktop audio or input.

Requires built simple-stt-infer and reader-tts, the selected model/runtime assets,
ffmpeg, and (for GPU) a working Vulkan/CUDA driver. This bypasses the production
residency policy; these timings measure warm inference, not retention or accuracy.
Results are written beneath artifacts/combined-app/benchmarks.
"""
import argparse
import csv
import json
import math
import os
from pathlib import Path
import queue
import shutil
import statistics
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'artifacts/combined-app/benchmarks'
HOME = Path.home()


def read_exact(stream, n):
    chunks = bytearray()
    while len(chunks) < n:
        part = stream.read(n - len(chunks))
        if not part:
            raise EOFError('worker output closed')
        chunks.extend(part)
    return bytes(chunks)


def memory(pid, gpu):
    result = {'pid': pid, 'ram_bytes': None, 'ram_kind': 'unknown', 'vram_bytes': 0 if not gpu else None, 'threads': None}
    try:
        result['threads'] = next(int(line.split()[1]) for line in Path(f'/proc/{pid}/status').read_text().splitlines() if line.startswith('Threads:'))
    except (OSError, ValueError, StopIteration, IndexError):
        pass
    for filename, field, kind in [('smaps_rollup', 'Pss:', 'pss'), ('status', 'VmRSS:', 'rss')]:
        try:
            lines = Path(f'/proc/{pid}/{filename}').read_text().splitlines()
            value = next(int(line.split()[1]) * 1024 for line in lines if line.startswith(field))
            result.update(ram_bytes=value, ram_kind=kind)
            break
        except (OSError, ValueError, StopIteration, IndexError):
            pass
    if gpu:
        try:
            output = subprocess.run(['nvidia-smi', '--query-compute-apps=pid,used_gpu_memory',
                                     '--format=csv,noheader,nounits'], capture_output=True,
                                    text=True, check=True, timeout=1).stdout
            for line in output.splitlines():
                number, value = line.split(',', 1)
                if int(number.strip()) == pid:
                    result['vram_bytes'] = int(value.strip()) * 1024 * 1024
                    break
        except (OSError, subprocess.SubprocessError, ValueError):
            pass
    return result


class Worker:
    def __init__(self, command, env, log_path, protocol):
        self.protocol = protocol
        self.log = open(log_path, 'wb')
        nice = shutil.which('nice')
        if nice:
            command = [nice, '-n', '10', *command]
        try:
            self.child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                          stderr=self.log, env=env)
        except Exception:
            self.log.close()
            raise
        self.messages = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        try:
            while True:
                if self.protocol == 'stt':
                    magic, version, kind, length, session = struct.unpack('<4sHHIQ', read_exact(self.child.stdout, 20))
                    if magic != b'UVX1' or version != 1 or length > 256 * 1024 * 1024:
                        raise ValueError('invalid STT frame')
                    self.messages.put((kind, session, read_exact(self.child.stdout, length)))
                else:
                    size = struct.unpack('<I', read_exact(self.child.stdout, 4))[0]
                    if not 1 < size <= 65536:
                        raise ValueError('invalid reader frame size')
                    frame = read_exact(self.child.stdout, size)
                    if frame[0] == 0:
                        self.messages.put(('control', json.loads(frame[1:]), b''))
                    elif frame[0] == 1:
                        length = struct.unpack('<I', frame[1:5])[0]
                        header = json.loads(frame[5:5 + length])
                        pcm = frame[5 + length:]
                        if len(pcm) != header['sample_count'] * 2:
                            raise ValueError('PCM sample count mismatch')
                        self.messages.put(('pcm', header, pcm))
                    else:
                        raise ValueError('unknown reader frame')
        except Exception as error:
            self.messages.put(('failure', str(error), b''))

    def write(self, data, timeout=15):
        outcome = queue.Queue(maxsize=1)
        def send():
            try:
                self.child.stdin.write(data)
                self.child.stdin.flush()
                outcome.put(None)
            except Exception as error:
                outcome.put(error)
        threading.Thread(target=send, daemon=True).start()
        error = outcome.get(timeout=timeout)
        if error:
            raise error

    def stt_send(self, kind, session=0, body=b''):
        self.write(struct.pack('<4sHHIQ', b'UVX1', 1, kind, len(body), session) + body)

    def reader_send(self, value):
        body = b'\0' + json.dumps(value, ensure_ascii=False).encode('utf-8')
        self.write(struct.pack('<I', len(body)) + body)

    def receive(self, timeout):
        message = self.messages.get(timeout=timeout)
        if message[0] == 'failure':
            raise RuntimeError(message[1])
        return message

    def close(self):
        try:
            if self.child.poll() is None:
                if self.protocol == 'stt':
                    self.stt_send(7)
                else:
                    self.reader_send({'type': 'shutdown'})
                self.child.wait(timeout=5)
        except (OSError, queue.Empty, subprocess.SubprocessError):
            pass
        finally:
            if self.child.poll() is None:
                self.child.kill()
                self.child.wait()
            self.log.close()


def require(paths):
    missing = [str(path) for path in paths if not path.exists()]
    if missing:
        raise FileNotFoundError('Missing assets: ' + ', '.join(missing))


def env_for(runtime, scratch):
    env = os.environ.copy()
    env.update(XDG_DATA_HOME=str(scratch), XDG_CONFIG_HOME=str(scratch),
               XDG_CACHE_HOME=str(scratch), XDG_RUNTIME_DIR=str(scratch),
               CUDA_CACHE_PATH=str(scratch / 'cuda-cache'), DISPLAY='', WAYLAND_DISPLAY='',
               DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-speech-benchmark-bus')
    paths = [runtime / 'lib', runtime / 'bin', runtime]
    env['LD_LIBRARY_PATH'] = os.pathsep.join(str(p) for p in paths if p.exists()) + os.pathsep + env.get('LD_LIBRARY_PATH', '')
    return env


def pcm_source(source):
    return subprocess.run(['ffmpeg', '-v', 'error', '-i', str(source), '-f', 's16le',
                           '-ac', '1', '-ar', '16000', 'pipe:1'], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60).stdout


def repeated_pcm(source, seconds):
    cycle = source + b'\0' * 3200  # 100 ms silence between repetitions
    needed = seconds * 16000 * 2
    if not source:
        raise ValueError('empty audio fixture')
    return (cycle * ((needed + len(cycle) - 1) // len(cycle)))[:needed]


def speech_texts(language):
    if language == 'en':
        sentence = 'The reader measures natural speech in a short local benchmark. '
        return ['Hello world.', sentence.strip(), (sentence * 4).strip()[:238]]
    sentence = 'هذا اختبار للقراءة المحلية يقيس سرعة الصوت ووضوح الكلمات. '
    long = ''
    while len((long + sentence).encode('utf-8')) <= 238:
        long += sentence
    return ['مرحباً بالعالم.', sentence.strip(), long.strip()]


def stt_case(worker, pcm, case_id, timeout):
    body = struct.pack('<II', 16000, len(pcm) // 2) + pcm
    start = time.monotonic()
    worker.stt_send(3, case_id, body)
    kind, session, response = worker.receive(timeout)
    elapsed = time.monotonic() - start
    if session != case_id or kind != 5:
        raise RuntimeError(f'STT returned kind {kind}: {response[:300]!r}')
    response.decode('utf-8')  # Transport check; do not assess transcript quality.
    return elapsed


def tts_case(worker, text, voice, lang, device, case_id, timeout):
    start = time.monotonic()
    worker.reader_send({'type': 'synthesize', 'job_id': str(case_id), 'generation': 1,
                        'sentence': 0, 'unit': 0, 'text': text, 'voice': voice,
                        'steps': 5, 'language': lang, 'speed': 1.0, 'device': device})
    deadline = time.monotonic() + timeout
    samples = 0
    rate = None
    while True:
        kind, header, pcm = worker.receive(max(0.001, deadline - time.monotonic()))
        if kind == 'pcm':
            if header['sample_offset'] != samples:
                raise RuntimeError('non-contiguous PCM')
            samples += len(pcm) // 2
            rate = header['sample_rate']
        elif header['type'] == 'error':
            raise RuntimeError(str(header))
        elif header['type'] == 'finished':
            break
    elapsed = time.monotonic() - start
    if not samples or not rate:
        raise RuntimeError('no generated PCM')
    return elapsed, samples / rate


def run_stt(args, rows, scratch):
    runtime = args.parakeet_runtime
    models = [('en', args.stt_en, args.audio_en), ('ar', args.stt_ar, args.audio_ar)]
    require([args.bin_dir / 'simple-stt-infer', runtime, *(p for _, model, source in models for p in (model, source))])
    for language, model, audio in models:
        source = pcm_source(audio)
        for device in ([args.device] if args.device != 'all' else ['cpu', 'gpu']):
            env = env_for(runtime, scratch)
            if device == 'cpu':
                env['PARAKEET_DEVICE'] = 'cpu'
            else:
                env['PARAKEET_DEVICE'] = 'Vulkan0'
            log = OUTPUT / f'stt-{runtime.name}-{language}-{device}.log'
            worker = Worker([str(args.bin_dir / 'simple-stt-infer'), '--runtime-dir', str(runtime),
                             '--model-path', str(model), '--log-path', str(OUTPUT / f'stt-{runtime.name}-{language}-{device}-component.log'),
                             '--log-level', 'normal', '--inference-device', device, '--language', language,
                             '--idle-timeout-secs', '3600'], env, log, 'stt')
            try:
                worker.stt_send(1)
                if worker.receive(args.timeout)[0] != 2:
                    raise RuntimeError('STT hello failed')
                worker.stt_send(11)
                if worker.receive(args.timeout)[0] != 12 or worker.receive(args.timeout)[0] != 13:
                    raise RuntimeError('STT warmup failed')
                sequence = 1
                fifteen_second_times = []
                for seconds in [1, 5, 15, 30]:
                    print(f'STT {language} {device} {seconds}s warmup', flush=True)
                    pcm = repeated_pcm(source, seconds)
                    for _ in range(3):
                        stt_case(worker, pcm, sequence, args.timeout)
                        sequence += 1
                    count = 3 if seconds == 30 and device == 'cpu' and fifteen_second_times and statistics.median(fifteen_second_times) > 15 else 5
                    for iteration in range(count):
                        elapsed = stt_case(worker, pcm, sequence, args.timeout)
                        sequence += 1
                        if seconds == 15:
                            fifteen_second_times.append(elapsed)
                        rows.append({'feature': 'stt', 'model': model.name, 'language': language, 'runtime': runtime.name,
                                     'device': device, 'case': f'{seconds}s', 'iteration': iteration + 1,
                                     'elapsed_seconds': elapsed, 'audio_seconds': seconds,
                                     'rtf': elapsed / seconds, **memory(worker.child.pid, device == 'gpu')})
                    print(f'STT {language} {device} {seconds}s median={statistics.median(r["elapsed_seconds"] for r in rows[-count:]):.3f}s', flush=True)
            finally:
                worker.close()


def run_tts(args, rows, scratch):
    require([args.bin_dir / 'reader-tts', args.kokoro, args.supertonic, args.tts_cpu, args.tts_cuda])
    for model, model_dir, language, voice in [('kokoro', args.kokoro, 'en', 'af_heart'),
                                              ('supertonic3', args.supertonic, 'ar', 'sid:0')]:
        for device, runtime in [('cpu', args.tts_cpu), ('cuda', args.tts_cuda)]:
            if args.device != 'all' and args.device != ('gpu' if device == 'cuda' else 'cpu'):
                continue
            worker = Worker([str(args.bin_dir / 'reader-tts'), '--model-dir', str(model_dir),
                             '--runtime-dir', str(runtime), '--device', device, '--threads', '2',
                             '--provider', 'local_kokoro' if model == 'kokoro' else 'local_supertonic'],
                            env_for(runtime, scratch), OUTPUT / f'tts-{model}-{device}.log', 'tts')
            try:
                worker.reader_send({'type': 'hello', 'protocol': 1, 'incarnation': 'benchmark'})
                kind, ready, _ = worker.receive(args.timeout)
                if kind != 'control' or ready['type'] != 'ready':
                    raise RuntimeError(f'TTS hello failed: {ready}')
                sequence = 0
                for name, text in zip(['short', 'medium', 'near_240_bytes'], speech_texts(language)):
                    print(f'TTS {model} {device} {name} warmup', flush=True)
                    for iteration in range(8):
                        sequence += 1
                        elapsed, audio_seconds = tts_case(worker, text, voice, language, device, sequence, args.timeout)
                        if iteration >= 3:
                            rows.append({'feature': 'tts', 'model': model, 'language': language, 'runtime': runtime.name,
                                         'device': device, 'case': name, 'text_bytes': len(text.encode('utf-8')),
                                         'iteration': iteration - 2, 'elapsed_seconds': elapsed,
                                         'audio_seconds': audio_seconds, 'rtf': elapsed / audio_seconds,
                                         **memory(worker.child.pid, device == 'cuda')})
                    print(f'TTS {model} {device} {name} median={statistics.median(r["elapsed_seconds"] for r in rows[-5:]):.3f}s', flush=True)
            finally:
                worker.close()


def save(rows, feature):
    OUTPUT.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime('%Y%m%d-%H%M%S')
    base = OUTPUT / f'{feature}-{stamp}'
    groups = {}
    for row in rows:
        key = (row['feature'], row['model'], row['language'], row['device'], row['case'])
        groups.setdefault(key, []).append(row['elapsed_seconds'])
    summaries = []
    for key, values in groups.items():
        ordered = sorted(values)
        summaries.append({'feature': key[0], 'model': key[1], 'language': key[2], 'device': key[3],
                          'case': key[4], 'n': len(values), 'median_seconds': statistics.median(values),
                          'p90_seconds': ordered[math.ceil(.9 * len(ordered)) - 1]})
    base.with_suffix('.json').write_text(json.dumps({'method': '3 warmups per case; repeated audio plus silence; direct workers; no accuracy assessment', 'runs': rows, 'summary': summaries}, ensure_ascii=False, indent=2))
    keys = sorted({key for row in rows for key in row})
    with base.with_suffix('.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=keys)
        writer.writeheader()
        writer.writerows(rows)
    print(f'Wrote {base}.json and {base}.csv')
    for item in summaries:
        print(item)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--feature', choices=['stt', 'tts'], required=True)
    parser.add_argument('--device', choices=['all', 'cpu', 'gpu'], default='all')
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/release')
    parser.add_argument('--timeout', type=float, default=300, help='per request timeout in seconds')
    parser.add_argument('--parakeet-runtime', type=Path, default=ROOT / 'external/parakeet-runtime/parakeet-linux-vulkan')
    parser.add_argument('--stt-en', type=Path, default=HOME / '.local/share/simple-stt/models/realtime_eou_120m-v1-q8_0.gguf')
    parser.add_argument('--stt-ar', type=Path, default=HOME / '.local/share/simple-stt/models/lemura-arabic-asr-lite-q8_0.gguf')
    parser.add_argument('--audio-en', type=Path, default=ROOT / 'fixtures/parakeet-smoke.wav')
    parser.add_argument('--audio-ar', type=Path, default=ROOT / 'artifacts/combined-app/pronunciation/supertonic3-dollar-words-prepared.wav')
    parser.add_argument('--kokoro', type=Path, default=HOME / '.local/share/simple-tts/models/kokoro-multi-lang-v1_0')
    parser.add_argument('--supertonic', type=Path, default=ROOT / 'artifacts/combined-app/downloads/sherpa-onnx-supertonic-3-tts-int8-2026-05-11')
    parser.add_argument('--tts-cpu', type=Path, default=ROOT / 'artifacts/combined-app/memory/cpu-runtime')
    parser.add_argument('--tts-cuda', type=Path, default=HOME / '.local/share/simple-tts/runtime')
    args = parser.parse_args()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    rows = []
    try:
        with tempfile.TemporaryDirectory(prefix='speech-benchmark-') as directory:
            if args.feature == 'stt':
                run_stt(args, rows, Path(directory))
            else:
                run_tts(args, rows, Path(directory))
    finally:
        if rows:
            save(rows, args.feature)


if __name__ == '__main__':
    main()
