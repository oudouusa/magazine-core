# Admin/viewer UI read-only evidence

- Date: 2026-07-02
- Scope: C6 v1 read-only viewer
- Contract impact: none

## Decision

`mh ui` now serves the first bundled read-only local viewer from the existing
Rust CLI binary. The v1 surface includes:

- core DB summary and per-source counts;
- source record browsing with performers, covers, page URLs, typed external
  links, and timestamps;
- typed `known_source_urls` state grouped by source;
- best-effort `plugins.d` command manifest listing.

The implementation stays contract-neutral:

- no `protocol_version` change;
- no `record_schema_version` change;
- no Python SDK root API change;
- no canonical SQLite schema change;
- no runtime Node.js or npm dependency.

## Boundary

The v1 command shape is:

```bash
mh ui --db <core.db> --plugins-dir <plugins.d> [--port N]
```

The server binds `127.0.0.1` only. `--host`, `--bind`, and `--manage` are
rejected in this read-only slice. HTTP routes accept only `GET` and `HEAD`;
other methods return `405 Method Not Allowed` before DB route handling. The
server does not emit permissive CORS headers.

`/api/plugins` reads JSON command manifests without executing plugin argv.
Manifest argv path arguments and secret-like environment key names are redacted,
and environment values are never returned.

## Failure Modes Prevented

- a standalone UI accidentally becoming a downstream dashboard copy;
- remote bind or management mode landing in the read-only v1 slice;
- plugin execution during manifest browsing;
- writable DB access from read-only API routes;
- protocol / SDK / canonical schema expansion hidden inside a product UI slice;
- local private paths or environment secrets leaking through plugin manifest
  JSON.

## Verification

Targeted characterization:

```bash
cargo test -p mh-db --locked
cargo test -p mh-host --locked inspect_plugin_manifests -- --nocapture
cargo test -p mh-cli --locked ui::tests -- --nocapture
```

Full verification:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
git diff --check
```

Results:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `CARGO_BUILD_JOBS=1 cargo test --workspace --locked`: pass, 73 Rust tests.
- `.venv/bin/python -m pytest sdk/python/tests`: pass, 35 tests.
- `bash conformance/check_golden.sh`: pass.
- `git diff --check`: pass.
- public-safety grep over changed Rust source for downstream/private names:
  pass, no matches.

CLI smoke:

```bash
cargo run -p mh-cli -- init-db /tmp/mh-ui-smoke-4YtrNB/core.db
cargo run -p mh-cli -- ui \
  --db /tmp/mh-ui-smoke-4YtrNB/core.db \
  --plugins-dir /tmp/mh-ui-smoke-4YtrNB/plugins.d \
  --port 18765
curl -sS http://127.0.0.1:18765/api/summary
curl -sS http://127.0.0.1:18765/api/plugins
curl -sS -i -X POST http://127.0.0.1:18765/api/summary
curl -sS -I http://127.0.0.1:18765/
```

Smoke result:

- `/api/summary`: returned schema version 1 with zero records for the scratch DB.
- `/api/plugins`: returned an empty plugin list for the scratch `plugins.d`.
- `POST /api/summary`: returned `405 Method Not Allowed`, `Allow: GET, HEAD`.
- `HEAD /`: returned `200 OK` with empty body.

Review result:

- read-only review found no blocking runtime or security-boundary issue.
- follow-up tests were added for `--bind` rejection, socket-level loopback
  binding, non-execution of plugin argv during manifest inspection, and
  secret-like environment key redaction.

## Adopt / Revert

Adopt if the `mh ui` v1 viewer remains read-only, loopback-only, plugin
manifest browsing stays non-executing, and all verification above remains
green.

Revert if any of the following appear:

- non-loopback binding or management mode enters v1;
- mutating HTTP endpoints are accepted without the later management-mode guard;
- manifest listing executes plugin code;
- protocol, Python SDK root API, or canonical DB schema changes are required;
- private downstream details or secrets leak into core.

## Residual Risks

- The HTTP routes are implementation details, not a stable remote API.
- v1 has no authentication because it is loopback-only and read-only. Remote
  access, auth, and management mode remain separate security-boundary work.
- UI smoke is local/manual in this slice; release artifact / quickstart
  integration remains the later C6 checkbox.
