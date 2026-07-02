# Standalone cold-start evidence

- Date: 2026-07-02
- Scope: C5 standalone distribution cold-start check
- Contract impact: none

## Decision

Release-tag hardening dispatches now run a Linux standalone cold-start job after
the Linux assets are published to the target GitHub Release. The job runs
`scripts/verify-standalone-quickstart.sh` with `RELEASE_TAG=<tag>` from a fresh
GitHub-hosted runner. The checker downloads the public Release artifacts into a
temporary directory, verifies `SHA256SUMS.txt`, installs the wheel with
`--no-index --no-deps`, writes a synthetic plugin outside repository paths, and
proves `init-db -> discover -> inspect`.

Hardening-only workflow dispatches without `release_tag` keep their existing
behavior and skip cold-start because there is no target public Release URL to
verify.

## Invariants

- No `protocol_version` change.
- No `record_schema_version` change.
- No Python SDK root API change.
- No canonical DB schema change.
- No new distribution channel.
- The quickstart checker still consumes only public Release artifacts for the
  binary, wheel, SBOM, and checksum file.

## Failure Mode Prevented

Before this slice, release hardening could publish assets without a CI-level
new-user check proving that public artifacts alone can run the standalone
quickstart. The new job makes the publish path fail if the uploaded binary,
wheel, checksum file, or quickstart contract is incomplete from a clean Linux
runner.

## Verification

Local verification:

```bash
bash -n scripts/verify-standalone-quickstart.sh
bash -n scripts/release-hardening.sh
bash -n scripts/upload-release-assets.sh
shellcheck scripts/verify-standalone-quickstart.sh scripts/release-hardening.sh scripts/upload-release-assets.sh
python3 scripts/check-version-discipline.py --root . --json
RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked
python3 -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

Results:

- `bash -n` for quickstart, release hardening, and release upload scripts: pass.
- `shellcheck scripts/verify-standalone-quickstart.sh scripts/release-hardening.sh scripts/upload-release-assets.sh`: pass.
- `check-version-discipline.py --root . --json`: pass.
- `RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh`: pass against public GitHub Release assets, with final inspect counts of one source post, performer, cover, page, and external link.
- `git diff --check`: pass.
- `cargo fmt`, `cargo clippy`, and `cargo test --workspace --locked`: pass, 64 Rust tests.
- Python SDK editable install and `pytest sdk/python/tests`: pass, 35 tests.
- `conformance/check_golden.sh`: pass.

## Adopt / Revert

Adopt if the existing `0.1.0-beta.2` standalone quickstart still passes and the
normal CI gates remain green.

Revert criteria:

- release-tag workflow dispatch can no longer publish assets before cold-start;
- hardening-only dispatches without `release_tag` unexpectedly run cold-start;
- the quickstart checker starts depending on repository plugins or SDK source
  paths instead of public artifacts.

## Residual Risks

- The automated cold-start job is Linux-only until additional prebuilt host
  binaries are published.
- `actionlint` may be unavailable locally; workflow YAML is also validated by
  GitHub when the PR is opened and the workflow is dispatched.
