#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-${ROOT}/dist/release-hardening}"
ARTIFACT_DIR="${OUT_DIR}/artifacts"
VENV_DIR="${OUT_DIR}/venv"
WHEEL_VENV_DIR="${OUT_DIR}/wheel-venv"
REPORT="${OUT_DIR}/release-hardening-report.md"
CARGO_METADATA="${OUT_DIR}/cargo-metadata.raw.json"
DEPENDENCY_INVENTORY="${OUT_DIR}/dependency-inventory.json"
SBOM="${ARTIFACT_DIR}/sbom.cyclonedx.json"
BINARY_NAME="mh"
EXPECTED_RELEASE_REF="${EXPECTED_RELEASE_REF:-main}"

release_ref="${RELEASE_REF_NAME:-${GITHUB_REF_NAME:-}}"
if [[ -z "${release_ref}" ]]; then
  release_ref="$(git -C "${ROOT}" symbolic-ref --quiet --short HEAD || git -C "${ROOT}" rev-parse --short HEAD)"
fi
github_ref="${GITHUB_REF:-}"
github_sha="${GITHUB_SHA:-}"

if [[ "${ALLOW_DIRTY:-0}" != "1" ]]; then
  dirty_status="$(git -C "${ROOT}" status --porcelain --untracked-files=all)"
  if [[ -n "${dirty_status}" ]]; then
    echo "release hardening requires a clean git worktree including untracked files; set ALLOW_DIRTY=1 for script development" >&2
    echo "${dirty_status}" >&2
    exit 1
  fi
  if [[ -n "${EXPECTED_RELEASE_REF}" && "${release_ref}" != "${EXPECTED_RELEASE_REF}" ]]; then
    echo "release hardening must run from ${EXPECTED_RELEASE_REF}; got ${release_ref}" >&2
    echo "set EXPECTED_RELEASE_REF=<ref> for an intentional non-main release ref or ALLOW_DIRTY=1 for script development" >&2
    exit 1
  fi
fi

rm -rf "${OUT_DIR}"
mkdir -p "${ARTIFACT_DIR}"

sha="$(git -C "${ROOT}" rev-parse HEAD)"
platform="$(uname -s)-$(uname -m)"
case "$(uname -s)" in
  Linux) os_slug="linux" ;;
  Darwin) os_slug="macos" ;;
  *) os_slug="$(uname -s | tr '[:upper:]' '[:lower:]')" ;;
esac
arch_slug="$(uname -m)"
platform_slug="${os_slug}-${arch_slug}"
binary_package="magazine-core-mh-${platform_slug}.tar.gz"
started_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

run() {
  echo "+ $*" >&2
  "$@"
}

checksum_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$@"
  else
    shasum -a 256 "$@"
  fi
}

run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets --locked -- -D warnings
run cargo test --workspace --locked
run bash "${ROOT}/conformance/check_golden.sh"

run python3 -m venv "${VENV_DIR}"
run "${VENV_DIR}/bin/python" -m pip install -e "${ROOT}/sdk/python" pytest
run "${VENV_DIR}/bin/python" -m pytest "${ROOT}/sdk/python/tests"
run "${VENV_DIR}/bin/python" -m pip install "setuptools>=68" wheel

run cargo build --release -p mh-cli --locked
run "${ROOT}/target/release/mh" --help
run "${ROOT}/target/release/mh" init-db "${OUT_DIR}/scratch.db"
run "${ROOT}/target/release/mh" inspect "${OUT_DIR}/scratch.db"
run "${ROOT}/target/release/mh" discover "${OUT_DIR}/scratch.db" "${ROOT}/plugins.d" example
run tar -czf "${ARTIFACT_DIR}/${binary_package}" -C "${ROOT}/target/release" "${BINARY_NAME}"
run "${VENV_DIR}/bin/python" -m pip wheel --no-deps --no-build-isolation -w "${ARTIFACT_DIR}" "${ROOT}/sdk/python"
wheel_files=("${ARTIFACT_DIR}"/magazine_core_plugin_sdk-*.whl)
if [[ "${wheel_files[0]}" == "${ARTIFACT_DIR}/magazine_core_plugin_sdk-*.whl" || ! -f "${wheel_files[0]}" ]]; then
  echo "Python SDK wheel was not produced" >&2
  exit 1
