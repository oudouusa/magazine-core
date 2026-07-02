#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

ARTIFACT_DIR="${TMP_DIR}/artifacts"
DRY_RUN_OUT="${TMP_DIR}/upload-release-assets.out"
MISSING_OUT="${TMP_DIR}/upload-release-assets-missing.out"
FAKE_BIN="${TMP_DIR}/bin"
FAKE_GH_LOG="${TMP_DIR}/gh.log"
mkdir -p "${ARTIFACT_DIR}"
mkdir -p "${FAKE_BIN}"

printf 'binary\n' > "${ARTIFACT_DIR}/magazine-core-mh-linux-x86_64.tar.gz"
printf 'wheel\n' > "${ARTIFACT_DIR}/magazine_core_plugin_sdk-0.1.0-py3-none-any.whl"
printf '{"bomFormat":"CycloneDX"}\n' > "${ARTIFACT_DIR}/sbom.cyclonedx.json"

(
  cd "${ARTIFACT_DIR}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum ./* > checksums.sha256
  else
    shasum -a 256 ./* > checksums.sha256
  fi
)

DRY_RUN=1 bash "${ROOT}/scripts/upload-release-assets.sh" 0.1.0-test.1 "${ARTIFACT_DIR}" >"${DRY_RUN_OUT}"
test -f "${ARTIFACT_DIR}/SHA256SUMS.txt"
grep -q 'magazine-core-mh-linux_x86_64.tar.gz' "${DRY_RUN_OUT}" && {
  echo "unexpected malformed asset name accepted" >&2
  exit 1
}
grep -q 'magazine-core-mh-linux-x86_64.tar.gz' "${DRY_RUN_OUT}"
grep -q 'magazine_core_plugin_sdk-0.1.0-py3-none-any.whl' "${DRY_RUN_OUT}"
grep -q 'sbom.cyclonedx.json' "${DRY_RUN_OUT}"

cat > "${FAKE_BIN}/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

case "$1 $2" in
  "release view")
    printf '%s\n' "${FAKE_RELEASE_TARGET:?}"
    ;;
  "release upload")
    printf '%s\n' "$@" > "${FAKE_GH_LOG:?}"
    ;;
  *)
    echo "unexpected gh invocation: $*" >&2
    exit 1
    ;;
esac
EOF
chmod +x "${FAKE_BIN}/gh"

FAKE_RELEASE_TARGET="$(git -C "${ROOT}" rev-parse HEAD)" \
  FAKE_GH_LOG="${FAKE_GH_LOG}" \
  PATH="${FAKE_BIN}:${PATH}" \
  bash "${ROOT}/scripts/upload-release-assets.sh" 0.1.0-test.1 "${ARTIFACT_DIR}"
grep -qx 'release' "${FAKE_GH_LOG}"
grep -qx 'upload' "${FAKE_GH_LOG}"
grep -qx '0.1.0-test.1' "${FAKE_GH_LOG}"
grep -qx "${ARTIFACT_DIR}/magazine-core-mh-linux-x86_64.tar.gz" "${FAKE_GH_LOG}"
grep -qx "${ARTIFACT_DIR}/magazine_core_plugin_sdk-0.1.0-py3-none-any.whl" "${FAKE_GH_LOG}"
grep -qx "${ARTIFACT_DIR}/sbom.cyclonedx.json" "${FAKE_GH_LOG}"
grep -qx "${ARTIFACT_DIR}/SHA256SUMS.txt" "${FAKE_GH_LOG}"
grep -qx -- '--clobber' "${FAKE_GH_LOG}"

rm "${ARTIFACT_DIR}"/magazine_core_plugin_sdk-0.1.0-py3-none-any.whl
if DRY_RUN=1 bash "${ROOT}/scripts/upload-release-assets.sh" 0.1.0-test.1 "${ARTIFACT_DIR}" >"${MISSING_OUT}" 2>&1; then
  echo "missing wheel fixture unexpectedly passed" >&2
  exit 1
fi
grep -q 'expected exactly one Python SDK wheel' "${MISSING_OUT}"

echo "upload-release-assets test: ok"
