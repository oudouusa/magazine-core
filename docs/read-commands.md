# read commands (`mh view`)

`mh view` answers a question about a core database once and exits. It is the
read path for anything that is not a browser: an operator script, a scheduler,
or an agent driving `mh` through a shell.

`mh inspect` returns whole-database counts only. Everything richer used to exist
solely behind the HTTP surface of `mh ui`, which means holding a port open —
and the read routes there are unauthenticated by design, so leaving one running
exposes collected data to anything local.

All three commands open the database **read-only** and print a single JSON
object to stdout. Errors go to stderr with a non-zero exit, so a caller can
branch on the exit code without parsing prose.

## `mh view sources <db-path>`

What has been collected, per source, plus whole-database totals.

```bash
mh view sources ./core.db
```

```json
{
  "schema_version": 1,
  "totals": {
    "schema_version": 1,
    "source_posts": 5,
    "performers": 4,
    "covers": 5,
    "pages": 158,
    "external_links": 0
  },
  "sources": [
    { "source_name": "example", "records": 5, "last_seen_at": "2026-07-28T00:44:58.175Z" }
  ]
}
```

## `mh view posts <db-path>`

A page of post metadata. **No URLs** — see `assets` below.

| option | default | meaning |
| --- | --- | --- |
| `--limit N` | 20 | page size, clamped to 200 |
| `--after-id N` | — | resume after this post id |
| `--source NAME` | all | restrict to one source |
| `--include-extra` | off | include the downstream-private `extra` payload |

```bash
mh view posts ./core.db --limit 2
```

```json
{
  "limit": 2,
  "after_id": null,
  "returned": 2,
  "next_after_id": 2,
  "posts": [
    {
      "id": 1,
      "source_name": "example",
      "title": "Example title",
      "brand_raw": "Example brand",
      "issue_no": "42",
      "release_date": "2026-06-25",
      "post_date": "2026-06-26",
      "last_seen_at": "2026-07-28T00:44:58.173Z",
      "performers_raw": ["Alice"],
      "counts": { "covers": 1, "pages": 44, "external_links": 0 }
    }
  ]
}
```

### Paging

Pages are keyed on `id`, not on an offset. Follow `next_after_id` until it is
`null`:

```bash
after=""
while :; do
  page=$(mh view posts ./core.db --limit 50 $after)
  echo "$page"
  next=$(echo "$page" | python3 -c 'import sys,json; n=json.load(sys.stdin)["next_after_id"]; print(n if n is not None else "")')
  [ -z "$next" ] && break
  after="--after-id $next"
done
```

The bundled UI's `/api/records` orders by `updated_at DESC`, which is a mutable
column: a record touched between two requests moves, so an offset walk over it
both repeats and skips rows. `mh view posts` orders by `id ASC` so a full walk
is stable even while collection is running.

A short page means the end of the table. `next_after_id` is `null` there, so a
caller stops rather than polling.

### `posts` answers "what exists", not "what changed"

`after_id` advances by insertion order. A caller that wants "new since I last
looked" remembers the last id it saw and passes it back. There is no
server-side cursor and no change log.

## `mh view assets <db-path> --post-ids <id,...>`

URL groups for posts you name explicitly. At most 50 ids per call; duplicates
are collapsed.

```bash
mh view assets ./core.db --post-ids 3,4
```

```json
{
  "assets": [
    {
      "id": 3,
      "source_name": "example",
      "performers_raw": ["Alice"],
      "cover_urls": ["https://example.test/cover-1.jpg"],
      "page_urls": ["https://example.test/page/1"],
      "external_links": []
    }
  ],
  "missing": [4]
}
```

Ids that do not exist come back in `missing` rather than being dropped, so a
typo cannot read as an empty result.

## Why URLs are separate

A listing is the output most likely to be forwarded somewhere else — into a
message, a log file, or a model's context — and the URLs are the bulk of the
collected third-party data. Keeping them behind an explicit `assets` call means
the caller decides when that material leaves the machine, instead of it riding
along with every listing by default.

This is a default, not an access control. Anyone who can run `mh view` can also
read the database file directly. It removes the accident, not the capability.

## Contract position

`mh view` is a CLI surface. Per `docs/compatibility-policy.md` it is additive
and changes no stable contract: `protocol_version`, `record_schema_version`, the
Python SDK root API, and the canonical SQLite schema are all untouched, and no
migration runs.
