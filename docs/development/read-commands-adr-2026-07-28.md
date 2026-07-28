# ADR: `mh view` read commands (2026-07-28)

**Status: ratified by the owner on 2026-07-28.** One implementation slice — the
`mh view` read subcommands — is permitted outside the implementation freeze on
the terms recorded below. No other slice is unfrozen by this decision.

The governing record lives in the downstream freeze document; the addendum text
under "Freeze position" is what was accepted. This file is the reasoning behind
it, kept in the repository the change lands in so a future reader finds the
justification next to the code.

## Context

`mh` exposes four subcommands: `init-db`, `inspect`, `discover`, `ui`. Of these
only `inspect` reads, and it returns whole-database counts — the per-source
breakdown exists nowhere outside the HTTP surface of `mh ui`.

That is a gap for any external reader that is not a browser: an operator script,
a scheduler, or an agent driving `mh` through a shell. Such a reader has three
options today, all bad:

1. Start `mh ui`, hold the port open, and poll HTTP. The read routes are
   unauthenticated by design (loopback trust model), so this leaves collected
   data open to anything local for as long as the process runs.
2. Open the SQLite file directly. This makes column names, indexes and row
   order a de facto public contract and removes the host's ability to migrate
   the canonical schema.
3. Do without.

## Decision

Add `mh view` with three read shapes and no more:

| command | returns |
| --- | --- |
| `mh view sources <db>` | per-source counts and last-seen, plus whole-database totals |
| `mh view posts <db> [--limit N] [--after-id N] [--source NAME] [--include-extra]` | keyset page of post metadata, **without URLs** |
| `mh view assets <db> --post-ids <id,...>` | URL groups for posts the caller names |

Design points that are load-bearing rather than incidental:

- **Keyset, not offset.** `mh ui`'s `/api/records` orders by `updated_at DESC`,
  a mutable column. A record touched between two pages moves, so an offset walk
  over it both repeats and skips rows. `view posts` orders by `id ASC` and
  resumes from `after_id`. A regression test walks a table while re-ingesting a
  row that already went past and asserts it does not reappear.
- **`posts` omits URLs.** A listing is the output most likely to be forwarded
  somewhere else — into a message, a log, or a model's context — and the URLs
  are the bulk of the third-party data (in the local sample, 158 URLs against
  5 titles). Splitting them into `assets` means the caller decides when that
  material leaves the machine. A test asserts no `http` substring survives
  serialization of a `posts` page.
- **`extra` is opt-in.** It carries downstream-private keys that no core code
  interprets.
- **Unknown ids are reported, not dropped.** `assets` returns a `missing` list
  so a typo cannot read as an empty result.
- **Read-only handle.** Every command opens through `Database::open_read_only`.

## Contract position

This is additive under `docs/compatibility-policy.md` "Additive Changes": a new
CLI subcommand with backward-compatible defaults. Specifically it does **not**
touch any stable surface:

- `protocol_version` / `record_schema_version`: unchanged
- Python SDK root API: unchanged
- canonical SQLite schema: unchanged (read paths only; no migration)
- golden fixtures / conformance: unchanged

No new plugin contract, no version namespace, no compatibility tier is created.
That was considered and rejected: a contract needs consumers, and the only two
candidates — a viewer and an agent bridge — were measured to share almost
nothing. See the note under "Rejected alternatives".

## Freeze position — ratified

The 2026-07-21 ratified decision froze implementation and capped the loop, with
the permitted categories being blocking, measurement, maintenance, and docs. A
new subcommand is implementation and therefore **outside that cap**. The
document states that conditions, caps and permit-lists change only by an
addendum to itself.

Accepted addendum text, in the style of the existing §7:

> ### ADR 2026-07-28: `mh view` read commands
>
> - **Decision**: permit one implementation slice in magazine-core — the `mh view`
>   read subcommands — outside the implementation freeze. No other slice is
>   unfrozen by this addendum.
> - **Rationale**: `mh` has no read path outside the HTTP surface of `mh ui`.
>   Every external consumer is therefore pushed either to run an unauthenticated
>   local server continuously or to read the SQLite file directly, which would
>   turn the canonical schema into a de facto public contract. Both are worse
>   for the freeze's own goals than a bounded read command.
> - **Investment ceiling**: production code within magazine-core only, no new
>   crate, no new dependency, no new stable surface. Exceeding this ends the
>   slice rather than extending it.
> - **Pre-registered stop conditions**: (i) if any 2026-08-10 registry-publish
>   precondition is unmet on 2026-08-05, this work stops and the publish takes
>   priority; (ii) if the change requires touching
>   `scripts/verify-standalone-quickstart.sh`, which the release-hardening path
>   invokes, it stops pending separate review.
> - **Falsification conditions** (how we would know this was wrong): (i) no
>   consumer calls `mh view` within one month of merge; (ii) a consumer calls it
>   but cannot express "what changed since last time" through `after_id` and
>   ends up keeping its own state anyway — that would mean the shape does not
>   fit the requirement it was justified by; (iii) the October review finds the
>   read surface has accreted options beyond the three shapes above.
> - **October disposition, registered now**: if magazine-core is ruled a no-go,
>   `mh view` is retained rather than removed — it costs nothing to keep, and
>   removal is itself a breaking change to a published CLI.

## Known limitation, stated rather than hidden

`view posts` answers "what exists", not "what changed since I last looked".
`after_id` advances by insertion, so a caller that wants the second question
must remember the last id it saw. That state lives with the caller, which sits
against the protocol's principle that the host mediates state.

This was left as-is deliberately: fixing it properly means either a change-log
table or a mutable-cursor concept, both of which are larger than the gap they
close and neither of which has a second consumer asking for it. It is written
here so a future reader treats it as a known cost rather than an oversight, and
it is one of the falsification conditions above.

## Rejected alternatives

- **A second plugin contract (DB read surface + UI extension point).** Designed
  and prototyped against two consumers; they shared two of three operation
  shapes and no UI surface at all. Fitting one contract to a viewer and an agent
  bridge made each half dead weight for the other consumer, and the agent side
  is served better by exactly this command-line read path.

  One caveat on that measurement, since it is easy to over-read: the UI half's
  static-asset facility recorded zero consumers, but both prototypes were
  deliberately dependency-free — the case that does not need asset serving. That
  number says nothing about a plugin that ships a bundled framework. See
  `docs/development/ui-plugin-contract-draft-2026-07-28.md`; it does not weaken
  the conclusion here, which rests on the two consumers not overlapping.
- **Serving reads from a long-running `mh ui`.** Requires an unauthenticated
  port to stay open, which is the opposite of the blocking work merged in #32.
- **Letting readers open the SQLite file.** Freezes column names and row order
  as a public contract and gives up fail-closed read-only enforcement.
