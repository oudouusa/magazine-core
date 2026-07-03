#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

FAKE_BIN="${TMP_DIR}/bin"
FAKE_GH_LOG="${TMP_DIR}/gh.log"
mkdir -p "${FAKE_BIN}"

cat > "${FAKE_BIN}/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf 'gh' >> "${FAKE_GH_LOG:?}"
printf ' %q' "$@" >> "${FAKE_GH_LOG:?}"
printf '\n' >> "${FAKE_GH_LOG:?}"

case "${1:-} ${2:-}" in
  "pr list")
    printf '%s\n' "${FAKE_OPEN_PRS:-0}"
    ;;
  "issue list")
    printf '%s\n' "${FAKE_OPEN_ISSUES:-0}"
    ;;
  "api -H"|"api --method")
    if [[ "$*" == *"security-advisories"* ]]; then
      printf '%s\n' "${FAKE_OPEN_ADVISORIES:-0}"
    elif [[ "$*" == *"releases/tags"* ]]; then
      printf '%s\n' "${FAKE_RELEASE_ID:-12345}"
    else
      printf '{}\n'
    fi
    ;;
  "release view")
    if [[ "${FAKE_RELEASE_EXISTS:-0}" == "1" ]]; then
      printf '%s\n' "${FAKE_RELEASE_TARGET:-}"
      exit 0
    fi
    echo "release not found" >&2
    exit 1
    ;;
  *)
    echo "unexpected gh invocation: $*" >&2
    exit 1
    ;;
esac
EOF
chmod +x "${FAKE_BIN}/gh"

setup_fixture() {
  local name="$1"
  local package_version="${2:-1.0.0}"
  local release_tag="${3:-1.0.0}"
  local include_release_notes="${4:-1}"
  local python_version="${5:-${package_version}}"
  local fixture_root="${TMP_DIR}/${name}"
  local origin_root="${TMP_DIR}/${name}.git"

  mkdir -p \
    "${fixture_root}/crates/mh-cli" \
    "${fixture_root}/sdk/python" \
    "${fixture_root}/docs/release" \
    "${fixture_root}/scripts"

  cat > "${fixture_root}/Cargo.toml" <<EOF
[workspace]
members = ["crates/mh-cli"]
EOF

  cat > "${fixture_root}/crates/mh-cli/Cargo.toml" <<EOF
[package]
name = "mh-cli"
version = "${package_version}"
EOF

  cat > "${fixture_root}/sdk/python/pyproject.toml" <<EOF
[project]
name = "magazine-core-plugin-sdk"
version = "${python_version}"
EOF

  cat > "${fixture_root}/CHANGELOG.md" <<EOF
# Changelog

## [Unreleased]

## [${release_tag}] - 2026-07-03

[${release_tag}]: https://github.com/oudouusa/magazine-core/releases/tag/${release_tag}
EOF

  if [[ "${include_release_notes}" == "1" ]]; then
    cat > "${fixture_root}/docs/release/${release_tag}.md" <<EOF
# magazine-core ${release_tag} release notes
EOF
  fi

  cp "${ROOT}/scripts/check-version-discipline.py" "${fixture_root}/scripts/check-version-discipline.py"

  git -C "${fixture_root}" init -b main >/dev/null
  git -C "${fixture_root}" config user.name "cut release test"
  git -C "${fixture_root}" config user.email "cut-release@example.invalid"
  git -C "${fixture_root}" add .
  git -C "${fixture_root}" commit -m "fixture" >/dev/null
  git init --bare -b main "${origin_root}" >/dev/null
  git -C "${fixture_root}" remote add origin "${origin_root}"
  git -C "${fixture_root}" push -u origin main >/dev/null 2>&1

  printf '%s\n' "${fixture_root}"
}

run_cut_release() {
  local fixture_root="$1"
  local output_path="$2"
  shift 2

  CUT_RELEASE_ROOT="${fixture_root}" \
    CUT_RELEASE_REPO="oudouusa/magazine-core-fixture" \
    DRY_RUN=1 \
    FAKE_GH_LOG="${FAKE_GH_LOG}" \
    PATH="${FAKE_BIN}:${PATH}" \
    bash "${ROOT}/scripts/cut-release.sh" "$@" >"${output_path}" 2>&1
}

