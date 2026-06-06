import io

import pytest

from uvox_worker.errors import ProtocolError
from uvox_worker.protocol import (
    FrameKind,
    decode_json,
    decode_pcm16,
    encode_frame,
    encode_json,
    encode_pcm16,
    read_frame,
)


def test_json_round_trip_preserves_unicode():
    frame = read_frame(io.BytesIO(encode_json({"type": "commit", "text": "héllo"})))
    assert decode_json(frame) == {"type": "commit", "text": "héllo"}


def test_pcm_round_trip():
    frame = read_frame(io.BytesIO(encode_pcm16(42, b"\x01\x00\xff\xff")))
    assert frame.kind is FrameKind.PCM16
    assert decode_pcm16(frame) == (42, b"\x01\x00\xff\xff")


def test_pcm_rejects_odd_byte_count():
    with pytest.raises(ProtocolError, match="even"):
        encode_pcm16(1, b"\x00")


def test_unknown_frame_kind_is_rejected():
    raw = bytes([99]) + (0).to_bytes(4, "little")
    with pytest.raises(ProtocolError, match="unsupported"):
        read_frame(io.BytesIO(raw))


def test_oversized_payload_is_rejected():
    with pytest.raises(ProtocolError, match="too large"):
        encode_frame(FrameKind.JSON, b"x" * (4 * 1024 * 1024 + 1))
