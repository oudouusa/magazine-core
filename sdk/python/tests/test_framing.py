from __future__ import annotations

import io
import struct

import pytest

from magazine_core_plugin_sdk.framing import (
    MAX_FRAME,
    FrameEof,
    FrameTooLarge,
    FrameTruncated,
    FrameUtf8Error,
    canonical_json,
    canonical_json_bytes,
    frame_bytes,
    read_frame,
    write_frame,
)


def test_canonical_json_sorts_nested_keys_and_uses_utf8() -> None:
    value = {"b": 1, "a": {"d": 2, "c": "雪"}, "items": [{"z": 0, "y": 1}]}

    encoded = canonical_json(value)

    assert encoded == '{"a":{"c":"雪","d":2},"b":1,"items":[{"y":1,"z":0}]}'
    assert canonical_json_bytes(value) == encoded.encode("utf-8")


def test_frame_bytes_uses_big_endian_payload_length() -> None:
    frame = frame_bytes({"jsonrpc": "2.0", "id": "h-1", "method": "initialize", "params": {}})
    payload = b'{"id":"h-1","jsonrpc":"2.0","method":"initialize","params":{}}'

    assert frame == struct.pack(">I", len(payload)) + payload


def test_read_write_frame_round_trip_and_boundary_eof() -> None:
    stream = io.BytesIO()
    write_frame(stream, '{"a":1}')
    stream.seek(0)

    assert read_frame(stream) == '{"a":1}'
    with pytest.raises(FrameEof):
        read_frame(stream)


def test_read_frame_rejects_truncated_prefix_and_body() -> None:
    with pytest.raises(FrameTruncated):
        read_frame(io.BytesIO(b"\x00\x00"))

    with pytest.raises(FrameTruncated):
        read_frame(io.BytesIO(struct.pack(">I", 5) + b"ab"))


def test_read_frame_rejects_oversized_and_invalid_utf8() -> None:
    with pytest.raises(FrameTooLarge):
        read_frame(io.BytesIO(struct.pack(">I", MAX_FRAME + 1)))

    with pytest.raises(FrameUtf8Error):
        read_frame(io.BytesIO(struct.pack(">I", 1) + b"\xff"))
