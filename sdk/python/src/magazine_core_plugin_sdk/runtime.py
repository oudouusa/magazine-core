"""Runtime helpers for writing magazine-core stdio plugins."""

from __future__ import annotations

import base64
import json
import os
import select
import struct
import sys
import threading
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from typing import Any, BinaryIO, Callable, Optional

from .framing import MAX_FRAME, read_json_frame, write_json_frame
from .models import SourceRecord
from .protocol import (
    MAX_RECORD_BATCH,
    PROTOCOL_VERSION,
    RECORD_SCHEMA_VERSION,
    Method,
    PluginIdGenerator,
    notification,
    plugin_id_generator,
    request,
    response,
    response_error,
)
from .stdout_guard import StdoutGuard


DiscoverCallback = Callable[["PluginContext", Mapping[str, Any]], Optional[int]]


@dataclass
class PluginManifest:
    source_name: str
    display_label: str
    allowed_domains: list[str] = field(default_factory=list)
    capabilities: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "source_name": self.source_name,
            "display_label": self.display_label,
            "allowed_domains": list(self.allowed_domains),
            "capabilities": list(self.capabilities),
        }


class HostRequestError(RuntimeError):
    """Raised when the host returns a JSON-RPC error for a plugin request."""

    def __init__(self, code: int, message: str, data: Any = None) -> None:
        self.code = code
        self.data = data
        super().__init__(message)


@dataclass(frozen=True)
class HostFetchResponse:
    """Decoded host_fetch response."""

    id: str
    status: int
    final_url: str
    body_base64: str

    @property
    def body(self) -> bytes:
        return base64.b64decode(self.body_base64)

    @property
    def text(self) -> str:
        return self.body.decode("utf-8")

    def json(self) -> Any:
        return json.loads(self.text)

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "HostFetchResponse":
        return cls(
            id=str(value["id"]),
            status=int(value["status"]),
            final_url=str(value["final_url"]),
            body_base64=str(value["body_base64"]),
        )


