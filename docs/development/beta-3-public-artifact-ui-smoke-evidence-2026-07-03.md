# 0.1.0-beta.3 public artifact UI smoke evidence

- Date: 2026-07-03
- Scope: UI-bearing public beta artifact and release-consuming UI smoke
- Contract impact: none

## Decision

`0.1.0-beta.3` was tagged at `53d6cae` (PR #25) and published as a GitHub
prerelease with the Linux binary tarball, canonical Python wheel (`0.1.0b3`),
CycloneDX SBOM, and `SHA256SUMS.txt`.

This closes the previously unmet `1.0.0` Eligibility Gate evidence item from
`docs/development/1-0-stable-tag-decision-2026-07-02.md`: public artifact
consumption was proven through `0.1.0-beta.2`, but that beta predated `mh ui`.
The UI smoke path is now proven by a public beta artifact that contains the UI.

## Boundary

- `protocol_version = 1`, `record_schema_version = 1`, the Python SDK root
  API, and the canonical SQLite schema are unchanged.
- No conformance fixture change.
- The release-prep diff (PR #25) is version metadata, `CHANGELOG.md`,
  `docs/release/0.1.0-beta.3.md`, and one version-discipline test expectation.

## Verification

- PR #25 `ci`: success; squash-merged as `53d6cae`.
- Release hardening run `28623809820`: failed at the publish gate with
  `release 0.1.0-beta.3 target main is not resolvable in this checkout` —
  an expected safety refusal (see operational lesson below), not an artifact
  defect.
- Release hardening run `28624010313`: success — Ubuntu and macOS hardening,
  `publish Linux release assets`, and `standalone-cold-start` verification
  against the public Release URL.
- Local release-consuming smoke against public assets only:

```text
REPO=oudouusa/magazine-core RELEASE_TAG=0.1.0-beta.3 VERIFY_UI=1 \
  bash scripts/verify-standalone-quickstart.sh
-> standalone UI quickstart verified
```

  covering checksum verification, binary extraction, wheel install,
  `init-db -> discover -> inspect` (`schema_version: 1`, 1 synthetic record
  across source_posts/performers/covers/pages/external_links), read-only
  `mh ui`, and guarded `--manage` discover.

## Operational lesson: Release target_commitish

If the tag is pushed before the GitHub Release is created, GitHub records the
Release `target_commitish` as the default branch name (`main`). The
`upload-release-assets.sh` gate then refuses in the workflow's detached tag
checkout because a branch name is not resolvable there. Remediation applied
here: `PATCH /releases/{id}` to set `target_commitish` to the tag commit SHA,
then re-dispatch `Release hardening`.

For the future stable tag: create the GitHub Release with an explicit commit
SHA target (or patch it to the SHA) before dispatching `Release hardening`.

## Adopt / Revert

Adopt: assets are published and both public-artifact verifications
(workflow `standalone-cold-start` and the local `VERIFY_UI=1` quickstart) are
green.

Revert: delete the Release and tag only if the published artifact set is
found defective. None observed.

## Residual risks

- Release-consuming UI smoke assets cover linux-x86_64 only; other platforms
  build from source (documented known limitation in the release notes).
- The current-surface beta stability window starts at asset publication
  (`2026-07-02T21:57:53Z`); any change to the four contract surfaces resets
  it per the `1.0.0` Eligibility Gate.
