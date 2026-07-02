# Changelog

All notable changes to magazine-core are documented here. This project adheres
to [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [1.0.0] - 2026-07-03

### Stable

- Promoted release metadata to `1.0.0` across Cargo packages, `Cargo.lock`,
  and the Python SDK project metadata (`1.0.0` under PEP 440).
- Recorded the `1.0.0` stability-window evidence: from `0.1.0-beta.2` through
  the release-prep base, the protocol source, Python SDK source, conformance
  fixtures, and canonical SQLite schema contract stayed unchanged.
- Added the `1.0.0` release notes that aggregate the public eligibility-gate
  evidence for the stability window, public artifact smoke, standalone
  cold-start, conformance, compatibility policy, and version discipline.

### Documentation

- Updated active release-facing docs for the stable `1.x` compatibility policy
  and release-prep status. There are no protocol, SDK root API, canonical schema,
  or conformance fixture changes from `0.1.0-beta.3`.

## [0.1.0-beta.3] - 2026-07-03

### Added

- Local `mh ui` read-only admin/viewer mode for inspecting summary counts,
  source records, typed state, and redacted plugin manifests from a core DB.
- Explicit `mh ui --manage` mode with request-level token protection,
  state-changing method guards, loopback request validation, bounded
  `init-db` / `discover`, single-run tracking, and cancellation for
  UI-started runs.
- Artifact-only standalone quickstart verification for UI-capable releases,
  covering local read-only browse and guarded management discover from the
  downloaded public binary and SDK wheel.
- Standalone quickstart docs and a release-consuming checker that verifies
  linux-x86_64 GitHub Release artifacts can run `init-db`, synthetic
  `discover`, and `inspect` without repo paths.
- Distribution-channel decision docs record GitHub Release artifacts as the
  beta source of truth and defer `cargo install`, PyPI, and Docker until they
  justify a separate release surface.
- Release hardening now checks version discipline across Cargo packages, the
  Python SDK project metadata that determines wheel versioning, and tag /
  changelog / release-note agreement when a version tag is provided.
- Release-tag hardening dispatches now run a standalone cold-start job after
  publishing Linux assets, verifying public GitHub Release artifacts with the
  artifact-only quickstart checker.

### Documentation

- Added the local UI ADR, read-only UI evidence, security trust model, and
  management-mode evidence while keeping the UI outside the protocol and SDK
  root API contracts.
- Formalized the `1.0.0` eligibility gate, including the still-open need for a
  public beta artifact containing the UI smoke path.
- Documented the post-`1.0.0` compatibility policy for stable `1.x` surfaces,
  additive changes, deprecations, breaking changes, and security exceptions.
- Recorded the decision not to cut `1.0.0` yet because the current-surface beta
  stability window and UI-bearing public artifact evidence are not complete.

## [0.1.0-beta.2] - 2026-07-02

### Added

- `mh discover` accepts optional discover limits:
  host-enforced `--max-records` plus page-scope hints `--max-pages` and
  `--per-page`.
- Protocol conformance now includes a pinned `discover` request fixture with
  optional discover limits.
- Python SDK authors can emit batched records with `send_records()` over the
  existing protocol v1 `records` notification shape.
- `mh discover` accepts a configurable plugin runtime budget with
  `--timeout-seconds`, using the existing protocol v1 `remaining_ms` field.
- Release hardening now emits the public `SHA256SUMS.txt` checksum asset and
  the manual workflow can publish Linux release assets to an existing GitHub
  Release after verifying the target commit.

### Fixed

- The host allows plugins that already sent a final discover response a short
  grace period to exit cleanly before enforcing fail-closed termination.

### Documentation

- Added the public two-repo development contract and refreshed agent guidance
  for evidence-driven post-beta development.

## [0.1.0-beta.1] - 2026-06-28

First public beta of magazine-core: a protocol-first ingestion core for
publication metadata.

### Included

- Rust host with a language-independent stdio plugin protocol
  (`protocol_version = 1`).
- Canonical SQLite schema and a minimal ingestor (`record_schema_version = 1`).
- Safe `host_fetch` broker: http/https only, allowed-domains allowlist, redirect
  re-validation, SSRF protection (private/loopback/link-local rejection after DNS
  resolution, opt-in only), timeouts, body-size cap, system-proxy disablement,
  credential/hop-by-hop header rejection.
- DB-backed typed `state_query`.
- Python plugin SDK (`magazine_core_plugin_sdk`) with a frozen, stable
  plugin-author root API.
- Synthetic examples and Rust/Python conformance fixtures.
- Release hardening tooling and public-contributor docs (CONTRIBUTING,
  CODE_OF_CONDUCT, issue/PR templates, SECURITY).

### Notes

- Plugins are **trusted executable code**. Subprocess isolation separates
  lifecycle and crashes; it is **not** a sandbox. Do not run untrusted plugins.
- This repository does not include production site adapters, anti-bot evasion
  (proxy/cookie/challenge), credentials, deployment config, production
  databases, or downloaded media.
- Crate and Python package versions remain `0.1.0`; `0.1.0-beta.1` is the
  release tag.

### Evidence

Release hardening (fmt, clippy, test, golden oracle parity, SDK pytest, CLI
smoke, binary build, wheel build + install smoke, SBOM, license inventory,
secret scan) passes on linux-x86_64 and macos-arm64. See
`docs/release/0.1.0-beta.1-candidate.md` and the run linked from the GitHub
Release for this tag.

### Known limitations

- The pure-Python wheel is not byte-reproducible across runners; the GitHub
  Release publishes a single canonical wheel.
- Prebuilt binaries are provided for linux-x86_64 and macos-arm64 only.

[1.0.0]: https://github.com/oudouusa/magazine-core/releases/tag/1.0.0
[0.1.0-beta.3]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.3
[0.1.0-beta.2]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.2
[0.1.0-beta.1]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.1
