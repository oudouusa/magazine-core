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
- SHA256 generation for release artifacts.
- Markdown report generation under `dist/release-hardening/`.

`dist/` is ignored because the generated artifacts are release outputs, not
source files. Commit the script and docs, run the script from a clean main
checkout, and then record the resulting checksums in the downstream lock or
release notes for the exact beta tag/SHA.

For repeatable pre-beta evidence, run the manual `Release hardening` workflow.
It executes the same script on Ubuntu and macOS and uploads the report, binary
tarball, Python wheel, CycloneDX SBOM, checksum file, dependency inventory,
license inventory, and secret scan output.