class PluginContext:
    def __init__(self, runtime: "PluginRuntime", request_id: str) -> None:
        self._runtime = runtime
        self.request_id = request_id
        self._records = 0
        self._cancelled = threading.Event()
        self._ids: PluginIdGenerator = plugin_id_generator()
        self._fetch_ids: PluginIdGenerator = PluginIdGenerator(prefix="fetch")
        self._state_ids: PluginIdGenerator = PluginIdGenerator(prefix="state")
        self._responses: dict[str, Mapping[str, Any]] = {}
        self._responses_cv = threading.Condition()

    @property
    def records(self) -> int:
        return self._records

    def is_cancelled(self) -> bool:
        return self._cancelled.is_set()

    def wait_cancelled(self, timeout: Optional[float] = None) -> bool:
        return self._cancelled.wait(timeout)

    def _mark_cancelled(self) -> None:
        self._cancelled.set()

    def send_record(self, record: SourceRecord | Mapping[str, Any]) -> None:
        payload = self._record_payload(record)
        self._runtime.write(
            notification(
                Method.RECORD,
                {"request_id": self.request_id, "record": payload},
            )
        )
        self._records += 1

    def send_records(self, records: Iterable[SourceRecord | Mapping[str, Any]]) -> None:
        batch: list[dict[str, Any]] = []
        for record in records:
            batch.append(self._record_payload(record))
            if len(batch) == MAX_RECORD_BATCH:
                self._send_record_batch(batch)
                batch = []
        if batch:
            self._send_record_batch(batch)

    @staticmethod
    def _record_payload(record: SourceRecord | Mapping[str, Any]) -> dict[str, Any]:
        return record.to_dict() if isinstance(record, SourceRecord) else dict(record)

    def _send_record_batch(self, records: list[dict[str, Any]]) -> None:
        self._runtime.write(
            notification(
                Method.RECORD,
                {"request_id": self.request_id, "records": records},
            )
        )
        self._records += len(records)

    def log(self, level: str, message: str) -> None:
        self._runtime.write(
            notification(Method.LOG, {"level": level, "message": message})
        )

    def host_fetch(
        self,
        url: str,
        *,
        method: str = "GET",
        headers: Optional[Mapping[str, str]] = None,
        timeout: Optional[float] = 30.0,
    ) -> HostFetchResponse:
        fetch_id = self._fetch_ids.next_id()
        result = self._request_host(
            Method.FETCH_REQUEST,
            {
                "id": fetch_id,
                "request": {
                    "url": url,
                    "method": method,
                    "headers": dict(headers or {}),
                },
            },
            timeout=timeout,
        )
        if not isinstance(result, Mapping):
            raise RuntimeError("fetch_request result must be an object")
        response = HostFetchResponse.from_dict(result)
        if response.id != fetch_id:
            raise RuntimeError("fetch_request result id mismatch")
        return response

    def _state_query(
        self,
        op: str,
        args: Optional[Mapping[str, Any]] = None,
        *,
        timeout: Optional[float] = 30.0,
    ) -> Any:
        query_id = self._state_ids.next_id()
        result = self._request_host(
            Method.STATE_QUERY,
            {
                "id": query_id,
                "op": op,
                "args": dict(args or {}),
            },
            timeout=timeout,
        )
        if not isinstance(result, Mapping):
            raise RuntimeError("state_query result must be an object")
        if str(result.get("id")) != query_id:
            raise RuntimeError("state_query result id mismatch")
        if "result" not in result:
            raise RuntimeError("state_query result missing result field")
        return result["result"]

    def known_source_urls(
        self,
        source_name: str,
        *,
        timeout: Optional[float] = 30.0,
    ) -> list[str]:
        result = self._state_query(
            "known_source_urls",
            {"source_name": source_name},
            timeout=timeout,
        )
        if not isinstance(result, list):
            raise RuntimeError("known_source_urls result must be a list")
        if not all(isinstance(url, str) for url in result):
            raise RuntimeError("known_source_urls result entries must be strings")
        return list(result)

    def source_post_summary(
        self,
        source_name: str,
        source_url: str,
        *,
        timeout: Optional[float] = 30.0,
    ) -> Mapping[str, Any] | None:
        result = self._state_query(
            "source_post_summary",
            {"source_name": source_name, "source_url": source_url},
            timeout=timeout,
        )
        if result is not None and not isinstance(result, Mapping):
            raise RuntimeError("source_post_summary result must be an object or null")
        return result

    def last_seen_at(
        self,
        source_name: str,
        *,
        timeout: Optional[float] = 30.0,
    ) -> str | None:
        result = self._state_query(
            "last_seen_at",
            {"source_name": source_name},
            timeout=timeout,
        )
        if result is not None and not isinstance(result, str):
            raise RuntimeError("last_seen_at result must be a string or null")
        return result

    def content_fingerprint(
        self,
        source_name: str,
        source_url: str,
        *,
        timeout: Optional[float] = 30.0,
    ) -> str | None:
        result = self._state_query(
            "content_fingerprint",
            {"source_name": source_name, "source_url": source_url},
            timeout=timeout,
        )
        if result is not None and not isinstance(result, str):
            raise RuntimeError("content_fingerprint result must be a string or null")
        return result

    def _request_host(
        self,
        method: Method | str,
        params: Mapping[str, Any],
        *,
        timeout: Optional[float],
    ) -> Any:
        rpc_id = self._ids.next_id()
        self._runtime.write(request(rpc_id, method, dict(params)))
        message = self._wait_response(rpc_id, timeout=timeout)
        if "error" in message:
            err = message["error"]
            if isinstance(err, Mapping):
                raise HostRequestError(
                    int(err.get("code", -32000)),
                    str(err.get("message", "host request failed")),
                    err.get("data"),
                )
            raise HostRequestError(-32000, "host request failed", err)
        return message.get("result")

    def _store_response(self, message_id: str, message: Mapping[str, Any]) -> None:
        with self._responses_cv:
            self._responses[message_id] = message
            self._responses_cv.notify_all()

    def _wait_response(
        self,
        message_id: str,
        *,
        timeout: Optional[float],
    ) -> Mapping[str, Any]:
        with self._responses_cv:
            if message_id not in self._responses:
                self._responses_cv.wait_for(
                    lambda: message_id in self._responses or self.is_cancelled(),
                    timeout=timeout,
                )
            if message_id in self._responses:
                return self._responses.pop(message_id)
        if self.is_cancelled():
            raise RuntimeError(f"cancelled while waiting for host response {message_id}")
        raise TimeoutError(f"timed out waiting for host response {message_id}")


