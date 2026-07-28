#!/usr/bin/env python3
"""Fail if a real-world hostname appears anywhere in the public tree.

The public-safe boundary (README.md, AGENTS.md) keeps real source sites out of
this repository: no site-specific adapters, no captured responses, no fixtures
derived from a live site. Until now nothing enforced that mechanically — the
release-hardening secret scan only looks for credential shapes.

This check is deliberately **structural rather than name-based**. Listing the
forbidden site names here would put exactly the material we are excluding into
the public repository, and it would only ever catch names someone remembered to
add. Instead every hostname must fall inside the ranges reserved for
documentation and testing (RFC 2606 / RFC 6761) or, in documentation and release
tooling only, a short allowlist of infrastructure we legitimately reach.
Anything else is reported.

Exit codes:
  0  no real-world hostname found
  1  at least one hostname outside the allowed set
  2  usage or I/O error
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import re
import sys
from pathlib import Path

# RFC 2606 / RFC 6761 reserved for documentation and testing. Anything under
# these can never resolve to a real service.
RESERVED_SUFFIXES = (
    ".test",
    ".example",
    ".invalid",
    ".localhost",
)
RESERVED_EXACT = {
    "localhost",
    "example.com",
    "example.net",
    "example.org",
}

# Hosts that documentation and tooling may reference: project infrastructure and
# specifications. Data-bearing files (fixtures, plugins, host source) get no such
# allowance — a real hostname there is either a captured response or a
# site-specific adapter, which is exactly what the boundary excludes.
INFRA_ALLOWED = {
    "github.com",
    "raw.githubusercontent.com",
    "api.github.com",
    "crates.io",
    "doc.rust-lang.org",
    "docs.rs",
    "www.rust-lang.org",
    "pypi.org",
    "packaging.python.org",
    "peps.python.org",
    "keepachangelog.com",
    "semver.org",
    "spdx.org",
    "cyclonedx.org",
    "www.contributor-covenant.org",
    "developer.mozilla.org",
    "www.rfc-editor.org",
    "datatracker.ietf.org",
}

# Files whose hostnames describe or reach project infrastructure rather than
# carrying source data: documentation, and the release/verification tooling that
# downloads our own published assets.
#
# The allowance is keyed on location as well as extension. A `.txt` sitting in
# `conformance/` is fixture data, not prose, and must not inherit the relaxed
# rule just because of its suffix.
INFRA_SUFFIXES = (".md", ".txt")
INFRA_DIRS = ("scripts",)
# Data-bearing trees. Nothing here may name a real host, whatever the extension.
DATA_DIRS = ("conformance", "crates", "examples", "plugins.d", "sdk")

URL_RE = re.compile(rb"https?://([A-Za-z0-9._~%-]+(?::\d+)?)")

SKIP_DIR_NAMES = {
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    ".pytest_cache",
    ".ruff_cache",
}


def is_reserved(host: str) -> bool:
    """True when the host can never designate a real service."""
    host = host.lower().rstrip(".")
    if host in RESERVED_EXACT:
        return True
    if any(host.endswith(suffix) for suffix in RESERVED_SUFFIXES):
        return True
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return False
    # Literal addresses are allowed only when they cannot leave the machine or
    # the local network: loopback, link-local, private, and unspecified.
    return (
        address.is_loopback
        or address.is_private
        or address.is_link_local
        or address.is_unspecified
    )


def iter_files(root: Path, relative_targets: list[str]):
    for target in relative_targets:
        path = root / target
        if path.is_file():
            yield path
            continue
        if not path.is_dir():
            continue
        for candidate in path.rglob("*"):
            if not candidate.is_file():
                continue
            if any(part in SKIP_DIR_NAMES for part in candidate.parts):
                continue
            yield candidate


def scan(root: Path, relative_targets: list[str]) -> list[dict]:
    findings: list[dict] = []
    for path in iter_files(root, relative_targets):
        try:
            blob = path.read_bytes()
        except OSError as err:  # unreadable file is a hard error, not a pass
            raise SystemExit(f"cannot read {path}: {err}") from err
        if b"\0" in blob[:4096]:  # binary asset
            continue
        relative = path.relative_to(root)
        top = relative.parts[0] if relative.parts else ""
        infra = top not in DATA_DIRS and (
            path.suffix.lower() in INFRA_SUFFIXES or top in INFRA_DIRS
        )
        for line_number, line in enumerate(blob.splitlines(), start=1):
            for match in URL_RE.finditer(line):
                authority = match.group(1).decode("utf-8", "replace")
                host = authority.rsplit(":", 1)[0] if ":" in authority else authority
                # Strip an IPv6 literal's brackets before classification.
                host = host.strip("[]")
                if is_reserved(host):
                    continue
                if infra and host.lower() in INFRA_ALLOWED:
                    continue
                findings.append(
                    {
                        "path": str(relative),
                        "line": line_number,
                        "host": host,
                        "infra": infra,
                    }
                )
    return findings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument("--json", action="store_true", help="emit JSON")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    if not root.is_dir():
        print(f"root is not a directory: {root}", file=sys.stderr)
        return 2

    targets = [
        "AGENTS.md",
        "CHANGELOG.md",
        "CONTRIBUTING.md",
        "README.md",
        "SECURITY.md",
        "conformance",
        "crates",
        "docs",
        "examples",
        "plugins.d",
        "scripts",
        "sdk",
    ]
    findings = scan(root, targets)

    if args.json:
        print(json.dumps({"ok": not findings, "findings": findings}, indent=2, sort_keys=True))
    elif findings:
        print("real-world hostnames found in the public tree:", file=sys.stderr)
        for item in findings:
            scope = "infra" if item["infra"] else "data"
            print(f"  {item['path']}:{item['line']} [{scope}] {item['host']}", file=sys.stderr)
        print(
            "\nEvery hostname must be RFC 2606/6761 reserved (.test/.example/.invalid),\n"
            "a loopback or private literal, or — in docs and scripts only — project\n"
            "infrastructure listed in INFRA_ALLOWED. A real source site must never\n"
            "reach this repository.",
            file=sys.stderr,
        )
    else:
        print("public_safe_hosts=ok")

    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
