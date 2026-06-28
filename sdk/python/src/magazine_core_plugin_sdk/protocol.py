"""JSON-RPC helpers for the magazine-core plugin protocol."""

from __future__ import annotations

from enum import Enum
from typing import Any, Iterator, Optional

JSONRPC_VERSION = "2.0"

PROTOCOL_VERSION = 1
RECORD_SCHEMA_VERSION = 1
MAX_RECORD_BATCH = 100

protocol_version = PROTOCOL_VERSION
record_schema_version = RECORD_SCHEMA_VERSION
max_record_batch = MAX_RECORD_BATCH


class Method(str, Enum):
    INITIALIZE = "initialize"
    DISCOVER = "discover"
    CANCEL = "cancel"
    FETCH_REQUEST = "fetch_request"
    STATE_QUERY = "state_query"
    RECORD = "record"
    LOG = "log"

    def as_str(self) -> str:
        return self.value


def _method_name(method: Method | str) -> str:
    if isinstance(method, Method):
        return method.value
    if isinstance(method, str):
        return method
    raise TypeError(f"method must be Method or str, got {type(method).__name__}")


def request(id: str | int, method: Method | str, params: Any) -> dict[str, Any]:
    """Build a JSON-RPC 2.0 request."""

    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": _method_name(method),
        "params": params,
    }


def notification(method: Method | str, params: Any) -> dict[str, Any]:
    """Build a JSON-RPC 2.0 notification."""

    return {
        "jsonrpc": JSONRPC_VERSION,
        "method": _method_name(method),
        "params": params,
    }


def response(id: str | int | None, result: Any) -> dict[str, Any]:
    """Build a successful JSON-RPC 2.0 response."""

    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "result": result,
    }


response_ok = response


def error(code: int, message: str, data: Any = None) -> dict[str, Any]:
    """Build a JSON-RPC 2.0 error object."""

    body: dict[str, Any] = {
        "code": code,
        "message": message,
    }
    if data is not None:
        body["data"] = data
    return body


def response_error(
    id: str | int | None,
    code: int,
    message: str,
    data: Any = None,
) -> dict[str, Any]:
    """Build an error JSON-RPC 2.0 response."""

    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "error": error(code, message, data),
    }


response_err = response_error


class PluginIdGenerator(Iterator[str]):
    """Generate plugin-originated JSON-RPC ids in the ``p-*`` namespace."""

    def __init__(self, start: int = 1, prefix: str = "p") -> None:
        if start < 1:
            raise ValueError("start must be >= 1")
        if not prefix:
            raise ValueError("prefix must not be empty")
        self._next = start
        self._prefix = prefix

    @property
    def prefix(self) -> str:
        return self._prefix

    def next_id(self) -> str:
        value = f"{self._prefix}-{self._next}"
        self._next += 1
        return value

    def next(self) -> str:
        return self.next_id()

    def __next__(self) -> str:
        return self.next_id()

    def __iter__(self) -> "PluginIdGenerator":
        return self

    def request(self, method: Method | str, params: Any, id: Optional[str] = None) -> dict[str, Any]:
        return request(id or self.next_id(), method, params)


def plugin_id_generator(start: int = 1) -> PluginIdGenerator:
    return PluginIdGenerator(start=start, prefix="p")
