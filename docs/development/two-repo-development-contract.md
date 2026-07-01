# Two-repo development contract

- Status: Active
- Scope:
  - `magazine-core`: public upstream contract and implementation
  - private downstream consumers: private production integration
- Purpose: keep reusable core improvements and private downstream operations clearly separated.

## 1. One-line rule

`magazine-core` owns reusable protocol, runtime, SDK, and canonical storage contracts.

Private downstream consumers own private adapters, production operations, source-specific extensions, deployments, credentials, data, and downstream compatibility.

Dependency direction is one-way:

```text
magazine-core
  -> versioned artifact / tag / commit / Python SDK / protocol
private downstream consumer
```

`magazine-core` must not import, name, depend on, or assume any private downstream repository.

Private downstream consumers may consume `magazine-core` through a pinned release, tag, commit SHA, binary, or Python SDK.

## 2. Repository responsibilities

| Area | `magazine-core` | Private downstream consumer |
| --- | --- | --- |
| Visibility | Public | Private |
| Role | Upstream contract source | Downstream production consumer |
| Runtime | Rust host, stdio protocol, process supervision | Runner, cron, dashboard, production orchestration |
| Protocol | `protocol_version`, `record_schema_version`, framing, JSON-RPC | Protocol consumer and integration evidence |
| SDK | Public Python plugin author SDK | Private plugin implementations |
| DB | Canonical SQLite schema for core records | Production DB, legacy compatibility, promotion/import paths |
| Fetch | Generic safe `host_fetch` broker | Private access policy and private source integration |
| Examples | Synthetic only | Real adapters and operational evidence |
| Tests | Synthetic conformance, public-safe fixtures | Real-source parity, production smoke, operations gates |
| Secrets/data | Never | Private-only |
| Release | Public tags/artifacts | Pinned lock file and downstream verification |

## 3. What belongs in `magazine-core`

A change belongs in `magazine-core` only if it is all of the following:

1. Generic across more than one downstream source or clearly part of the public contract.
2. Reproducible using synthetic fixtures.
3. Free of private source names, real scraped data, credentials, internal paths, production hostnames, screenshots, and logs.
4. Useful to an external plugin author.
5. Does not require private access state or production-only assumptions.
6. Can be tested in public CI.

Good core candidates:

```text
- batched SourceRecord emission in the Python SDK
- host-side max_records enforcement
- bounded plugin runtime limits
- clearer JSON-RPC failure behavior
- protocol conformance fixtures
- safe host_fetch policy hardening
- SDK ergonomics for typed state_query / host_fetch
- release artifact checks and public documentation
```

## 4. What stays downstream

A change stays in a private downstream consumer if it is source-specific, private, operational, or production-data-dependent.

Keep downstream:

```text
- real site adapters
- source-specific parsing
- private access implementation details
- downstream matching and suggestions
- downstream enrichment
- reader/package materialization
- production DB migrations and compatibility
- runner / cron / dashboard routes
- credentials, private env names, live URLs, real fixtures
- operations runbooks and incident evidence
- accepted residual runtime schema exceptions
```

Private downstream code may prove that a generic gap exists, but the proof must be reduced to a synthetic, public-safe case before it is promoted to `magazine-core`.

## 5. Promotion rule: downstream to core

A downstream issue may be promoted to `magazine-core` only when it satisfies this checklist:

```text
[ ] The issue is not source-specific.
[ ] The issue can be described without private adapter names or real data.
[ ] The issue can be reproduced with a synthetic plugin or fixture.
[ ] The fix does not require private credentials, private access state, or production-only behavior.
[ ] The fix improves the public protocol, host, SDK, schema, conformance, or docs.
[ ] The expected behavior can be tested in magazine-core CI.
[ ] The downstream workaround can be removed or simplified after the core change.
```

If any item fails, keep the work downstream.

## 6. Change routing decision tree

Use this before starting a PR.

```text
Question 1:
  Does this touch protocol_version, record_schema_version, public SDK API,
  canonical DB schema, host runtime behavior, or conformance?

  yes -> start in magazine-core
  no  -> continue

Question 2:
  Does this require real source behavior, private credentials, real DB state,
  private scraper settings, or source-specific parsing?

  yes -> keep downstream
  no  -> continue

Question 3:
  Can the behavior be reproduced with a synthetic plugin and public fixture?

  yes -> consider magazine-core
  no  -> keep downstream

Question 4:
  Is this only a downstream lock bump, route change, production observation,
  parity report, or rollback/runbook update?

  yes -> keep downstream
```

