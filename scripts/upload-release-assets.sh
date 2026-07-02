#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_TAG="${1:-}"
ARTIFACT_DIR="${2:-${ROOT}/dist/release-hardening/artifacts}"
DRY_RUN="${DRY_RUN:-0}"

usage() {
  cat >&2 <<'EOF'
usage: scripts/upload-release-assets.sh <release-tag> [artifact-dir]

Uploads the Linux release hardening assets to an existing GitHub Release after
verifying checksums and, unless DRY_RUN=1, that the Release target commit is the
current checkout.
EOF
}

fail() {
  echo "upload-release-assets: $*" >&2
  exit 1
}

checksum_verify() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1"
  else
    shasum -a 256 -c "$1"
  fi
}

require_file() {
  local path="$1"
  [[ -f "${path}" ]] || fail "required asset missing: ${path}"
}

if [[ -z "${RELEASE_TAG}" || "${RELEASE_TAG}" == "-h" || "${RELEASE_TAG}" == "--help" ]]; then
  usage
  [[ "${RELEASE_TAG}" == "-h" || "${RELEASE_TAG}" == "--help" ]] && exit 0
  exit 2
fi

if [[ ! "${RELEASE_TAG}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
  fail "release tag must contain only ASCII letters, digits, dot, underscore, or dash and must not start with punctuation"
fi

[[ -d "${ARTIFACT_DIR}" ]] || fail "artifact directory does not exist: ${ARTIFACT_DIR}"

linux_binary="${ARTIFACT_DIR}/magazine-core-mh-linux-x86_64.tar.gz"
sbom="${ARTIFACT_DIR}/sbom.cyclonedx.json"
checksum_asset="${ARTIFACT_DIR}/SHA256SUMS.txt"
legacy_checksum="${ARTIFACT_DIR}/checksums.sha256"

require_file "${linux_binary}"
require_file "${sbom}"

shopt -s nullglob
wheel_files=("${ARTIFACT_DIR}"/magazine_core_plugin_sdk-*.whl)
shopt -u nullglob
if [[ "${#wheel_files[@]}" -ne 1 ]]; then
  fail "expected exactly one Python SDK wheel in ${ARTIFACT_DIR}; found ${#wheel_files[@]}"
fi
python_wheel="${wheel_files[0]}"

if [[ ! -f "${checksum_asset}" ]]; then
  require_file "${legacy_checksum}"
  cp "${legacy_checksum}" "${checksum_asset}"
fi

for asset in "${linux_binary}" "${python_wheel}" "${sbom}"; do
  asset_name="$(basename "${asset}")"
  if ! awk -v file="${asset_name}" '{path=$2; sub(/^\.\//, "", path); if (path == file) found=1} END {exit found ? 0 : 1}' "${checksum_asset}"; then
    fail "checksum file does not contain ${asset_name}"
  fi
done

(
  cd "${ARTIFACT_DIR}"
  checksum_verify "$(basename "${checksum_asset}")" >/dev/null
)

assets=("${linux_binary}" "${python_wheel}" "${sbom}" "${checksum_asset}")

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "dry-run: release ${RELEASE_TAG}"
  printf 'dry-run: asset %s\n' "${assets[@]}"
  exit 0
fi

command -v gh >/dev/null 2>&1 || fail "gh CLI is required unless DRY_RUN=1"

current_sha="$(git -C "${ROOT}" rev-parse HEAD)"
target_commitish="$(gh release view "${RELEASE_TAG}" --json targetCommitish --jq .targetCommitish)"
[[ -n "${target_commitish}" && "${target_commitish}" != "null" ]] || fail "release ${RELEASE_TAG} has no targetCommitish"

if ! release_sha="$(git -C "${ROOT}" rev-list -n1 "${target_commitish}" 2>/dev/null)"; then
  fail "release ${RELEASE_TAG} target ${target_commitish} is not resolvable in this checkout"
fi

if [[ "${release_sha}" != "${current_sha}" ]]; then
  fail "release ${RELEASE_TAG} targets ${release_sha}, but current checkout is ${current_sha}"
fi

gh release upload "${RELEASE_TAG}" "${assets[@]}" --clobber
