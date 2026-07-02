# release hardening

Run release hardening before cutting a beta tag or downstream beta-candidate
SHA. The script is intentionally self-contained so a downstream lock update can
cite artifact checksums from a repeatable command.

```bash
bash scripts/release-hardening.sh
```

By default, the script requires a clean `main` worktree, including no untracked
files. Use `ALLOW_DIRTY=1` only while developing the script. For an intentional
non-main release ref, set `EXPECTED_RELEASE_REF=<ref>`.

The script performs:

- Rust fmt, clippy, workspace tests, and golden oracle parity.
- Python SDK editable install and pytest in an isolated venv under `dist/`.
- Release build of the `mh` CLI.
- Platform-specific tarball packaging for the `mh` binary.
- Python SDK wheel packaging and install smoke from a second clean venv.
- Sanitized Cargo/Python dependency inventory.
- CycloneDX JSON SBOM generation from Cargo metadata and Python package
  metadata.
- License metadata check for all Cargo packages and MIT license check for
  workspace crates.
- Worktree and git-history secret-pattern scans for common token/private-key
  shapes.
- SHA256 generation for release artifacts (`checksums.sha256` and the public
  release asset name `SHA256SUMS.txt`).
- Linux release artifact quickstart with local UI smoke. The checker consumes
  the freshly generated tarball, wheel, and `SHA256SUMS.txt` through the same
  artifact path used by public cold-start verification, then proves read-only
  UI browsing and guarded `--manage` discover.
- Version discipline check for Cargo package versions, the Python SDK project
  metadata that determines wheel versioning, and release tag / changelog /
  release-note agreement when a version tag is provided.
- Markdown report generation under `dist/release-hardening/`.

`dist/` is ignored because the generated artifacts are release outputs, not
source files. Commit the script and docs, run the script from a clean main
checkout, and then record the resulting checksums in the downstream lock or
release notes for the exact beta tag/SHA.

For repeatable pre-beta evidence, run the manual `Release hardening` workflow.
It executes the same script on Ubuntu and macOS and uploads the report, binary
tarball, Python wheel, CycloneDX SBOM, checksum file, dependency inventory,
license inventory, and secret scan output.
It also uploads `version-discipline.json`, which records the package version
and whether a version tag was compared.

To publish the Linux downstream-consumable assets to an existing GitHub
Release, create the tag/Release so it targets the same commit as the workflow
checkout, then dispatch `Release hardening` with `release_tag` set. After both
hardening jobs pass, the workflow downloads the Linux hardening artifact and
uploads:

- `magazine-core-mh-linux-x86_64.tar.gz`
- the canonical `magazine_core_plugin_sdk-*.whl`
- `sbom.cyclonedx.json`
- `SHA256SUMS.txt`

The publish step verifies the checksum file and refuses to upload if the
Release target commit differs from the checked-out commit. Leave `release_tag`
empty for hardening-only runs.

For release-tag dispatches, the workflow then runs a Linux standalone
cold-start job against the public GitHub Release URL. That job executes
`scripts/verify-standalone-quickstart.sh` with `RELEASE_TAG=<tag>`, downloads
the just-published public artifacts into a temporary directory, verifies
`SHA256SUMS.txt`, installs the wheel with `--no-index --no-deps`, and proves
`init-db -> discover -> inspect` with the synthetic quickstart plugin. For
releases whose binary advertises `mh ui`, the same checker also proves local
read-only UI browse, management disabled in default mode, token/method/bounds
guards in `--manage` mode, and a guarded management discover.

When `release_tag` looks like a release version such as `0.1.0-beta.3`,
release hardening also requires all Cargo packages, the Python SDK version
source from `sdk/python/pyproject.toml` after PEP 440 normalization,
`CHANGELOG.md`, and `docs/release/<tag>.md` to agree with that tag.
Non-version refs such as `main` still verify package metadata consistency but
skip tag-specific changelog and release-note checks.
Historical beta releases that intentionally documented metadata drift, such as
`0.1.0-beta.2`, remain consumable as already-published GitHub Release assets,
but rerunning publish hardening for that tag is intentionally blocked until the
tag, packages, changelog, and release notes agree.

For the beta line, these GitHub Release assets are the authoritative
distribution channel. `cargo install`, PyPI, and Docker are deferred channels
until their release cost, trust boundary, and version discipline are explicitly
accepted; see `docs/development/distribution-channel-decision-2026-07-02.md`.
