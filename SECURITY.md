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

Do not use this framework to access third-party sites without authorisation or
in violation of their terms; ToS/legal compliance is the operator's
responsibility.

## Reporting

Report vulnerabilities privately to the maintainer rather than opening a public
issue.
