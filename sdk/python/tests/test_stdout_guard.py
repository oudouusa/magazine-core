from __future__ import annotations

import os
import struct
import subprocess
import sys
from pathlib import Path


def test_stdout_guard_routes_noisy_stdout_to_stderr() -> None:
    root = Path(__file__).resolve().parents[3]
    env = os.environ.copy()
    env["PYTHONPATH"] = str(root / "sdk/python/src")
    script = r"""
import os
import subprocess
import sys

from magazine_core_plugin_sdk.framing import write_frame
from magazine_core_plugin_sdk.stdout_guard import StdoutGuard

with StdoutGuard() as guard:
    print("print-noise")
    os.write(1, b"native-noise\n")
    subprocess.run(
        [sys.executable, "-c", "print('child-noise')"],
        check=True,
    )
    write_frame(guard.protocol, '{"jsonrpc":"2.0","ok":true}')
"""
    result = subprocess.run(
        [sys.executable, "-c", script],
        check=True,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    payload = b'{"jsonrpc":"2.0","ok":true}'

    assert result.stdout == struct.pack(">I", len(payload)) + payload
    assert b"print-noise" in result.stderr
    assert b"native-noise" in result.stderr
    assert b"child-noise" in result.stderr
