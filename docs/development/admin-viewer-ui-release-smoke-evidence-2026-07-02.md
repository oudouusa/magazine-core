# Admin/viewer UI release-smoke evidence

- Date: 2026-07-02
- Scope: C6 UI release artifact / quickstart / conformance smoke integration
- Contract impact: none

## Decision

The release-consuming standalone quickstart now verifies the local UI when the
released binary supports `mh ui`. This keeps the C6 UI distribution guarantee in
the same artifact-only path used by release-tag cold-start jobs:

```bash
RELEASE_TAG=<tag> bash scripts/verify-standalone-quickstart.sh
```

The checker remains backward compatible with older tags by defaulting
`VERIFY_UI=auto`. If the downloaded binary does not advertise `mh ui`, UI smoke
is skipped. Release hardening forces `VERIFY_UI=1` for the freshly generated
Linux artifact through `RELEASE_BASE_URL=file://...`, so UI-capable release
artifacts cannot pass hardening without UI smoke.

## Boundary

This slice stays contract-neutral:

- no `protocol_version` change;
- no `record_schema_version` change;
- no Python SDK root API change;
- no canonical SQLite schema change;
- no new runtime Node.js or npm dependency;
- no protocol conformance fixture change.

UI verification remains a host artifact smoke, not a protocol conformance
surface. The checker continues to write the synthetic plugin and manifest in a
temporary artifact-only workspace and does not consume repo-local `plugins.d`,
`examples`, or `sdk/python/src` after downloading assets.

## Coverage

The artifact-only checker still verifies:

- `SHA256SUMS.txt` and all listed assets;
- Linux binary tarball extraction and `mh --help`;
- SDK wheel install with `--no-index --no-deps`;
- `init-db -> discover -> inspect` with one synthetic record.

For UI-capable binaries it also verifies:

- read-only `GET /api/summary`;
- read-only source record browse through `GET /api/records`;
- non-executing `GET /api/plugins` with local workdir redaction;
- `POST /api/summary` returns `405` and emits no permissive CORS headers;
- default read-only mode reports `manage=false`;
- default read-only mode rejects management `POST /api/manage/discover` with
  `403`;
- `--manage` exposes a 64-character same-origin local token in the HTML config;
- bad-token `POST /api/manage/init-db` returns `403` with no permissive CORS;
- state-changing `GET /api/manage/discover` returns `405` and `Allow: POST`;
- oversized management `max_pages=1001` returns `400`;
- guarded management `discover` succeeds and reports one discovered, spooled,
  and ingested record.

## Failure Modes Prevented

- Release artifacts can publish while the bundled UI command is broken.
- Cold-start only proves CLI `discover` and misses UI packaging regressions.
- The quickstart accidentally depends on repo-local plugin or SDK source paths.
- Historical `0.1.0-beta.2` verification fails because it predates `mh ui`.
- Management smoke proves `--manage` exists but not method, token, bound, or
  read-only-disable guards.

## Verification

Targeted checks:

```text
bash -n scripts/verify-standalone-quickstart.sh scripts/release-hardening.sh: pass
shellcheck scripts/verify-standalone-quickstart.sh scripts/release-hardening.sh: pass
RELEASE_TAG=0.1.0-beta.2 bash scripts/verify-standalone-quickstart.sh: pass, UI auto-skip
RELEASE_TAG=codex/ui-release-smoke-20260702 \
  RELEASE_BASE_URL=file:///tmp/mh-release-hardening-ui-smoke/artifacts \
  VERIFY_UI=1 bash scripts/verify-standalone-quickstart.sh: pass
```

Full release hardening final run:

```text
ALLOW_DIRTY=1 CARGO_BUILD_JOBS=1 \
  bash scripts/release-hardening.sh /tmp/mh-release-hardening-ui-smoke-final
```

Result: pass.

Report:
`/tmp/mh-release-hardening-ui-smoke-final/release-hardening-report.md`

Key report entries:

- `git_sha`: `01c52a38aea908ca7c5f03d25d07ab9d93855b06`
- `cargo fmt --all -- --check`: pass
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass
- `cargo test --workspace --locked`: pass
- `conformance/check_golden.sh`: pass
- Python SDK pytest: pass, 35 tests
- CLI init-db / inspect / discover smoke: pass
- binary package: pass
- Python SDK wheel package and install smoke: pass
- release artifact quickstart + UI smoke: pass
- SBOM generation, license scan, secret scans: pass

Final artifact checksums from that run:

```text
7a85b7e9e0593c8e4fbfa71fe491d0ba1617c0daefb8b3d47b6add695f203b42  ./magazine-core-mh-linux-x86_64.tar.gz
b208c2c2e260af735fbb716c75fd019bd37babe5f7d3adcbe3cf03c558324e27  ./magazine_core_plugin_sdk-0.1.0-py3-none-any.whl
06ae8bad1ffe621bd74a9c422b8b88a94a84f29ce2ed4bc027f6ce9f71f3f3f3  ./sbom.cyclonedx.json
```

## Adopt / Revert

Adopt if the artifact-only checker remains backward-compatible with
`0.1.0-beta.2`, release hardening forces UI smoke for UI-capable Linux
artifacts, and full release hardening stays green.

Revert if the quickstart starts requiring repo-local plugin / SDK source paths,
breaks historical artifact verification, weakens management request guards, or
adds protocol / SDK root API / canonical schema changes.

## Residual Risks

- Automated public cold-start is still Linux x86_64 only until additional
  prebuilt host binaries are accepted.
- UI HTTP routes remain local host implementation details and are not a remote
  API contract.
- The next front is C7 1.0 criteria formalization; no new release was cut in
  this slice.
