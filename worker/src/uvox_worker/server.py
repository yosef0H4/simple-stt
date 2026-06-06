"""TCP worker server. Rust owns the listener; Python connects back using a random token."""

from __future__ import annotations

import socket
from dataclasses import dataclass
from typing import Callable

from .engine import LiveEngine
from .errors import ProtocolError
from .nemotron import NemotronStreamingRecognizer, StreamingConfig
from .protocol import FrameKind, decode_json, decode_pcm16, read_frame, send_json


@dataclass(frozen=True)
class ServerConfig:
    connect: str
    token: str
    lookahead_ms: int = 80
    backend: str = "nemotron"


class EchoRecognizer:
    """Tiny deterministic backend used only by tests and protocol debugging."""

    def __init__(self) -> None:
        self.config = type("EchoConfig", (), {"chunk_samples": 320})()
        self.counter = 0

    def reset(self) -> None:
        self.counter = 0

    def transcribe_chunk(self, pcm16_bytes: bytes) -> str:
        self.counter += 1
        return "hello world " if self.counter >= 1 else ""


def _connect(address: str) -> socket.socket:
    host, raw_port = address.rsplit(":", 1)
    return socket.create_connection((host, int(raw_port)), timeout=30)


def serve(config: ServerConfig, recognizer_factory: Callable[[], object] | None = None) -> None:
    sock = _connect(config.connect)
    reader = sock.makefile("rb")
    send_json(sock, {"type": "hello", "token": config.token, "protocol": 1})
    send_json(sock, {"type": "status", "state": "loading_model"})

    if recognizer_factory is not None:
        recognizer = recognizer_factory()
    elif config.backend == "echo":
        recognizer = EchoRecognizer()
    else:
        recognizer = NemotronStreamingRecognizer(StreamingConfig(lookahead_ms=config.lookahead_ms))

    engine = LiveEngine(recognizer, lambda message: send_json(sock, message))  # type: ignore[arg-type]
    send_json(sock, {"type": "status", "state": "ready"})

    while True:
        frame = read_frame(reader)
        if frame.kind is FrameKind.PCM16:
            session_id, pcm = decode_pcm16(frame)
            engine.push_pcm16(session_id, pcm)
            continue
        message = decode_json(frame)
        kind = message.get("type")
        if kind == "start":
            engine.start(int(message["session_id"]))
        elif kind == "cancel":
            engine.cancel(int(message["session_id"]))
        elif kind == "shutdown":
            send_json(sock, {"type": "status", "state": "shutting_down"})
            return
        elif kind == "ping":
            send_json(sock, {"type": "pong"})
        else:
            raise ProtocolError(f"unsupported command: {kind!r}")
