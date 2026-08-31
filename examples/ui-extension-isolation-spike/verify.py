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


def run_browser_once(browser: str, url: str) -> dict[str, object]:
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
            "--virtual-time-budget=8000",
            "--dump-dom",
            url,
        ]
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
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


def run_browser(browser: str, url: str) -> dict[str, object]:
    for attempt in range(1, 4):
        try:
            return run_browser_once(browser, url)
        except SpikeFailure as exc:
            if "browser result is not JSON: 'pending'" not in str(exc) or attempt == 3:
                raise
    raise SpikeFailure("browser verification did not settle")


def run_network_probe(
    browser: str,
    url: str,
    sandbox_probe: UdpProbe,
    separate_probe: UdpProbe,
) -> tuple[int, int]:
    for _attempt in range(3):
        sandbox_probe.reset()
        separate_probe.reset()
        with tempfile.TemporaryDirectory(prefix="mh-ui-extension-network-") as profile:
            process = subprocess.Popen(
                [
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
                    "--remote-debugging-port=0",
                    url,
                ],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            try:
                sandbox_packets = sandbox_probe.wait_for_packet(timeout=5)
                separate_packets = separate_probe.wait_for_packet(timeout=5)
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
        if sandbox_packets > 0 and separate_packets > 0:
            return sandbox_packets, separate_packets
    raise SpikeFailure("the real browser did not reproduce the WebRTC outbound bypass")


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
        ),
        "separate": (
            "parent_dom_blocked",
            "token_unreadable",
            "management_fetch_rejected",
            "outbound_blocked",
            "read_via_exact_cors",
            "deep_link",
            "keyboard_navigation",
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
            verification_url = (
                f"{server.state.shell_origin}/?probe_webrtc=0#work=synthetic-work-001"
            )
            network_url = (
                f"{server.state.shell_origin}/?probe_webrtc=1#work=synthetic-work-001"
            )
            first = run_browser(browser, verification_url)
            assert_candidate_result(first)
            first_sandbox_packets, first_separate_packets = run_network_probe(
                browser, network_url, sandbox_probe, separate_probe
            )

            second = run_browser(browser, verification_url)
            assert_candidate_result(second)
            second_sandbox_packets, second_separate_packets = run_network_probe(
                browser, network_url, sandbox_probe, separate_probe
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
