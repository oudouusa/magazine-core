# Standalone quickstart

This quickstart verifies the standalone distribution shape: public
GitHub Release artifacts alone can run `install -> init-db -> discover ->
inspect` with a synthetic plugin. For releases whose `./mh --help` advertises
`mh ui`, the same artifact-only flow can also browse the scratch DB locally, and
the automated checker proves guarded `--manage` discover from the released
binary. It does not require a source checkout after the assets are downloaded.

Current published prebuilt host coverage is linux-x86_64. Other platforms can
build from source until a matching host binary is published.

The distribution source of truth is the GitHub Release for the selected
tag: host binary tarball, Python SDK wheel, CycloneDX SBOM, and
`SHA256SUMS.txt`. `cargo install`, PyPI, and Docker images are not stable
distribution channels yet; see
`docs/development/distribution-channel-decision-2026-07-02.md`.

## Fast verification

From a magazine-core checkout, run the release-consuming checker:

```bash
RELEASE_TAG=1.3.0 VERIFY_UI=1 VERIFY_TRUSTED_UI=1 \
  bash scripts/verify-standalone-quickstart.sh
```

For the current release, require both local UI binaries:
`RELEASE_TAG=1.3.0 VERIFY_UI=1 VERIFY_TRUSTED_UI=1`.

The checker creates a temporary directory, downloads the Release assets,
verifies `SHA256SUMS.txt`, installs the attached SDK wheel into a fresh virtual
environment with `--no-index`, writes a synthetic plugin and manifest in the
temporary directory, and asserts that `discover` ingests one record. When the
release binary advertises `mh ui`, the checker starts the local UI in read-only
mode and management mode, then verifies DB browsing, method rejection, token
guarding, bounded discover rejection, and a guarded management discover.

The checker intentionally avoids `plugins.d/`, `examples/`, and
`sdk/python/src` from this repository.

## Manual artifact-only flow

Create an empty workspace:

```bash
mkdir magazine-core-quickstart
cd magazine-core-quickstart
```

Download the Release assets:

```bash
release_tag=1.3.0
base_url="https://github.com/oudouusa/magazine-core/releases/download/${release_tag}"

curl -fsSLO "${base_url}/SHA256SUMS.txt"
curl -fsSLO "${base_url}/magazine-core-mh-linux-x86_64.tar.gz"
curl -fsSLO "${base_url}/magazine_core_plugin_sdk-1.3.0-py3-none-any.whl"
curl -fsSLO "${base_url}/sbom.cyclonedx.json"
sha256sum -c SHA256SUMS.txt
```

Extract the host binary and install the SDK wheel locally:

```bash
tar -xzf magazine-core-mh-linux-x86_64.tar.gz
python3 -m venv .venv
.venv/bin/python -m pip install --no-index --no-deps \
  magazine_core_plugin_sdk-1.3.0-py3-none-any.whl
./mh --help
./mh-ui-ext --help
```

Write a synthetic plugin:

```bash
cat > quickstart_plugin.py <<'PY'
from __future__ import annotations

from magazine_core_plugin_sdk import ExternalLink, SourceRecord, run_plugin


def discover(context, _params):
    context.log("info", "standalone quickstart discover")
    context.send_record(
        SourceRecord(
            source_name="quickstart",
            source_url="synthetic://quickstart/1",
            title="Standalone Quickstart One",
            brand_raw="Synthetic Brand",
            performers_raw=["Alice Example"],
            cover_urls=["https://example.invalid/cover.jpg"],
            page_urls=["https://example.invalid/page/1"],
            issue_no="1",
            external_links=[
                ExternalLink(
                    url="https://example.invalid/retail/1",
                    provider="example",
                    label="Example Retail",
                    kind="retail",
                    external_id="retail-1",
                    metadata={"fixture": "standalone-quickstart"},
                )
            ],
            release_date="2026-07-02",
            extra={"fixture": "standalone-quickstart"},
        )
    )


if __name__ == "__main__":
    run_plugin(
        source_name="quickstart",
        display_label="Standalone Quickstart",
        discover=discover,
        allowed_domains=["example.invalid"],
        capabilities=["discover"],
    )
PY
```

Create a plugin manifest that uses the virtual environment Python directly:

```bash
mkdir -p plugins.d
python3 - <<'PY'
import json
from pathlib import Path

work_dir = Path.cwd()
manifest = {
    "id": "quickstart",
    "argv": [str(work_dir / ".venv/bin/python"), "quickstart_plugin.py"],
    "working_dir": str(work_dir),
}
Path("plugins.d/quickstart.json").write_text(
    json.dumps(manifest, indent=2) + "\n",
    encoding="utf-8",
)
PY
```

Initialize a database, run discovery, and inspect the result:

```bash
./mh init-db scratch.db
./mh discover scratch.db ./plugins.d quickstart \
  --max-pages 1 \
  --per-page 30 \
  --max-records 30 \
  --timeout-seconds 60
./mh inspect scratch.db
```

The final `inspect` output should include one source post, one performer, one
cover, one page, and one external link:

```json
{
  "schema_version": 1,
  "source_posts": 1,
  "performers": 1,
  "covers": 1,
  "pages": 1,
  "external_links": 1
}
```

If `./mh --help` advertises `mh ui`, browse the scratch DB with the bundled
local UI:

```bash
./mh ui --db scratch.db --plugins-dir ./plugins.d --port 8765
```

Open `http://127.0.0.1:8765` from the same machine. The default UI is read-only.
For an opt-in local management session, restart it with:

```bash
./mh ui --db scratch.db --plugins-dir ./plugins.d --port 8765 --manage
```

The browser UI uses a per-process local token for management requests. The
automated checker covers the token-guarded `init-db` / bounded `discover` /
cancel surface so the manual quickstart does not require copying tokens.

## Trust boundary

Plugins are trusted executable code. The host isolates lifecycle and crashes,
but it is not a sandbox. The UI binds to `127.0.0.1`; do not expose it through a
public interface, tunnel, reverse proxy, or shared remote host. This quickstart
uses only a synthetic local plugin and does not fetch external pages.
