from __future__ import annotations

import os
import subprocess
import sys

from magazine_core_plugin_sdk import ExternalLink, SourceRecord, run_plugin


def discover(context, _params):
    if os.environ.get("MH_SYNTHETIC_NOISE") == "1":
        print("synthetic print noise")
        os.write(1, b"synthetic native stdout noise\n")
        subprocess.run(
            [sys.executable, "-c", "print('synthetic child stdout noise')"],
            check=True,
        )

    context.log("info", "synthetic discover")
    context.send_record(
        SourceRecord(
            source_name="synthetic",
            source_url="synthetic://python/1",
            title="Synthetic Python One",
            brand_raw="Synthetic Brand",
            performers_raw=["Alice Example"],
            cover_urls=["https://example.test/cover.jpg"],
            page_urls=["https://example.test/page/1"],
            issue_no="1",
            external_links=[
                ExternalLink(
                    url="https://example.test/retail/1",
                    provider="example",
                    label="Example Retail",
                    kind="retail",
                    external_id="retail-1",
                    metadata={"source": "synthetic"},
                )
            ],
            release_date="2026-06-25",
            post_date=None,
            brand_normalized=None,
            normalizer_id=None,
            normalizer_version=None,
            extra={"fixture": "python-synthetic"},
        )
    )


if __name__ == "__main__":
    run_plugin(
        source_name="synthetic",
        display_label="Synthetic Python",
        discover=discover,
        allowed_domains=["example.test"],
        capabilities=["discover"],
    )
