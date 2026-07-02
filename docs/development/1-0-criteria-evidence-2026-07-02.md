# 1.0 criteria evidence

- Date: 2026-07-02
- Scope: C7 1.0 criteria formalization
- Contract impact: none

## Decision

The stable-tag gate now lives in the public release checklist:
`docs/release/public-visibility-checklist.md` section `1.0.0 Eligibility Gate`.
`docs/next-implementation-plan.md` points to that gate instead of carrying the
only criteria copy.

No `1.0.0` tag is cut by this slice. The gate blocks stable tagging until the
release notes link evidence for the beta stability window, release artifact
consumption, empty evidence queue, docs/conformance completeness, standalone
distribution, admin/viewer UI, post-1.0 compatibility policy, and version
discipline.

## Boundary

This slice is docs-only:

- no `protocol_version` change;
- no `record_schema_version` change;
- no Python SDK root API change;
- no canonical SQLite schema change;
- no package metadata, workflow, script, conformance fixture, or code change;
- no release tag.

Stale exact beta release copies in agent/development docs were pointerized to
`README.md` and GitHub Releases so future beta tags do not drift in policy
documents.

## Failure Modes Prevented

- A `1.0.0` tag is cut because release hardening passes, while compatibility
  criteria remain informal in the roadmap.
- Agent-facing docs keep stale exact beta release values and contradict
  README/GitHub Releases.
- Public users cannot find the stable-tag criteria from README or release docs.
- UI and standalone distribution DoD are treated as product notes rather than
  required stable-tag evidence.

## Verification

Local checks:

```text
git diff --check: pass
git diff --exit-code -- Cargo.toml Cargo.lock crates sdk conformance scripts .github/workflows: pass, no output
python3 scripts/check-version-discipline.py --root . --release-ref main --json: pass, ok=true
git tag -l '1.0.0' 'v1.0.0': pass, no output
git ls-remote --tags origin 'refs/tags/1.0.0' 'refs/tags/v1.0.0': pass, no output
discoverability grep for README / release hardening / migration checklist / roadmap: pass
public-safety grep over changed public docs: pass, no output
stale-placeholder grep for obsolete C7 planning phrases: pass, no output
stable-status grep for accidental stable-availability claims: pass, no output
```

## Adopt / Revert

Adopt if the PR remains docs-only, the formal gate is discoverable from README,
release hardening, the release checklist, migration checklist, and the roadmap,
stale exact beta release copies are removed from policy docs, and no docs claim
that a stable tag already exists.

Revert if any code / packaging / workflow / conformance path changes, private
downstream details leak, the gate weakens the "cut 1.0 only after" condition,
or a stable tag is implied without evidence.

## Residual Risks

- The post-`1.0.0` compatibility policy is required by the gate but still needs
  a dedicated C7 slice.
- The stable tag decision itself remains open; this slice only defines the
  criteria.
