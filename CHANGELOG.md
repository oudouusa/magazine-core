# Changelog

All notable changes to magazine-core are documented here. This project adheres
to [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [1.3.0] - 2026-08-31

### Added

- Added optional command-manifest `env_from` names so a one-shot host can pass
  only explicitly declared parent environment values to a trusted plugin
  without storing those values in the manifest.

### Security

- Inherited environment declarations reject invalid names, case-insensitive
  duplicates, and overlap with literal `env` during manifest discovery.
  Missing and non-UTF-8 optional values are skipped.
  Undeclared parent variables remain unavailable after `env_clear`, and
  manifest inspection and `PluginDefinition` debug output never expose values.

### Compatibility

- Existing command manifests and the public Rust `PluginDefinition` shape remain
  valid. `protocol_version = 1`,
  `record_schema_version = 1`, the Python SDK root API, and the canonical
  SQLite schema are unchanged from 1.2.1.

## [1.2.1] - 2026-08-31

### Fixed

- Shortened Unix socket fixture paths so the trusted provider permission test
  also fits macOS `sockaddr_un` limits when CI supplies a long temporary root.
- Separated the browser isolation DOM assertions from the bounded WebRTC UDP
  observation so slow ICE completion cannot leave the CI result at `pending`.

### Compatibility

- Runtime behavior and all four stable contracts are unchanged from 1.2.0.

## [1.2.0] - 2026-08-31

### Added

- Added the explicit `mh-ui-ext` binary for trusted local UI extensions. It
  binds only to IPv4 loopback, serves extension assets on a separate origin,
  and brokers only bounded `gallery.list` and `graph.detail` reads over an
  owner-only Unix socket to an operator-started provider.
- Added a real-Chrome regression that performs gallery-to-graph reads in the
  same sandboxed iframe and is now part of CI.
- Added `mh-ui-ext` to the Linux release tarball, CycloneDX SBOM, dependency
  inventory, checksums, and artifact-consuming trusted UI smoke.

### Fixed

- Fixed the browser broker's graph work-key validation, whose control-character
  expression was incorrectly escaped in the generated shell JavaScript.
- Release quickstart verification now auto-detects older artifacts without
  `mh-ui-ext`, while release hardening requires it for 1.2.0 artifacts.
- Chrome gate cleanup now terminates and waits for the complete browser process
  group, preventing profile cleanup races.

### Compatibility

- `protocol_version = 1`, `record_schema_version = 1`, the Python SDK root API,
  and the canonical SQLite schema are unchanged from 1.1.0.

## [1.1.0] - 2026-07-03

### Added

- Added `mh discover --dev-allow-loopback-fetch`, an explicit development-only
  opt-in for local synthetic `host_fetch` smoke tests. The default fetch policy
  still rejects loopback/private-style resolved IPs, and the opt-in only allows
  loopback addresses while preserving allowed-domain, redirect, timeout, body
  size, and header-policy checks.
- Added a repository-only WordPress REST plugin template with synthetic tests,
  example profile/manifest files, and README guidance. Release artifacts do not
  include files under `examples/`.
- Added `scripts/cut-release.sh` and coverage for release execution preflight:
  exact commit SHA validation, release-note/version-discipline checks, tag and
  GitHub Release absence checks, queue checks, exact `target_commitish`
  patch/verification, and release-hardening dispatch.

### Documentation

- Documented the dev-only loopback fetch allowance in the protocol and plugin
  host docs as an operator CLI flag, not a wire-protocol, schema, SDK root API,
  or conformance fixture change.
- Added the WordPress REST example README `Local Smoke (Dev-Only)` flow showing
  the fail-closed loopback default and the explicit development opt-in.
- Clarified that repository examples are outside the stable `1.x` compatibility
  surface and that release execution should use `scripts/cut-release.sh`.

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

[1.3.0]: https://github.com/oudouusa/magazine-core/releases/tag/1.3.0
[1.2.1]: https://github.com/oudouusa/magazine-core/releases/tag/1.2.1
[1.2.0]: https://github.com/oudouusa/magazine-core/releases/tag/1.2.0
[1.1.0]: https://github.com/oudouusa/magazine-core/releases/tag/1.1.0
[1.0.0]: https://github.com/oudouusa/magazine-core/releases/tag/1.0.0
[0.1.0-beta.3]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.3
[0.1.0-beta.2]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.2
[0.1.0-beta.1]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.1
