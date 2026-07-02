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
