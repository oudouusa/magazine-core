from __future__ import annotations

import base64
import io
import json
import subprocess
import sys
from pathlib import Path

import pytest

from magazine_core_plugin_sdk.framing import frame_bytes, read_json_frame
from magazine_core_plugin_sdk.models import SourceRecord
from magazine_core_plugin_sdk.protocol import (
    MAX_RECORD_BATCH,
    Method,
    notification,
    request,
    response,
    response_error,
)
from magazine_core_plugin_sdk.runtime import HostRequestError, PluginManifest, PluginRuntime


def _frames(*messages: dict) -> io.BytesIO:
    return io.BytesIO(b"".join(frame_bytes(message) for message in messages))


def test_runtime_initialize_discover_record_and_log() -> None:
    def discover(context, _params):
        context.log("info", "running")
        context.send_record(
            SourceRecord(
                source_name="synthetic",
                source_url="synthetic://post/1",
                title="Synthetic One",
                brand_raw="Synthetic Brand",
            )
        )

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-1", "limits": {}, "remaining_ms": 1000},
        ),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    writer.seek(0)
    init_response = read_json_frame(writer)
    log = read_json_frame(writer)
    record = read_json_frame(writer)
    discover_response = read_json_frame(writer)
    assert init_response["result"]["manifest"]["source_name"] == "synthetic"
    assert log["method"] == "log"
    assert record["method"] == "record"
    assert record["params"]["record"]["source_url"] == "synthetic://post/1"
    assert discover_response["result"] == {"records": 1}


def test_runtime_send_records_emits_batched_record_notifications() -> None:
    record_count = MAX_RECORD_BATCH * 2 + 5

    def discover(context, _params):
        context.send_records(
            {
                "source_name": "synthetic",
                "source_url": f"synthetic://post/{index}",
                "title": f"Synthetic {index}",
                "brand_raw": "Synthetic Brand",
            }
            for index in range(record_count)
        )

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-batch", "limits": {}, "remaining_ms": 1000},
        ),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    writer.seek(0)
    init_response = read_json_frame(writer)
    batches = [read_json_frame(writer), read_json_frame(writer), read_json_frame(writer)]
    discover_response = read_json_frame(writer)

    assert init_response["result"]["manifest"]["source_name"] == "synthetic"
    assert [len(batch["params"]["records"]) for batch in batches] == [
        MAX_RECORD_BATCH,
        MAX_RECORD_BATCH,
        5,
    ]
    for batch in batches:
        assert batch["method"] == "record"
        assert batch["params"]["request_id"] == "run-batch"
        assert "record" not in batch["params"]
    assert batches[0]["params"]["records"][0]["source_url"] == "synthetic://post/0"
    assert batches[2]["params"]["records"][4]["source_url"] == "synthetic://post/204"
    assert discover_response["result"] == {"records": record_count}


def test_runtime_send_records_empty_iterable_emits_no_record_frame() -> None:
    def discover(context, _params):
        context.send_records([])

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-empty-batch", "limits": {}, "remaining_ms": 1000},
        ),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    writer.seek(0)
    init_response = read_json_frame(writer)
    discover_response = read_json_frame(writer)
    assert init_response["result"]["manifest"]["source_name"] == "synthetic"
    assert discover_response["result"] == {"records": 0}


def test_runtime_cancel_watcher_marks_context_cancelled() -> None:
    observed = {}

    def discover(context, _params):
        observed["cancelled"] = context.wait_cancelled(1.0)
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-cancel", "limits": {}, "remaining_ms": 1000},
        ),
        notification(Method.CANCEL, {"request_id": "run-cancel"}),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {"cancelled": True}


def test_runtime_host_fetch_routes_response_to_discover_context() -> None:
    observed = {}

    def discover(context, _params):
        fetched = context.host_fetch(
            "https://example.test/feed",
            headers={"Accept": "application/json"},
        )
        observed["status"] = fetched.status
        observed["final_url"] = fetched.final_url
        observed["json"] = fetched.json()
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-fetch", "limits": {}, "remaining_ms": 1000},
        ),
        response(
            "p-1",
            {
                "id": "fetch-1",
                "status": 200,
                "final_url": "https://example.test/feed",
                "body_base64": base64.b64encode(b'{"ok": true}').decode("ascii"),
            },
        ),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic", allowed_domains=["example.test"]),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    writer.seek(0)
    read_json_frame(writer)
    fetch_request = read_json_frame(writer)
    discover_response = read_json_frame(writer)
    assert fetch_request["method"] == "fetch_request"
    assert fetch_request["id"] == "p-1"
    assert fetch_request["params"]["id"] == "fetch-1"
    assert fetch_request["params"]["request"]["url"] == "https://example.test/feed"
    assert observed == {
        "status": 200,
        "final_url": "https://example.test/feed",
        "json": {"ok": True},
    }
    assert discover_response["result"] == {"records": 0}


