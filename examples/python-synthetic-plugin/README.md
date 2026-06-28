# python synthetic plugin

This example is site-generic. It exercises the Python SDK protocol runtime,
stdout guard, and Rust host ingest path without using real website data.

Run from the repository root:

```bash
cargo run -p mh-cli -- discover ./scratch.db ./plugins.d example
```

The plugin emits one synthetic `SourceRecord` and optional stdout noise. The
stdout guard redirects normal stdout to stderr so framed protocol traffic remains
valid on the original host pipe.