fi
python_wheel="${wheel_files[0]}"
run python3 -m venv "${WHEEL_VENV_DIR}"
run "${WHEEL_VENV_DIR}/bin/python" -m pip install --no-deps "${python_wheel}"
run "${WHEEL_VENV_DIR}/bin/python" - <<'PY'
import io

import magazine_core_plugin_sdk as sdk
from magazine_core_plugin_sdk.framing import read_json_frame, write_json_frame

record = sdk.SourceRecord(
    source_name="wheel-smoke",
    source_url="golden://wheel-smoke/1",
    title="Wheel Smoke",
    brand_raw="Smoke",
    page_urls=["golden://wheel-smoke/1/page/1"],
    external_links=[sdk.ExternalLink(url="https://example.invalid/item/1", provider="example")],
)
payload = record.to_dict()
assert sdk.PROTOCOL_VERSION == 1
assert sdk.RECORD_SCHEMA_VERSION == 1
assert payload["page_urls"] == ["golden://wheel-smoke/1/page/1"]
buffer = io.BytesIO()
write_json_frame(buffer, {"jsonrpc": "2.0", "method": "record", "params": payload})
buffer.seek(0)
assert read_json_frame(buffer)["params"]["external_links"][0]["provider"] == "example"
PY

run cargo metadata --locked --format-version 1 > "${CARGO_METADATA}"

python3 - "${ROOT}" "${CARGO_METADATA}" > "${OUT_DIR}/license-inventory.txt" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

root = Path(sys.argv[1])
metadata = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
missing = []
licenses = Counter()
workspace_missing_mit = []

for package in metadata["packages"]:
    license_value = package.get("license")
    license_file = package.get("license_file")
    if not license_value and not license_file:
        missing.append(f"{package['name']} {package['version']}")
    licenses[license_value or f"file:{license_file}"] += 1
    manifest = package.get("manifest_path") or ""
    if manifest.startswith(str(root)) and package.get("source") is None:
        if license_value != "MIT":
            workspace_missing_mit.append(f"{package['name']} {package['version']}: {license_value!r}")

if missing:
    raise SystemExit("packages missing license metadata:\n" + "\n".join(missing))
if workspace_missing_mit:
    raise SystemExit("workspace packages without MIT license:\n" + "\n".join(workspace_missing_mit))

print(f"packages={len(metadata['packages'])}")
print("missing_license_metadata=0")
print("workspace_license=MIT")
print("licenses:")
for name, count in sorted(licenses.items()):
    print(f"- {name}: {count}")
PY

python3 - "${ROOT}" "${CARGO_METADATA}" "${SBOM}" "${DEPENDENCY_INVENTORY}" "${sha}" "${platform_slug}" "${binary_package}" "$(basename "${python_wheel}")" <<'PY'
import json
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from uuid import UUID, uuid5

root = Path(sys.argv[1])
metadata = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
out = Path(sys.argv[3])
inventory_out = Path(sys.argv[4])
sha = sys.argv[5]
platform_slug = sys.argv[6]
binary_package = sys.argv[7]
python_wheel = sys.argv[8]
python_project = tomllib.loads((root / "sdk/python/pyproject.toml").read_text(encoding="utf-8"))["project"]


def license_entries(package: dict) -> list[dict]:
    license_value = package.get("license")
    if license_value:
        return [{"license": {"expression": license_value}}]
    license_file = package.get("license_file")
    if license_file:
        return [{"license": {"name": f"file:{license_file}"}}]
    return []


