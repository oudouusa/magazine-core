# Admin/viewer UI ADR

- Date: 2026-07-02
- Scope: C6 admin/viewer UI
- Status: Accepted
- Contract impact: none

## Decision

`magazine-core` will ship a bundled local web UI served by the Rust CLI host.
The user-facing command is `mh ui`; the implementation belongs to the
`mh-cli` package and must be distributed as part of the existing host binary.

Defaults:

- bind to `127.0.0.1` only;
- read-only mode by default;
- no authentication for read-only browsing in the default local trust model;
- no Node.js or npm runtime dependency in the distributed artifact;
- no change to `protocol_version`, `record_schema_version`, the Python SDK
  stable root API, or the canonical SQLite schema.

The UI is a product surface for standalone `magazine-core`, not a downstream
dashboard replacement copied into core. It must stay generic, public-safe, and
synthetic-example compatible.

## Initial CLI Shape

The planned command shape is:

```bash
mh ui --db <core.db> --plugins-dir <plugins.d>
```

The command may accept additional local-only options such as `--port` and
`--open`, but C6 does not accept non-loopback binding. If a later release adds a
host or bind-address option, it must either validate loopback-only addresses or
land as a separate security-boundary ADR with an explicit remote-access model.

Mutating operations are disabled unless the process starts with an explicit
management flag:

```bash
mh ui --db <core.db> --plugins-dir <plugins.d> --manage
```

The `--manage` flag is a process-level opt-in. It does not make plugins
untrusted code, add remote multi-user auth, or weaken the local-only default.
It is necessary but not sufficient for mutating HTTP routes: management
endpoints must also require request-level protection.

## Management Scope

### v1: Read-only viewer

The first implementation slice should expose:

- database summary counts for records, sources, and typed state;
- source record browsing, including `external_links`, covers, and `page_urls`;
- typed state browsing for `known_source_urls`;
- `plugins.d` manifest listing and basic validation status.

The v1 viewer reads only existing core data and manifests. It must not add new
tables, protocol methods, SDK root APIs, or plugin lifecycle semantics.

### v2: Explicit management mode

After the read-only viewer is verified, management mode may add:

- `init-db`;
- bounded `discover` with explicit `max_pages`, `per_page`, `max_records`, and
  timeout limits;
- cancellation for a running local discover process started by the UI.

All write or process-control operations require the explicit `--manage` opt-in.
Management endpoints must also require an unguessable per-process local token
or equivalent CSRF-resistant request guard, reject state-changing `GET`
requests, and validate loopback `Host` / `Origin` assumptions before any
mutation runs. Bounds must use existing CLI/runtime concepts rather than new
protocol semantics.

### Future extension points

The design should leave room for run history, scheduling, and plugin lifecycle
views, but this ADR does not accept those as current implementation scope.
Future additions must pass the same public-safe and contract-neutral checks.

## Asset And Runtime Model

The distributed artifact must not require Node.js, npm, or a separate web app
server at runtime. UI assets should be served by the Rust host as embedded or
otherwise packaged static assets.

If a later implementation uses frontend tooling, the build output must be
generated before packaging and included in release hardening. Runtime execution
from the published binary must still work without installing frontend tooling.

Internal HTTP routes used by `mh ui` are implementation details of the host.
They are not part of the plugin stdio protocol and do not create a public
remote API contract.

## Security Model

The default UI is a local operator tool:

- localhost bind by default;
- no default remote access;
- no default auth layer for read-only local browsing;
- read-only default mode;
- explicit `--manage` opt-in for mutating operations;
- per-request local token or equivalent CSRF-resistant guard for mutating
  operations;
- plugins remain trusted executable code, as documented in `SECURITY.md`.

Opening the UI on a non-local interface, adding authentication, or supporting a
remote multi-user deployment is out of scope for C6. If accepted later, it must
be documented as a separate security-boundary change.

The dedicated C6 security-docs slice should update `SECURITY.md` with this
local trust model before any UI management mode is shipped.

## Alternatives Considered

### TUI

Rejected for the primary surface. A TUI would keep the artifact small and avoid
browser assets, but it is a poor fit for browsing records, covers, links,
typed state, and future run-management views.

### Separate web app or downstream dashboard

Rejected for the core product surface. A separate app would add another
distribution and versioning surface. A downstream dashboard would preserve the
wrong ownership boundary: generic management UI belongs in standalone core.

### Runtime Node.js web server

Rejected. Runtime Node.js would break the standalone binary distribution goal
and make public release artifacts harder to verify. Build-time tooling can be
reconsidered only if release hardening embeds the resulting static assets.

## Verification

This ADR slice is docs-only. It should be verified with:

```bash
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m venv .venv
.venv/bin/python -m pip install -e sdk/python pytest
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

Results on 2026-07-02:

```text
git diff --check: pass
cargo fmt --all -- --check: pass
cargo clippy --workspace --all-targets --locked -- -D warnings: pass
CARGO_BUILD_JOBS=1 cargo test --workspace --locked: pass
.venv/bin/python -m pip install -e sdk/python pytest: pass
.venv/bin/python -m pytest sdk/python/tests: pass, 35 passed
bash conformance/check_golden.sh: pass
code / SDK / conformance / scripts / workflow diff check: pass, no changes
public-safety grep: pass, no ADR-private detail matches
```

The ADR is adopted if the public plan now fixes the UI form, scope,
alternatives, and security boundary while leaving protocol, SDK root API, and
canonical schema unchanged. Revert if private downstream details, real data, or
a contract change enter the slice.
