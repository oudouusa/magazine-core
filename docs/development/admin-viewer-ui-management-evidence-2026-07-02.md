# Admin/viewer UI management-mode evidence

- Date: 2026-07-02
- Scope: C6 v2 explicit management mode
- Contract impact: none

## Decision

`mh ui` now accepts an explicit `--manage` process opt-in for local management
operations while preserving the default read-only loopback UI. This slice adds:

- guarded `POST /api/manage/init-db`;
- guarded `POST /api/manage/discover` with explicit positive and policy-capped
  `max_pages`, `per_page`, `max_records`, and `timeout_seconds` bounds;
- guarded `POST /api/manage/cancel` for the single active UI-started discover
  run;
- read-only `GET /api/manage/status` for the UI-started run controller.

The implementation stays contract-neutral:

- no `protocol_version` change;
- no `record_schema_version` change;
- no Python SDK root API change;
- no canonical SQLite schema change;
- no runtime Node.js or npm dependency.

## Boundary

The default command remains read-only:

```bash
mh ui --db <core.db> --plugins-dir <plugins.d> [--port N]
```

Management mode is explicit:

```bash
mh ui --db <core.db> --plugins-dir <plugins.d> [--port N] --manage
```

Every mutating or process-control route requires all of the following:

- `--manage` enabled for the process;
- `POST`, with state-changing `GET` rejected;
- loopback `Host` matching the bound `127.0.0.1` / `localhost` port;
- absent or same-origin loopback `Origin`;
- `X-MH-UI-Token` matching the unguessable per-process local token.

The UI does not emit permissive CORS headers. Management token values are not
accepted in query strings.

The discover controller allows at most one active UI-started run. Cancellation
targets only that active run through the new host `CancellationToken`; it sends
the existing protocol `cancel` notification and uses the same process-tree
shutdown path as timeout cleanup. After host discovery completes, the run
transitions through a non-cancellable ingest gate before writing records, so an
accepted cancel cannot be followed by ingest of already-spooled records. It
does not kill arbitrary PIDs or non-UI runs.

## Failure Modes Prevented

- `--manage` alone enabling mutation without request-level protection;
- state-changing `GET`;
- cross-origin or wrong-host mutation against the loopback UI;
- unbounded discover launched from the UI;
- multiple concurrent UI discover runs racing against the same DB;
- cancelling or killing processes that were not started by this UI process;
- partial ingest after a cancelled UI-started discover;
- protocol / SDK root API / canonical schema expansion hidden inside UI work.

## Verification

Targeted characterization:

```bash
cargo test -p mh-cli --locked ui::tests -- --nocapture
cargo test -p mh-host --locked external_cancel_token_sends_cancel_before_terminating -- --nocapture
cargo test -p mh-cli --locked
cargo test -p mh-host --locked
```

Results:

- `cargo test -p mh-cli --locked ui::tests -- --nocapture`: pass, 12 UI tests.
- `cargo test -p mh-host --locked external_cancel_token_sends_cancel_before_terminating -- --nocapture`:
  pass, 1 test.
- `cargo test -p mh-cli --locked`: pass, 19 tests.
- `cargo test -p mh-host --locked`: pass, 27 tests.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.

Full core verification:

- `CARGO_BUILD_JOBS=1 cargo test --workspace --locked`: pass, 80 Rust tests.
- `.venv/bin/python -m pip install -e sdk/python pytest`: pass.
- `.venv/bin/python -m pytest sdk/python/tests`: pass, 35 Python tests.
- `bash conformance/check_golden.sh`: pass.
- `git diff --check`: pass.
- public-safety grep over changed code/docs for downstream/private names:
  pass, no new private/downstream matches.

Live local management smoke:

```bash
cargo run -p mh-cli -- ui \
  --db /tmp/mh-ui-manage-smoke-final-rTMgrO/core.db \
  --plugins-dir /tmp/mh-ui-manage-smoke-final-rTMgrO/plugins.d \
  --port 18769 \
  --manage
```

Smoke result:

- `GET /` exposed local management config with `manage=true` and a 64-character
  per-process token for the same-origin UI.
- bad-token `POST /api/manage/init-db` returned `403 Forbidden` with no
  `Access-Control-*` headers.
- `GET /api/manage/discover` returned `405 Method Not Allowed` and
  `Allow: POST`.
- guarded `POST /api/manage/discover` with `max_pages=1001` returned
  `400 Bad Request`.
- guarded `POST /api/manage/discover` with `plugin_id`, `max_pages=1`,
  `per_page=30`, `max_records=30`, and `timeout_seconds=5` returned
  `202 Accepted`.
- `GET /api/manage/status` reported the UI-started run as `succeeded` with
  `discover_records=1`, `spooled_records=1`, and `ingested_records=1`.
- `GET /api/summary` reported schema version 1 with `source_posts=1` for the
  synthetic scratch DB.

## Adopt / Revert

Adopt if management mode remains explicit, local-only, token-guarded, bounded,
single-run, and cancellable with no protocol / SDK root API / canonical schema
change, and the full core gate is green.

Revert if a mutating route can run without all guards, if state-changing `GET`
is accepted, if cancellation can target arbitrary processes, if partial records
are ingested from cancelled runs, or if private downstream details enter core.

## Residual Risks

- Management HTTP routes remain implementation details of the local host UI,
  not a remote API contract.
- Remote access, authentication, and multi-user deployment remain out of scope.
- The cancel-after-spool race is covered by the non-cancellable ingest gate
  regression and UI cancel integration test; there is no test-only hook between
  a successful host return and the ingest gate beyond that lock-level
  regression.
- Release artifact / quickstart / conformance smoke integration remains the
  next C6 slice.
