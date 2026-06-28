from __future__ import annotations

import base64

import magazine_core_plugin_sdk as sdk
from magazine_core_plugin_sdk.framing import FrameTooLarge, frame_bytes
from magazine_core_plugin_sdk.protocol import Method, request


def test_package_root_exports_stable_plugin_author_api() -> None:
    assert sdk.__all__ == [
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
    for name in sdk.__all__:
        assert getattr(sdk, name) is not None


def test_root_imports_are_sufficient_for_plugin_author_record_shape() -> None:
    link = sdk.ExternalLink(
        url="https://example.test/retail/1",
        provider="example",
        label="Example Retail",
        kind="retail",
        external_id="retail-1",
        metadata={"source": "synthetic"},
    )
    record = sdk.SourceRecord(
        source_name="synthetic",
        source_url="synthetic://post/1",
        title="Synthetic One",
        brand_raw="Synthetic Brand",
        brand_normalized="synthetic brand",
        performers_raw=["Alice Example"],
        cover_urls=["https://example.test/cover.jpg"],
        page_urls=["https://example.test/page/1"],
        issue_no="1",
        external_links=[link],
        release_date="2026-06-25",
        post_date="2026-06-26",
        extra={"fixture": "public-api"},
        normalizer_id="synthetic-normalizer",
        normalizer_version="2026.6",
    )

    round_tripped = sdk.SourceRecord.from_dict(record.to_dict())

    assert round_tripped == record
    assert round_tripped.external_links[0] == link


def test_plugin_context_public_cancellation_api_is_method_based() -> None:
    assert hasattr(sdk.PluginContext, "is_cancelled")
    assert hasattr(sdk.PluginContext, "wait_cancelled")
    assert not hasattr(sdk.PluginContext, "cancelled")


def test_host_fetch_response_and_error_public_attributes() -> None:
    response = sdk.HostFetchResponse(
        id="fetch-1",
        status=200,
        final_url="https://example.test/feed",
        body_base64=base64.b64encode(b'{"ok": true}').decode("ascii"),
    )
    error = sdk.HostRequestError(-32010, "fetch blocked", {"url": "https://blocked.test"})

    assert response.body == b'{"ok": true}'
    assert response.text == '{"ok": true}'
    assert response.json() == {"ok": True}
    assert error.code == -32010
    assert error.data == {"url": "https://blocked.test"}
    assert str(error) == "fetch blocked"


def test_advanced_protocol_and_framing_helpers_remain_submodule_api() -> None:
    frame = frame_bytes(request("p-1", Method.LOG, {"level": "info", "message": "ok"}))

    assert "frame_bytes" not in sdk.__all__
    assert "Method" not in sdk.__all__
    assert "PluginRuntime" not in sdk.__all__
    assert not hasattr(sdk, "frame_bytes")
    assert not hasattr(sdk, "Method")
    assert not hasattr(sdk, "PluginRuntime")
    assert frame.startswith(b"\x00\x00\x00")
    assert issubclass(FrameTooLarge, Exception)
