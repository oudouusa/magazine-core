# Admin/viewer UI security-boundary evidence

- Date: 2026-07-02
- Scope: C6 local UI trust model documentation
- Contract impact: none

## Decision

`SECURITY.md` now documents the `mh ui` local trust model before any
management-mode endpoint is implemented. The documented boundary keeps C6
contract-neutral:

- no `protocol_version` change;
- no `record_schema_version` change;
- no Python SDK root API change;
- no canonical SQLite schema change;
- no management-mode implementation in this slice.

## Boundary

The security policy records the current read-only UI assumptions:

- `mh ui` is a local operator tool;
- the default viewer binds `127.0.0.1` only;
- the default viewer is read-only and exposes no management endpoints;
- default read-only local browsing has no authentication layer;
- read-only routes accept only `GET` and `HEAD`;
- the server does not emit permissive CORS headers;
- `plugins.d` manifest listing does not execute plugin commands, and redacts
  local path arguments and secret-like environment metadata.

The policy also makes future mutating UI routes conditional on both an
explicit management-mode process opt-in and request-level protection:
unguessable per-process local token or equivalent CSRF-resistant guard,
state-changing `GET` rejection, and loopback `Host` / `Origin` validation
before any mutation or local process-control route runs.

## Failure Modes Prevented

- management endpoints being added after the read-only viewer without a
  documented local trust boundary;
- treating loopback read-only browsing as a remote multi-user security model;
- assuming an explicit management flag is enough without request-level
  protection;
- exposing `mh ui` through a tunnel, reverse proxy, public interface, or shared
  remote host without a separate remote-access model;
- weakening the trusted-plugin model by implying manifest browsing sandboxes or
  executes plugins.

## Verification

This slice is documentation-only, but touches a security boundary. Targeted
checks verify the runtime claims referenced by `SECURITY.md`:

```bash
cargo test -p mh-cli --locked ui::tests -- --nocapture
cargo test -p mh-host --locked inspect_plugin_manifests -- --nocapture
cargo test -p mh-db --locked ui_summary_records_and_known_source_state_are_read_only -- --nocapture
```

Results:

- `cargo test -p mh-cli --locked ui::tests -- --nocapture`: pass, 7 UI tests.
- `cargo test -p mh-host --locked inspect_plugin_manifests -- --nocapture`:
  pass, 1 test.
- `cargo test -p mh-db --locked ui_summary_records_and_known_source_state_are_read_only -- --nocapture`:
  pass, 1 test.

Full core verification:

- `git diff --check`: pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `CARGO_BUILD_JOBS=1 cargo test --workspace --locked`: pass, 74 Rust
  tests.
- `.venv/bin/python -m pip install -e sdk/python pytest`: pass.
- `.venv/bin/python -m pytest sdk/python/tests`: pass, 35 Python tests.
- `bash conformance/check_golden.sh`: pass.
- public-safety grep over changed docs for downstream/private names: pass, no
  matches.

Successful `GET /api/summary` smoke:

```bash
cargo run -p mh-cli -- init-db /tmp/mh-ui-security-smoke-n10HUP/core.db
cargo run -p mh-cli -- ui \
  --db /tmp/mh-ui-security-smoke-n10HUP/core.db \
  --plugins-dir /tmp/mh-ui-security-smoke-n10HUP/plugins.d \
  --port 18766
curl -sS -D - -o /tmp/mh-ui-security-smoke-n10HUP/summary-body.json \
  http://127.0.0.1:18766/api/summary
```

Smoke result:

- `/api/summary`: returned `200 OK`, JSON content type, `Cache-Control:
  no-store`, `X-Content-Type-Options: nosniff`, and no `Access-Control-*`
  headers.
- response body reported schema version 1 with zero records for the scratch DB.

## Adopt / Revert

Adopt if the public security policy clearly documents the local-only,
read-only, no-management default and the future management-mode request guard
requirements while leaving protocol, SDK root API, and canonical schema
unchanged.

Revert if private downstream details enter the public repo, the docs imply
remote access is supported by default, or a management-mode implementation
lands in this slice.

## Residual Risks

- Remote access, authentication, and multi-user deployment remain out of scope.
- Future management-mode implementation still needs dedicated runtime tests for
  request guards, methods, Host / Origin validation, and process-control
  bounds.
