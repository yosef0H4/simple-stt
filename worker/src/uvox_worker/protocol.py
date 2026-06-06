"""Length-prefixed binary IPC used between the Rust manager and Python worker."""

from __future__ import annotations

import json
import socket
import struct
from dataclasses import dataclass
from enum import IntEnum
from typing import Any, BinaryIO

from .errors import ProtocolError

_HEADER = struct.Struct("<BI")
_SESSION = struct.Struct("<Q")
MAX_PAYLOAD_BYTES = 4 * 1024 * 1024


class FrameKind(IntEnum):
    JSON = 1
    PCM16 = 2


@dataclass(frozen=True)
class Frame:
    kind: FrameKind
    payload: bytes


def encode_frame(kind: FrameKind, payload: bytes) -> bytes:
    if len(payload) > MAX_PAYLOAD_BYTES:
        raise ProtocolError(f"payload too large: {len(payload)} bytes")
    return _HEADER.pack(int(kind), len(payload)) + payload


def encode_json(message: dict[str, Any]) -> bytes:
    payload = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return encode_frame(FrameKind.JSON, payload)


def encode_pcm16(session_id: int, pcm_bytes: bytes) -> bytes:
    if len(pcm_bytes) % 2:
        raise ProtocolError("PCM16 payload must contain an even number of bytes")
    return encode_frame(FrameKind.PCM16, _SESSION.pack(session_id) + pcm_bytes)


def decode_json(frame: Frame) -> dict[str, Any]:
    if frame.kind is not FrameKind.JSON:
        raise ProtocolError(f"expected JSON frame, got {frame.kind.name}")
    try:
        value = json.loads(frame.payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolError("invalid JSON frame") from exc
    if not isinstance(value, dict):
        raise ProtocolError("JSON message must be an object")
    return value


def decode_pcm16(frame: Frame) -> tuple[int, bytes]:
    if frame.kind is not FrameKind.PCM16:
        raise ProtocolError(f"expected PCM16 frame, got {frame.kind.name}")
    if len(frame.payload) < _SESSION.size:
        raise ProtocolError("PCM16 frame is missing session_id")
    session_id = _SESSION.unpack_from(frame.payload)[0]
    pcm = frame.payload[_SESSION.size :]
    if len(pcm) % 2:
        raise ProtocolError("PCM16 frame contains an odd number of bytes")
    return session_id, pcm


def _read_exact(reader: BinaryIO, size: int) -> bytes:
    chunks: list[bytes] = []
    remaining = size
    while remaining:
        chunk = reader.read(remaining)
        if not chunk:
            raise EOFError("IPC connection closed")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_frame(reader: BinaryIO) -> Frame:
    raw_header = _read_exact(reader, _HEADER.size)
    raw_kind, length = _HEADER.unpack(raw_header)
    if length > MAX_PAYLOAD_BYTES:
        raise ProtocolError(f"payload too large: {length} bytes")
    try:
        kind = FrameKind(raw_kind)
    except ValueError as exc:
        raise ProtocolError(f"unsupported frame kind: {raw_kind}") from exc
    return Frame(kind=kind, payload=_read_exact(reader, length))


def send_json(sock: socket.socket, message: dict[str, Any]) -> None:
    sock.sendall(encode_json(message))
