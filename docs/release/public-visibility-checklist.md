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
      wheel build + install smoke, SBOM, license inventory, secret scan. When
      the workflow is dispatched with `release_tag`, the publish path must also
      pass the standalone cold-start job against the public GitHub Release URL.
- [ ] Secret scan (worktree + full git history) returns zero matches.
- [ ] Examples are synthetic only; no captured site data, credentials, or
      private paths/hostnames anywhere in the tree or history.
- [ ] Community files present: `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`,
      `CODE_OF_CONDUCT.md`, issue templates, PR template.

## 1.0.0 Eligibility Gate

Do not cut a `1.0.0` tag until every item in this section is true and the
release notes link the supporting evidence. A prerelease or beta tag may ship
with fewer guarantees, but it must not claim stable status.

- [ ] A beta stability window is identified by tag or commit range, and that
      window has zero changes to `protocol_version`, `record_schema_version`,
      the Python SDK root plugin-author API, or the canonical SQLite schema.
- [ ] The downstream-consuming release artifact path is proven from public
      GitHub Release assets, not a dirty local checkout: binary tarball, Python
      SDK wheel, `SHA256SUMS.txt`, SBOM, and standalone quickstart all verify.
- [ ] The evidence-driven queue is empty: no accepted generic gap note,
      contract-change PR, public support issue, open PR, or security advisory
      is blocking the stable tag.
- [ ] Protocol, plugin host, Python SDK, release hardening, standalone
      quickstart, and migration/adoption docs describe the same stable
      contract and have no unresolved `1.0` placeholder language.
- [ ] Conformance fixtures are complete for the stable surface and
      `bash conformance/check_golden.sh` passes without fixture drift.
- [ ] The standalone distribution goal is met: a clean user can install from
      Release artifacts and run `init-db -> discover -> inspect` with the
      synthetic quickstart plugin.
- [ ] The admin/viewer UI goal is met: a packaged `mh ui` can browse the
      synthetic DB locally, default management is disabled, `--manage` is
      guarded, and release hardening covers the UI smoke for UI-capable
      artifacts.
- [ ] Post-`1.0.0` compatibility policy is documented before tagging,
      including how additive changes, deprecations, and compatibility windows
      are handled.
- [ ] Version discipline is green for `1.0.0`: Cargo package metadata, Python
      SDK project metadata, `CHANGELOG.md`, `docs/release/1.0.0.md`, and the
      GitHub Release tag agree.

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
- [ ] Standalone synthetic quickstart consumes public Release artifacts only:
      `RELEASE_TAG=<version> bash scripts/verify-standalone-quickstart.sh`
      (or the `standalone cold-start` job from the release-tag workflow run).
      For releases containing `mh ui`, this includes local read-only UI browse
      and guarded `--manage` discover smoke.
- [ ] Published asset SHA-256 values match `SHA256SUMS.txt`.

## Scope reminder

magazine-core ships the generic core only. Production site adapters, anti-bot
evasion (proxy/cookie/challenge), credentials, deployment config, production
databases, and downloaded media are out of scope — see `CONTRIBUTING.md` and
`SECURITY.md`. Plugins are trusted executable code; the host is not a sandbox.

> Making a repository public publishes its entire git history and cannot be
> cleanly undone. Treat the switch as irreversible and complete the pre-release
> gate before flipping visibility.
