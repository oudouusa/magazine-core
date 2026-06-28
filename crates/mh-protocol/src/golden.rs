//! Pinned golden vectors (CONTRACT §9). Every field value is fixed so that
//! independent implementations produce byte-identical frames; the fixtures in
//! `golden/*.hex` are the cross-implementation conformance oracle.

use serde_json::{json, Value};

/// The canonical messages with fully-pinned field values.
/// Returned as `(fixture_name, message)` pairs.
pub fn messages() -> Vec<(&'static str, Value)> {
    vec![
        (
            "initialize",
            json!({
                "jsonrpc": "2.0",
                "id": "h-1",
                "method": "initialize",
                "params": {"protocol_version": 1, "host_version": "golden-host"}
            }),
        ),
        (
            "record",
            json!({
                "jsonrpc": "2.0",
                "method": "record",
                "params": {
                    "request_id": "run-1",
                    "record": {
                        "source_name": "golden",
                        "source_url": "golden://1",
                        "title": "Golden One",
                        "brand_raw": "Golden Brand",
                        "brand_normalized": "golden brand",
                        "normalizer_id": "golden",
                        "normalizer_version": "1",
                        "performers_raw": ["P1", "P2"],
                        "cover_urls": ["golden://c1"],
                        "page_urls": ["golden://page/1"],
                        "issue_no": "No.1",
                        "external_links": [
                            {
                                "url": "https://example.test/p1",
                                "provider": "example",
                                "label": null,
                                "kind": "retail",
                                "external_id": "X1",
                                "metadata": {}
                            }
                        ],
                        "release_date": "2026-06-01",
                        "post_date": null,
                        "extra": {}
                    }
                }
            }),
        ),
        (
            "fetch_request",
            json!({
                "jsonrpc": "2.0",
                "id": "p-1",
                "method": "fetch_request",
                "params": {
                    "id": "p-2",
                    "request": {"url": "http://127.0.0.1:8080/p1", "method": "GET", "headers": {}}
                }
            }),
        ),
        (
            "state_query",
            json!({
                "jsonrpc": "2.0",
                "id": "p-3",
                "method": "state_query",
                "params": {
                    "id": "state-1",
                    "op": "known_source_urls",
                    "args": {"source_name": "golden"}
                }
            }),
        ),
    ]
}

/// Hex of the framed bytes for a message (length prefix + canonical payload).
/// Golden messages are tiny and always under [`crate::MAX_FRAME`].
pub fn frame_hex(value: &Value) -> String {
    crate::framing::frame_bytes(value)
        .expect("golden vectors are within MAX_FRAME")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