run_cut_release_skip_queue() {
  local fixture_root="$1"
  local output_path="$2"
  shift 2

  CUT_RELEASE_ROOT="${fixture_root}" \
    CUT_RELEASE_REPO="oudouusa/magazine-core-fixture" \
    DRY_RUN=1 \
    FAKE_GH_LOG="${FAKE_GH_LOG}" \
    PATH="${FAKE_BIN}:${PATH}" \
    bash "${ROOT}/scripts/cut-release.sh" --skip-queue-check "$@" >"${output_path}" 2>&1
}

expect_fail() {
  local description="$1"
  shift

  if "$@"; then
    echo "${description} unexpectedly passed" >&2
    exit 1
  fi
}

NORMAL_ROOT="$(setup_fixture normal)"
NORMAL_SHA="$(git -C "${NORMAL_ROOT}" rev-parse HEAD)"
NORMAL_OUT="${TMP_DIR}/normal.out"
: > "${FAKE_GH_LOG}"
run_cut_release "${NORMAL_ROOT}" "${NORMAL_OUT}" 1.0.0 "${NORMAL_SHA}"
grep -q "cut-release: queue check passed" "${NORMAL_OUT}"
grep -q "dry-run: git -C .* tag -a 1.0.0 ${NORMAL_SHA}" "${NORMAL_OUT}"
grep -q "dry-run: git -C .* push origin refs/tags/1.0.0" "${NORMAL_OUT}"
grep -q "dry-run: gh release create 1.0.0 --repo oudouusa/magazine-core-fixture --notes-file .*docs/release/1.0.0.md --verify-tag" "${NORMAL_OUT}"
grep -q "dry-run: gh api --method PATCH .*target_commitish=${NORMAL_SHA}" "${NORMAL_OUT}"
grep -q "dry-run: gh workflow run Release\\\\ hardening --repo oudouusa/magazine-core-fixture --ref 1.0.0 -f release_tag=1.0.0" "${NORMAL_OUT}"
grep -q "gh pr list" "${FAKE_GH_LOG}"
grep -q "gh issue list" "${FAKE_GH_LOG}"
grep -q "security-advisories" "${FAKE_GH_LOG}"
if grep -q "gh release create" "${FAKE_GH_LOG}"; then
  echo "dry-run unexpectedly invoked gh release create" >&2
  exit 1
fi

BETA_ROOT="$(setup_fixture beta 1.0.0-beta.1 1.0.0-beta.1 1 1.0.0b1)"
BETA_SHA="$(git -C "${BETA_ROOT}" rev-parse HEAD)"
BETA_OUT="${TMP_DIR}/beta.out"
run_cut_release "${BETA_ROOT}" "${BETA_OUT}" 1.0.0-beta.1 "${BETA_SHA}"
grep -q "dry-run: gh release create 1.0.0-beta.1 --repo oudouusa/magazine-core-fixture --notes-file .*docs/release/1.0.0-beta.1.md --verify-tag --prerelease" "${BETA_OUT}"

SKIP_ROOT="$(setup_fixture skip)"
SKIP_SHA="$(git -C "${SKIP_ROOT}" rev-parse HEAD)"
SKIP_OUT="${TMP_DIR}/skip.out"
CUT_RELEASE_ROOT="${SKIP_ROOT}" \
  CUT_RELEASE_REPO="oudouusa/magazine-core-fixture" \
  DRY_RUN=1 \
  FAKE_GH_LOG="${FAKE_GH_LOG}" \
  PATH="${FAKE_BIN}:${PATH}" \
  bash "${ROOT}/scripts/cut-release.sh" --skip-queue-check 1.0.0 "${SKIP_SHA}" >"${SKIP_OUT}" 2>&1
grep -q "queue check skipped by --skip-queue-check" "${SKIP_OUT}"
grep -q "cut-release: GitHub Release absent" "${SKIP_OUT}"

OPEN_PR_ROOT="$(setup_fixture open-pr)"
OPEN_PR_SHA="$(git -C "${OPEN_PR_ROOT}" rev-parse HEAD)"
OPEN_PR_OUT="${TMP_DIR}/open-pr.out"
expect_fail "open PR preflight" env \
  CUT_RELEASE_ROOT="${OPEN_PR_ROOT}" \
  CUT_RELEASE_REPO="oudouusa/magazine-core-fixture" \
  DRY_RUN=1 \
  FAKE_GH_LOG="${FAKE_GH_LOG}" \
  FAKE_OPEN_PRS=1 \
  PATH="${FAKE_BIN}:${PATH}" \
  bash "${ROOT}/scripts/cut-release.sh" 1.0.0 "${OPEN_PR_SHA}" >"${OPEN_PR_OUT}" 2>&1
