# Release Artifact Automation Evidence — 2026-07-02

## Scope

This slice automates the durable Release asset publication step for downstream
artifact consumption. It keeps release creation and tag selection manual, but
lets the `Release hardening` workflow attach the Linux downstream-consumable
assets to an existing GitHub Release.

The workflow publishes:

- `magazine-core-mh-linux-x86_64.tar.gz`
- the canonical `magazine_core_plugin_sdk-*.whl`
- `sbom.cyclonedx.json`
- `SHA256SUMS.txt`

## Invariants

- `protocol_version = 1` and `record_schema_version = 1` remain unchanged.
- The Python SDK root API remains unchanged.
- The core repository remains public-safe and does not reference downstream
  consumers, private source names, real scraped data, credentials, or
  operational details.
- Release upload fails closed when the GitHub Release target commit does not
  match the checked-out commit.
- Secret values are not echoed by the upload helper.

## Failure Mode Prevented

Before this slice, release hardening produced expiring Actions artifacts and a
local `checksums.sha256` file, while the durable GitHub Release assets had to
be attached manually. That left downstream checksum locks dependent on a manual
step and made the public checksum asset name (`SHA256SUMS.txt`) implicit.

## Implementation

- `scripts/release-hardening.sh` now writes both `checksums.sha256` and the
  public Release checksum name `SHA256SUMS.txt`.
- `scripts/upload-release-assets.sh` validates the release tag, verifies the
  Linux binary, canonical wheel, SBOM, and checksum file, checks the Release
  target commit against the current checkout, and uploads with `gh release
  upload --clobber`.
- `.github/workflows/release-hardening.yml` accepts an optional `release_tag`.
  Hardening-only runs keep `contents: read`; publish runs add a separate
  `publish-release-assets` job with `contents: write`, download the Linux
  hardening artifact, and invoke the upload helper.
- `scripts/test-upload-release-assets.sh` covers dry-run asset selection,
  checksum alias creation, fake-`gh` non-dry upload command construction, and
  missing wheel failure.

## Verification

All commands passed:

```bash
bash -n scripts/release-hardening.sh
bash -n scripts/upload-release-assets.sh
bash -n scripts/test-upload-release-assets.sh
bash scripts/test-upload-release-assets.sh
shellcheck scripts/release-hardening.sh scripts/upload-release-assets.sh scripts/test-upload-release-assets.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
git diff --check
```

Development-mode full hardening also passed, using a shared Cargo target to
avoid duplicating build output in the low-disk worktree:

```bash
env ALLOW_DIRTY=1 CARGO_TARGET_DIR=<existing-build-target> bash scripts/release-hardening.sh
cd dist/release-hardening/artifacts && sha256sum -c SHA256SUMS.txt
```

The generated Linux artifact checksum file verified:

```text
./magazine-core-mh-linux-x86_64.tar.gz: OK
./magazine_core_plugin_sdk-0.1.0-py3-none-any.whl: OK
./sbom.cyclonedx.json: OK
```

`actionlint` was not installed in the local environment, so workflow syntax is
covered by review and the next GitHub workflow dispatch.

## Decision

Adopt this automation slice. It changes only release packaging/workflow
machinery and docs; no protocol, schema, SDK root API, or runtime contract
changes are made.

## Residual Risks

- A real GitHub Release upload is intentionally not performed by this PR. After
  merge, cut or select the next beta Release for the target commit, dispatch
  `Release hardening` with `release_tag`, and verify the uploaded checksums.
- Downstream release-mode checksum consumption remains a separate small
  downstream PR after durable Release assets exist for the locked commit.
