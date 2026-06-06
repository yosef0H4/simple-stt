"""TCP worker server. Rust owns the listener; Python connects back using a random token."""

from __future__ import annotations

import socket
from dataclasses import dataclass
from typing import Callable

from .engine import LiveEngine
from .errors import ProtocolError
from .nemotron import NemotronStreamingRecognizer, StreamingConfig
from .parakeet import ParakeetFileRecognizer
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


class ParakeetRecordEngine:
    """Accumulates one complete recording and emits a commit after key release."""

    def __init__(self, recognizer: ParakeetFileRecognizer, emit):
        self.recognizer = recognizer
        self.emit = emit
        self.active_session_id: int | None = None
        self.recorded = bytearray()

    def start(self, session_id: int) -> None:
        self.active_session_id = session_id
        self.recorded.clear()
        self.emit({"type": "session_started", "session_id": session_id})

    def push_pcm16(self, session_id: int, pcm: bytes) -> None:
        if self.active_session_id != session_id:
            return
        self.recorded.extend(pcm)

    def finish(self, session_id: int) -> None:
        if self.active_session_id != session_id:
            return
        pcm = bytes(self.recorded)
        self.active_session_id = None
        self.recorded.clear()
        text = self.recognizer.transcribe_pcm16(pcm)
        self.emit({"type": "commit", "session_id": session_id, "text": text})

    def cancel(self, session_id: int) -> None:
        if self.active_session_id == session_id:
            self.active_session_id = None
            self.recorded.clear()
        self.emit({"type": "session_cancelled", "session_id": session_id})


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
    elif config.backend == "parakeet-record":
        recognizer = ParakeetFileRecognizer()
    else:
        recognizer = NemotronStreamingRecognizer(StreamingConfig(lookahead_ms=config.lookahead_ms))

    emit = lambda message: send_json(sock, message)
    if config.backend == "parakeet-record":
        engine = ParakeetRecordEngine(recognizer, emit)  # type: ignore[arg-type]
    else:
        engine = LiveEngine(recognizer, emit)  # type: ignore[arg-type]
    send_json(sock, {"type": "status", "state": "ready"})

    while True:
        frame = read_frame(reader)
        if frame.kind is FrameKind.PCM16:
            session_id, pcm = decode_pcm16(frame)
            engine.push_pcm16(session_id, pcm)
            continue
        message = decode_json(frame)
        kind = message.get("type")
        if kind in {"start", "transcribe_recording"}:
            engine.start(int(message["session_id"]))
        elif kind == "cancel":
            engine.cancel(int(message["session_id"]))
        elif kind == "finish_recording":
            if not isinstance(engine, ParakeetRecordEngine):
                raise ProtocolError("finish_recording is only supported by parakeet-record")
            engine.finish(int(message["session_id"]))
        elif kind == "shutdown":
            send_json(sock, {"type": "status", "state": "shutting_down"})
            return
        elif kind == "ping":
            send_json(sock, {"type": "pong"})
        else:
            raise ProtocolError(f"unsupported command: {kind!r}")
