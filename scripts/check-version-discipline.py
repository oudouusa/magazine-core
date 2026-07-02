#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any


SEMVER_TAG_RE = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z][0-9A-Za-z.-]*)?)$")
PYTHON_VERSION_RE = re.compile(r"^(?P<base>\d+\.\d+\.\d+)(?:(?P<pre>a|b|rc)(?P<num>\d+))?$")
PRE_MAP = {"a": "alpha", "b": "beta", "rc": "rc"}


def fail(message: str) -> None:
    raise SystemExit(message)


def read_toml(path: Path) -> dict[str, Any]:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        fail(f"missing required file: {path}")


def normalize_release_ref(value: str) -> str | None:
    match = SEMVER_TAG_RE.match(value)
    if not match:
        return None
    return match.group("version")


def normalize_python_version(value: str) -> str:
    match = PYTHON_VERSION_RE.match(value)
    if not match:
        fail(
            "Python SDK version must be PEP 440 base/pre-release form like "
            f"0.1.0 or 0.1.0b3; got {value!r}"
        )
    base = match.group("base")
    pre = match.group("pre")
    if not pre:
        return base
    return f"{base}-{PRE_MAP[pre]}.{match.group('num')}"


def cargo_package_version(manifest: dict[str, Any], workspace_version: str | None, path: Path) -> str:
    package = manifest.get("package") or {}
    version = package.get("version")
    if isinstance(version, str):
        return version
    if isinstance(version, dict) and version.get("workspace") is True and workspace_version:
        return workspace_version
    fail(f"{path} must define package.version or package.version.workspace = true")


def collect_cargo_versions(root: Path) -> list[dict[str, str]]:
    workspace_manifest = read_toml(root / "Cargo.toml")
    workspace = workspace_manifest.get("workspace") or {}
    workspace_package = workspace.get("package") or {}
    workspace_version = workspace_package.get("version")
    members = workspace.get("members") or []
    if not members:
        fail("Cargo workspace has no members")

    packages = []
    for member in members:
        manifest_path = root / member / "Cargo.toml"
        manifest = read_toml(manifest_path)
        package = manifest.get("package") or {}
        name = package.get("name")
        if not isinstance(name, str) or not name:
            fail(f"{manifest_path} has no package.name")
        packages.append(
            {
                "name": name,
                "version": cargo_package_version(manifest, workspace_version, manifest_path),
                "manifest": str(manifest_path.relative_to(root)),
            }
        )
    return packages


def require_changelog_entry(root: Path, release_version: str) -> None:
    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    if f"## [{release_version}]" not in changelog:
        fail(f"CHANGELOG.md is missing release section ## [{release_version}]")
    if f"[{release_version}]:" not in changelog:
        fail(f"CHANGELOG.md is missing link reference [{release_version}]:")


def require_release_doc(root: Path, release_version: str) -> None:
    release_doc = root / "docs" / "release" / f"{release_version}.md"
    if not release_doc.is_file():
        fail(f"missing release notes: {release_doc.relative_to(root)}")
    lines = release_doc.read_text(encoding="utf-8").splitlines()
    if not lines:
        fail(f"{release_doc.relative_to(root)} is empty")
    first_line = lines[0].strip()
    if release_version not in first_line:
        fail(f"{release_doc.relative_to(root)} title does not contain {release_version}")


def build_report(root: Path, release_ref: str) -> dict[str, Any]:
    cargo_packages = collect_cargo_versions(root)
    cargo_versions = sorted({package["version"] for package in cargo_packages})
    if len(cargo_versions) != 1:
        fail(f"Cargo workspace packages have multiple versions: {', '.join(cargo_versions)}")
    cargo_version = cargo_versions[0]

    python_project = read_toml(root / "sdk" / "python" / "pyproject.toml").get("project") or {}
    python_name = python_project.get("name")
    python_raw_version = python_project.get("version")
    if not isinstance(python_name, str) or not isinstance(python_raw_version, str):
        fail("sdk/python/pyproject.toml must define project.name and project.version")
    python_release_version = normalize_python_version(python_raw_version)

    if cargo_version != python_release_version:
        fail(
            "Cargo package version and Python SDK version disagree after normalization: "
            f"cargo={cargo_version}, python={python_raw_version} -> {python_release_version}"
        )

    release_version = normalize_release_ref(release_ref)
    if release_version is not None:
        if release_version != cargo_version:
            fail(
                "Release tag/ref and package metadata disagree: "
                f"release={release_version}, package={cargo_version}"
            )
        require_changelog_entry(root, release_version)
        require_release_doc(root, release_version)

    return {
        "ok": True,
        "release_ref": release_ref,
        "release_version": release_version,
        "package_version": cargo_version,
        "python_project": {
            "name": python_name,
            "version": python_raw_version,
            "normalized_release_version": python_release_version,
        },
        "cargo_packages": cargo_packages,
        "release_checks": {
            "tag_compared": release_version is not None,
            "changelog_required": release_version is not None,
            "release_doc_required": release_version is not None,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Check magazine-core release version discipline.")
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument("--release-ref", default="", help="release tag/ref name; non-version refs skip tag checks")
    parser.add_argument("--json", action="store_true", help="write JSON report")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    report = build_report(root, args.release_ref)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        release = report["release_version"] or "not a release tag"
        print(f"version discipline: ok (package={report['package_version']}, release={release})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit as exc:
        if isinstance(exc.code, str):
            print(f"check-version-discipline: {exc.code}", file=sys.stderr)
            raise SystemExit(1)
        raise
