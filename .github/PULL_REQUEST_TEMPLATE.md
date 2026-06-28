<!--
magazine-core is the generic core only. Keep production site adapters, anti-bot
evasion (proxy/cookie/challenge), real site data, credentials, private paths,
and hostnames out of this repo. Examples must stay synthetic.
-->

## What

<!-- One or two sentences on what this change does and why. -->

## Contract impact

- [ ] No change to `protocol_version` / `record_schema_version`
- [ ] No change to the Python SDK stable root API
- [ ] If any of the above changed: docs (`docs/protocol-v1.md`), Rust host,
      Python SDK, and golden fixtures are all updated and stay byte-identical

## Checklist

- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` clean
- [ ] `cargo test --workspace --locked` passes
- [ ] Python SDK tests pass (`pytest sdk/python/tests`)
- [ ] Golden oracle parity (`bash conformance/check_golden.sh`) passes
- [ ] Examples are synthetic; no real site names, captured responses, cookies,
      proxy/challenge logic, credentials, private paths, or hostnames
- [ ] Commits are focused and bisectable
