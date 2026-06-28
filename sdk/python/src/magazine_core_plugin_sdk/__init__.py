"""Python SDK for the magazine-core plugin protocol.

The package root exposes the stable plugin-author API for ``0.1.0-beta``.
Protocol/conformance helpers remain available from their submodules.
"""

from .models import ExternalLink, SourceRecord
from .protocol import MAX_RECORD_BATCH, PROTOCOL_VERSION, RECORD_SCHEMA_VERSION
from .runtime import (
    HostFetchResponse,
    HostRequestError,
    PluginContext,
    PluginManifest,
    run_plugin,
)
from .stdout_guard import StdoutGuard, protect_stdout

__all__ = [
    "ExternalLink",
    "HostFetchResponse",
    "HostRequestError",
    "MAX_RECORD_BATCH",
    "PROTOCOL_VERSION",
    "PluginContext",
    "PluginManifest",
    "RECORD_SCHEMA_VERSION",
    "SourceRecord",
    "StdoutGuard",
    "run_plugin",
    "protect_stdout",
]
