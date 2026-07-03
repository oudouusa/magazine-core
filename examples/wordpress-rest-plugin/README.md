# WordPress REST plugin template

This is a template for WordPress sites that expose the standard WordPress REST
posts endpoint. Users write their own site profile for sites they are allowed to
fetch, then run the template through the magazine-core host.

Use of this template is your responsibility. You are responsible for complying
with the target site's terms of service, robots policy, copyright rules, load
expectations, and applicable law. This repository does not provide permission to
fetch any third-party site.

## What It Does

- loads one TOML site profile;
- initializes the plugin manifest with the profile's `source_name`,
  `display_label`, and `allowed_domains`;
- fetches WordPress posts only through magazine-core `host_fetch`;
- maps standard WordPress REST fields into `SourceRecord`;
- emits `performers_raw = []` and `page_urls = []`.

The template does not perform direct network access. It does not use cookies,
proxies, challenge handling, or any site-specific parser.

## Profile

Copy `wordpress-rest-sites.example.toml` and edit it for a site you are allowed
to fetch.

The template reads TOML with Python's standard `tomllib` on Python 3.11 and
newer. On Python 3.9 or 3.10, install the optional `tomli` package in the plugin
environment.

```toml
profile_version = 1
source_name = "my_wordpress"
display_label = "My WordPress"
base_url = "https://example.com"
allowed_domains = ["example.com"]
endpoint = "/wp-json/wp/v2/posts"
category_ids = []
default_per_page = 20
default_max_pages = 1
default_max_records = 20
brand_raw = "Example WordPress"
```

`profile_version = 1` is a template config version, not a magazine-core stable
contract. The `source_name` is written to every `SourceRecord`. Keep the
`plugins.d` manifest `id` and the profile `source_name` aligned unless you have
a clear reason not to, because `mh discover` selects by manifest `id` while the
database record identity uses `source_name`.

## Allowed Domains

The host is the authoritative enforcement point. During `initialize`, this
plugin reflects `allowed_domains` into the plugin manifest, and the magazine-core
fetch broker enforces that every `host_fetch` URL and redirect hop is an exact
host match or subdomain match for that manifest list.

The plugin also fails closed before startup when:

- `allowed_domains` is empty;
- `base_url` is not inside `allowed_domains` by exact host or subdomain match.

The plugin rejects non-HTTP(S) `base_url` values and path-only endpoints that are
not rooted at `/`.

## Run

From the repository root:

```bash
cargo run -p mh-cli -- init-db ./scratch-wordpress.db
cargo run -p mh-cli -- discover ./scratch-wordpress.db ./examples/wordpress-rest-plugin/plugins.d my_wordpress --max-pages 1 --per-page 20 --max-records 20 --timeout-seconds 60
cargo run -p mh-cli -- inspect ./scratch-wordpress.db
cargo run -p mh-cli -- ui --db ./scratch-wordpress.db --plugins-dir ./examples/wordpress-rest-plugin/plugins.d --port 8765
```

The checked-in example profile uses `example.com`. Replace it with a profile
for a site you are responsible for before expecting `discover` to fetch real
posts.

## Local Smoke (Dev-Only)

Use this only for a synthetic local host_fetch smoke. The host default rejects
loopback resolved IPs. `--dev-allow-loopback-fetch` is an explicit local
development opt-in; it allows loopback only and prints a stderr warning. Never
use it for production.

From the repository root, start a synthetic WordPress JSON fixture server:

```bash
python3 -m http.server 8766 --bind 127.0.0.1 --directory examples/wordpress-rest-plugin/tests
```

In another shell, create a temporary profile and command manifest:

```bash
mkdir -p tmp/wordpress-local-smoke/plugins.d
cat > tmp/wordpress-local-smoke/profile.toml <<'TOML'
profile_version = 1
source_name = "local_wordpress_smoke"
display_label = "Local WordPress Smoke"
base_url = "http://127.0.0.1:8766"
allowed_domains = ["127.0.0.1"]
endpoint = "/fixture-posts.json"
category_ids = []
default_per_page = 3
default_max_pages = 1
default_max_records = 3
brand_raw = "Synthetic WordPress"
TOML
cat > tmp/wordpress-local-smoke/plugins.d/local-wordpress.json <<'JSON'
{
  "id": "local_wordpress_smoke",
  "argv": [
    "python3",
    "examples/wordpress-rest-plugin/wordpress_rest_plugin.py",
    "tmp/wordpress-local-smoke/profile.toml"
  ],
  "env": {
    "PYTHONPATH": "sdk/python/src"
  },
  "working_dir": "../../.."
}
JSON
```

The fail-closed side should reject loopback and ingest zero records:

```bash
cargo run -p mh-cli -- init-db tmp/wordpress-local-smoke/smoke.db
cargo run -p mh-cli -- discover tmp/wordpress-local-smoke/smoke.db tmp/wordpress-local-smoke/plugins.d local_wordpress_smoke --max-pages 1 --per-page 3 --max-records 3 --timeout-seconds 10
```

Expected stderr includes `resolved IP 127.0.0.1 is not allowed`. With the dev
opt-in, the same synthetic fixture should ingest records:

```bash
cargo run -p mh-cli -- discover tmp/wordpress-local-smoke/smoke.db tmp/wordpress-local-smoke/plugins.d local_wordpress_smoke --max-pages 1 --per-page 3 --max-records 3 --timeout-seconds 10 --dev-allow-loopback-fetch
cargo run -p mh-cli -- inspect tmp/wordpress-local-smoke/smoke.db
```

Expected stderr includes the development-only warning, and `inspect` reports
`source_posts` greater than zero. The fixture records use `example.invalid`
links and media URLs.

## Mapping

- `title`: `post.title.rendered`, with HTML stripped;
- `source_url`: `post.link`;
- `post_date`: the date portion of `post.date`;
- `brand_raw`: profile `brand_raw`;
- `cover_urls`: one absolute HTTP(S) image from `post.content.rendered` only
  when exactly one content `<img src>` is present;
- `performers_raw`: empty list;
- `page_urls`: empty list;
- `extra`: `wordpress_id`, `categories`, `tags`, and `slug`.

`max_pages`, `per_page`, `max_records`, and `remaining_ms` are taken from the
host `discover` request when present. Profile defaults are used only when the
host does not specify a value.

## Compatibility Scope

Files under `examples/` are not part of the magazine-core stable `1.x` surface
and may change without notice. This template's `profile_version = 1` is included
to make profile changes explicit, but it is not a stable public contract.

## Distribution

This example is distributed in the repository only. GitHub Release artifacts
such as the host binary and Python SDK wheel do not include files from
`examples/`.

## Relationship To Private Downstream Plugins

This template is an entry point for external users who want to write their own
WordPress REST site profile. It does not replace existing private downstream
plugins, which may keep private access policy, operational behavior, or
site-specific logic outside this public repository.

## Non-Goals

- `self_fetch`;
- cookie, proxy, or challenge handling;
- image download;
- reader package generation;
- site-specific parsers;
- include or exclude regular expressions;
- custom field mappers;
- real-site fixtures.
