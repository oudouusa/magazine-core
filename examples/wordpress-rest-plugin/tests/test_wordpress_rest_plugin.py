from __future__ import annotations

import base64
import io
import json
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import pytest

from magazine_core_plugin_sdk.framing import frame_bytes, read_json_frame
from magazine_core_plugin_sdk.protocol import Method, request, response
from magazine_core_plugin_sdk.runtime import PluginManifest, PluginRuntime

PLUGIN_DIR = Path(__file__).resolve().parents[1]
if str(PLUGIN_DIR) not in sys.path:
    sys.path.insert(0, str(PLUGIN_DIR))

import wordpress_rest_plugin as plugin


def _profile(**overrides):
    raw = {
        "profile_version": 1,
        "source_name": "my_wordpress",
        "display_label": "My WordPress",
        "base_url": "https://example.invalid",
        "allowed_domains": ["example.invalid"],
        "endpoint": "/wp-json/wp/v2/posts",
        "category_ids": [10, 20],
        "default_per_page": 20,
        "default_max_pages": 1,
        "default_max_records": 20,
        "brand_raw": "Synthetic WordPress",
    }
    raw.update(overrides)
    return plugin.profile_from_mapping(raw)


def _fixture_posts():
    return json.loads((PLUGIN_DIR / "tests/fixture-posts.json").read_text(encoding="utf-8"))


def _frames(*messages: dict) -> io.BytesIO:
    return io.BytesIO(b"".join(frame_bytes(message) for message in messages))


def test_fixture_posts_map_to_source_records() -> None:
    records = [
        plugin.record_from_post(_profile(), post)
        for post in _fixture_posts()
    ]

    assert [record.title for record in records] == [
        "Synthetic & One",
        "Synthetic Two",
        "Synthetic Three",
    ]
    assert records[0].source_name == "my_wordpress"
    assert records[0].source_url == "https://example.invalid/posts/synthetic-one"
    assert records[0].post_date == "2026-01-02"
    assert records[0].brand_raw == "Synthetic WordPress"
    assert records[0].performers_raw == []
    assert records[0].page_urls == []
    assert records[0].cover_urls == ["https://example.invalid/media/synthetic-one.jpg"]
    assert records[0].extra == {
        "wordpress_id": 101,
        "categories": [10, 20],
        "tags": [30],
        "slug": "synthetic-one",
    }
    assert records[1].cover_urls == []
    assert records[2].cover_urls == []


def test_runtime_uses_host_fetch_and_manifest_allowed_domains() -> None:
    profile = _profile(category_ids=[])
    reader = _frames(
        request("h-1", Method.INITIALIZE, {"protocol_version": 1, "host_version": "test"}),
        request(
            "h-2",
            Method.DISCOVER,
            {"request_id": "run-wp", "limits": {"max_records": 1}, "remaining_ms": 1000},
        ),
        response(
            "p-1",
            {
                "id": "fetch-1",
                "status": 200,
                "final_url": "https://example.invalid/wp-json/wp/v2/posts?per_page=20&page=1",
                "body_base64": base64.b64encode(
                    json.dumps(_fixture_posts()).encode("utf-8")
                ).decode("ascii"),
            },
        ),
    )
    writer = io.BytesIO()
    runtime = PluginRuntime(
        PluginManifest(
            profile.source_name,
            profile.display_label,
            allowed_domains=profile.allowed_domains,
            capabilities=["discover", "host_fetch"],
        ),
        plugin._discover_callback(profile),
        reader=reader,
        writer=writer,
    )

    runtime.run()

    writer.seek(0)
    init_response = read_json_frame(writer)
    fetch_request = read_json_frame(writer)
    record = read_json_frame(writer)
    discover_response = read_json_frame(writer)
    assert init_response["result"]["manifest"]["allowed_domains"] == ["example.invalid"]
    assert fetch_request["method"] == "fetch_request"
    assert fetch_request["params"]["request"] == {
        "url": "https://example.invalid/wp-json/wp/v2/posts?per_page=20&page=1",
        "method": "GET",
        "headers": {},
    }
    assert record["method"] == "record"
    assert record["params"]["records"][0]["source_url"] == (
        "https://example.invalid/posts/synthetic-one"
    )
    assert discover_response["result"] == {"records": 1}


def test_profile_rejects_empty_allowed_domains() -> None:
    with pytest.raises(plugin.ProfileError, match="allowed_domains"):
        _profile(allowed_domains=[])


def test_profile_rejects_base_url_host_outside_allowed_domains() -> None:
    with pytest.raises(plugin.ProfileError, match="base_url host"):
        _profile(
            base_url="https://blocked.example.invalid",
            allowed_domains=["other.example.invalid"],
        )


def test_discover_records_uses_host_limits_and_timeout() -> None:
    profile = _profile(default_max_pages=10, default_max_records=10, default_per_page=99)
    fetched: list[tuple[str, float]] = []

    def fetch_json(url: str, timeout: float):
        fetched.append((url, timeout))
        post = dict(_fixture_posts()[len(fetched) - 1])
        post["id"] = 200 + len(fetched)
        post["link"] = f"https://example.invalid/posts/page-{len(fetched)}"
        return [post]

    records = plugin.discover_records(
        profile,
        {
            "limits": {"max_pages": 2, "max_records": 5, "per_page": 7},
            "remaining_ms": 2500,
        },
        fetch_json,
    )

    assert len(records) == 2
    assert [record.source_url for record in records] == [
        "https://example.invalid/posts/page-1",
        "https://example.invalid/posts/page-2",
    ]
    assert [timeout for _, timeout in fetched] == [2.5, 2.5]
    parsed = [urlparse(url) for url, _ in fetched]
    assert [item.scheme for item in parsed] == ["https", "https"]
    assert [item.netloc for item in parsed] == ["example.invalid", "example.invalid"]
    assert [parse_qs(item.query)["page"] for item in parsed] == [["1"], ["2"]]
    assert [parse_qs(item.query)["per_page"] for item in parsed] == [["7"], ["7"]]
    assert [parse_qs(item.query)["categories"] for item in parsed] == [["10,20"], ["10,20"]]


def test_discover_records_stops_at_host_max_records() -> None:
    fetched: list[str] = []

    def fetch_json(url: str, _timeout: float):
        fetched.append(url)
        return _fixture_posts()

    records = plugin.discover_records(
        _profile(default_max_records=20),
        {
            "limits": {"max_pages": 3, "max_records": 1, "per_page": 20},
            "remaining_ms": 1000,
        },
        fetch_json,
    )

    assert len(records) == 1
    assert records[0].source_url == "https://example.invalid/posts/synthetic-one"
    assert len(fetched) == 1
