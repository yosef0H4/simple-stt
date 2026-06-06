import socket
import threading

from uvox_worker.protocol import decode_json, encode_json, encode_pcm16, read_frame
from uvox_worker.server import ServerConfig, serve


def test_echo_server_handshake_and_shutdown():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    host, port = listener.getsockname()
    thread = threading.Thread(
        target=serve,
        args=(ServerConfig(connect=f"{host}:{port}", token="secret", backend="echo"),),
        daemon=True,
    )
    thread.start()
    conn, _ = listener.accept()
    reader = conn.makefile("rb")
    assert decode_json(read_frame(reader))["token"] == "secret"
    assert decode_json(read_frame(reader)) == {"type": "status", "state": "loading_model"}
    assert decode_json(read_frame(reader)) == {"type": "status", "state": "ready"}
    conn.sendall(encode_json({"type": "start", "session_id": 3}))
    assert decode_json(read_frame(reader)) == {"type": "session_started", "session_id": 3}
    conn.sendall(encode_pcm16(3, b"\0" * 640))
    partial = decode_json(read_frame(reader))
    assert partial["type"] == "partial"
    assert decode_json(read_frame(reader)) == {
        "type": "commit",
        "session_id": 3,
        "text": "hello world ",
    }
    conn.sendall(encode_json({"type": "shutdown"}))
    assert decode_json(read_frame(reader)) == {"type": "status", "state": "shutting_down"}
    thread.join(timeout=2)
    assert not thread.is_alive()


class FakeParakeetRecognizer:
    def __init__(self):
        self.seen = b""

    def transcribe_pcm16(self, pcm):
        self.seen = pcm
        return "recorded text"


def test_parakeet_record_server_commits_finished_recording():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    host, port = listener.getsockname()
    fake = FakeParakeetRecognizer()
    thread = threading.Thread(
        target=serve,
        args=(
            ServerConfig(connect=f"{host}:{port}", token="secret", backend="parakeet-record"),
            lambda: fake,
        ),
        daemon=True,
    )
    thread.start()
    conn, _ = listener.accept()
    reader = conn.makefile("rb")
    assert decode_json(read_frame(reader))["token"] == "secret"
    assert decode_json(read_frame(reader)) == {"type": "status", "state": "loading_model"}
    assert decode_json(read_frame(reader)) == {"type": "status", "state": "ready"}

    conn.sendall(encode_json({"type": "transcribe_recording", "session_id": 9}))
    assert decode_json(read_frame(reader)) == {"type": "session_started", "session_id": 9}
    conn.sendall(encode_pcm16(9, b"\x01\x00\x02\x00"))
    conn.sendall(encode_json({"type": "finish_recording", "session_id": 9}))
    assert decode_json(read_frame(reader)) == {
        "type": "commit",
        "session_id": 9,
        "text": "recorded text",
    }
    assert fake.seen == b"\x01\x00\x02\x00"
    conn.sendall(encode_json({"type": "shutdown"}))
    assert decode_json(read_frame(reader)) == {"type": "status", "state": "shutting_down"}
    thread.join(timeout=2)
    assert not thread.is_alive()
