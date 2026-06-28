# Python SDK public API

This document defines the Python SDK API tier for `0.1.0-beta`.

The SDK is a convenience layer for writing trusted plugins that speak
`protocol_version = 1` over framed stdin/stdout. It does not change the wire
contract in `docs/protocol-v1.md`.

## Stable Plugin-Author API

Plugin authors should import from the package root:

```python
from magazine_core_plugin_sdk import (
    ExternalLink,
    HostFetchResponse,
    HostRequestError,
    PluginContext,
    PluginManifest,
    SourceRecord,
    StdoutGuard,
    run_plugin,
)
```

The stable root API is:

- `SourceRecord`
- `ExternalLink`
- `PluginManifest`
- `PluginContext`
- `HostFetchResponse`
- `HostRequestError`
- `StdoutGuard`
- `protect_stdout`
- `run_plugin`
- `PROTOCOL_VERSION`
- `RECORD_SCHEMA_VERSION`
- `MAX_RECORD_BATCH`

`SourceRecord` and `ExternalLink` field names and `to_dict()` / `from_dict()`
serialization are frozen for `record_schema_version = 1`.

## Minimal Plugin

```python
from magazine_core_plugin_sdk import SourceRecord, run_plugin


def discover(context, params):
    context.send_record(
        SourceRecord(
            source_name="synthetic",
            source_url="synthetic://post/1",
            title="Synthetic One",
            brand_raw="Synthetic Brand",
        )
    )


run_plugin(
    source_name="synthetic",
    display_label="Synthetic",
    discover=discover,
)
```

The discover callback receives a `PluginContext` and the raw `discover` params.
It may return an integer record count. If it returns `None`, the runtime returns
the number of records sent through `context.send_record()`.

## Context Methods

Stable `PluginContext` methods:

- `send_record(record)`
- `log(level, message)`
- `host_fetch(url, method="GET", headers=None, timeout=30.0)`
- `known_source_urls(source_name, timeout=30.0)`
- `source_post_summary(source_name, source_url, timeout=30.0)`
- `last_seen_at(source_name, timeout=30.0)`
- `content_fingerprint(source_name, source_url, timeout=30.0)`
- `is_cancelled()`
- `wait_cancelled(timeout=None)`

`host_fetch()` raises `HostRequestError` when the host returns a JSON-RPC error.
Typed state helpers also raise `HostRequestError` for host-side errors and
`RuntimeError` for malformed host results.

Use `is_cancelled()` or `wait_cancelled()` for cancellation. The underlying
event object is an implementation detail and is not part of the frozen
plugin-author API.

## Validation Responsibility

`SourceRecord` and `ExternalLink` are serialization helpers. They do not perform
complete host validation. The host remains authoritative for v1 validation:

- required string fields
- absolute URI checks
- date shape checks
- `source_name == manifest.source_name`
- host-fetch policy and state-query operation validation

## Advanced / Conformance API

Low-level framing and JSON-RPC helpers remain available from submodules for
tests, conformance tooling, and custom runtimes:

```python
from magazine_core_plugin_sdk.framing import frame_bytes, read_json_frame
from magazine_core_plugin_sdk.protocol import Method, request, response_error
from magazine_core_plugin_sdk.runtime import PluginRuntime
```

These helpers are not the default plugin-author API. Changes to their behavior
still require conformance care because they are used by tests and tooling, but
new plugin code should prefer the stable root imports.

## Stdout Guard

Protocol stdout must contain framed messages only. `run_plugin()` installs the
SDK stdout guard so normal `print(...)`, native fd1 writes, and child-process
stdout are redirected away from the protocol pipe. Use `context.log()` for
plugin logs.
