from __future__ import annotations

import struct
from pathlib import Path

from magazine_core_plugin_sdk.framing import frame_bytes
from magazine_core_plugin_sdk.models import ExternalLink, SourceRecord
from magazine_core_plugin_sdk.protocol import (
    MAX_RECORD_BATCH,
    PROTOCOL_VERSION,
    RECORD_SCHEMA_VERSION,
    Method,
    PluginIdGenerator,
    notification,
    request,
    response_err,
    response_ok,
)


def test_protocol_constants_and_json_rpc_helpers() -> None:
    assert PROTOCOL_VERSION == 1
    assert RECORD_SCHEMA_VERSION == 1
    assert MAX_RECORD_BATCH == 100

    assert request("h-1", Method.INITIALIZE, {"protocol_version": 1}) == {
        "jsonrpc": "2.0",
        "id": "h-1",
        "method": "initialize",
        "params": {"protocol_version": 1},
    }
    assert notification(Method.RECORD, {"request_id": "run-1"}) == {
        "jsonrpc": "2.0",
        "method": "record",
        "params": {"request_id": "run-1"},
    }
    assert response_ok("h-1", {"ok": True}) == {
        "jsonrpc": "2.0",
        "id": "h-1",
        "result": {"ok": True},
    }
    assert response_err("h-1", -32601, "unknown method") == {
        "jsonrpc": "2.0",
        "id": "h-1",
        "error": {"code": -32601, "message": "unknown method"},
    }


def test_plugin_id_generator_uses_p_namespace() -> None:
    ids = PluginIdGenerator()

    assert ids.next_id() == "p-1"
    assert next(ids) == "p-2"
    assert ids.request(Method.FETCH_REQUEST, {"request": {}})["id"] == "p-3"


def test_source_record_defaults_serialize_to_protocol_shape() -> None:
    record = SourceRecord(
        source_name="synthetic",
        source_url="synthetic://post/1",
        title="Title",
        brand_raw="Brand",
    )

    assert record.to_dict() == {
        "source_name": "synthetic",
        "source_url": "synthetic://post/1",
        "title": "Title",
        "brand_raw": "Brand",
        "performers_raw": [],
        "cover_urls": [],
        "page_urls": [],
        "issue_no": None,
        "external_links": [],
        "release_date": None,
        "post_date": None,
        "brand_normalized": None,
        "normalizer_id": None,
        "normalizer_version": None,
        "extra": {},
    }


def test_source_record_with_external_links_matches_golden_record_frame() -> None:
    record = _golden_record()
    message = notification(
        Method.RECORD,
        {"request_id": "run-1", "record": record},
    )

    frame = frame_bytes(message)
    payload = (
        b'{"jsonrpc":"2.0","method":"record","params":{"record":{"brand_normalized":"golden brand",'
        b'"brand_raw":"Golden Brand","cover_urls":["golden://c1"],"external_links":[{"external_id":"X1",'
        b'"kind":"retail","label":null,"metadata":{},"provider":"example","url":"https://example.test/p1"}],'
        b'"extra":{},"issue_no":"No.1","normalizer_id":"golden","normalizer_version":"1","page_urls":["golden://page/1"],'
        b'"performers_raw":["P1","P2"],"post_date":null,"release_date":"2026-06-01","source_name":"golden",'
        b'"source_url":"golden://1","title":"Golden One"},"request_id":"run-1"}}'
    )
    assert frame == struct.pack(">I", len(payload)) + payload


def test_sdk_frames_match_pinned_golden_fixtures() -> None:
    root = Path(__file__).resolve().parents[3]
    messages = {
        "initialize": request(
            "h-1",
            Method.INITIALIZE,
            {"protocol_version": 1, "host_version": "golden-host"},
        ),
        "record": notification(
            Method.RECORD,
            {"request_id": "run-1", "record": _golden_record()},
        ),
        "fetch_request": request(
            "p-1",
            Method.FETCH_REQUEST,
            {
                "id": "p-2",
                "request": {
                    "url": "http://127.0.0.1:8080/p1",
                    "method": "GET",
                    "headers": {},
                },
            },
        ),
        "state_query": request(
            "p-3",
            Method.STATE_QUERY,
            {
                "id": "state-1",
                "op": "known_source_urls",
                "args": {"source_name": "golden"},
            },
        ),
    }

    for name, message in messages.items():
        expected = (
            root / "crates" / "mh-protocol" / "golden" / f"{name}.hex"
        ).read_text(encoding="utf-8").strip()
        assert frame_bytes(message).hex() == expected


def _golden_record() -> SourceRecord:
    record = SourceRecord(
        source_name="golden",
        source_url="golden://1",
        title="Golden One",
        brand_raw="Golden Brand",
        brand_normalized="golden brand",
        normalizer_id="golden",
        normalizer_version="1",
        performers_raw=["P1", "P2"],
        cover_urls=["golden://c1"],
        page_urls=["golden://page/1"],
        issue_no="No.1",
        external_links=[
            ExternalLink(
                url="https://example.test/p1",
                provider="example",
                label=None,
                kind="retail",
                external_id="X1",
                metadata={},
            )
        ],
        release_date="2026-06-01",
        post_date=None,
        extra={},
    )
    return record
