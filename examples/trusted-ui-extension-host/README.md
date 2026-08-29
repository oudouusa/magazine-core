# Trusted UI extension host gate

This gate launches the production `mh-ui-ext` binary, an owner-only synthetic
Unix-socket provider, and a clean real Chromium profile. It verifies the fixed
two-operation contract, generation binding, sandboxed parent-DOM isolation,
the asset CSP boundary, and the absence of a management route. UI-specific
deep-link and keyboard behavior stays in the downstream panel contract tests.

Build both CLI binaries first, then run the gate:

```bash
cargo build -p mh-cli --bins --locked
python3 examples/trusted-ui-extension-host/verify.py
```

`BROWSER_BIN`, `MH_BIN`, and `MH_UI_EXT_BIN` may select explicit binaries. The
fixture is local and synthetic. This gate does not claim network confinement
for arbitrary JavaScript; the separate WebRTC isolation sentinel continues to
document why this host is explicitly trusted and opt-in.