components = []
component_refs = []
package_refs = {}
seen_refs = {}
for package in sorted(metadata["packages"], key=lambda value: value["id"]):
    base_ref = f"pkg:cargo/{package['name']}@{package['version']}"
    seen_count = seen_refs.get(base_ref, 0)
    seen_refs[base_ref] = seen_count + 1
    bom_ref = base_ref if seen_count == 0 else f"{base_ref}#{seen_count + 1}"
    package_refs[package["id"]] = bom_ref
    component = {
        "type": "library",
        "name": package["name"],
        "version": package["version"],
        "bom-ref": bom_ref,
        "purl": f"pkg:cargo/{package['name']}@{package['version']}",
    }
    licenses = license_entries(package)
    if licenses:
        component["licenses"] = licenses
    if package.get("source") is None:
        component["scope"] = "required"
    components.append(component)
    component_refs.append(bom_ref)

python_ref = f"pkg:pypi/{python_project['name']}@{python_project['version']}"
components.append(
    {
        "type": "library",
        "name": python_project["name"],
        "version": python_project["version"],
        "bom-ref": python_ref,
        "purl": python_ref,
        "licenses": [{"license": {"expression": python_project["license"]["text"]}}],
        "properties": [{"name": "release.wheel", "value": python_wheel}],
    }
)
component_refs.append(python_ref)

binary_ref = f"artifact:{binary_package}"
components.append(
    {
        "type": "application",
        "name": "magazine-core-mh",
        "version": sha,
        "bom-ref": binary_ref,
        "properties": [
            {"name": "release.platform", "value": platform_slug},
            {"name": "release.package", "value": binary_package},
        ],
    }
)
component_refs.append(binary_ref)

dependencies = [{"ref": "magazine-core", "dependsOn": component_refs}]
for node in metadata.get("resolve", {}).get("nodes", []):
    dependencies.append(
        {
            "ref": package_refs.get(node["id"], node["id"]),
            "dependsOn": [package_refs.get(dep["pkg"], dep["pkg"]) for dep in node.get("deps", [])],
        }
    )
dependencies.append({"ref": python_ref, "dependsOn": list(python_project.get("dependencies", []))})
dependencies.append({"ref": binary_ref, "dependsOn": []})

bom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{uuid5(UUID('b4b0bb66-f3a7-44f2-a0af-79c6b0a7dd80'), sha + platform_slug)}",
    "version": 1,
    "metadata": {
        "timestamp": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "component": {
            "type": "application",
            "name": "magazine-core",
            "version": sha,
            "bom-ref": "magazine-core",
        },
        "tools": {
            "components": [
                {
                    "type": "application",
                    "name": "scripts/release-hardening.sh",
                    "version": "1",
                }
            ]
        },
    },
    "components": components,
    "dependencies": dependencies,
}
out.write_text(json.dumps(bom, indent=2, sort_keys=True) + "\n", encoding="utf-8")

inventory = {
    "schema": "magazine-core.release-dependency-inventory.v1",
    "git_sha": sha,
    "platform": platform_slug,
    "cargo_packages": [
        {
            "name": package["name"],
            "version": package["version"],
            "license": package.get("license"),
            "source": "workspace" if package.get("source") is None else "registry",
            "purl": f"pkg:cargo/{package['name']}@{package['version']}",
        }
        for package in sorted(metadata["packages"], key=lambda value: (value["name"], value["version"], value["id"]))
    ],
    "python_packages": [
        {
            "name": python_project["name"],
            "version": python_project["version"],
            "license": python_project["license"]["text"],
            "dependencies": list(python_project.get("dependencies", [])),
            "purl": python_ref,
        }
    ],
}
inventory_out.write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

secret_pattern='(AKIA[0-9A-Z]{16}|-----BEGIN (RSA|DSA|EC|OPENSSH) PRIVATE KEY-----|ghp_[A-Za-z0-9_]{36,}|xox[baprs]-[A-Za-z0-9-]{10,})'
set +e
rg -n "${secret_pattern}" \
  "${ROOT}/.github" \
  "${ROOT}/conformance" \
  "${ROOT}/crates" \
  "${ROOT}/docs" \
  "${ROOT}/examples" \
  "${ROOT}/plugins.d" \
  "${ROOT}/sdk" \
  "${ROOT}/AGENTS.md" \
  "${ROOT}/Cargo.toml" \
  "${ROOT}/Cargo.lock" \
  "${ROOT}/README.md" \
  "${ROOT}/SECURITY.md" > "${OUT_DIR}/secret-scan.txt"
