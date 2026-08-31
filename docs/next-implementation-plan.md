# next implementation plan

`magazine-core` has prepared the additive `1.2.0` minor release on the stable
`1.x` line (expected GitHub Release artifact set: Linux binary, canonical
Python wheel, SBOM, and `SHA256SUMS.txt`). `protocol_version = 1` and
`record_schema_version = 1` are unchanged stable contract identifiers.
Downstream adapter evidence has
exercised the host-fetch and extension-heavy migration shapes this repo was
designed to support:

```text
host-fetch + typed state + page metadata
multi-stage discovery + retail/cross-source extensions
trusted downstream self_fetch kept outside the core broker
```

The protocol-v1 audit found no required semantic contract, Rust host/domain,
DB, or Python SDK model change from that evidence. CLI-driven optional
discover limits (`max_pages` / `max_records` / `per_page`) were added as a
generic gap, and the host now allows prompt plugin shutdown with a short
grace for browser-backed plugins to close descendant processes.

After `1.0.0`, a downstream hub gap note showed the need for local synthetic
`host_fetch` smoke through a loopback fixture. The accepted core change is a
dev-only operator flag, `mh discover --dev-allow-loopback-fetch`, with the
default production fetch policy unchanged.

Do not add speculative core capabilities before a real adapter exposes a
generic gap. Maintainer-approved product scope (standalone distribution and
the admin/viewer UI) is tracked separately below and stays contract-neutral.

## Completed Foundation

Completed at or before the `0.1.0-beta.1` release (history predating the
fresh public repo lives in a retired private build log; PR numbers in this
document refer to current-repo PRs only):

- protocol foundation: framing codec, JSON-RPC types, pinned golden fixtures
- domain / DB / CLI foundation: SourceRecord, `0001_initial`, `init-db` /
  `inspect`, transaction and rollback
- plugin host runtime with a fail-closed subprocess boundary
- Python SDK foundation with the frozen root plugin-author API
- DB-backed typed state provider and the safe `host_fetch` broker
- protocol v1 audit from downstream evidence
- conformance fixture inventory: typed state, discover limits, non-empty
  `page_urls`
- release hardening script and manual workflow (binary / wheel / SBOM
  checksums)

Post-release on the current repo:

