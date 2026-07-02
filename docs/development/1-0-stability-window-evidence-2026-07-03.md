# 1.0 stability window evidence

- Date: 2026-07-03
- Scope: S3 stable release-prep eligibility evidence
- Contract impact: none

## Decision

Adopt the `1.0.0` beta stability window by observed commit range, not by waiting
for another calendar interval. This follows the 2026-07-03 owner decision that
the relevant release-readiness signal is the absence of contract changes across
the public contract surfaces after `0.1.0-beta.2`.

Window assessed:

```text
start tag: 0.1.0-beta.2
start commit: 26e723a06eec0bb59d916fce7a8a6b33a6894ff7
public UI artifact tag: 0.1.0-beta.3
public UI artifact commit: 53d6cae24cfd3071d3b87276a847b5357ddcfa19
S3 release-prep base commit: 3f32e040a4c15305e3124112f0e9b6a0cfb7d5be
range: 0.1.0-beta.2..3f32e040a4c15305e3124112f0e9b6a0cfb7d5be
```

The S3 prep commit itself changes release metadata and docs for `1.0.0`; it
does not change protocol source, Python SDK source, conformance fixtures, or
canonical SQLite DDL.

## Boundary

- `protocol_version = 1` is unchanged. `crates/mh-protocol/src` has no diff in
  the window.
- `record_schema_version = 1` is unchanged. No protocol or SourceRecord schema
  change is present.
- The Python SDK root plugin-author API is unchanged. `sdk/python/src` has no
  diff in the window, covering the root import surface and `__all__`.
- The canonical SQLite schema is unchanged. `crates/mh-db/src/lib.rs` changes
  are read-only UI query helpers, typed view structs, and tests. The existing
  DDL and migration behavior are unchanged.
- Conformance fixtures are unchanged from before `0.1.0-beta.2`; `0.1.0-beta.3`
  release hardening and the S3 release-prep verification keep
  `bash conformance/check_golden.sh` green.
- Public-safe only: no private downstream repository names, site-specific
  adapter names, credentials, private paths, real responses, or production
  hostnames are introduced.

Evidence queue status as of 2026-07-03: no accepted generic gap note,
contract-change PR, public support issue, open PR, or security advisory blocks
the stable tag.

## Verification

Contract-surface diff checks:

```bash
git diff --name-status 0.1.0-beta.2..3f32e040a4c15305e3124112f0e9b6a0cfb7d5be -- crates/mh-protocol/src sdk/python/src conformance
# no output

git diff --name-status 0.1.0-beta.2..3f32e040a4c15305e3124112f0e9b6a0cfb7d5be -- crates/mh-db/src
# M crates/mh-db/src/lib.rs

git diff --unified=0 0.1.0-beta.2..3f32e040a4c15305e3124112f0e9b6a0cfb7d5be -- crates/mh-db/src/lib.rs
# additions are read-only open/query helpers, typed UI view structs, and tests
```

DDL guard:

```bash
git diff --unified=0 0.1.0-beta.2..3f32e040a4c15305e3124112f0e9b6a0cfb7d5be -- crates/mh-db/src/lib.rs \
  | rg '^[+][^+].*(CREATE TABLE|ALTER TABLE|DROP TABLE|mh_schema_migrations|SCHEMA_VERSION)'
# no added DDL or migration-version output
```

Release-prep verification commands:

```bash
python3 scripts/check-version-discipline.py --root . --release-ref 1.0.0 --json
bash scripts/test-version-discipline.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

Tag boundary checks before S4:

```bash
git tag -l '1.0.0' 'v1.0.0'
git ls-remote --tags origin 'refs/tags/1.0.0' 'refs/tags/v1.0.0'
# no output before S4
```

## Adopt / Revert

Adopt if the release-prep verification commands pass, the version discipline
report shows package version `1.0.0`, and the S3 diff remains limited to version
metadata plus public-safe release docs.

Revert if any protocol source, Python SDK root API, conformance fixture, or
canonical SQLite DDL change appears in this slice, or if the evidence queue is
found non-empty before S4.

## Residual Risks

- S3 does not create the stable tag, GitHub Release, or release artifacts. S4
  must create the `1.0.0` tag and GitHub Release at the exact S3 commit SHA,
  then dispatch release hardening.
- Public artifact smoke for UI-bearing assets is proven on linux-x86_64. Other
  platforms remain source-build paths until their prebuilt binaries are added.
- The evidence queue status is a point-in-time owner check on 2026-07-03; it
  must be rechecked immediately before S4 if time or repository state changes.
