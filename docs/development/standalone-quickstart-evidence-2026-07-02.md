# Standalone quickstart evidence

- Date: 2026-07-02
- Repo: `magazine-core`
- Scope: C5 standalone distribution first slice
- Contract impact: none

## Decision

The first standalone distribution slice is a release-consuming quickstart
checker and public docs. It does not create a new distribution channel and does
not change protocol, SDK root API, host runtime behavior, or canonical schema.

GitHub Release artifacts remain the source of truth for this slice. The checker
downloads the published linux-x86_64 host binary, canonical Python SDK wheel,
CycloneDX SBOM, and `SHA256SUMS.txt`, verifies checksums, and then proves
`install -> init-db -> synthetic discover -> inspect` in a temporary directory.

## Invariants

- `protocol_version = 1` remains unchanged.
- `record_schema_version = 1` remains unchanged.
- Python SDK root API remains unchanged.
- Canonical SQLite schema remains unchanged.
- All examples are synthetic and public-safe.
- The checker does not read repo `plugins.d/`, `examples/`, or `sdk/python/src`.

## Failure Mode Prevented

Before this slice, the release hardening smoke used source checkout paths and
the release notes referenced `./plugins.d` without proving where it came from
in an artifact-only install. That could make a release look standalone while a
new user still needed the repo checkout to run `discover`.

This slice makes that boundary executable:

- download assets from the GitHub Release,
- verify `SHA256SUMS.txt`,
- install the downloaded wheel with `--no-index --no-deps`,
- generate a synthetic plugin and manifest in the temporary workspace,
- run `mh init-db`, `mh discover`, and `mh inspect`,
- assert one record was discovered, spooled, ingested, and visible in inspect.

## Verification

Commands:

```bash
bash -n scripts/verify-standalone-quickstart.sh
RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

Results:

```text
bash -n scripts/verify-standalone-quickstart.sh
  pass

RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh
  pass
  downloaded SHA256SUMS.txt, magazine-core-mh-linux-x86_64.tar.gz,
  magazine_core_plugin_sdk-0.1.0-py3-none-any.whl, and sbom.cyclonedx.json
  checksum verification: OK for all assets
  pip install: --no-index --no-deps magazine_core_plugin_sdk-0.1.0-py3-none-any.whl
  discover output: discover_records=1, spooled_records=1, ingested_records=1
  final inspect: source_posts=1, performers=1, covers=1, pages=1, external_links=1

git diff --check
  pass

cargo fmt --all -- --check
  pass

cargo clippy --workspace --all-targets --locked -- -D warnings
  pass

cargo test --workspace --locked
  pass, 64 passed

python3 -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
  pass, 35 passed

bash conformance/check_golden.sh
  pass, golden fixtures match the python oracle
```

Note: this host environment does not provide a `python` executable, so the SDK
venv command used `python3`. The resulting SDK pytest command used the created
`.venv/bin/python`.

## Residual Risk

- The prebuilt artifact-only quickstart is linux-x86_64 only until additional
  host binaries are published.
- The SDK wheel version remains `0.1.0` while the GitHub Release tag is
  `0.1.0-beta.2`; version discipline is the next C5 slice.
- The checker is networked and release-consuming, so it should be a release
  gate or explicit cold-start check rather than a default local unit test.
