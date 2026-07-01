"""Produce the pinned golden frames (CONTRACT §9) as hex, one per line, in the
same order as mh_protocol::golden::messages(). Canonical JSON = sorted keys, no
whitespace, UTF-8. This is the independent (non-Rust) oracle."""
import json
import struct
import sys

MESSAGES = [
    ("initialize", {"jsonrpc": "2.0", "id": "h-1", "method": "initialize",
                    "params": {"protocol_version": 1, "host_version": "golden-host"}}),
    ("discover", {"jsonrpc": "2.0", "id": "h-2", "method": "discover", "params": {
        "request_id": "run-1",
        "limits": {"max_pages": 2, "max_records": 3, "per_page": 16},
        "remaining_ms": 1000}}),
    ("record", {"jsonrpc": "2.0", "method": "record", "params": {
        "request_id": "run-1",
        "record": {
            "source_name": "golden", "source_url": "golden://1", "title": "Golden One",
            "brand_raw": "Golden Brand", "brand_normalized": "golden brand",
            "normalizer_id": "golden", "normalizer_version": "1",
            "performers_raw": ["P1", "P2"], "cover_urls": ["golden://c1"], "page_urls": ["golden://page/1"], "issue_no": "No.1",
            "external_links": [{"url": "https://example.test/p1", "provider": "example", "label": None, "kind": "retail", "external_id": "X1", "metadata": {}}],
            "release_date": "2026-06-01", "post_date": None, "extra": {}}}}),
    ("record_batch", {"jsonrpc": "2.0", "method": "record", "params": {
        "request_id": "run-1",
        "records": [
            {
                "source_name": "golden", "source_url": "golden://1", "title": "Golden One",
                "brand_raw": "Golden Brand", "brand_normalized": "golden brand",
                "normalizer_id": "golden", "normalizer_version": "1",
                "performers_raw": ["P1", "P2"], "cover_urls": ["golden://c1"], "page_urls": ["golden://page/1"], "issue_no": "No.1",
                "external_links": [{"url": "https://example.test/p1", "provider": "example", "label": None, "kind": "retail", "external_id": "X1", "metadata": {}}],
                "release_date": "2026-06-01", "post_date": None, "extra": {},
            },
            {
                "source_name": "golden", "source_url": "golden://2", "title": "Golden Two",
                "brand_raw": "Golden Brand", "brand_normalized": "golden brand",
                "normalizer_id": "golden", "normalizer_version": "1",
                "performers_raw": ["P1", "P2"], "cover_urls": ["golden://c1"], "page_urls": ["golden://page/1"], "issue_no": "No.1",
                "external_links": [{"url": "https://example.test/p1", "provider": "example", "label": None, "kind": "retail", "external_id": "X1", "metadata": {}}],
                "release_date": "2026-06-01", "post_date": None, "extra": {},
            },
        ]}}),
    ("fetch_request", {"jsonrpc": "2.0", "id": "p-1", "method": "fetch_request", "params": {
        "id": "p-2", "request": {"url": "http://127.0.0.1:8080/p1", "method": "GET", "headers": {}}}}),
    ("state_query", {"jsonrpc": "2.0", "id": "p-3", "method": "state_query", "params": {
        "id": "state-1", "op": "known_source_urls", "args": {"source_name": "golden"}}}),
]


def frame_hex(value):
    payload = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return (struct.pack(">I", len(payload)) + payload).hex()


if __name__ == "__main__":
    outdir = sys.argv[1] if len(sys.argv) > 1 else None
    for name, msg in MESSAGES:
        h = frame_hex(msg)
        if outdir:
            with open(f"{outdir}/{name}.hex", "w") as f:
                f.write(h + "\n")
        else:
            print(f"{name} {h}")
