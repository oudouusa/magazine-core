#!/usr/bin/env python3
"""Real-browser isolation gate for a future ``mh ui`` extension boundary."""
from __future__ import annotations

import html
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from common import ExtensionRegistry, RESULT_RE, SpikeFailure, UdpProbe
from server import SpikeServer


def find_browser() -> str:
    override = os.environ.get("BROWSER_BIN")
    if override:
        path = shutil.which(override) if os.path.sep not in override else override
        if path and Path(path).is_file():
            return str(path)
        raise SpikeFailure(f"BROWSER_BIN does not resolve to a file: {override}")
    for candidate in (
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ):
        path = shutil.which(candidate)
        if path:
            return path
    raise SpikeFailure("a Chromium-family browser is required for this real-browser gate")


def run_browser(browser: str, url: str) -> dict[str, object]:
    with tempfile.TemporaryDirectory(prefix="mh-ui-extension-browser-") as profile:
        command = [
            browser,
            "--headless=new",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--disable-background-networking",
            "--disable-default-apps",
            "--disable-extensions",
            "--disable-sync",
            "--metrics-recording-only",
            "--no-first-run",
            "--no-default-browser-check",
            "--no-proxy-server",
            f"--user-data-dir={profile}",
            "--virtual-time-budget=20000",
            "--dump-dom",
            url,
        ]
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=45,
        )
    if completed.returncode != 0:
        raise SpikeFailure(
            "browser failed: "
            f"exit={completed.returncode}\nstdout={completed.stdout[-2000:]}\n"
            f"stderr={completed.stderr[-2000:]}"
        )
    match = RESULT_RE.search(completed.stdout)
    if not match:
        raise SpikeFailure(
            "browser DOM did not contain a verification result\n"
            f"stdout tail={completed.stdout[-4000:]}\n"
            f"stderr tail={completed.stderr[-2000:]}"
        )
    payload = html.unescape(match.group("payload"))
    try:
        parsed = json.loads(payload)
    except json.JSONDecodeError as exc:
        raise SpikeFailure(f"browser result is not JSON: {payload!r}") from exc
    if not isinstance(parsed, dict):
        raise SpikeFailure("browser result must be an object")
    return parsed


def assert_candidate_result(result: dict[str, object]) -> None:
    expected_true = {
        "sandboxed": (
            "parent_dom_blocked",
            "token_unreadable",
            "direct_management_blocked",
            "outbound_blocked",
            "read_via_broker",
            "deep_link",
            "keyboard_navigation",
            "webrtc_api_available",
            "webrtc_probe_started",
        ),
        "separate": (
            "parent_dom_blocked",
            "token_unreadable",
            "management_fetch_rejected",
            "outbound_blocked",
            "read_via_exact_cors",
            "deep_link",
            "keyboard_navigation",
            "webrtc_api_available",
            "webrtc_probe_started",
        ),
    }
    for candidate, keys in expected_true.items():
        payload = result.get(candidate)
        if not isinstance(payload, dict):
            raise SpikeFailure(f"missing candidate result: {candidate}")
        for key in keys:
            if payload.get(key) is not True:
                raise SpikeFailure(f"{candidate}.{key} did not pass: {payload!r}")
        if "fatal" in payload:
            raise SpikeFailure(f"{candidate} reported fatal error: {payload['fatal']}")

    if result.get("broker_requests") != 2:
        raise SpikeFailure(f"unexpected broker request count: {result.get('broker_requests')!r}")
    if result.get("accepted_management_mutations") != 0:
        raise SpikeFailure("an extension reached the management mutation path")
    if result.get("shell_hash") != "#work=synthetic-work-001":
        raise SpikeFailure(f"shell deep link was not retained: {result.get('shell_hash')!r}")


def run_registry_checks(assets_root: Path) -> None:
    registry = ExtensionRegistry()
    registered = registry.register("sandboxed", assets_root, "sandboxed.html")
    if registered.entry != (assets_root / "sandboxed.html").resolve():
        raise SpikeFailure("registered entry was not resolved deterministically")
    if registry.resolve_asset("sandboxed", "sandboxed.html") != registered.entry:
        raise SpikeFailure("registered asset resolution changed")

    rejected = 0
    for bad_path in (
        "../sandboxed.html",
        "%2e%2e/sandboxed.html",
        "/sandboxed.html",
        "nested/../../sandboxed.html",
        "nested\\sandboxed.html",
    ):
        try:
            registry.resolve_asset("sandboxed", bad_path)
        except SpikeFailure:
            rejected += 1
    if rejected != 5:
        raise SpikeFailure(f"path traversal gate rejected {rejected}/5 cases")

    try:
        registry.register("sandboxed", assets_root, "sandboxed.html")
    except SpikeFailure:
        pass
    else:
        raise SpikeFailure("duplicate route registration did not fail closed")

    try:
        registry.register("../bad", assets_root, "sandboxed.html")
    except SpikeFailure:
        pass
    else:
        raise SpikeFailure("invalid route registration did not fail closed")


def main() -> int:
    assets_root = Path(__file__).with_name("assets")
    run_registry_checks(assets_root)
    browser = find_browser()

    with UdpProbe() as sandbox_probe, UdpProbe() as separate_probe:
        with SpikeServer(
            assets_root,
            sandbox_stun_port=sandbox_probe.port,
            separate_stun_port=separate_probe.port,
        ) as server:
            url = f"{server.state.shell_origin}/#work=synthetic-work-001"
            first = run_browser(browser, url)
            assert_candidate_result(first)
            first_sandbox_packets = sandbox_probe.wait_for_packet()
            first_separate_packets = separate_probe.wait_for_packet()
            if first_sandbox_packets < 1 or first_separate_packets < 1:
                raise SpikeFailure(
                    "the real browser did not reproduce the WebRTC outbound bypass"
                )

            sandbox_probe.reset()
            separate_probe.reset()
            second = run_browser(browser, url)
            assert_candidate_result(second)
            second_sandbox_packets = sandbox_probe.wait_for_packet()
            second_separate_packets = separate_probe.wait_for_packet()
            if second_sandbox_packets < 1 or second_separate_packets < 1:
                raise SpikeFailure(
                    "the reload gate did not reproduce the WebRTC outbound bypass"
                )

    summary = {
        "browser": Path(browser).name,
        "registry": "pass",
        "parent_and_token_isolation": "pass",
        "management_mutations": 0,
        "csp_fetch_block": "pass",
        "deep_link_reload": "pass",
        "sandboxed_webrtc_packets": first_sandbox_packets,
        "separate_origin_webrtc_packets": first_separate_packets,
        "network_isolation": "reject: WebRTC bypass observed",
        "decision": (
            "do not treat arbitrary-JS UI extensions as untrusted; "
            "use trusted opt-in or a no-script declarative surface"
        ),
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, SpikeFailure, subprocess.TimeoutExpired) as exc:
        print(f"ui extension isolation spike failed: {exc}", file=os.sys.stderr)
        raise SystemExit(1) from exc
