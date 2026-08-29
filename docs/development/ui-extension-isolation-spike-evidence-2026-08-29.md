# UI extension isolation spike evidence (2026-08-29)

- Status: E1 complete; E2 reject/pivot decision recorded
- Scope: executable, synthetic-only browser security experiment
- Related: issue #36, `downstream-ui-extension-evidence-2026-08-29.md`
- Contract impact: none

## Question

Can a useful `mh ui` extension display both a gallery-shaped read model and a
graph-shaped read model while arbitrary extension JavaScript is prevented from
reaching the shell's management token, management routes, parent DOM, or any
outbound network?

The experiment compares the two candidates named by the downstream evidence:

1. a separate loopback origin with exact read-only CORS;
2. a sandboxed opaque-origin iframe with a narrowly typed `postMessage` broker.

## Invariants

The spike preserves all of the following:

- the current production `mh ui` implementation is unchanged;
- extensions are disabled unless this standalone example is explicitly run;
- `protocol_version = 1` and `record_schema_version = 1` are unchanged;
- the Python SDK root API and canonical SQLite schema are unchanged;
- no runtime Node dependency, bundled framework, private source, real record,
  real URL, screenshot, credential, production path, or operational log enters
  the repository;
- a result is accepted only when observed in a real Chromium-family browser,
  not a DOM shim;
- the outbound probe targets local loopback UDP only.

## Synthetic consumers

The same shell serves two synthetic read shapes:

- `gallery.list`: a generation identifier and two stable opaque item keys;
- `graph.detail`: a stable opaque parent key, members, a typed relation, and a
  generation-ready flag.

The shapes intentionally do not share a domain vocabulary. Their shared surface
is registration, path-confined assets, isolated execution, and fixed read
operations.

## Gates that passed

The executable example proves:

- route names are bounded and normalized;
- duplicate route registration is rejected;
- plain, encoded, absolute, nested, and backslash traversal attempts are
  rejected before file access;
- a sandboxed extension cannot inspect `window.parent.document` or read the
  management-token meta element;
- sandbox/CSP blocks ordinary direct management and external `fetch`;
- the parent broker accepts messages only from the registered iframe window and
  only for the two fixed operations;
- a separate-origin extension cannot inspect the parent document;
- exact CORS permits only selected read routes from the registered origin;
- the management route does not opt into CORS and accepts no mutation;
- both consumers retain a stable deep-link hash and keyboard navigation;
- the complete result repeats on a second clean browser navigation.

The parent shell's `frame-src` allowlist also prevents an iframe from navigating
to an unlisted external origin. That closes the simple location-navigation
channel but not every browser networking API.

## Blocking finding: WebRTC is still a network channel

Each candidate creates an `RTCPeerConnection` configured with a synthetic STUN
server on a local UDP port. In the real browser, both candidates emit STUN
traffic even though:

- the iframe is sandboxed;
- the opaque-origin candidate has no `allow-same-origin`;
- CSP contains `default-src 'none'` and `connect-src 'none'`;
- camera, microphone, geolocation, display capture, and other permissions are
  denied;
- no external URL is contacted by the test.

Therefore iframe sandboxing and ordinary response headers do not establish a
complete outbound-network boundary for arbitrary JavaScript.

## Observed result

The local real-browser run completed with the following shape; packet counts may
vary because STUN retries are implementation-dependent:

```json
{
  "browser": "chromium",
  "csp_fetch_block": "pass",
  "decision": "do not treat arbitrary-JS UI extensions as untrusted; use trusted opt-in or a no-script declarative surface",
  "deep_link_reload": "pass",
  "management_mutations": 0,
  "network_isolation": "reject: WebRTC bypass observed",
  "parent_and_token_isolation": "pass",
  "registry": "pass",
  "sandboxed_webrtc_packets": 1,
  "separate_origin_webrtc_packets": 2
}
```

The repository CI runs the same command. A missing browser or failure to
reproduce the security finding is not silently skipped.

## Decision

**Reject both candidates as an untrusted arbitrary-JavaScript extension
boundary.** This satisfies the preregistered reject condition: unrestricted
outbound networking cannot be claimed to be separated by the tested browser
mechanisms.

Do not remove the successful controls. Sandboxing, parent-window validation,
fixed read operations, strict CSP, exact CORS, path confinement, duplicate-route
rejection, and management-token separation remain useful defense in depth. The
rejected part is the stronger claim that they make malicious JavaScript unable
to transmit data.

## Pivot for the next implementation slice

For the private-panel use case that produced this evidence, the practical next
candidate is an **experimental trusted opt-in extension surface**:

- installing/enabling an extension explicitly grants trusted local code access
  to the read data delivered to it;
- extension mode and `--manage` are mutually exclusive, so no management token
  or mutation route is active in the same shell;
- the iframe remains sandboxed and uses a fixed read-only broker;
- CSP and Permissions Policy remain defense in depth, not a network-sandbox
  promise;
- startup prints a warning that a malicious extension may transmit displayed
  data;
- no write operation, arbitrary SQL, host-executed manifest query,
  cross-extension communication, or implicit discovery is added.

When untrusted content is a requirement, use a separate **no-script declarative
surface** rendered by the core shell. That path requires its own evidence before
a schema is designed; it must not be smuggled into the trusted-JavaScript slice.

The next production-code PR must reproduce the parent/token/management/path and
read-broker portions of this browser gate against the actual Rust-served UI and
must document the intentional trusted-code boundary in `SECURITY.md`.
