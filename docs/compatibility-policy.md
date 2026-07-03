# Compatibility policy

This policy applies to the `1.0.0` stable tag and later `1.x` releases. It does
not by itself create a tag or GitHub Release; release execution is governed by
`docs/release/public-visibility-checklist.md` and the per-release notes.

The `1.0.0` eligibility evidence is collected in `docs/release/1.0.0.md`.

## Stable Surfaces

For the `1.x` line, magazine-core treats these as stable public surfaces:

- protocol v1 wire contract: framing, JSON-RPC methods, `protocol_version`,
  `record_schema_version`, fail-closed behavior, frame limits, `SourceRecord`,
  typed `state_query`, `host_fetch`, cancel, and pinned conformance fixtures;
- canonical SQLite schema and migration behavior for core databases;
- Python SDK root plugin-author API documented in `docs/python-sdk.md`;
- documented CLI quickstart path: install from Release artifacts, `init-db`,
  synthetic `discover`, `inspect`, and local `mh ui` browse / guarded
  management smoke when the packaged binary advertises `mh ui`;
- public Release artifact set: host binary tarball, Python SDK wheel, SBOM,
  `SHA256SUMS.txt`, and release notes.

These surfaces may receive additive compatible improvements in `1.x` releases.
Changing or removing them incompatibly requires a new major version, except for
the security exception below.

## Limited Or Unstable Surfaces

The following are not stable public contracts:

- Python SDK low-level submodules and runtime internals outside the documented
  root plugin-author API;
- repository-only examples under `examples/`, including template config files
  and tests;
- implementation details of the local UI HTTP routes. The packaged UI behavior
  is supported, but those routes are not a remote API contract;
- unpublished distribution channels such as PyPI, Docker images, or
  `cargo install` until a release-channel decision accepts them;
- private adapters, source-specific parsing, production operations, private DB
  migrations, private credentials, and downstream compatibility code.

## Additive Changes

Additive changes in `1.x` must be evidence-driven and public-safe.

Acceptable additive changes include optional protocol capabilities, new typed
`state_query` operations, new Python SDK root helpers, new CLI flags with
backward-compatible defaults, new release artifacts, and new conformance
fixtures.

For any contract-facing additive change:

- old plugins and existing core DBs must keep working;
- new behavior must be optional or negotiated through explicit versioned
  fields;
- docs, tests, and conformance fixtures must land in the same PR when they are
  relevant;
- release notes must name the new capability and its fallback behavior.

## Deprecations

Deprecation is a three-step process:

1. Introduce the replacement first.
2. Accept old and new behavior together while plugin authors and downstream
   consumers migrate.
3. Remove the old behavior only at the documented removal point.

For stable public surfaces, removal waits for a new major version unless the
security exception applies. During the compatibility window, release notes and
migration docs must describe the replacement, the first deprecated release, and
the earliest removal release.

## Breaking Changes

Breaking changes to stable public surfaces require a new major version. When a
breaking change affects the wire protocol, record shape, Python SDK root API,
or canonical DB schema, the corresponding version identifier or documented API
tier must change in the same release plan.

Major-version work must include:

- public migration docs;
- updated protocol / SDK / schema docs as applicable;
- conformance coverage for the new contract;
- release notes that identify the compatibility window and removal boundary.

## Security Exception

Security fixes may restrict unsafe behavior sooner than the normal compatibility
window. Examples include tightening fetch safety, redaction, UI loopback
validation, token handling, or fail-closed behavior.

Security exceptions still require public-safe release notes and migration
guidance. Sensitive details should follow `SECURITY.md` and private reporting
until disclosure is safe.

## Downstream Consumption

Downstream consumers should pin exact tags or commits and verify release
artifacts before adopting a new version. Lock bump PRs should state whether
`protocol_version`, `record_schema_version`, Python SDK root API, or canonical
SQLite schema changed.

Core changes do not modify private downstream adapters or operations directly.
If a private downstream consumer needs a compatibility bridge, that bridge
stays downstream unless it can be reduced to a synthetic, public-safe core
case.
