#!/usr/bin/env python3
"""Real-Chrome gate for the production ``mh-ui-ext`` trusted host."""
from __future__ import annotations

import html
import http.client
import json
import os
from pathlib import Path
import re
import select
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.parse


WIRE_SCHEMA = "mh-ui-read-provider.v1"
HOST_RE = re.compile(r"mh-ui-ext listening on (http://127\.0\.0\.1:(\d+))")


class GateFailure(RuntimeError):
    pass


def read_exact(stream, size: int) -> bytes:
    value = bytearray()
    while len(value) < size:
        chunk = stream.read(size - len(value))
        if not chunk:
            raise EOFError
        value.extend(chunk)
    return bytes(value)


def read_frame(stream) -> dict:
    length = struct.unpack(">I", read_exact(stream, 4))[0]
    if not 0 < length <= 8 * 1024 * 1024:
        raise GateFailure("provider received an invalid frame length")
    value = json.loads(read_exact(stream, length).decode("utf-8"))
    if not isinstance(value, dict):
        raise GateFailure("provider received a non-object frame")
    return value


def write_frame(stream, value: dict) -> None:
    body = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    stream.write(struct.pack(">I", len(body)) + body)
    stream.flush()


class Provider(threading.Thread):
    def __init__(self, path: Path):
        super().__init__(name="mh-ui-ext-real-browser-provider", daemon=True)
        self.path = path
        self.ready = threading.Event()
        self.changed = threading.Condition()
        self.requests: list[dict] = []
        self.failure: BaseException | None = None

    def run(self) -> None:
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            listener.bind(str(self.path))
            os.chmod(self.path, 0o600)
            listener.listen(1)
            self.ready.set()
            connection, _ = listener.accept()
            with connection, connection.makefile("rwb", buffering=0) as stream:
                write_frame(stream, {
                    "schema": WIRE_SCHEMA,
                    "type": "hello",
                    "generation": "real-browser-generation",
                    "operations": ["gallery.list", "graph.detail"],
                })
                while True:
                    try:
                        request = read_frame(stream)
                    except EOFError:
                        return
                    expected = {
                        "schema", "type", "request_id", "generation",
                        "operation", "arguments",
                    }
                    if set(request) != expected:
                        raise GateFailure(f"provider request shape changed: {request!r}")
                    if request["schema"] != WIRE_SCHEMA or request["type"] != "read":
                        raise GateFailure(f"provider request contract changed: {request!r}")
                    if request["generation"] != "real-browser-generation":
                        raise GateFailure("provider generation was not pinned")
                    operation = request["operation"]
                    if operation not in ("gallery.list", "graph.detail"):
                        raise GateFailure(f"unexpected operation: {operation!r}")
                    with self.changed:
                        self.requests.append(request)
                        self.changed.notify_all()
                    payload = (
                        {
                            "generation": "real-browser-generation",
                            "total": 1,
                            "limit": 1,
                            "offset": 0,
                            "items": [{
                                "key": "source:v1:synthetic",
                                "title": "Synthetic",
                                "source_name": "fixture",
                                "release_date": "2026-08-29",
                                "cover_url": None,
                                "page_count": 1,
                                "primary_work_key": "stable-work-001",
                            }],
                        }
                        if operation == "gallery.list"
                        else {
                            "generation": "real-browser-generation",
                            "ready": True,
                            "work": {"key": "stable-work-001", "title": "Synthetic"},
                            "members": [],
                            "relations": [],
                        }
                    )
                    write_frame(stream, {
                        "schema": WIRE_SCHEMA,
                        "type": "read-result",
                        "request_id": request["request_id"],
                        "generation": "real-browser-generation",
                        "ok": True,
                        "payload": payload,
                    })
        except BaseException as exc:  # surfaced in the main gate
            self.failure = exc
            self.ready.set()
            with self.changed:
                self.changed.notify_all()
        finally:
            listener.close()

    def wait_for(self, gallery: int, graph: int, timeout: float = 15.0) -> None:
        deadline = time.monotonic() + timeout
        with self.changed:
            while time.monotonic() < deadline:
                if self.failure is not None:
                    raise GateFailure(f"provider failed: {self.failure}")
                operations = [request["operation"] for request in self.requests]
                if operations.count("gallery.list") >= gallery and operations.count("graph.detail") >= graph:
                    return
                self.changed.wait(deadline - time.monotonic())
        raise GateFailure(f"browser did not complete broker checks: {self.requests!r}")


