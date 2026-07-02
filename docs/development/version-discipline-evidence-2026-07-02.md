# Version discipline evidence

- Date: 2026-07-02
- Scope: C5 standalone distribution version discipline
- Contract impact: none

## Decision

Release hardening now has an explicit version discipline gate. The gate verifies
that all Cargo workspace package versions agree with the Python SDK wheel
version after PEP 440 pre-release normalization. When the release ref is a
version tag, it also requires the package version, tag, `CHANGELOG.md`, and
`docs/release/<tag>.md` to agree.

This is a fail-closed release gate for future tags. Existing beta releases keep
their historical notes; `0.1.0-beta.2` remains the release artifact consumed by
the standalone quickstart. Rerunning publish hardening for that historical tag
is intentionally blocked; a future version tag must align package metadata
before release hardening can publish assets.

## Invariants

- No `protocol_version` change.
- No `record_schema_version` change.
- No Python SDK root API change.
- No canonical DB schema change.
- No new distribution channel.

## Failure Mode Prevented

Before this slice, a release tag could diverge from Cargo package versions, the
Python SDK wheel version, changelog entries, or release notes while release
hardening still produced uploadable artifacts. The new checker makes those
release-tag mismatches blocking.

## Verification

Local verification:

```bash
python3 scripts/check-version-discipline.py --root . --json
python3 scripts/check-version-discipline.py --root . --release-ref main --json
python3 -m py_compile scripts/check-version-discipline.py
bash scripts/test-version-discipline.sh
bash -n scripts/release-hardening.sh
bash -n scripts/test-version-discipline.sh
shellcheck scripts/release-hardening.sh scripts/test-version-discipline.sh
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

Results:

- `check-version-discipline.py --root . --json`: pass, package version `0.1.0`.
- `check-version-discipline.py --root . --release-ref main --json`: pass with tag-specific checks skipped.
- `py_compile`, script syntax checks, and `test-version-discipline.sh`: pass.
- `shellcheck scripts/release-hardening.sh scripts/test-version-discipline.sh`: pass.
- `git diff --check`: pass.
- `cargo fmt`, `cargo clippy`, and `cargo test --workspace --locked`: pass, 64 Rust tests.
- Python SDK editable install and `pytest sdk/python/tests`: pass, 35 tests.
- `conformance/check_golden.sh`: pass.
- Full `release-hardening.sh` packaging was not run locally because the checkout has only about 2 GiB free; CI release hardening remains the packaging-level verifier.
- `actionlint` was not installed locally; workflow syntax is left to GitHub Actions.

## Residual Risks

- Historical `0.1.0-beta.2` artifacts keep their existing wheel-version note.
- Future release tags must update package metadata, changelog, and release
  notes before the publish workflow runs.
