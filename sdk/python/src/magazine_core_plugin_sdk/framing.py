"""Frame codec and canonical JSON for the magazine-core plugin protocol."""

from __future__ import annotations

import json
import struct
from collections.abc import Mapping
from dataclasses import asdict, is_dataclass
from typing import Any, BinaryIO

MAX_FRAME = 8 * 1024 * 1024
MAX_FRAME_BYTES = MAX_FRAME


class FrameError(Exception):
    """Base class for frame read/write errors."""


class FrameEof(FrameError):
    """Clean end of stream at a frame boundary."""


FrameEOF = FrameEof


class FrameTruncated(FrameError):
    """Stream ended in the middle of a frame prefix or payload."""


class FrameTooLarge(FrameError):
    """Declared or actual payload length exceeds ``MAX_FRAME``."""

    def __init__(self, size: int, limit: int = MAX_FRAME) -> None:
        self.size = size
        self.limit = limit
        super().__init__(f"frame too large: {size} > {limit}")


class FrameUtf8Error(FrameError):
    """Payload bytes were not valid UTF-8."""


def _to_json_value(value: Any) -> Any:
    if hasattr(value, "to_dict") and callable(value.to_dict):
        return _to_json_value(value.to_dict())
    if is_dataclass(value) and not isinstance(value, type):
        return _to_json_value(asdict(value))
    if isinstance(value, Mapping):
        out: dict[str, Any] = {}
        for key, item in value.items():
            if not isinstance(key, str):
                raise TypeError(f"JSON object keys must be str, got {type(key).__name__}")
            out[key] = _to_json_value(item)
        return out
    if isinstance(value, (list, tuple)):
        return [_to_json_value(item) for item in value]
    return value


def canonical_json(value: Any) -> str:
    """Encode JSON with recursively sorted object keys and no whitespace."""

    return json.dumps(
        _to_json_value(value),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )


def canonical_json_bytes(value: Any) -> bytes:
    """Return canonical JSON encoded as UTF-8 bytes."""

    return canonical_json(value).encode("utf-8")


def _payload_bytes(payload: str | bytes | bytearray | memoryview) -> bytes:
    if isinstance(payload, str):
        return payload.encode("utf-8")
    if isinstance(payload, bytes):
        return payload
    if isinstance(payload, (bytearray, memoryview)):
        return bytes(payload)
    raise TypeError(f"payload must be str or bytes-like, got {type(payload).__name__}")


def frame_payload(payload: str | bytes | bytearray | memoryview) -> bytes:
    """Frame an already-encoded payload with a 4-byte big-endian length."""

    body = _payload_bytes(payload)
    if len(body) > MAX_FRAME:
        raise FrameTooLarge(len(body))
    return struct.pack(">I", len(body)) + body


def frame_bytes(value: Any) -> bytes:
    """Canonicalize a JSON value and return length-prefixed frame bytes."""

    return frame_payload(canonical_json_bytes(value))


def _read_exact(reader: BinaryIO, length: int, *, boundary: bool) -> bytes:
    chunks = bytearray()
    while len(chunks) < length:
        chunk = reader.read(length - len(chunks))
        if chunk is None:
            chunk = b""
        if chunk == b"":
            if boundary and not chunks:
                raise FrameEof()
            raise FrameTruncated()
        chunks.extend(chunk)
    return bytes(chunks)


def read_frame(reader: BinaryIO) -> str:
    """Read one UTF-8 frame, distinguishing boundary EOF from truncation."""

    prefix = _read_exact(reader, 4, boundary=True)
    length = struct.unpack(">I", prefix)[0]
    if length > MAX_FRAME:
        raise FrameTooLarge(length)
    payload = _read_exact(reader, length, boundary=False) if length else b""
    try:
        return payload.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise FrameUtf8Error("invalid utf-8 payload") from exc


def read_json_frame(reader: BinaryIO) -> Any:
    """Read one frame and decode its JSON payload."""

    return json.loads(read_frame(reader))


def write_frame(writer: BinaryIO, payload: str | bytes | bytearray | memoryview) -> None:
    """Write one raw UTF-8 payload frame and flush if supported."""

    writer.write(frame_payload(payload))
    flush = getattr(writer, "flush", None)
    if callable(flush):
        flush()


def write_json_frame(writer: BinaryIO, value: Any) -> None:
    """Canonicalize a JSON value, write one frame, and flush if supported."""

    writer.write(frame_bytes(value))
    flush = getattr(writer, "flush", None)
    if callable(flush):
        flush()
