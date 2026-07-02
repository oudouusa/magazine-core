#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

CURRENT_OUT="${TMP_DIR}/current.json"
MISMATCH_OUT="${TMP_DIR}/mismatch.out"
FIXTURE_ROOT="${TMP_DIR}/fixture"
FIXTURE_OUT="${TMP_DIR}/fixture.json"

python3 "${ROOT}/scripts/check-version-discipline.py" --root "${ROOT}" --json > "${CURRENT_OUT}"
grep -q '"ok": true' "${CURRENT_OUT}"
grep -q '"package_version": "0.1.0-beta.3"' "${CURRENT_OUT}"

if python3 "${ROOT}/scripts/check-version-discipline.py" --root "${ROOT}" --release-ref 0.1.0-beta.2 >"${MISMATCH_OUT}" 2>&1; then
  echo "historical beta2 mismatch unexpectedly passed" >&2
  exit 1
fi
grep -q "Release tag/ref and package metadata disagree" "${MISMATCH_OUT}"

mkdir -p \
  "${FIXTURE_ROOT}/crates/mh-cli" \
  "${FIXTURE_ROOT}/sdk/python" \
  "${FIXTURE_ROOT}/docs/release"

cat > "${FIXTURE_ROOT}/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/mh-cli"]
EOF

cat > "${FIXTURE_ROOT}/crates/mh-cli/Cargo.toml" <<'EOF'
[package]
name = "mh-cli"
version = "0.1.0-beta.3"
EOF

cat > "${FIXTURE_ROOT}/sdk/python/pyproject.toml" <<'EOF'
[project]
name = "magazine-core-plugin-sdk"
version = "0.1.0b3"
EOF

cat > "${FIXTURE_ROOT}/CHANGELOG.md" <<'EOF'
# Changelog

## [Unreleased]

## [0.1.0-beta.3] - 2026-07-02

[0.1.0-beta.3]: https://github.com/oudouusa/magazine-core/releases/tag/0.1.0-beta.3
EOF

cat > "${FIXTURE_ROOT}/docs/release/0.1.0-beta.3.md" <<'EOF'
# magazine-core 0.1.0-beta.3 release notes
EOF

python3 "${ROOT}/scripts/check-version-discipline.py" \
  --root "${FIXTURE_ROOT}" \
  --release-ref 0.1.0-beta.3 \
  --json > "${FIXTURE_OUT}"
grep -q '"package_version": "0.1.0-beta.3"' "${FIXTURE_OUT}"
grep -q '"version": "0.1.0b3"' "${FIXTURE_OUT}"
grep -q '"release_version": "0.1.0-beta.3"' "${FIXTURE_OUT}"

rm "${FIXTURE_ROOT}/docs/release/0.1.0-beta.3.md"
if python3 "${ROOT}/scripts/check-version-discipline.py" \
  --root "${FIXTURE_ROOT}" \
  --release-ref 0.1.0-beta.3 >"${MISMATCH_OUT}" 2>&1; then
  echo "fixture without release notes unexpectedly passed" >&2
  exit 1
fi
grep -q "missing release notes" "${MISMATCH_OUT}"

touch "${FIXTURE_ROOT}/docs/release/0.1.0-beta.3.md"
if python3 "${ROOT}/scripts/check-version-discipline.py" \
  --root "${FIXTURE_ROOT}" \
  --release-ref 0.1.0-beta.3 >"${MISMATCH_OUT}" 2>&1; then
  echo "fixture with empty release notes unexpectedly passed" >&2
  exit 1
fi
grep -q "is empty" "${MISMATCH_OUT}"

echo "version discipline test: ok"