PLUGIN_HTML = r'''<!doctype html><meta charset="utf-8"><title>gate</title>
<script>
(() => {
  "use strict";
  const CHANNEL = "mh-ui-read-broker.v1";
  let sequence = 0;
  const pending = new Map();
  addEventListener("message", event => {
    const value = event.data;
    if (event.source !== parent || !value || value.channel !== CHANNEL ||
        value.type !== "read-result" || typeof value.request_id !== "string") return;
    const entry = pending.get(value.request_id);
    if (!entry || value.op !== entry.op) return;
    pending.delete(value.request_id);
    if (value.ok) entry.resolve(value.payload); else entry.reject(new Error("read failed"));
  });
  function request(op, payload) {
    const request_id = `gate_${++sequence}`;
    return new Promise((resolve, reject) => {
      pending.set(request_id, {op, resolve, reject});
      parent.postMessage({channel: CHANNEL, type: "read", request_id, op, payload}, "*");
    });
  }
  async function run() {
    let parentBlocked = false;
    try { void parent.document.body; } catch (_error) { parentBlocked = true; }
    if (!parentBlocked) return;
    if ("__OPERATION__" === "graph.detail") {
      await request("graph.detail", {work_key: "stable-work-001"});
    } else {
      await request("gallery.list", {limit: 1, offset: 0});
    }
  }
  run().catch(() => {});
})();
</script>'''


def find_binary(env_name: str, default: Path) -> str:
    override = os.environ.get(env_name)
    if override:
        resolved = shutil.which(override) if os.path.sep not in override else override
        if resolved and Path(resolved).is_file():
            return str(resolved)
        raise GateFailure(f"{env_name} does not resolve to a file: {override}")
    if default.is_file():
        return str(default)
    raise GateFailure(f"build the required binary first: {default}")


def find_browser() -> str:
    override = os.environ.get("BROWSER_BIN")
    candidates = (override,) if override else (
        "google-chrome", "google-chrome-stable", "chromium", "chromium-browser",
    )
    for candidate in candidates:
        if not candidate:
            continue
        resolved = shutil.which(candidate) if os.path.sep not in candidate else candidate
        if resolved and Path(resolved).is_file():
            return str(resolved)
    raise GateFailure("a Chromium-family browser is required")


def wait_for_host(process: subprocess.Popen[str]) -> tuple[str, int]:
    assert process.stdout is not None
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        ready, _, _ = select.select([process.stdout], [], [], 0.25)
        if not ready:
            if process.poll() is not None:
                break
            continue
        line = process.stdout.readline()
        match = HOST_RE.search(line)
        if match:
            return match.group(1), int(match.group(2))
    stderr = process.stderr.read() if process.stderr is not None and process.poll() is not None else ""
    raise GateFailure(f"mh-ui-ext did not become ready: {stderr[-2000:]}")