- README post-release status (#1) and downstream gap-intake criteria (#2)
- CLI-driven optional discover limits
- prompt plugin shutdown grace in the host
- two-repo development contract docs (#4)
- AGENTS.md redesign for post-beta evidence-driven development (#5)
- Python SDK batched record emission over the existing protocol v1 `records`
  notification shape, with host regressions and a pinned conformance fixture
- CLI-configurable discover timeout budget with public-safe synthetic coverage,
  reusing the existing protocol v1 `remaining_ms` field and keeping timeout
  failures fail-closed
- Release hardening now emits public `SHA256SUMS.txt` checksums and the manual
  workflow can publish the Linux binary, canonical Python wheel, SBOM, and
  checksums to an existing GitHub Release after verifying the release target
  commit.
- `scripts/cut-release.sh` now performs exact-SHA release execution preflight,
  tag / GitHub Release absence checks, queue checks, `target_commitish`
  patch/verification, and release-hardening dispatch.
- A repository-only WordPress REST plugin template provides a public-safe
  `host_fetch` example with synthetic tests. Files under `examples/` are not
  included in release artifacts and are not stable `1.x` contracts.
- `mh discover --dev-allow-loopback-fetch` allows local synthetic loopback
  smoke while keeping the default SSRF policy fail-closed and preserving the
  `1.x` protocol, SDK root API, and canonical schema contracts.
- The separate `mh-ui-ext` binary provides a loopback-only trusted extension
  shell with a fixed read broker; release artifacts include and smoke-test it.

## Protocol v1 Audit Result

Downstream evidence exercised:

- source records with `issue_no`, `page_urls`, `brand_normalized`, and typed
  `external_links`
- `host_fetch` with typed state skip
- post-ingest matching / suggestion extension state kept out of core discovery
- multi-stage discovery with retail and cross-source extension parity compared
  separately from discovery parity

Result:

```text
protocol_version = 1 remains unchanged
record_schema_version = 1 remains unchanged
semantic contract now includes optional discover limits
golden fixtures now cover typed state, discover limits, and non-empty page URLs
```

The v1 contract already contains the generic fields and boundaries required
for the audited downstream evidence. Remaining work must avoid site-specific
names, real responses, cookies, proxy/challenge logic, and private extension
code.

## Evidence-Driven Queue Status

Current status as of 2026-08-31:

- release artifact consumption proof for the stable base is collected in
  `docs/release/1.0.0.md`, with the public artifact UI smoke path evidenced by
  `docs/development/beta-3-public-artifact-ui-smoke-evidence-2026-07-03.md`.
  `1.1.0` assets were verified after release execution. The 1.2.0 candidate
  adds the trusted UI artifact gate described in `docs/release/1.2.0.md`.
- downstream core-improvement intake is empty after closing private-only
  manifest handling, accepted runtime DDL residual monitoring, and the local
  synthetic `host_fetch` loopback smoke gap out of the core promotion queue.
- GitHub open issues, open pull requests, and repository security advisories
  are checked again by the exact-SHA release preflight.

Standing evidence-driven intake remains:

- **Conformance fixtures** for downstream-proven edge cases, synthetic only.
- **Python SDK ergonomics** only when real wrappers need them.

Do not add speculative core capabilities before a real adapter exposes a
generic gap.

## Current Priority: 1.2.0 Additive Minor Release Prep

Current bounded work prepares `1.2.0` from `main` `2109985` without creating a
tag or GitHub Release in the prep branch. The release packages the
contract-neutral trusted `mh-ui-ext` binary and its browser/artifact gates.
Release execution after merge must run
`scripts/cut-release.sh 1.2.0 <exact-release-commit-sha>` from a clean checkout
where that commit is reachable from `origin/main`.

Completed `1.0.0` stabilization slices:

1. Completed in the current standalone quickstart slice: documented and
   verified a linux-x86_64 quickstart where public Release artifacts alone run
   install -> `init-db` -> synthetic example `discover` -> `inspect`.
2. Completed in the current distribution-channel decision slice: GitHub Release
   artifacts are the beta source of truth; `cargo install`, PyPI, and Docker
   are deferred until their cost/benefit justifies another release surface.
3. Completed in the current version-discipline slice: release hardening checks
   Cargo package versions, Python SDK project metadata, and release tag /
   changelog / release-note agreement for version tags.
4. Completed in the current cold-start slice: release-tag hardening dispatches
   run a Linux standalone cold-start job after publishing public GitHub Release
   assets, using only the Release URL, checksum file, binary tarball, and wheel.
5. Completed in the current admin/viewer UI ADR slice:
   `docs/development/admin-viewer-ui-adr-2026-07-02.md` accepts a bundled local
   web UI served by `mh ui`, with `127.0.0.1` bind, read-only default, explicit
   `--manage` opt-in plus request-level protection for mutating operations, no
   runtime Node dependency, and no protocol / SDK root API / canonical schema
   change.
6. Completed in the current read-only viewer slice:
   `docs/development/admin-viewer-ui-readonly-evidence-2026-07-02.md` records
   `mh ui --db <core.db> --plugins-dir <plugins.d> [--port N]`, loopback-only
   binding, `GET`/`HEAD`-only routes, read-only DB projections for summary /
   records / typed `known_source_urls`, and non-executing `plugins.d` manifest
   listing with local path and secret-like environment redaction.
7. Completed in the current security-boundary docs slice:
   `docs/development/admin-viewer-ui-security-evidence-2026-07-02.md` records
   the `SECURITY.md` local UI trust model before any management-mode endpoint
   is implemented: loopback-only read-only default, no default auth for local
   read-only browsing, no remote-access support, and request-level protection
   requirements for future mutating routes.
8. Completed in the current explicit management-mode slice:
   `docs/development/admin-viewer-ui-management-evidence-2026-07-02.md`
   records `mh ui --manage` with request-level token guard, state-changing
   `GET` rejection, loopback `Host` / `Origin` validation, guarded `init-db`,
   policy-capped bounded `discover`, single active UI-started run tracking,
   and cancellation for that UI-started run.
9. Completed in the current UI release-smoke slice:
   `docs/development/admin-viewer-ui-release-smoke-evidence-2026-07-02.md`
   records artifact-only quickstart coverage for local read-only UI browse and
   guarded `--manage` discover. Release hardening now forces the UI smoke for
   freshly generated Linux artifacts while `VERIFY_UI=auto` keeps older
   pre-UI tags consumable.
10. Completed in the current C7 criteria slice:
    `docs/release/public-visibility-checklist.md` now contains the formal
    `1.0.0` eligibility gate. The gate keeps stable tagging blocked until the
    beta stability window, release artifact consumption, empty evidence queue,
    docs/conformance completeness, standalone distribution, admin/viewer UI,
    compatibility policy, and version discipline are all evidenced.
11. Completed in the current C7 compatibility-policy slice:
    `docs/compatibility-policy.md` documents stable `1.x` surfaces, additive
    changes, deprecations, breaking changes, security exceptions, and
    old+new compatibility windows.
12. Completed in the current C7 stable-tag decision slice:
    `docs/development/1-0-stable-tag-decision-2026-07-02.md` records that
    `1.0.0` should not be cut yet. The release-readiness blockers are the
    missing current-surface beta stability window, no public artifact containing
    the UI smoke path yet, and failing `1.0.0` version discipline because
    package metadata, changelog, release notes, and tag agreement are not
    prepared.
13. Completed in the S3 stable release-prep slice:
    `docs/development/1-0-stability-window-evidence-2026-07-03.md` identifies
    the stability window by commit range, `docs/release/1.0.0.md` aggregates
    the eligibility-gate evidence, and package metadata / changelog / release
    notes now agree on `1.0.0`. S3 intentionally creates no tag, GitHub
    Release, or push.

## Maintainer Product Scope (contract-neutral)

Approved maintainer scope, tracked separately from the evidence queue.
Neither item may change `protocol_version`, `record_schema_version`, the
Python SDK root API, or the canonical DB schema:

- **Standalone distribution**: a quickstart where install -> `init-db` ->
  synthetic example `discover` -> `inspect` works from public release
  artifacts alone, verified with a cold-start check in a clean environment.
- **Admin/viewer UI**: a bundled local web UI, ADR first. Defaults:
  `127.0.0.1` bind and read-only; mutating operations (`init-db`, bounded
  `discover`, run cancel) require an explicit opt-in flag and request-level
  protection; no runtime Node dependency in the distribution. The accepted ADR
  is `docs/development/admin-viewer-ui-adr-2026-07-02.md`.

## 1.0 Criteria (completed)

The formal gate is `docs/release/public-visibility-checklist.md` section
`1.0.0 Eligibility Gate`. Cut `1.0.0` only after every item in that gate is
true and the release notes link the supporting evidence. The gate evidence is
collected in `docs/release/1.0.0.md`. At a minimum this required:

- a beta window with zero contract changes
- release artifact consumption working downstream
- an empty evidence queue
- complete protocol / SDK / conformance docs
- the standalone distribution and admin/viewer UI goals above
- a documented post-`1.0.0` compatibility policy
- version metadata, changelog, release notes, and GitHub Release tag agreement

Current decision: `1.0.0` is the stable base for later `1.x` additive releases.
New stable release execution uses `scripts/cut-release.sh` to create the tag and
GitHub Release at the exact merged release-prep commit SHA, then dispatch
release hardening.

## Core Follow-Up Policy

When downstream finds a gap:

1. Classify whether the missing behavior is generic core behavior or private
   adapter logic.
2. If generic, fix it in `magazine-core` first.
3. Keep site-specific names, real responses, cookies, proxy/challenge logic,
   and private extension code out of this repo.
4. Update protocol docs and golden fixtures in the same PR for any contract
   change.
5. Cut a new beta or stable SHA/tag as appropriate and let downstream lock
   files pin it. Stable tags must satisfy the public `1.0.0` eligibility gate.
