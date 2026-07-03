#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CUT_RELEASE_ROOT:-$(cd "${SCRIPT_DIR}/.." && pwd)}"
DRY_RUN="${DRY_RUN:-0}"
SKIP_QUEUE_CHECK=0

usage() {
  cat >&2 <<'EOF'
usage: scripts/cut-release.sh [--skip-queue-check] <release-tag> <commit-sha>

Creates an annotated tag, GitHub Release, pins the Release target_commitish to
the exact commit SHA, verifies that target, and dispatches Release hardening.

Set DRY_RUN=1 to print mutating commands without running them.
EOF
}

fail() {
  echo "cut-release: $*" >&2
  exit 1
}

print_command() {
  printf 'dry-run:'
  printf ' %q' "$@"
  printf '\n'
}

run_change() {
  if [[ "${DRY_RUN}" == "1" ]]; then
    print_command "$@"
  else
    "$@"
  fi
}

require_count_zero() {
  local label="$1"
  local count="$2"

  [[ "${count}" =~ ^[0-9]+$ ]] || fail "${label} check returned a non-numeric count: ${count}"
  [[ "${count}" == "0" ]] || fail "${label} must be empty before cutting a release; found ${count}"
}

github_count() {
  "$@"
}

detect_repo_slug() {
  local origin_url
  local slug

  origin_url="$(git -C "${ROOT}" remote get-url origin)" || fail "origin remote is required"
  case "${origin_url}" in
    https://github.com/*)
      slug="${origin_url#https://github.com/}"
      ;;
    git@github.com:*)
      slug="${origin_url#git@github.com:}"
      ;;
    ssh://git@github.com/*)
      slug="${origin_url#ssh://git@github.com/}"
      ;;
    *)
      fail "origin remote must be a GitHub repository URL or CUT_RELEASE_REPO must be set"
      ;;
  esac

  slug="${slug%.git}"
  [[ "${slug}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] \
    || fail "could not derive GitHub owner/repo from origin remote: ${origin_url}"
  printf '%s\n' "${slug}"
}

check_clean_worktree() {
  local status

  status="$(git -C "${ROOT}" status --porcelain=v1)"
  [[ -z "${status}" ]] || fail "working tree is not clean"
  echo "cut-release: working tree clean"
}

normalize_commit_sha() {
  local input_sha="$1"
  local lower_input
  local resolved_sha

  [[ "${input_sha}" =~ ^[0-9A-Fa-f]{40}$ ]] || fail "commit-sha must be a full 40-character hex SHA"
  lower_input="$(printf '%s' "${input_sha}" | tr '[:upper:]' '[:lower:]')"
  resolved_sha="$(git -C "${ROOT}" rev-parse --verify "${input_sha}^{commit}")" \
    || fail "commit-sha is not a commit in this checkout: ${input_sha}"
  [[ "${resolved_sha}" == "${lower_input}" ]] || fail "commit-sha does not resolve exactly to itself: ${input_sha}"
  printf '%s\n' "${resolved_sha}"
}

check_commit_on_origin_main() {
  local commit_sha="$1"

  git -C "${ROOT}" rev-parse --verify "origin/main^{commit}" >/dev/null \
    || fail "origin/main is not available in this checkout"
  git -C "${ROOT}" merge-base --is-ancestor "${commit_sha}" origin/main \
    || fail "commit ${commit_sha} is not reachable from origin/main"
  echo "cut-release: commit is reachable from origin/main"
}

check_release_notes() {
  local release_tag="$1"
  local notes_path="${ROOT}/docs/release/${release_tag}.md"

  [[ -f "${notes_path}" ]] || fail "missing release notes: docs/release/${release_tag}.md"
  echo "cut-release: release notes present: docs/release/${release_tag}.md"
}

check_version_discipline() {
  local release_tag="$1"

  python3 "${ROOT}/scripts/check-version-discipline.py" --root "${ROOT}" --release-ref "${release_tag}" \
    || fail "version discipline failed for ${release_tag}"
}

check_tag_absent() {
  local release_tag="$1"
  local ls_remote_status

  if git -C "${ROOT}" rev-parse -q --verify "refs/tags/${release_tag}" >/dev/null; then
    fail "tag already exists locally: ${release_tag}"
  fi

  set +e
  git -C "${ROOT}" ls-remote --exit-code --tags origin "refs/tags/${release_tag}" >/dev/null 2>&1
  ls_remote_status=$?
  set -e

  case "${ls_remote_status}" in
    0)
      fail "tag already exists on origin: ${release_tag}"
      ;;
    2)
      ;;
    *)
      fail "unable to check remote tag refs for ${release_tag}"
      ;;
  esac

  echo "cut-release: tag is absent locally and on origin"
}

check_release_absent() {
  local release_tag="$1"
  local output
  local view_status

  command -v gh >/dev/null 2>&1 || fail "gh CLI is required to verify GitHub Release absence"

  set +e
  output="$(gh release view "${release_tag}" --repo "${REPO_SLUG}" 2>&1 >/dev/null)"
  view_status=$?
  set -e

  [[ "${view_status}" != "0" ]] || fail "GitHub Release already exists: ${release_tag}"
  if ! grep -Eiq 'not found|HTTP 404|release not found' <<<"${output}"; then
    fail "unable to verify GitHub Release absence for ${release_tag}: ${output}"
  fi
  echo "cut-release: GitHub Release absent"
}

