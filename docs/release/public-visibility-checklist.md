# Public release / visibility checklist

A sanitized checklist for taking a magazine-core release public. It captures the
gates and verification steps without any deployment-specific operational detail.

## Pre-release gate

- [ ] `main` is clean; no open PRs that must ship first.
- [ ] `protocol_version = 1` and `record_schema_version = 1` are unchanged and
      consistent across `docs/protocol-v1.md`, the Rust host, and the Python SDK.
- [ ] Golden fixtures match the Python oracle (`bash conformance/check_golden.sh`).
- [ ] Release hardening is green on supported platforms
      (`bash scripts/release-hardening.sh`, or the manual `Release hardening`
      workflow): fmt, clippy, `cargo test`, SDK pytest, CLI smoke, binary build,
      wheel build + install smoke, SBOM, license inventory, secret scan.
- [ ] Secret scan (worktree + full git history) returns zero matches.
- [ ] Examples are synthetic only; no captured site data, credentials, or
      private paths/hostnames anywhere in the tree or history.
- [ ] Community files present: `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`,
      `CODE_OF_CONDUCT.md`, issue templates, PR template.

## Cut the release

- [ ] Update `CHANGELOG.md` and `docs/release/<version>.md`.
- [ ] Tag the release commit and publish a GitHub Release.
- [ ] Attach a single canonical Python wheel, prebuilt host binaries for the
      supported platforms, the CycloneDX SBOM, and a `SHA256SUMS.txt`.
- [ ] Record the hardening run and artifact checksums in the Release notes.

## After going public

Verify from a clean, unauthenticated clone:

- [ ] `git clone` works anonymously; README, Actions, tags, and the Release render.
- [ ] `cargo build --release -p mh-cli --locked` succeeds.
- [ ] `pip install` the released wheel into a fresh venv; `import magazine_core_plugin_sdk`.
- [ ] Synthetic quickstart: `mh init-db` / `mh inspect` / `mh discover ... example`.
- [ ] Published asset SHA-256 values match `SHA256SUMS.txt`.

## Scope reminder

magazine-core ships the generic core only. Production site adapters, anti-bot
evasion (proxy/cookie/challenge), credentials, deployment config, production
databases, and downloaded media are out of scope — see `CONTRIBUTING.md` and
`SECURITY.md`. Plugins are trusted executable code; the host is not a sandbox.

> Making a repository public publishes its entire git history and cannot be
> cleanly undone. Treat the switch as irreversible and complete the pre-release
> gate before flipping visibility.
