# 1.0 stable tag decision

- Date: 2026-07-02
- Scope: C7 stable tag decision
- Decision: do not cut `1.0.0` yet
- Contract impact: none

## Decision

Do not cut `1.0.0` on 2026-07-02.

The current public release remains `0.1.0-beta.2`. The stable-tag blockers are
release-readiness evidence and version discipline, not a known required change
to `protocol_version`, `record_schema_version`, the Python SDK root API, or the
canonical SQLite schema.

## Gate Status

Satisfied or sufficiently evidenced:

- Evidence-driven queue is empty for this decision window: GitHub open PRs,
  open issues, and open security advisories all returned `[]`.
- Conformance inventory and golden checks are documented in the existing
  conformance evidence, and no conformance fixture change is proposed here.
- Standalone release artifact consumption is proven for `0.1.0-beta.2`.
- C6 admin/viewer UI implementation and local artifact-smoke coverage are
  complete.
- Post-`1.0.0` compatibility policy is documented in
  `docs/compatibility-policy.md`.

Not satisfied:

- No current-surface beta stability window has elapsed after the same-day C6
  UI release-smoke and C7 release-governance changes.
- Public artifact consumption is proven through `0.1.0-beta.2`, but that public
  beta predates `mh ui`; the UI smoke path is not yet proven by a public beta
  artifact that contains the UI.
- `1.0.0` version discipline fails by design today. Cargo packages and the
  Python SDK project metadata are still `0.1.0`, `CHANGELOG.md` has no
  `1.0.0` entry, `docs/release/1.0.0.md` does not exist, and no local or remote
  `1.0.0` / `v1.0.0` tag exists.
- The stable release notes cannot yet link a complete evidence set because
  there is no stable release-note document.

## Verification

Local and GitHub checks:

```text
gh pr list --repo oudouusa/magazine-core --state open --json number,title,headRefName,isDraft: []
gh issue list --repo oudouusa/magazine-core --state open --json number,title,labels: []
gh api repos/oudouusa/magazine-core/security-advisories?state=open: []
python3 scripts/check-version-discipline.py --root . --release-ref main --json: pass, ok=true
python3 scripts/check-version-discipline.py --root . --release-ref 1.0.0 --json: expected failure, release=1.0.0 and package=0.1.0 disagree
git tag -l '1.0.0' 'v1.0.0': pass, no output
git ls-remote --tags origin 'refs/tags/1.0.0' 'refs/tags/v1.0.0': pass, no output
ls docs/release/1.0.0.md: expected failure, file does not exist
rg for `1.0.0` package metadata / changelog / release note entries: no stable release entries
```

## Adopt / Revert

Adopt this decision note because the public `1.0.0 Eligibility Gate` is not yet
fully satisfied and this slice does not create a release tag.

Revert if a stable tag is intentionally prepared in the same PR with package
metadata, changelog, release notes, release hardening, and public artifact
evidence. That is not this slice.

## Next Stable-Tag Attempt

Before reconsidering `1.0.0`, prepare a stable release slice that includes:

- an identified beta stability window for the final stable surface;
- a public release artifact containing the UI-capable binary and passing the
  release-consuming quickstart with UI smoke;
- package metadata, changelog, `docs/release/1.0.0.md`, and tag agreement;
- release hardening and public artifact evidence for the stable candidate.
