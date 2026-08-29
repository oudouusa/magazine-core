"""Ephemeral loopback servers for the UI extension isolation spike."""
from __future__ import annotations

import json
import threading
import urllib.parse
from dataclasses import dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from common import (
    ExtensionRegistry,
    GALLERY_FIXTURE,
    GRAPH_FIXTURE,
    HOST,
    SYNTHETIC_TOKEN,
    SpikeFailure,
)
from pages import separate_html, shell_html


@dataclass
class ServerState:
    shell_origin: str = ""
    separate_origin: str = ""
    accepted_management_mutations: int = 0


class QuietThreadingHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = False


class SpikeServer:
    def __init__(
        self,
        assets_root: Path,
        *,
        sandbox_stun_port: int,
        separate_stun_port: int,
    ) -> None:
        self.assets_root = assets_root.resolve(strict=True)
        self.sandbox_stun_port = sandbox_stun_port
        self.separate_stun_port = separate_stun_port
        self.registry = ExtensionRegistry()
        self.registry.register("sandboxed", self.assets_root, "sandboxed.html")
        self.state = ServerState()
        self._shell: QuietThreadingHTTPServer | None = None
        self._separate: QuietThreadingHTTPServer | None = None
        self._threads: list[threading.Thread] = []

    def __enter__(self) -> "SpikeServer":
        owner = self

        class ShellHandler(BaseHTTPRequestHandler):
            server_version = "mh-ui-isolation-spike"

            def log_message(self, _format: str, *_args: object) -> None:
                return

            def do_OPTIONS(self) -> None:  # noqa: N802 - stdlib hook
                origin = self.headers.get("Origin")
                if self.path.startswith("/api/read/") and origin == owner.state.separate_origin:
                    self.send_response(HTTPStatus.NO_CONTENT)
                    self.send_header("Access-Control-Allow-Origin", origin)
                    self.send_header("Access-Control-Allow-Methods", "GET")
                    self.send_header("Access-Control-Allow-Headers", "Content-Type")
                    self.send_header("Access-Control-Max-Age", "0")
                    self.send_header("Vary", "Origin")
                    self.end_headers()
                    return
                self._json(HTTPStatus.FORBIDDEN, {"error": "cors denied"})

            def do_GET(self) -> None:  # noqa: N802 - stdlib hook
                parsed = urllib.parse.urlsplit(self.path)
                if parsed.path == "/":
                    self._html(shell_html(owner))
                    return
                if parsed.path == "/api/read/gallery":
                    self._read_json(GALLERY_FIXTURE)
                    return
                if parsed.path == "/api/read/graph":
                    self._read_json(GRAPH_FIXTURE)
                    return
                if parsed.path == "/api/stats":
                    self._json(
                        HTTPStatus.OK,
                        {
                            "accepted_management_mutations": owner.state.accepted_management_mutations
                        },
                    )
                    return
                prefix = "/extensions/sandboxed/"
                if parsed.path.startswith(prefix):
                    requested = parsed.path.removeprefix(prefix)
                    self._extension_asset(requested)
                    return
                self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})

            def do_POST(self) -> None:  # noqa: N802 - stdlib hook
                if self.path != "/api/manage/mutate":
                    self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})
                    return
                origin = self.headers.get("Origin")
                token = self.headers.get("X-MH-UI-Token")
                if origin != owner.state.shell_origin or token != SYNTHETIC_TOKEN:
                    self._json(HTTPStatus.FORBIDDEN, {"error": "management denied"})
                    return
                owner.state.accepted_management_mutations += 1
                self._json(HTTPStatus.OK, {"accepted": True})

            def _read_json(self, payload: object) -> None:
                origin = self.headers.get("Origin")
                if origin != owner.state.separate_origin:
                    self._json(HTTPStatus.FORBIDDEN, {"error": "read origin denied"})
                    return
                self._json(
                    HTTPStatus.OK,
                    payload,
                    extra_headers={
                        "Access-Control-Allow-Origin": origin,
                        "Vary": "Origin",
                    },
                )

            def _extension_asset(self, requested: str) -> None:
                try:
                    asset = owner.registry.resolve_asset("sandboxed", requested)
                except SpikeFailure as exc:
                    self._json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
                    return
                if asset.suffix != ".html":
                    self._json(HTTPStatus.UNSUPPORTED_MEDIA_TYPE, {"error": "unsupported asset"})
                    return
                body = asset.read_bytes()
                self.send_response(HTTPStatus.OK)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Content-Type-Options", "nosniff")
                self.send_header("Referrer-Policy", "no-referrer")
                self.send_header("X-DNS-Prefetch-Control", "off")
                self.send_header(
                    "Permissions-Policy",
                    "camera=(), microphone=(), geolocation=(), display-capture=(), "
                    "fullscreen=(), payment=(), usb=(), serial=(), hid=()",
                )
                self.send_header("Cross-Origin-Resource-Policy", "same-origin")
                self.send_header(
                    "Content-Security-Policy",
                    "default-src 'none'; script-src 'nonce-sandboxed-spike'; "
                    "style-src 'nonce-sandboxed-spike'; connect-src 'none'; "
                    "img-src 'none'; font-src 'none'; media-src 'none'; "
                    "object-src 'none'; frame-src 'none'; worker-src 'none'; "
                    "manifest-src 'none'; base-uri 'none'; "
                    "form-action 'none'; frame-ancestors 'self'",
                )
                self.end_headers()
                self.wfile.write(body)

            def _html(self, body: str) -> None:
                raw = body.encode("utf-8")
                self.send_response(HTTPStatus.OK)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(raw)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Content-Type-Options", "nosniff")
                self.send_header("Referrer-Policy", "no-referrer")
                self.send_header("X-DNS-Prefetch-Control", "off")
                self.send_header(
                    "Permissions-Policy",
                    "camera=(), microphone=(), geolocation=(), display-capture=(), "
                    "fullscreen=(), payment=(), usb=(), serial=(), hid=()",
                )
                self.send_header(
                    "Content-Security-Policy",
                    "default-src 'none'; script-src 'nonce-shell-spike'; "
                    "style-src 'nonce-shell-spike'; connect-src 'self'; "
                    f"frame-src 'self' {owner.state.separate_origin}; "
                    "img-src 'none'; object-src 'none'; base-uri 'none'; "
                    "form-action 'none'; frame-ancestors 'none'",
                )
                self.end_headers()
                self.wfile.write(raw)

            def _json(
                self,
                status: HTTPStatus,
                payload: object,
                *,
                extra_headers: dict[str, str] | None = None,
            ) -> None:
                raw = json.dumps(payload, sort_keys=True).encode("utf-8")
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Content-Type-Options", "nosniff")
                if extra_headers:
                    for key, value in extra_headers.items():
                        self.send_header(key, value)
                self.end_headers()
                self.wfile.write(raw)

        class SeparateHandler(BaseHTTPRequestHandler):
            server_version = "mh-ui-separate-origin-spike"

            def log_message(self, _format: str, *_args: object) -> None:
                return

            def do_GET(self) -> None:  # noqa: N802 - stdlib hook
                parsed = urllib.parse.urlsplit(self.path)
                if parsed.path != "/index.html":
                    self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})
                    return
                body = separate_html(owner).encode("utf-8")
                self.send_response(HTTPStatus.OK)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Content-Type-Options", "nosniff")
                self.send_header("Referrer-Policy", "no-referrer")
                self.send_header("X-DNS-Prefetch-Control", "off")
                self.send_header(
                    "Permissions-Policy",
                    "camera=(), microphone=(), geolocation=(), display-capture=(), "
                    "fullscreen=(), payment=(), usb=(), serial=(), hid=()",
                )
                self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
                self.send_header(
                    "Content-Security-Policy",
                    "default-src 'none'; script-src 'nonce-separate-spike'; "
                    "style-src 'nonce-separate-spike'; "
                    f"connect-src {owner.state.shell_origin}; "
                    "img-src 'none'; font-src 'none'; media-src 'none'; "
                    "object-src 'none'; frame-src 'none'; worker-src 'none'; "
                    "manifest-src 'none'; base-uri 'none'; "
                    "form-action 'none'; "
                    f"frame-ancestors {owner.state.shell_origin}",
                )
                self.end_headers()
                self.wfile.write(body)

            def _json(self, status: HTTPStatus, payload: object) -> None:
                raw = json.dumps(payload, sort_keys=True).encode("utf-8")
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

        self._shell = QuietThreadingHTTPServer((HOST, 0), ShellHandler)
        self._separate = QuietThreadingHTTPServer((HOST, 0), SeparateHandler)
        shell_port = self._shell.server_address[1]
        separate_port = self._separate.server_address[1]
        self.state.shell_origin = f"http://{HOST}:{shell_port}"
        self.state.separate_origin = f"http://{HOST}:{separate_port}"

        for name, server in (("shell", self._shell), ("separate", self._separate)):
            thread = threading.Thread(
                target=server.serve_forever,
                name=f"ui-extension-spike-{name}",
                daemon=True,
            )
            thread.start()
            self._threads.append(thread)
        return self

    def __exit__(self, *_exc: object) -> None:
        for server in (self._shell, self._separate):
            if server is not None:
                server.shutdown()
                server.server_close()
        for thread in self._threads:
            thread.join(timeout=5)
