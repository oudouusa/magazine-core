# next implementation plan

`magazine-core` is public as `0.1.0-beta.1` (GitHub prerelease).
`protocol_version = 1` and `record_schema_version = 1` are frozen for the
beta. Downstream adapter evidence has exercised the host-fetch and
extension-heavy migration shapes this repo was designed to support:

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

## Current Priority: Evidence-Driven Queue

Ordered by downstream evidence; each item lands as a small PR set with
synthetic tests and public-safe docs:

1. **Batched record emission** — first downstream-evidenced generic gap:
   one-frame-per-record emission can hit host queue/runtime limits, forcing
   downstream consumers to cap record counts per run. Prefer an additive
   optional capability within protocol v1; decide between SDK-level batching
   and host queue behavior from a synthetic reproduction.
2. **Bounded plugin runtime limits** — implement only if a synthetic plugin
   reproduces a generic runtime-limit gap; otherwise this stays a downstream
   concern.
3. **Release artifact automation** — attach hardened binaries / wheels /
   checksums / SBOM to GitHub Releases so downstream consumers can verify and
   consume by version/checksum without a local checkout.
4. **Conformance fixtures** for downstream-proven edge cases, synthetic only.
5. **Python SDK ergonomics** only when real wrappers need them.

## Maintainer Product Scope (contract-neutral)

Approved maintainer scope, tracked separately from the evidence queue.
Neither item may change `protocol_version`, `record_schema_version`, the
Python SDK root API, or the canonical DB schema:

- **Standalone distribution**: a quickstart where install -> `init-db` ->
  synthetic example `discover` -> `inspect` works from public release
  artifacts alone, verified with a cold-start check in a clean environment.
- **Admin/viewer UI**: a bundled local web UI, ADR first. Defaults:
  `127.0.0.1` bind and read-only; mutating operations (`init-db`, bounded
  `discover`, run cancel) require an explicit opt-in flag; no runtime Node
  dependency in the distribution.

## 1.0 Criteria (to be formalized)

Cut `1.0.0` only after all of the following hold, and formalize them in a
dedicated docs PR before tagging:

- a beta window with zero contract changes
- release artifact consumption working downstream
- an empty evidence queue
- complete protocol / SDK / conformance docs
- the standalone distribution and admin/viewer UI goals above

## Core Follow-Up Policy

When downstream finds a gap:

1. Classify whether the missing behavior is generic core behavior or private
   adapter logic.
2. If generic, fix it in `magazine-core` first.
3. Keep site-specific names, real responses, cookies, proxy/challenge logic,
   and private extension code out of this repo.
4. Update protocol docs and golden fixtures in the same PR for any contract
   change.
5. Cut a new beta SHA or tag and let downstream lock files pin it.
