# registry publishing (crates.io / PyPI)

GitHub Release artifacts remain the distribution source of truth for the host
binary and the canonical Python SDK wheel (see
`docs/development/distribution-channel-decision-2026-07-02.md`). Registry
publishing is an **additional** channel for consumers who install from source or
from PyPI, not a replacement.

This document records the exact procedure because **registry publishing is not
reversible and cannot be restarted from the middle**: a crates.io version is
permanent once published (it can only be yanked, and a yanked version still
occupies its version number), and the crates must go up in dependency order.

## Preconditions

- The release tag exists and its GitHub Release assets are verified
  (`docs/release/public-visibility-checklist.md`).
- `cargo publish --dry-run` succeeds for every crate that has no unpublished
  internal dependency (see the note on ordering below).
- No real-site fixture or source-specific name is present anywhere in the tree.
  `scripts/release-hardening.sh` runs this scan; it is a hard gate.

## Internal dependency versions are load-bearing

Every internal dependency in `[workspace.dependencies]` carries **both** `path`
and `version`:

```toml
mh-domain = { path = "crates/mh-domain", version = "1.1.0" }
```

`cargo publish` rejects a path-only dependency with:

```
all dependencies must have a version requirement specified when publishing.
dependency `mh-domain` does not specify a version
```

Local builds keep using the workspace copy through `path`; registry consumers
resolve `version`. **Bump these together with the crate versions in every
release-prep change** — a mismatch is not caught until publish time.

## Publish order

The order is fixed by the internal dependency graph:

```
mh-protocol   (no internal deps)
mh-domain     (no internal deps)
mh-fetch      (no internal deps)
mh-db         -> mh-domain
mh-host       -> mh-domain, mh-fetch, mh-protocol
mh-cli        -> mh-db, mh-host
```

Publish top to bottom. Each crate only becomes resolvable for the next once it
is live on the index, so `cargo publish --dry-run` for a crate with an
unpublished internal dependency fails with:

```
no matching package named `mh-domain` found
location searched: crates.io index
```

**That failure is expected before the dependency is live and is not a defect.**
Verify the leaf crates with `--dry-run`, then publish in order for real, waiting
for the index to update between steps.

```bash
# leaf crates: verify first
cargo publish --dry-run -p mh-protocol
cargo publish --dry-run -p mh-domain
cargo publish --dry-run -p mh-fetch

# then publish in dependency order
cargo publish -p mh-protocol
cargo publish -p mh-domain
cargo publish -p mh-fetch
cargo publish -p mh-db
cargo publish -p mh-host
cargo publish -p mh-cli
```

## What a registry install does and does not give you

State this in the release notes, because it is the most likely source of
confusion for a first-time consumer:

- `cargo install mh-cli` builds the `mh` host from source and needs a Rust
  toolchain at the pinned `rust-version`.
- `pip install magazine-core-plugin-sdk` installs **only the Python plugin-author
  SDK**. It does not install the host. A plugin needs `mh` to run, obtained
  either from the GitHub Release tarball or from `cargo install`.
- Files under `examples/` are repository-only and are not part of any published
  artifact (`docs/compatibility-policy.md`).

## After publishing

- Record the published versions and the observation window start in the release
  notes for that version.
- Registry download counts include mirrors and CI, so they cannot by themselves
  distinguish external adoption from self-traffic. Treat stars / issues / pull
  requests as the signals that are attributable.