def test_runtime_host_fetch_error_raises_host_request_error() -> None:
    observed = {}

    def discover(context, _params):
        try:
            context.host_fetch("https://blocked.test/feed")
        except HostRequestError as exc:
            observed["code"] = exc.code
            observed["message"] = str(exc)
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-fetch", "limits": {}, "remaining_ms": 1000},
        ),
        response_error("p-1", -32010, "fetch blocked"),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {"code": -32010, "message": "fetch blocked"}


def test_runtime_known_source_urls_routes_response_to_discover_context() -> None:
    observed = {}

    def discover(context, _params):
        observed["known"] = context.known_source_urls("synthetic")
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-state", "limits": {}, "remaining_ms": 1000},
        ),
        response(
            "p-1",
            {
                "id": "state-1",
                "result": ["synthetic://post/existing"],
            },
        ),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    writer.seek(0)
    read_json_frame(writer)
    state_query = read_json_frame(writer)
    discover_response = read_json_frame(writer)
    assert state_query["method"] == "state_query"
    assert state_query["id"] == "p-1"
    assert state_query["params"] == {
        "id": "state-1",
        "op": "known_source_urls",
        "args": {"source_name": "synthetic"},
    }
    assert observed == {"known": ["synthetic://post/existing"]}
    assert discover_response["result"] == {"records": 0}


def test_runtime_known_source_urls_error_raises_host_request_error() -> None:
    observed = {}

    def discover(context, _params):
        try:
            context.known_source_urls("synthetic")
        except HostRequestError as exc:
            observed["code"] = exc.code
            observed["message"] = str(exc)
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-state", "limits": {}, "remaining_ms": 1000},
        ),
        response_error("p-1", -32020, "state unavailable"),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {"code": -32020, "message": "state unavailable"}


def test_runtime_known_source_urls_fails_closed_for_malformed_result() -> None:
    observed = {}

    def discover(context, _params):
        try:
            context.known_source_urls("synthetic")
        except RuntimeError as exc:
            observed["message"] = str(exc)
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-state", "limits": {}, "remaining_ms": 1000},
        ),
        response("p-1", {"id": "state-1", "result": ["ok", 123]}),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {"message": "known_source_urls result entries must be strings"}


def test_runtime_state_query_fails_closed_for_missing_inner_result() -> None:
    observed = {}

    def discover(context, _params):
        try:
            context.known_source_urls("synthetic")
        except RuntimeError as exc:
            observed["message"] = str(exc)
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-state", "limits": {}, "remaining_ms": 1000},
        ),
        response("p-1", {"id": "state-1"}),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {"message": "state_query result missing result field"}


@pytest.mark.parametrize(
    ("operation", "host_result", "expected_message"),
    [
        ("known_source_urls", {"not": "a-list"}, "known_source_urls result must be a list"),
        (
            "known_source_urls",
            ["synthetic://post/1", 123],
            "known_source_urls result entries must be strings",
        ),
        (
            "source_post_summary",
            ["not-an-object"],
            "source_post_summary result must be an object or null",
        ),
        ("last_seen_at", 123, "last_seen_at result must be a string or null"),
        (
            "content_fingerprint",
            {"not": "a-string"},
            "content_fingerprint result must be a string or null",
        ),
    ],
)
def test_runtime_typed_state_wrappers_fail_closed_for_malformed_results(
    operation: str,
    host_result,
    expected_message: str,
) -> None:
    observed = {}

    def discover(context, _params):
        try:
            if operation == "known_source_urls":
                context.known_source_urls("synthetic")
            elif operation == "source_post_summary":
                context.source_post_summary("synthetic", "synthetic://post/1")
            elif operation == "last_seen_at":
                context.last_seen_at("synthetic")
            elif operation == "content_fingerprint":
                context.content_fingerprint("synthetic", "synthetic://post/1")
            else:
                raise AssertionError(f"unhandled operation: {operation}")
        except RuntimeError as exc:
            observed["message"] = str(exc)
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-state", "limits": {}, "remaining_ms": 1000},
        ),
        response("p-1", {"id": "state-1", "result": host_result}),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {"message": expected_message}


