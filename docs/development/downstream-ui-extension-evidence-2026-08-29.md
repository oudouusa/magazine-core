# Downstream UI extension evidence (2026-08-29)

- Status: evidence note; not a contract, implementation, or schedule
- Scope: public-safe findings from a private downstream read-only UI consumer
- Related: `ui-plugin-contract-draft-2026-07-28.md`, issue #36

## Why this note exists

A private downstream consumer now has a second substantial UI shape in addition
to the earlier dependency-free gallery prototype. The new consumer presents a
read-only graph over collected source records: a stable parent entity, its
members, typed relations, containment, and provenance.

This is useful evidence for the eventual `mh ui` extension boundary. It is **not**
evidence that the graph domain, its schema, its resolver, or its production
operations belong in `magazine-core`.

No private repository name, source name, real record, URL, screenshot,
production path, credential, or operational log is reproduced here. Any future
core test must use synthetic fixtures.

## What the consumer actually requires

The downstream implementation reduces to the following generic UI needs:

1. **Stable opaque deep links.** UI routes must use a stable external key or a
   digest of one. Extension-local SQLite integer IDs must not become route or
   cross-process identities.
2. **Generation-bound reads.** A panel must not mix summaries, members, and
   edges from different materialization generations. Missing or incomplete
   readiness evidence disables the panel rather than showing a partial graph.
3. **Fail-closed conflict handling.** Duplicate stable keys, conflicting
   summaries, ambiguous identity resolution, and malformed relations must not
   be resolved by first-row-wins behavior.
4. **Read-only ownership.** The browser UI is not the durable writer or the
   materialization scheduler. It renders an already-approved read model.
5. **Bounded list/detail navigation.** The shell needs namespaced navigation and
   stable reloadable detail routes; the extension owns the domain-specific
   fields rendered inside them.
6. **Inspectability without domain promotion.** Provenance and relation reasons
   are useful to the operator, but their vocabulary can remain extension-owned
   data. Core does not need to understand the graph domain to host the panel.

These requirements are materially different from a static image gallery, yet
both consumers still converge on the same small shell boundary: registration,
asset delivery, isolated execution, and a narrow read path.

## Promotion classification

| Finding | `magazine-core` candidate | Remains downstream |
| --- | --- | --- |
| Stable route identity | Generic opaque route-key rules and synthetic tests | Domain key construction and matching |
| Asset delivery | Namespaced, path-confined deterministic serving | Framework choice and built assets |
| Browser isolation | Token/parent isolation, outbound policy, management-route separation | Panel-specific JavaScript |
| Data access | Fixed read-only operations or an operator-approved read provider | Graph schema, resolver, generation tables |
| Read consistency | Generation token/ETag-style coherence and fail-closed duplicate handling | Production promotion and rollback workflow |
| Navigation | Route and label registration | Domain-specific page layout and terminology |

## Boundaries retained from the two-repo contract

The following are deliberately **not** promoted:

- Work, membership, relation, containment, or provenance tables in the canonical
  core SQLite schema;
- matcher, resolver, quarantine, confidence, or first-party domain semantics;
- production migrations, materialization, scheduling, backup, promotion,
  rollback, or marker handling;
- private dashboard routes and private source projections;
- arbitrary SQL, manifest-declared host queries, write APIs, or settings APIs;
- real data or fixtures derived from a real source.

The four stable `1.x` contract surfaces remain unchanged:

```text
protocol_version = 1
record_schema_version = 1
Python SDK root API = unchanged
canonical SQLite schema = unchanged
```

## Security implication

The new consumer strengthens the case that a useful extension may ship
non-trivial JavaScript. It also confirms that serving those assets at the shell's
origin without isolation is not an acceptable default: an extension must not be
able to read the `--manage` token, inspect the parent document, call management
routes, inspect another extension, or send collected data to arbitrary external
origins.

Two minimal candidates remain for a real-browser spike:

### Separate loopback origin

- serve extension assets on a distinct loopback origin;
- expose only selected read-only endpoints through an exact CORS allowlist;
- never expose management routes or their token cross-origin;
- apply a restrictive extension response CSP.

### Sandboxed iframe with a narrow broker

- use `sandbox="allow-scripts"` without `allow-same-origin`;
- pass only fixed read operations through a validated `postMessage` broker;
- do not provide arbitrary URL fetch, arbitrary SQL, or host-executed manifest
  queries;
- keep all management state in the parent shell.

The choice must be verified in a real browser. The earlier DOM-shim prototype
cannot establish browser sandbox, origin, CORS, or CSP semantics.

## Synthetic verification plan

Use two fixtures that share the same proposed shell surface:

1. a gallery panel with cards, deep links, a lightbox, and keyboard navigation;
2. a graph panel with stable opaque keys, member edges, typed relations,
   provenance, and a generation-ready flag.

The spike passes only when all of the following are observed in Chromium or
Firefox:

- the extension cannot read the parent DOM or management token;
- the extension cannot call a management route;
- the approved read-only operation succeeds;
- an external-origin fetch is blocked by policy;
- path traversal and duplicate route registration fail closed;
- deep-link reload and keyboard navigation still work;
- disabling extensions leaves the current `mh ui` behavior and attack surface
  unchanged.

## Decision rule

Adopt an extension surface only if both synthetic consumers use the same small
capabilities and no domain-specific Rust route or schema is needed. Reject it if
safe isolation requires weakening the browser security model, if each panel
requires new core code, or if management credentials cannot remain unreachable.

Until that browser evidence exists, this note changes neither the accepted
ordering nor the status of the UI plugin design draft.