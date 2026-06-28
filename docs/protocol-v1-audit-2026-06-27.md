# protocol v1 audit (2026-06-27)

## Scope

This audit reviewed downstream evidence after two migration shapes and checked
the existing trusted self-fetch boundary without changing it:

- host-fetch + typed state + page metadata
- multi-stage discovery + retail/cross-source extension parity

The audit asks whether `protocol_version = 1` or `record_schema_version = 1`
needs a contract, golden vector, Rust host/domain, DB, or Python SDK model
change before public stabilization.

## Decision

No v1 contract change is required.

The current protocol already covers the generic requirements observed by the
audited downstream evidence:

- opaque absolute `source_url`
- `issue_no`
- `page_urls`
- `brand_normalized`
- typed `external_links`
- typed `state_query`
- host-managed `fetch_request`
- trusted `self_fetch` boundary remains specified, but ADR status and final
  self-fetch acceptance remain downstream-owned
- discovery parity separated from private extension parity

The golden vectors already include `issue_no` and typed `external_links`, so no
golden regeneration is part of this audit.

## Docs Updated

- `README.md`: status now reflects protocol v1 foundation without asserting
  downstream ADR status.
- `AGENTS.md`: current state and next tasks now point to stabilization rather
  than initial adapter proof.
- `docs/agent-coordination.md`: old `external_links` ownership note is marked
  resolved.
- `docs/adoption-guide.md` and `docs/migration-checklist.md`: discovery parity
  fields now consistently include `brand_normalized`.
- `docs/next-implementation-plan.md`: rewritten as the post-evidence
  stabilization plan.

## Verification

This is a docs/status audit. It intentionally does not change protocol
semantics, Rust crates, Python SDK code, conformance oracle, or `.hex` golden
fixtures. Later docs-only PRs may add non-semantic status notes to
`docs/protocol-v1.md` without changing the v1 contract.

Required checks:

```text
rtk git diff --check -- AGENTS.md README.md docs
rtk git diff --name-status
rtk rg -n "ADR-0001|downstream-owned|host-fetch|self-fetch|host_fetch|self_fetch|golden|conformance|external_links|record_schema_version" AGENTS.md README.md docs
```

Full Rust / conformance gates are required only when a future PR changes the
protocol contract, golden vectors, Rust/Python protocol code, SDK model behavior,
or makes fresh test-pass claims.

## Next

Conformance fixture inventory now covers host_fetch, typed state, trusted
self_fetch boundary, external links, page URLs, and fail-closed behavior for
`0.1.0-beta` readiness. Proceed to release hardening. Keep ADR status
downstream-owned; downstream self-fetch evidence does not require a core
protocol change unless a future adapter exposes a generic contract gap.
