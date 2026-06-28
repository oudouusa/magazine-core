# conformance fixture inventory — 2026-06-27

This inventory records which checked-in fixtures and tests cover the protocol
areas required before a `0.1.0-beta` candidate. It is generic and contains no
private source names, real site responses, cookies, proxy/challenge logic, or
downstream extension code.

## summary

| area | fixture / test evidence | status |
|---|---|---|
| host_fetch | `crates/mh-protocol/golden/fetch_request.hex`, `conformance/golden.py`, `crates/mh-protocol/src/golden.rs`, `crates/mh-host/src/lib.rs::fetch_request_returns_provider_response_during_discover_loop`, `crates/mh-fetch/src/lib.rs` safe fetch policy tests, Python SDK `test_runtime_host_fetch_*` | covered |
| typed state | `crates/mh-protocol/golden/state_query.hex`, `conformance/golden.py`, `crates/mh-protocol/src/golden.rs`, `crates/mh-host/src/lib.rs::state_query_returns_provider_values_during_discover_loop`, DB-backed state tests in `crates/mh-db/src/lib.rs`, CLI provider tests in `crates/mh-cli/src/main.rs`, Python SDK typed state tests | covered |
| trusted self_fetch boundary | `docs/protocol-v1.md` §8/§9 and `SECURITY.md`; no `self_fetch` JSON-RPC method exists in `crates/mh-protocol/src/message.rs` | boundary documented; no core broker fixture by design |
| external links | `record.hex` pins a typed non-empty `external_links` array; Rust domain/DB tests and Python SDK model/public API tests round-trip the same typed shape | covered |
| page URLs | `record.hex` pins non-empty `page_urls`; Rust domain/DB tests, Python SDK public API tests, and the synthetic plugin example use non-empty `page_urls` | covered |
| failure behavior | host fail-closed tests for missing providers, malformed requests, early/late frames, record-count mismatch, byte limits, non-clean exit, timeout/cancel/process-group cleanup; DB rollback test; Python SDK malformed host response tests | covered by regression tests |

## fixture decisions

- `state_query.hex` pins a `known_source_urls` request. The other typed state
  operations are behavior-tested in Rust host/DB/CLI and Python SDK tests
  because their wire envelope is identical apart from `op` and `args`.
- `record.hex` pins non-empty `page_urls` and typed `external_links` in the
  same `SourceRecord` fixture.
- `self_fetch` remains a trusted plugin capability boundary, not a host broker
  or JSON-RPC method. Core conformance therefore records the absence of a
  `self_fetch` wire method plus the trust-boundary documentation. Downstream
  adapters own executable evidence for actual trusted self-fetch behavior.
- Negative/failure conformance remains regression-test based instead of pinned
  invalid frames. Invalid frame shape is intentionally broader than a single
  hex fixture, and the host tests assert fail-closed behavior at the handling
  boundaries.

## verification

Run these checks for this inventory:

```bash
cargo test -p mh-protocol --locked
cargo test -p mh-domain --locked
cargo test -p mh-db --locked
cargo test -p mh-host --locked
cargo test -p mh-fetch --locked
cargo test -p mh-cli --locked
python3 -m pytest sdk/python/tests
bash conformance/check_golden.sh
```
