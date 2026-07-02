# Bounded Plugin Runtime Limits Evidence

- Date: 2026-07-02
- Scope: public-safe synthetic proof for CLI-configurable discover runtime budget
- Contract impact: no `protocol_version` change, no `record_schema_version`
  change, no Python SDK root API change, no canonical DB schema change

## Downstream Pressure

Private downstream routes sometimes need to choose between smaller route splits,
a larger runtime budget, or treating a plugin as stalled. The generic part is
not browser handling, credential management, retries, or source-specific split
policy. The generic part is that the public `mh discover` command previously
used a fixed 60-second plugin deadline while the protocol already carries
`remaining_ms`.

## Synthetic Reproduction

`crates/mh-cli/src/main.rs` includes a synthetic plugin test that:

1. Initializes with a public-safe synthetic manifest.
2. Records the `discover.params.remaining_ms` value.
3. Sleeps for a configurable duration.
4. Returns a valid zero-record discover response when the configured budget
   permits it.

The test proves:

- `mh discover --timeout-seconds 5` succeeds when the synthetic plugin sleeps
  below the chosen budget.
- The plugin receives a `remaining_ms` value that does not exceed the selected
  5-second budget.
- `mh discover --timeout-seconds 1` fails closed with the existing timeout
  error when the same synthetic plugin exceeds the chosen budget.

Existing host tests continue to cover process-tree termination and cancel
notification on timeout.

## Behavior

`mh discover` now accepts:

```text
--timeout-seconds N
```

`N` must be a positive integer number of seconds. If the option is omitted, the
default remains 60 seconds. The host uses the selected budget for the existing
discover deadline and for the `remaining_ms` value sent to the plugin. Timeout
failures remain fail-closed.

## Verification

Targeted during development:

```bash
cargo test -p mh-cli discover_timeout --locked
cargo test -p mh-cli parse_discover_options --locked
```

Full verification for the PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
.venv/bin/python -m pytest sdk/python/tests
bash conformance/check_golden.sh
```

## Downstream Follow-Up

Downstream lock consumption should stay in a separate downstream PR. This core
change can replace only generic timeout-budget plumbing. Private fetcher
details, source-specific split policy, retries, and production cap adoption
gates remain downstream-owned.