class PluginRuntime:
    def __init__(
        self,
        manifest: PluginManifest,
        discover: DiscoverCallback,
        *,
        reader: Optional[BinaryIO] = None,
        writer: Optional[BinaryIO] = None,
    ) -> None:
        self.manifest = manifest
        self.discover = discover
        self.reader = reader if reader is not None else getattr(
            sys.stdin.buffer, "raw", sys.stdin.buffer
        )
        self.writer = writer if writer is not None else sys.stdout.buffer
        self._write_lock = threading.Lock()

    def write(self, message: Mapping[str, Any]) -> None:
        with self._write_lock:
            write_json_frame(self.writer, dict(message))

    def run(self) -> None:
        init = read_json_frame(self.reader)
        self._handle_initialize(init)
        discover = read_json_frame(self.reader)
        self._handle_discover(discover)

    def _handle_initialize(self, message: Mapping[str, Any]) -> None:
        if message.get("method") != Method.INITIALIZE.value:
            self.write(
                response_error(message.get("id"), -32601, "expected initialize")
            )
            return
        params = message.get("params") or {}
        if params.get("protocol_version") != PROTOCOL_VERSION:
            self.write(
                response_error(
                    message.get("id"),
                    -32602,
                    "unsupported protocol_version",
                )
            )
            return
        self.write(
            response(
                message.get("id"),
                {
                    "protocol_version": PROTOCOL_VERSION,
                    "record_schema_version": RECORD_SCHEMA_VERSION,
                    "manifest": self.manifest.to_dict(),
                },
            )
        )

    def _handle_discover(self, message: Mapping[str, Any]) -> None:
        if message.get("method") != Method.DISCOVER.value:
            self.write(response_error(message.get("id"), -32601, "expected discover"))
            return
        params = message.get("params") or {}
        request_id = params.get("request_id")
        if not isinstance(request_id, str) or not request_id:
            self.write(response_error(message.get("id"), -32602, "missing request_id"))
            return
        context = PluginContext(self, request_id)
        watcher = self._start_cancel_watcher(context)
        try:
            returned_records = self.discover(context, params)
            records = context.records if returned_records is None else returned_records
        except Exception as exc:
            watcher.stop()
            self.write(response_error(message.get("id"), -32000, str(exc)))
            raise
        watcher.stop()
        self.write(response(message.get("id"), {"records": records}))

    def _start_cancel_watcher(self, context: PluginContext) -> "_CancelWatcher":
        watcher = _CancelWatcher(self.reader, context)
        watcher.start()
        return watcher


class _CancelWatcher:
    def __init__(self, reader: BinaryIO, context: PluginContext) -> None:
        self._reader = reader
        self._context = context
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self._join_on_stop = True

    def start(self) -> None:
        try:
            fd = self._reader.fileno()
        except (AttributeError, OSError):
            fd = None

        if isinstance(fd, int) and fd >= 0:
            target = lambda: self._watch_fd(fd)
            self._join_on_stop = True
        else:
            target = self._watch_stream
            self._join_on_stop = False

        self._thread = threading.Thread(
            target=target,
            name="mh-plugin-cancel",
            daemon=not self._join_on_stop,
        )
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread is not None and self._join_on_stop:
            self._thread.join(timeout=1.0)

    def _handle_message(self, message: Mapping[str, Any]) -> bool:
        message_id = message.get("id")
        if message_id is not None and ("result" in message or "error" in message):
            self._context._store_response(str(message_id), message)
            return False
        if message.get("method") != Method.CANCEL.value:
            return False
        params = message.get("params") or {}
        if params.get("request_id") == self._context.request_id:
            self._context._mark_cancelled()
            return True
        return False

    def _watch_stream(self) -> None:
        try:
            while not self._stop.is_set() and not self._context.is_cancelled():
                if self._handle_message(read_json_frame(self._reader)):
                    return
        except Exception:
            return

    def _watch_fd(self, fd: int) -> None:
        try:
            while not self._stop.is_set() and not self._context.is_cancelled():
                message = self._read_json_frame_fd(fd)
                if message is None:
                    return
                if self._handle_message(message):
                    return
        except Exception:
            return

    def _read_json_frame_fd(self, fd: int) -> Optional[Mapping[str, Any]]:
        prefix = self._read_exact_fd(fd, 4)
        if prefix is None:
            return None
        length = struct.unpack(">I", prefix)[0]
        if length > MAX_FRAME:
            return None
        payload = self._read_exact_fd(fd, length) if length else b""
        if payload is None:
            return None
        value = json.loads(payload.decode("utf-8"))
        return value if isinstance(value, Mapping) else None

    def _read_exact_fd(self, fd: int, length: int) -> Optional[bytes]:
        chunks = bytearray()
        while len(chunks) < length:
            if self._stop.is_set():
                return None
            ready, _, _ = select.select([fd], [], [], 0.05)
            if not ready:
                continue
            try:
                chunk = os.read(fd, length - len(chunks))
            except BlockingIOError:
                continue
            if not chunk:
                return None
            chunks.extend(chunk)
        return bytes(chunks)


def run_plugin(
    *,
    source_name: str,
    display_label: str,
    discover: DiscoverCallback,
    allowed_domains: Optional[list[str]] = None,
    capabilities: Optional[list[str]] = None,
) -> None:
    manifest = PluginManifest(
        source_name=source_name,
        display_label=display_label,
        allowed_domains=allowed_domains or [],
        capabilities=capabilities or [],
    )
    with StdoutGuard() as guard:
        PluginRuntime(manifest, discover, writer=guard.protocol).run()