secret_status=$?
set -e
if [[ "${secret_status}" -eq 0 ]]; then
  echo "secret-like material found; see ${OUT_DIR}/secret-scan.txt" >&2
  exit 1
elif [[ "${secret_status}" -eq 1 ]]; then
  echo "secret_scan_matches=0" > "${OUT_DIR}/secret-scan.txt"
else
  echo "secret scan failed with rg exit ${secret_status}" >&2
  exit "${secret_status}"
fi

set +e
# git rev-list emits revision arguments; intentional splitting feeds git grep.
# shellcheck disable=SC2046
git -C "${ROOT}" grep -I -n -E "${secret_pattern}" $(git -C "${ROOT}" rev-list --all) \
  -- . > "${OUT_DIR}/secret-history-scan.txt"
history_secret_status=$?
set -e
if [[ "${history_secret_status}" -eq 0 ]]; then
  echo "secret-like material found in git history; see ${OUT_DIR}/secret-history-scan.txt" >&2
  exit 1
elif [[ "${history_secret_status}" -eq 1 ]]; then
  echo "secret_history_scan_matches=0" > "${OUT_DIR}/secret-history-scan.txt"
else
  echo "git history secret scan failed with exit ${history_secret_status}" >&2
  exit "${history_secret_status}"
fi

(
  cd "${ARTIFACT_DIR}"
  checksum_file ./* > checksums.sha256
)

binary_sha="$(awk -v file="${binary_package}" '{path=$2; sub(/^\.\//, "", path); if (path == file) print $1}' "${ARTIFACT_DIR}/checksums.sha256")"
python_sha="$(awk '{path=$2; sub(/^\.\//, "", path); if (path ~ /^magazine_core_plugin_sdk-.*\.whl$/) print $1}' "${ARTIFACT_DIR}/checksums.sha256")"
sbom_sha="$(awk '{path=$2; sub(/^\.\//, "", path); if (path == "sbom.cyclonedx.json") print $1}' "${ARTIFACT_DIR}/checksums.sha256")"
finished_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

cat > "${REPORT}" <<EOF
# release hardening report

- git_sha: ${sha}
- release_ref: ${release_ref}
- expected_release_ref: ${EXPECTED_RELEASE_REF}
- github_ref: ${github_ref:-n/a}
- github_sha: ${github_sha:-n/a}
- platform: ${platform}
- started_at: ${started_at}
- finished_at: ${finished_at}
- rustc: $(rustc --version)
- cargo: $(cargo --version)
- python: $("${VENV_DIR}/bin/python" --version)

## checks

- cargo fmt --all -- --check: pass
- cargo clippy --workspace --all-targets --locked -- -D warnings: pass
- cargo test --workspace --locked: pass
- conformance/check_golden.sh: pass
- python sdk pytest: pass
- Python SDK build backend install: pass
- cargo metadata license inventory: pass
- rg secret pattern scan: pass
- mh CLI init-db/inspect/discover smoke: pass
- binary package (${binary_package}): pass
- Python SDK wheel package: pass
- Python SDK wheel install smoke: pass
- CycloneDX SBOM generation: pass
- worktree common secret-pattern scan: pass
- git history common secret-pattern scan: pass

## artifacts

- binary_package: ${binary_package}
- binary_package_sha256: ${binary_sha}
- python_sdk_sha256: ${python_sha}
- sbom_cyclonedx_sha256: ${sbom_sha}

Artifact files and checksums are in \`artifacts/\`.
Dependency inventory is \`dependency-inventory.json\`.
CycloneDX SBOM is \`artifacts/sbom.cyclonedx.json\`.
License summary is \`license-inventory.txt\`.
Secret scan summary is \`secret-scan.txt\`.
Secret history scan summary is \`secret-history-scan.txt\`.
EOF

echo "release hardening report: ${REPORT}"
cat "${ARTIFACT_DIR}/checksums.sha256"