def test_runtime_typed_state_wrappers_validate_nullable_results() -> None:
    observed = {}

    def discover(context, _params):
        observed["summary"] = context.source_post_summary("synthetic", "synthetic://post/1")
        observed["last_seen_at"] = context.last_seen_at("synthetic")
        observed["fingerprint"] = context.content_fingerprint("synthetic", "synthetic://post/1")
        return 0

    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-state", "limits": {}, "remaining_ms": 1000},
        ),
        response("p-1", {"id": "state-1", "result": {"exists": True, "title": "One"}}),
        response("p-2", {"id": "state-2", "result": None}),
        response("p-3", {"id": "state-3", "result": "fp-1"}),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest("synthetic", "Synthetic"),
        discover,
        reader=reader,
        writer=writer,
    )

    runtime.run()

    assert observed == {
        "summary": {"exists": True, "title": "One"},
        "last_seen_at": None,
        "fingerprint": "fp-1",
    }


def test_python_synthetic_plugin_ingests_via_rust_host(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[3]
    db_path = tmp_path / "synthetic.db"

    result = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "mh-cli",
            "--",
            "discover",
            str(db_path),
            "plugins.d",
            "example",
        ],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    output = json.loads(result.stdout)

    assert output["plugin_id"] == "example"
    assert output["source_name"] == "synthetic"
    assert output["discover_records"] == 1
    assert output["spooled_records"] == 1
    assert output["ingested_records"] == 1


def test_python_send_records_ingests_via_rust_host(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[3]
    plugin = tmp_path / "batch_plugin.py"
    plugin.write_text(
        """
from magazine_core_plugin_sdk import MAX_RECORD_BATCH, SourceRecord, run_plugin

def discover(context, _params):
    records = [
        SourceRecord(
            source_name="synthetic",
            source_url=f"synthetic://batch/{index}",
            title=f"Synthetic Batch {index}",
            brand_raw="Synthetic Brand",
        )
        for index in range(MAX_RECORD_BATCH + 1)
    ]
    context.send_records(records)

if __name__ == "__main__":
    run_plugin(
        source_name="synthetic",
        display_label="Batch",
        discover=discover,
    )
""".lstrip(),
        encoding="utf-8",
    )
    plugins_dir = tmp_path / "plugins.d"
    plugins_dir.mkdir()
    (plugins_dir / "batch.json").write_text(
        json.dumps(
            {
                "id": "batch",
                "argv": [sys.executable, str(plugin)],
                "env": {"PYTHONPATH": str(root / "sdk/python/src")},
            }
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "mh-cli",
            "--",
            "discover",
            str(tmp_path / "batch.db"),
            str(plugins_dir),
            "batch",
        ],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    output = json.loads(result.stdout)

    assert output["plugin_id"] == "batch"
    assert output["source_name"] == "synthetic"
    assert output["discover_records"] == MAX_RECORD_BATCH + 1
    assert output["spooled_records"] == MAX_RECORD_BATCH + 1
    assert output["ingested_records"] == MAX_RECORD_BATCH + 1


def test_python_plugin_record_count_mismatch_fails_closed(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[3]
    plugin = tmp_path / "mismatch_plugin.py"
    plugin.write_text(
        """
from magazine_core_plugin_sdk.models import SourceRecord
from magazine_core_plugin_sdk.runtime import run_plugin

def discover(context, _params):
    context.send_record(SourceRecord(
        source_name="synthetic",
        source_url="synthetic://mismatch/1",
        title="Mismatch",
        brand_raw="Synthetic Brand",
    ))
    return 2

if __name__ == "__main__":
    run_plugin(
        source_name="synthetic",
        display_label="Mismatch",
        discover=discover,
    )
""".lstrip(),
        encoding="utf-8",
    )
    plugins_dir = tmp_path / "plugins.d"
    plugins_dir.mkdir()
    (plugins_dir / "mismatch.json").write_text(
        json.dumps(
            {
                "id": "mismatch",
                "argv": [sys.executable, str(plugin)],
                "env": {"PYTHONPATH": str(root / "sdk/python/src")},
            }
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "mh-cli",
            "--",
            "discover",
            str(tmp_path / "mismatch.db"),
            str(plugins_dir),
            "mismatch",
        ],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )

    assert result.returncode != 0
    assert "did not match spooled records" in result.stderr
