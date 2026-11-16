#!/usr/bin/env python3
"""Check the installed reader never keeps Preparing after a terminal error."""
import argparse
import json
from pathlib import Path
import socket
import struct


def reader_status(path):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(3)
        connection.connect(str(path))
        payload = b'\x00' + json.dumps({'op': 'status'}).encode()
        connection.sendall(struct.pack('<I', len(payload)) + payload)

        def receive(size):
            result = bytearray()
            while len(result) < size:
                chunk = connection.recv(size - len(result))
                if not chunk:
                    raise RuntimeError('reader closed its status connection')
                result.extend(chunk)
            return result

        size, = struct.unpack('<I', receive(4))
        assert 2 <= size <= 65536, 'invalid reader status frame'
        frame = receive(size)
        assert frame[0] == 0
        reply = json.loads(frame[1:])
        assert reply['ok'], reply.get('message')
        return reply['state']


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--socket', type=Path, default=Path.home() / '.local/share/simple-stt/instances/installed/reader/simple-tts.sock')
    args = parser.parse_args()
    state = reader_status(args.socket)
    notice = state.get('notice_text', '')
    failed = any(word in notice.lower() for word in ('paused:', 'failed', 'no readable text'))
    print(json.dumps({key: state.get(key) for key in ('state', 'read_pending', 'model_status', 'notice_text')}, ensure_ascii=False))
    idle_without_work = state.get('state') == 'idle' and state.get('model_status') == 'Unloaded' and state.get('activity', {}).get('phase') == 'idle'
    assert not (state.get('read_pending') and (failed or idle_without_work)), 'FAIL: failed read-along leaves Preparing active'
    print('PASS: terminal read-along failure does not leave Preparing active')
