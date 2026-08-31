# Security policy

## Scope and intent

magazine-core is a generic ingestion framework. It deliberately does **not**
include anti-bot evasion (proxy rotation, cookie-profile spoofing, challenge
solving, browser impersonation). Those, and any site-specific adapters, live in
a separate private deployment and are out of scope here.

The framework **does** own generic safety as first-class behaviour:

- host-mediated fetch enforces http/https only, an allowed-domains allowlist,
  redirect re-validation, SSRF protection (private/loopback/link-local IP
  rejection after DNS resolution, opt-in only), connect and total timeouts
  with body reads included in the total deadline, a 5 MiB raw response body cap,
  system proxy disablement, and rejection of credential or hop-by-hop request
  headers supplied by plugins.
- plugins are trusted executable code; process separation isolates crashes and
  lifecycle, not authority. Untrusted-plugin sandboxing is out of scope.

Security fixes may tighten behavior before the normal compatibility window when
unsafe behavior would otherwise remain exposed. The general post-`1.0.0`
compatibility rules and this security exception are documented in
`docs/compatibility-policy.md`.

## Local UI trust model

`mh ui` is a local operator tool served by the CLI host. The default C6 viewer
is intentionally narrow:

- binds to `127.0.0.1` only;
- stays read-only;
- exposes no management endpoints;
- uses no authentication for default read-only browsing under the local
  loopback trust model;
- accepts only `GET` and `HEAD` routes in the read-only slice;
- does not emit permissive CORS headers;
- lists `plugins.d` manifests without executing plugin commands, and redacts
  local path arguments and secret-like environment metadata.

Do not expose `mh ui` through a public interface, tunnel, reverse proxy, or
shared remote host unless a separate security-boundary change defines and
implements an explicit remote-access model. Loopback binding and read-only
browsing are the security assumptions for the default UI.

## Trusted UI extension host

`mh-ui-ext` is a separate, explicit opt-in binary. It binds only to
`127.0.0.1`, is read-only, accepts no management token or management/write
route, and requires an operator-started owner-only Unix socket provider. The
provider is never started from an extension manifest. Its provider channel is
bounded and generation-bound, and exposes only the fixed `gallery.list` and
`graph.detail` reads.

Installing or enabling an extension grants its packaged JavaScript trusted local
code status. That code can read data delivered through the broker and can
transmit that data to external systems. The shell's sandboxed iframe, strict
CSP, and Permissions Policy are useful defense-in-depth controls, but they are
not a network sandbox and must not be treated as one. The shell and asset
servers use separate loopback origins, namespaced path-confined assets, and a
broker that maps a request to the registered iframe window rather than trusting
an extension name supplied by the message.

Do not run `mh-ui-ext` with extensions that have not been reviewed and
explicitly approved by the operator. Do not expose either loopback listener
through a tunnel, reverse proxy, or shared remote host.

Mutating UI operations require an explicit management-mode process opt-in, but
that opt-in is not sufficient by itself. Before any mutation or local
process-control route runs, the route also requires an unguessable per-process
local token, rejects state-changing `GET` requests, and validates the loopback
`Host` and `Origin` assumptions documented by the admin/viewer UI ADR.

Do not use this framework to access third-party sites without authorisation or
in violation of their terms; ToS/legal compliance is the operator's
responsibility.

## Reporting

Report vulnerabilities privately to the maintainer rather than opening a public
issue.