def run_browser(browser: str, origin: str) -> None:
    with tempfile.TemporaryDirectory(prefix="mh-ui-ext-chrome-") as profile:
        completed = subprocess.run([
            browser,
            "--headless=new", "--no-sandbox", "--disable-gpu",
            "--disable-dev-shm-usage", "--disable-background-networking",
            "--disable-default-apps", "--disable-extensions", "--disable-sync",
            "--metrics-recording-only", "--no-first-run",
            "--no-default-browser-check", "--no-proxy-server",
            f"--user-data-dir={profile}", "--virtual-time-budget=10000",
            "--dump-dom", origin,
        ], capture_output=True, text=True, timeout=30, check=False)
    if completed.returncode != 0:
        raise GateFailure(
            f"browser failed with {completed.returncode}: {completed.stderr[-2000:]}"
        )
    if "mh-ui-ext-shell" not in html.unescape(completed.stdout):
        raise GateFailure("browser did not load the production shell")


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    mh = find_binary("MH_BIN", root / "target/debug/mh")
    host = find_binary("MH_UI_EXT_BIN", root / "target/debug/mh-ui-ext")
    browser = find_browser()

    with tempfile.TemporaryDirectory(prefix="mh-ui-ext-real-browser-") as raw:
        temp = Path(raw)
        os.chmod(temp, 0o700)
        database = temp / "core.db"
        subprocess.run([mh, "init-db", str(database)], check=True, capture_output=True, text=True)
        extension = temp / "extensions" / "browser-gate"
        extension.mkdir(parents=True)
        (extension / "plugin.json").write_text(json.dumps({
            "name": "browser-gate", "title": "Browser gate", "entry": "index.html",
        }), encoding="utf-8")
        plugin_script = PLUGIN_HTML.split("<script>", 1)[1].rsplit("</script>", 1)[0]
        (extension / "index.html").write_text(
            '<!doctype html><meta charset="utf-8"><title>gate</title>'
            '<script src="gate.js"></script>',
            encoding="utf-8",
        )
        gate_script = extension / "gate.js"
        gate_script.write_text(
            plugin_script.replace("__OPERATION__", "gallery.list"), encoding="utf-8"
        )
        socket_dir = temp / "provider"
        socket_dir.mkdir(mode=0o700)
        socket_path = socket_dir / "read.sock"
        provider = Provider(socket_path)
        provider.start()
        if not provider.ready.wait(5) or provider.failure is not None:
            raise GateFailure(f"provider did not start: {provider.failure}")

        process = subprocess.Popen([
            host, "--db", str(database), "--trusted-extensions-dir",
            str(extension.parent), "--provider-socket", str(socket_path), "--port", "0",
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        try:
            origin, port = wait_for_host(process)
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
            connection.request("GET", "/")
            shell = connection.getresponse()
            shell_body = shell.read().decode("utf-8")
            connection.close()
            if shell.status != 200 or 'sandbox="allow-scripts"' not in shell_body:
                raise GateFailure("production shell did not apply the iframe sandbox")
            if "allow-same-origin" in shell_body:
                raise GateFailure("production shell enabled iframe same-origin access")
            asset_match = re.search(
                r'src="http://127\.0\.0\.1:(\d+)/extensions/browser-gate/index\.html"',
                shell_body,
            )
            if not asset_match:
                raise GateFailure("production shell did not use the separate asset origin")
            asset_connection = http.client.HTTPConnection(
                "127.0.0.1", int(asset_match.group(1)), timeout=5
            )
            asset_connection.request("GET", "/extensions/browser-gate/index.html")
            asset = asset_connection.getresponse()
            asset.read()
            asset_connection.close()
            asset_csp = asset.getheader("Content-Security-Policy") or ""
            if (
                asset.status != 200
                or "connect-src 'none'" not in asset_csp
                or f"frame-ancestors {origin}" not in asset_csp
            ):
                raise GateFailure(f"production asset CSP changed: {asset_csp!r}")

            direct_query = urllib.parse.urlencode({
                "request_id": "direct_graph",
                "op": "graph.detail",
                "payload": json.dumps({"work_key": "stable-work-001"}),
            })
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
            connection.request(
                "GET", f"/__mh_ui_ext/read/browser-gate?{direct_query}"
            )
            direct_graph = connection.getresponse()
            direct_payload = json.loads(direct_graph.read())
            connection.close()
            if direct_graph.status != 200 or direct_payload.get("ok") is not True:
                raise GateFailure(f"graph broker failed: {direct_payload!r}")

            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
            connection.request("POST", "/api/manage/mutate")
            management = connection.getresponse()
            management.read()
            connection.close()
            if management.status != 405:
                raise GateFailure(f"management route was not absent: {management.status}")

            run_browser(browser, origin)
            provider.wait_for(gallery=1, graph=1)
            run_browser(browser, origin)
            provider.wait_for(gallery=2, graph=1)
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        provider.join(timeout=5)
        if provider.failure is not None:
            raise GateFailure(f"provider failed: {provider.failure}")

    print(json.dumps({
        "browser": Path(browser).name,
        "production_host": "pass",
        "fixed_provider_contract": "pass",
        "parent_dom_isolation": "pass",
        "asset_csp": "pass",
        "management_route": "absent",
        "browser_runs": 2,
    }, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (GateFailure, OSError, subprocess.SubprocessError) as exc:
        print(f"trusted UI extension host gate failed: {exc}", file=os.sys.stderr)
        raise SystemExit(1) from exc
