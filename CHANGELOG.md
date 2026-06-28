# Changelog

All notable changes to magazine-core are documented here. This project adheres
to [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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

[0.1.0-beta.1]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.1