grep -q "open PR must be empty" "${OPEN_PR_OUT}"

LOCAL_TAG_ROOT="$(setup_fixture local-tag)"
LOCAL_TAG_SHA="$(git -C "${LOCAL_TAG_ROOT}" rev-parse HEAD)"
LOCAL_TAG_OUT="${TMP_DIR}/local-tag.out"
git -C "${LOCAL_TAG_ROOT}" tag -a 1.0.0 "${LOCAL_TAG_SHA}" -m "fixture tag"
expect_fail "local tag preflight" run_cut_release_skip_queue "${LOCAL_TAG_ROOT}" "${LOCAL_TAG_OUT}" 1.0.0 "${LOCAL_TAG_SHA}"
grep -q "tag already exists locally" "${LOCAL_TAG_OUT}"

REMOTE_TAG_ROOT="$(setup_fixture remote-tag)"
REMOTE_TAG_SHA="$(git -C "${REMOTE_TAG_ROOT}" rev-parse HEAD)"
REMOTE_TAG_OUT="${TMP_DIR}/remote-tag.out"
git -C "${REMOTE_TAG_ROOT}" tag -a 1.0.0 "${REMOTE_TAG_SHA}" -m "fixture tag"
git -C "${REMOTE_TAG_ROOT}" push origin refs/tags/1.0.0 >/dev/null 2>&1
git -C "${REMOTE_TAG_ROOT}" tag -d 1.0.0 >/dev/null
expect_fail "remote tag preflight" run_cut_release_skip_queue "${REMOTE_TAG_ROOT}" "${REMOTE_TAG_OUT}" 1.0.0 "${REMOTE_TAG_SHA}"
grep -q "tag already exists on origin" "${REMOTE_TAG_OUT}"

MISSING_NOTES_ROOT="$(setup_fixture missing-notes 1.0.0 1.0.0 0)"
MISSING_NOTES_SHA="$(git -C "${MISSING_NOTES_ROOT}" rev-parse HEAD)"
MISSING_NOTES_OUT="${TMP_DIR}/missing-notes.out"
expect_fail "missing release notes preflight" run_cut_release_skip_queue "${MISSING_NOTES_ROOT}" "${MISSING_NOTES_OUT}" 1.0.0 "${MISSING_NOTES_SHA}"
grep -q "missing release notes" "${MISSING_NOTES_OUT}"

MISMATCH_ROOT="$(setup_fixture mismatch 1.0.1 1.0.0 1)"
MISMATCH_SHA="$(git -C "${MISMATCH_ROOT}" rev-parse HEAD)"
MISMATCH_OUT="${TMP_DIR}/mismatch.out"
expect_fail "version discipline mismatch preflight" run_cut_release_skip_queue "${MISMATCH_ROOT}" "${MISMATCH_OUT}" 1.0.0 "${MISMATCH_SHA}"
grep -q "version discipline failed" "${MISMATCH_OUT}"

DIRTY_ROOT="$(setup_fixture dirty)"
DIRTY_SHA="$(git -C "${DIRTY_ROOT}" rev-parse HEAD)"
DIRTY_OUT="${TMP_DIR}/dirty.out"
printf 'dirty\n' > "${DIRTY_ROOT}/untracked.txt"
expect_fail "dirty worktree preflight" run_cut_release_skip_queue "${DIRTY_ROOT}" "${DIRTY_OUT}" 1.0.0 "${DIRTY_SHA}"
grep -q "working tree is not clean" "${DIRTY_OUT}"

UNPUSHED_ROOT="$(setup_fixture unpushed)"
printf 'local\n' > "${UNPUSHED_ROOT}/local.txt"
git -C "${UNPUSHED_ROOT}" add local.txt
git -C "${UNPUSHED_ROOT}" commit -m "local only" >/dev/null
UNPUSHED_SHA="$(git -C "${UNPUSHED_ROOT}" rev-parse HEAD)"
UNPUSHED_OUT="${TMP_DIR}/unpushed.out"
expect_fail "origin/main ancestry preflight" run_cut_release_skip_queue "${UNPUSHED_ROOT}" "${UNPUSHED_OUT}" 1.0.0 "${UNPUSHED_SHA}"
grep -q "not reachable from origin/main" "${UNPUSHED_OUT}"

echo "cut-release test: ok"
