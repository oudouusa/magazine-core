# Distribution channel decision

- Date: 2026-07-02
- Scope: C5 standalone distribution
- Contract impact: none

## Decision

For the beta line, GitHub Release artifacts are the authoritative distribution
channel for magazine-core.

Each release intended for standalone use should publish:

- a versioned Git tag,
- `magazine-core-mh-linux-x86_64.tar.gz`,
- the canonical `magazine_core_plugin_sdk-*.whl`,
- `sbom.cyclonedx.json`,
- `SHA256SUMS.txt`.

The standalone quickstart and downstream locks consume those artifacts by tag
and checksum. Source checkouts remain the contributor path, not the primary
end-user distribution channel.

## Non-Goals For This Slice

This slice does not add a new package registry, container image, installer, or
protocol surface. It only records the channel decision so the beta distribution
shape is explicit before version discipline and cold-start automation work.

## Deferred Channels

### `cargo install`

Not adopted now.

Benefits:

- familiar for Rust users,
- builds the CLI from source for platforms without prebuilt binaries.

Costs and blockers:

- requires a Rust toolchain and native build dependencies,
- needs crates.io publishing discipline for every workspace crate that becomes
  part of the install path,
- does not install the Python SDK wheel or synthetic plugin examples,
- can create a separate version surface from the GitHub Release unless release
  hardening verifies it.

Reconsider when the Rust package publishing path is version-locked and the
standalone quickstart can verify the resulting CLI without weakening the
artifact checksum story.

### PyPI

Not adopted now.

Benefits:

- natural installation channel for Python plugin authors,
- makes the SDK easier to consume outside this repository.

Costs and blockers:

- the SDK wheel alone is not a complete magazine-core installation because the
  Rust `mh` host binary is still required,
- publishing the SDK separately adds release coordination and rollback burden,
- the current next C5 item must first align tag, binary, wheel, and changelog
  version discipline.

Reconsider after version discipline is locked and the SDK package can be
published without implying that `pip install` alone installs the host.

### Docker

Not adopted now.

Benefits:

- reproducible host environment,
- possible future fit for a bundled local UI or tutorial environment.

Costs and blockers:

- plugins are trusted executables and need explicit local filesystem/process
  boundaries; a generic container image could make that trust boundary less
  obvious,
- users still need to mount databases and plugin manifests intentionally,
- registry maintenance, tags, SBOM, and image scanning would become another
  release surface.

Reconsider when the admin/viewer UI shape is decided and there is a clear
container use case that preserves local trust-boundary documentation.

## Current User Path

For standalone beta users:

1. Open the GitHub Release for the target tag.
2. Download the binary tarball, SDK wheel, SBOM, and `SHA256SUMS.txt`.
3. Verify checksums.
4. Install the wheel into a local virtual environment.
5. Run `mh init-db`, synthetic plugin `discover`, and `mh inspect`.

The executable check is:

```bash
RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh
```

## Verification

Commands:

```bash
git diff --check
bash -n scripts/verify-standalone-quickstart.sh
RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

Results:

```text
git diff --check
  pass

bash -n scripts/verify-standalone-quickstart.sh
  pass

RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh
  pass
  checksum verification OK for all downloaded Release assets
  discover_records=1, spooled_records=1, ingested_records=1
  final inspect: source_posts=1, performers=1, covers=1, pages=1, external_links=1

cargo fmt --all -- --check
  pass

cargo clippy --workspace --all-targets --locked -- -D warnings
  pass

cargo test --workspace --locked
  first attempt failed while the root filesystem was full:
    rust-lld bus error, then no space left on device while writing cargo artifacts
  cleanup:
    cargo clean in the current distribution-channel worktree
    cargo clean in the previous standalone-quickstart worktree
    reclaimed about 1.7 GiB of generated build artifacts
  retry:
    CARGO_BUILD_JOBS=1 cargo test --workspace --locked
    pass, 64 passed

python3 -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
  pass, 35 passed

bash conformance/check_golden.sh
  pass, golden fixtures match the python oracle
```

## Residual Risks

- GitHub Release artifacts currently provide a prebuilt linux-x86_64 host only.
- Independent artifact signatures are not part of this decision.
- Adding PyPI, Docker, or `cargo install` later will require its own release
  hardening and version discipline checks.