check_queue_empty() {
  local pr_count
  local issue_count
  local advisory_count=0
  local state
  local state_count

  if [[ "${SKIP_QUEUE_CHECK}" == "1" ]]; then
    echo "cut-release: queue check skipped by --skip-queue-check"
    return
  fi

  command -v gh >/dev/null 2>&1 || fail "gh CLI is required unless --skip-queue-check is used"

  pr_count="$(github_count gh pr list --repo "${REPO_SLUG}" --state open --limit 1 --json number --jq 'length')"
  issue_count="$(github_count gh issue list --repo "${REPO_SLUG}" --state open --limit 1 --json number --jq 'length')"
  for state in triage draft published; do
    state_count="$(
      github_count gh api \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "repos/${REPO_SLUG}/security-advisories?state=${state}&per_page=1" \
        --jq 'length'
    )"
    require_count_zero "open security advisory (${state})" "${state_count}"
    advisory_count=$((advisory_count + state_count))
  done

  require_count_zero "open PR" "${pr_count}"
  require_count_zero "open issue" "${issue_count}"
  require_count_zero "open security advisory" "${advisory_count}"
  echo "cut-release: queue check passed"
}

is_prerelease_tag() {
  local release_tag="$1"
  local lower_tag

  lower_tag="$(printf '%s' "${release_tag}" | tr '[:upper:]' '[:lower:]')"
  [[ "${lower_tag}" == *beta* || "${lower_tag}" == *rc* || "${lower_tag}" == *alpha* ]]
}

patch_release_target() {
  local release_tag="$1"
  local commit_sha="$2"
  local release_id

  if [[ "${DRY_RUN}" == "1" ]]; then
    print_command gh api --method PATCH "repos/${REPO_SLUG}/releases/<release-id>" -f "target_commitish=${commit_sha}"
    return
  fi

  release_id="$(gh api "repos/${REPO_SLUG}/releases/tags/${release_tag}" --jq .id)"
  [[ -n "${release_id}" && "${release_id}" != "null" ]] || fail "unable to resolve GitHub Release id for ${release_tag}"
  gh api --method PATCH "repos/${REPO_SLUG}/releases/${release_id}" -f "target_commitish=${commit_sha}" >/dev/null
}

verify_release_target() {
  local release_tag="$1"
  local commit_sha="$2"
  local target_commitish

  if [[ "${DRY_RUN}" == "1" ]]; then
    return
  fi

  target_commitish="$(gh release view "${release_tag}" --repo "${REPO_SLUG}" --json targetCommitish --jq .targetCommitish)"
  [[ "${target_commitish}" == "${commit_sha}" ]] \
    || fail "release ${release_tag} targetCommitish is ${target_commitish}, expected ${commit_sha}"
  echo "cut-release: release targetCommitish verified: ${commit_sha}"
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --skip-queue-check)
      SKIP_QUEUE_CHECK=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      usage
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

RELEASE_TAG="${1:-}"
COMMIT_SHA_INPUT="${2:-}"

if [[ -z "${RELEASE_TAG}" || -z "${COMMIT_SHA_INPUT}" || "$#" -ne 2 ]]; then
  usage
  exit 2
fi

if [[ ! "${RELEASE_TAG}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
  fail "release tag must contain only ASCII letters, digits, dot, underscore, or dash and must not start with punctuation"
fi

COMMIT_SHA="$(normalize_commit_sha "${COMMIT_SHA_INPUT}")"
RELEASE_NOTES="${ROOT}/docs/release/${RELEASE_TAG}.md"
REPO_SLUG="${CUT_RELEASE_REPO:-$(detect_repo_slug)}"

check_clean_worktree
check_commit_on_origin_main "${COMMIT_SHA}"
check_release_notes "${RELEASE_TAG}"
check_version_discipline "${RELEASE_TAG}"
check_tag_absent "${RELEASE_TAG}"
check_queue_empty
check_release_absent "${RELEASE_TAG}"

release_create_cmd=(gh release create "${RELEASE_TAG}" --repo "${REPO_SLUG}" --notes-file "${RELEASE_NOTES}" --verify-tag)
if is_prerelease_tag "${RELEASE_TAG}"; then
  release_create_cmd+=(--prerelease)
fi

run_change git -C "${ROOT}" tag -a "${RELEASE_TAG}" "${COMMIT_SHA}" -m "magazine-core ${RELEASE_TAG}"
run_change git -C "${ROOT}" push origin "refs/tags/${RELEASE_TAG}"
run_change "${release_create_cmd[@]}"
patch_release_target "${RELEASE_TAG}" "${COMMIT_SHA}"
verify_release_target "${RELEASE_TAG}" "${COMMIT_SHA}"
run_change gh workflow run "Release hardening" --repo "${REPO_SLUG}" --ref "${RELEASE_TAG}" -f "release_tag=${RELEASE_TAG}"

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "cut-release: dry-run complete for ${RELEASE_TAG} at ${COMMIT_SHA}"
else
  echo "cut-release: dispatched Release hardening for ${RELEASE_TAG} at ${COMMIT_SHA}"
fi