## 7. PR patterns

### 7.1 Core-only PR

Use when changing public contract or generic runtime behavior.

```text
Repo:
  magazine-core

Examples:
  - SDK API improvement
  - host runtime hardening
  - protocol docs
  - conformance fixtures
  - release artifact workflow

Required:
  - public-safe docs
  - synthetic tests
  - no private source names
  - no real data
  - no downstream-specific assumptions
```

### 7.2 Downstream-only PR

Use when changing private downstream behavior.

```text
Repo:
  private downstream consumer

Examples:
  - adapter wrapper
  - production observation
  - operations gate
  - downstream extension
  - accepted residual monitoring
  - rollback runbook
```

### 7.3 Cross-repo PR set

Use when a generic core improvement is needed and a downstream consumer must consume it.

```text
Step 1:
  magazine-core PR
  - implement generic fix
  - add synthetic tests
  - update public docs
  - publish a release, tag, or commit SHA

Step 2:
  downstream PR
  - bump the core lock
  - verify downstream behavior
  - remove downstream workaround where safe
  - record production or parity evidence if needed
```

Do not patch a copy of core logic inside a private downstream consumer.

## 8. Versioning and locking

`magazine-core` controls public versioning.

Private downstream consumers pin the exact core version or commit in their own lock files.

Rules:

```text
[ ] A downstream PR that consumes a new core behavior must update its lock deliberately.
[ ] The lock bump PR must state whether protocol_version or record_schema_version changed.
[ ] If the core change is behavior-only, say so explicitly.
[ ] Downstream must not assume unpublished core behavior.
[ ] Downstream must not depend on a dirty local core checkout.
```

## 9. Contract stability

Current beta contract:

```text
release = 0.1.0-beta.1
protocol_version = 1
record_schema_version = 1
```

Changing either `protocol_version` or `record_schema_version` requires a `magazine-core` PR first.

A downstream source-specific failure is not enough to change the protocol. First prove a generic protocol gap.

## 10. Verification policy

### `magazine-core`

Use public-safe verification:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
Python SDK tests
golden / conformance checks
synthetic plugin E2E
release hardening checks when relevant
```

### Private downstream consumer

Use local-first private verification documented by the downstream repository:

```text
downstream local CI or verification script
core lock install / contract verification
source-specific preflight when relevant
operations gate when production routing changes
next-action or queue checks when the downstream has roadmap automation
```

Do not rely on private CI as the only gate unless that repository explicitly enables it as a primary gate.

## 11. Production promotion rule

A downstream source can move toward production core routing only when all relevant gates are green:

```text
[ ] discovery parity
[ ] extension parity, if applicable
[ ] copied-DB route smoke
[ ] explicit rollback drill
[ ] operations gate
[ ] scheduled production observation
[ ] downstream migration matrix update
[ ] next-action helper reports no blocker, if one exists
```

Generic core changes do not automatically promote downstream production routes.

## 12. Runtime schema rule

Routine production runtime schema mutation should not be part of normal downstream routing.

Accepted residual runtime schema exceptions are downstream-owned and must be documented with:

```text
[ ] owner
[ ] reason
[ ] allowed caller surface
[ ] migration ownership
[ ] review or retirement condition
[ ] checker coverage
```

Accepted residuals are not automatic deletion targets. They are monitored separately by the downstream consumer.

## 13. Do not do

```text
Do not add real adapters to magazine-core.
Do not add private source names to magazine-core public docs.
Do not move private access logic into magazine-core.
Do not put real HTML, real JSON responses, production DBs, screenshots, or logs in magazine-core.
Do not change protocol_version or record_schema_version from downstream.
Do not add runtime schema mutation to routine production paths.
Do not route a downstream source to core without rollback and operations evidence.
Do not use broad git staging for cross-repo work.
```

## 14. Current posture

Expected current state:

```text
magazine-core:
  public upstream beta
  owns protocol / host / SDK / conformance

private downstream consumers:
  consume pinned core versions
  keep private integrations downstream
  promote only synthetic-reproducible generic gaps to core
```

The next class of core work should focus on generic downstream pressure that is synthetic-reproducible, such as:

```text
- batched record emission
- bounded plugin runtime limits
- conformance fixtures for downstream-proven edge cases
- public SDK ergonomics
- release artifact hardening
```
