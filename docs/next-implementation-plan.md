# next implementation plan

`magazine-core` main contains the protocol v1 foundation through the safe host
fetch broker and Python SDK helpers. Downstream adapter evidence has exercised
the host-fetch and extension-heavy migration shapes this repo was designed to
support:

```text
host-fetch + typed state + page metadata
multi-stage discovery + retail/cross-source extensions
```

The first protocol-v1 audit found no required semantic contract, Rust
host/domain, DB, or Python SDK model change from that evidence. Trusted
`self_fetch` remains a downstream-owned validation and ADR-status gate; the
existing v1 boundary remains unchanged by this docs-only audit. Public docs now
describe the beta-readiness boundary; conformance fixture inventory covers the
remaining protocol areas for beta preparation. The next work is release
hardening. Do not add speculative core capabilities before a real adapter
exposes a generic gap.

## Completed Foundation

- PR #1: protocol foundation
- PR #2: domain / DB / CLI foundation
- PR #3: plugin host runtime and fail-closed subprocess boundary
- PR #6: Python SDK foundation
- PR #7: DB-backed typed state provider
- PR #8: safe host fetch broker
- PR #14: protocol v1 audit from downstream evidence
- PR #15: Python SDK root plugin-author API freeze
- PR #16+: public docs cleanup and beta-readiness docs alignment

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
semantic contract remains unchanged
golden fixtures now cover typed state and non-empty page URLs
```

The v1 contract already contains the generic fields and boundaries required for
the audited downstream evidence. Remaining work should avoid site-specific
names, real responses, cookies, proxy/challenge logic, and private extension
code.

## Current Priority: Public Stabilization

1. Keep the frozen Python SDK root plugin-author API stable for `0.1.0-beta`;
   low-level framing/runtime helpers remain advanced submodule APIs.
2. Run release hardening:
   - clean install from a fresh checkout
   - Linux and macOS smoke where available
   - license / SBOM / secret scan
   - packaged binary and SDK artifact checks
3. Use `scripts/release-hardening.sh` and the manual `Release hardening`
   workflow for repeatable package/checksum evidence.
4. Cut a beta tag or beta-candidate SHA only after the fixture inventory and
   release hardening evidence are recorded.

## Core Follow-Up Policy

When downstream finds a gap:

1. Classify whether the missing behavior is generic core behavior or private
   adapter logic.
2. If generic, fix it in `magazine-core` first.
3. Keep site-specific names, real responses, cookies, proxy/challenge logic, and
   private extension code out of this repo.
4. Update protocol docs and golden fixtures in the same PR for any contract
   change.
5. Cut a new alpha/beta SHA or tag and update downstream lock files.

Likely future core PRs should be small and evidence-backed:

- Python SDK ergonomics only if real wrappers need them
- typed state edge cases only if real skip-state runs expose them
- fetch broker policy gaps exposed by real redirects or response limits
- release hardening for beta/public readiness
