"""Protect protocol stdout from accidental plugin output."""

from __future__ import annotations

import os
import sys
from types import TracebackType
from typing import BinaryIO, Optional, Type


class StdoutGuard:
    """Redirect normal stdout to stderr while preserving a protocol writer.

    The host protocol uses fd 1. At plugin startup, this guard duplicates the
    original fd 1 for framed protocol writes, then redirects fd 1 itself to
    stderr. That catches `print(...)`, `os.write(1, ...)`, and child process
    stdout inherited after the guard is installed.
    """

    def __init__(self) -> None:
        self._restore_fd: Optional[int] = None
        self._protocol_fd: Optional[int] = None
        self._protocol: Optional[BinaryIO] = None

    def __enter__(self) -> "StdoutGuard":
        sys.stdout.flush()
        sys.stderr.flush()
        self._restore_fd = os.dup(1)
        self._protocol_fd = os.dup(1)
        self._protocol = os.fdopen(self._protocol_fd, "wb", buffering=0)
        self._protocol_fd = None
        os.dup2(2, 1)
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc: Optional[BaseException],
        tb: Optional[TracebackType],
    ) -> None:
        sys.stdout.flush()
        sys.stderr.flush()
        if self._protocol is not None:
            self._protocol.flush()
            self._protocol.close()
            self._protocol = None
        if self._restore_fd is not None:
            os.dup2(self._restore_fd, 1)
            os.close(self._restore_fd)
            self._restore_fd = None
        if self._protocol_fd is not None:
            os.close(self._protocol_fd)
            self._protocol_fd = None

    @property
    def protocol(self) -> BinaryIO:
        if self._protocol is None:
            raise RuntimeError("stdout guard is not active")
        return self._protocol


def protect_stdout() -> StdoutGuard:
    """Return a stdout guard context manager."""

    return StdoutGuard()
