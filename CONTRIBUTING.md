# Contributing to magazine-core

Thanks for your interest in magazine-core. This document explains what this
project is, what belongs here, and how to propose changes.

magazine-core is a protocol-first ingestion core for publication metadata: a
Rust host, a language-independent stdio plugin protocol, a canonical SQLite
schema, and a Python plugin SDK. It deliberately does **not** include production
site adapters, anti-bot evasion, credentials, deployment configuration,
production databases, or downloaded media. Those live in a separate private
deployment and are out of scope here.

## Scope: what belongs in this repo

In scope:

- protocol v1 and `record_schema` v1 correctness and clarity
- the Rust host, canonical SQLite schema, minimal ingestor, and safe
  `host_fetch` broker
- the Python plugin SDK (the stable plugin-author API and its advanced tiers)
- synthetic examples and conformance fixtures
- docs, release hardening, and packaging

Out of scope (will be declined):

- production site adapters or anything tied to a specific real site
- anti-bot evasion: proxy rotation, cookie-profile spoofing, challenge solving,
  browser impersonation
- real site HTML/JSON, captured responses, downloaded media, real databases,
  logs, or screenshots
- private deployment, scheduling, dashboard, or operations config
- a plugin sandbox (see "Plugin trust model" below and `SECURITY.md`)

If you want to ingest a specific site, write that adapter as your own plugin in
a separate package. The plugin protocol exists precisely so site-specific code
can live outside this repo.

## Plugin trust model

Plugins are **trusted executable code**. Installing and running a plugin grants
it the permissions of the host process. Subprocess isolation separates lifecycle
and crashes; it is not a sandbox. Do not run untrusted plugins. See `SECURITY.md`.

## Protocol and schema changes

`protocol_version` and `record_schema_version` are frozen at `1` for the beta.
Any change to the wire protocol or the `SourceRecord` contract must:

1. keep `docs/protocol-v1.md`, the Rust host, and the Python SDK in agreement;
2. update the golden fixtures so the Rust and Python oracles stay byte-identical;
3. be justified by a real, generic gap — not a speculative or site-specific need.

The Python SDK root API is the stable plugin-author surface. Low-level
framing/runtime helpers are advanced submodule APIs with no stability guarantee.

## Development

```bash
# Rust host
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# Python SDK
python -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests

# Golden oracle parity
bash conformance/check_golden.sh

# CLI smoke
cargo run -p mh-cli -- init-db ./scratch.db
cargo run -p mh-cli -- inspect ./scratch.db
cargo run -p mh-cli -- discover ./scratch.db ./plugins.d example --max-pages 1 --per-page 30 --max-records 30
```

Run the full set before opening a pull request.

## Pull requests

- Keep changes focused and bisectable; one logical change per commit.
- Tests must pass and `fmt`/`clippy` must be clean.
- Examples must stay synthetic. Do not add real site names, captured responses,
  cookies, proxy/challenge logic, private paths, or hostnames.
- Fill in the pull request template, including the contract-impact section.

## Reporting bugs and requesting features

Use the issue templates. For anything that looks like a security issue, follow
`SECURITY.md` and report privately instead of opening a public issue.

## License

By contributing, you agree that your contributions are licensed under the
project's MIT License (see `LICENSE`).
