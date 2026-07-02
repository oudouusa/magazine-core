# 1.0 compatibility policy evidence

- Date: 2026-07-02
- Scope: C7 post-1.0 compatibility policy
- Contract impact: none

## Decision

The post-`1.0.0` compatibility policy now lives in
`docs/compatibility-policy.md`. It defines the stable `1.x` surfaces, limited
or unstable surfaces, additive changes, deprecations, breaking changes, security
exceptions, and downstream consumption rules.

No `1.0.0` tag is cut by this slice. The stable tag decision remains a separate
C7 gate after this policy and the other `1.0.0 Eligibility Gate` items are
evidenced.

## Boundary

This slice is docs-only:

- no `protocol_version` change;
- no `record_schema_version` change;
- no Python SDK root API change;
- no canonical SQLite schema change;
- no package metadata, workflow, script, conformance fixture, or code change;
- no release tag.

The policy describes future compatibility behavior but does not change the
current beta contract.

## Failure Modes Prevented

- Stable public surfaces are removed or renamed in `1.x` without an old+new
  compatibility window.
- Release docs require a compatibility policy, but public users cannot find the
  canonical policy.
- UI HTTP implementation details are mistaken for a remote public API contract.
- Security fixes are blocked by normal deprecation windows even when unsafe
  behavior needs to be restricted.

## Verification

Local checks:

```text
git diff --check: pass
git diff --exit-code -- Cargo.toml Cargo.lock crates sdk conformance scripts .github pyproject.toml rust-toolchain.toml: pass, no output
python3 scripts/check-version-discipline.py --root . --release-ref main --json: pass, ok=true
contract-doc diff for `docs/protocol-v1.md` and `docs/python-sdk.md`: reviewed, cross-link only
local and remote `1.0.0` / `v1.0.0` tag checks: no output
public-safety grep over changed public docs: pass, no output
stable-status grep for accidental stable-availability claims: pass, no output
stale-C7-placeholder grep: pass, no output
discoverability grep: README, release hardening, release checklist, next plan, two-repo contract, protocol docs, SDK docs, migration checklist, adoption guide, and SECURITY.md
```

## Adopt / Revert

Adopt if the PR remains docs-only, the policy is discoverable from README and
release docs, it covers additive changes, deprecations, breaking changes,
security exceptions, unsupported surfaces, and old+new compatibility windows,
and no docs imply that `1.0.0` already exists.

Revert if code / packaging / workflow / conformance paths change, private
downstream details leak, the policy weakens the `1.0.0` eligibility gate, or
the policy changes protocol / SDK / schema semantics instead of documenting
future compatibility rules.

## Residual Risks

- The stable tag decision was later recorded in
  `docs/development/1-0-stable-tag-decision-2026-07-02.md`: do not cut yet.
