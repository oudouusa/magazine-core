"""Shared synthetic fixtures and fail-closed helpers for the UI spike."""
from __future__ import annotations

import re
import socket
import threading
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Final

HOST: Final = "127.0.0.1"
ROUTE_RE: Final = re.compile(r"^[a-z][a-z0-9-]{0,31}$")
RESULT_RE: Final = re.compile(
    r'<pre id="verification-result">(?P<payload>.*?)</pre>', re.DOTALL
)
SYNTHETIC_TOKEN: Final = "synthetic-management-token-not-a-secret"
CHANNEL: Final = "mh-ui-extension-isolation-spike-v1"

GALLERY_FIXTURE: Final = {
    "generation": "synthetic-generation-001",
    "items": [
        {"key": "gallery-001", "title": "Synthetic cover one"},
        {"key": "gallery-002", "title": "Synthetic cover two"},
    ],
}
GRAPH_FIXTURE: Final = {
    "generation": "synthetic-generation-001",
    "work": {"key": "synthetic-work-001", "title": "Synthetic parent work"},
    "members": [
        {"key": "source-001", "role": "primary"},
        {"key": "source-002", "role": "member"},
    ],
    "relations": [{"kind": "related", "target": "synthetic-work-002"}],
    "ready": True,
}


class SpikeFailure(RuntimeError):
    """Raised for a fail-closed spike validation error."""


class UdpProbe:
    """Records local STUN traffic emitted by a browser WebRTC probe."""

    def __init__(self) -> None:
        self._socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self._socket.bind((HOST, 0))
        self._socket.settimeout(0.1)
        self.port = int(self._socket.getsockname()[1])
        self._count = 0
        self._lock = threading.Lock()
        self._closed = threading.Event()
        self._thread = threading.Thread(
            target=self._receive,
            name=f"ui-extension-stun-probe-{self.port}",
            daemon=True,
        )

    def __enter__(self) -> "UdpProbe":
        self._thread.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self._closed.set()
        self._thread.join(timeout=2)
        self._socket.close()

    def reset(self) -> None:
        with self._lock:
            self._count = 0

    def wait_for_packet(self, timeout: float = 3.0) -> int:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                count = self._count
            if count > 0:
                return count
            time.sleep(0.05)
        with self._lock:
            return self._count

    def _receive(self) -> None:
        while not self._closed.is_set():
            try:
                payload, _address = self._socket.recvfrom(64 * 1024)
            except (TimeoutError, socket.timeout):
                continue
            except OSError:
                return
            if payload:
                with self._lock:
                    self._count += 1


@dataclass(frozen=True)
class RegisteredExtension:
    route: str
    root: Path
    entry: Path


class ExtensionRegistry:
    """Small path-confined registry used by both candidate implementations."""

    def __init__(self) -> None:
        self._routes: dict[str, RegisteredExtension] = {}

    def register(self, route: str, root: Path, entry: str) -> RegisteredExtension:
        if not ROUTE_RE.fullmatch(route):
            raise SpikeFailure(f"invalid extension route: {route!r}")
        if route in self._routes:
            raise SpikeFailure(f"duplicate extension route: {route}")

        root = root.resolve(strict=True)
        entry_path = self._resolve_confined(root, entry)
        if not entry_path.is_file():
            raise SpikeFailure(f"extension entry is not a file: {entry}")

        registered = RegisteredExtension(route=route, root=root, entry=entry_path)
        self._routes[route] = registered
        return registered

    def resolve_asset(self, route: str, requested: str) -> Path:
        try:
            extension = self._routes[route]
        except KeyError as exc:
            raise SpikeFailure(f"unknown extension route: {route}") from exc
        resolved = self._resolve_confined(extension.root, requested)
        if not resolved.is_file():
            raise SpikeFailure(f"extension asset is not a file: {requested}")
        return resolved

    @staticmethod
    def _resolve_confined(root: Path, requested: str) -> Path:
        decoded = urllib.parse.unquote(requested)
        if "\x00" in decoded or "\\" in decoded:
            raise SpikeFailure("extension asset contains a forbidden character")
        relative = Path(decoded)
        if relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
            raise SpikeFailure(f"extension asset is not a clean relative path: {requested!r}")
        resolved = (root / relative).resolve(strict=False)
        if not resolved.is_relative_to(root):
            raise SpikeFailure(f"extension asset escapes its root: {requested!r}")
        return resolved
