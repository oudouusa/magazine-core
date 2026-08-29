# UI extension isolation spike

This public-safe example characterizes two browser isolation candidates for a
future `mh ui` extension surface. It is an executable design experiment, not a
stable contract and not production integration.

The fixtures are synthetic and exercise two different consumers through the
same small shell boundary:

- a gallery list with stable item keys;
- a generation-bound graph detail with members and typed relations.

## Candidates

### Sandboxed iframe with a narrow broker

The extension is loaded with `sandbox="allow-scripts"` and without
`allow-same-origin`. It receives only fixed read operations over a validated
`postMessage` channel. Its response CSP uses `connect-src 'none'`.

### Separate loopback origin

The extension is served from a second loopback port inside a sandboxed iframe.
The shell exposes only selected read-only routes through an exact CORS
allowlist. Management routes do not opt into CORS. The extension CSP permits
normal scripted connections only to the exact shell origin.

## Run

A Chromium-family browser must be installed and runnable in headless mode.
`BROWSER_BIN` may select an explicit executable.

```bash
python examples/ui-extension-isolation-spike/verify.py
```

The command starts ephemeral loopback HTTP servers and two local UDP listeners,
launches a clean headless browser profile twice, and verifies all of these in a
real browser:

- parent DOM and management token are unreadable;
- no management mutation is accepted;
- ordinary external `fetch` is blocked by CSP;
- gallery and graph reads succeed through the candidate's narrow read path;
- deep links survive a second navigation;
- keyboard navigation runs;
- path traversal, invalid route names, and duplicate registration fail closed;
- **both arbitrary-JavaScript candidates can still emit WebRTC STUN traffic to
  a loopback UDP listener despite the iframe sandbox and `connect-src 'none'`.**

The UDP probe never contacts an external host. It exists to make the remaining
network channel deterministic and public-safe.

No browser package or JavaScript runtime is added to the release artifact. The
script uses only the Python standard library and an already-installed browser.

## Result

The experiment rejects a security model in which arbitrary extension
JavaScript is treated as untrusted and network-confined by iframe/CSP alone.
Those controls still protect the parent document, management token, management
routes, and common fetch channels, but they are not a complete network sandbox.

Two honest follow-up shapes remain:

1. **trusted opt-in extension** — treat packaged JavaScript like the existing
   trusted executable plugin model, prohibit simultaneous `--manage`, retain
   sandbox/CSP as defense in depth, and state plainly that displayed data may be
   transmitted by a malicious extension;
2. **no-script declarative extension** — accept only validated data and layout
   declarations rendered by the core shell when untrusted extension content is
   required.

See
`docs/development/ui-extension-isolation-spike-evidence-2026-08-29.md` for the
adopt/reject decision and next implementation boundary.
