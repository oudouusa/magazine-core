#!/usr/bin/env bash
set -euo pipefail

REPO="${REPO:-oudouusa/magazine-core}"
RELEASE_TAG="${RELEASE_TAG:-0.1.0-beta.2}"
RELEASE_BASE_URL="${RELEASE_BASE_URL:-https://github.com/${REPO}/releases/download/${RELEASE_TAG}}"
WORK_DIR="${WORK_DIR:-}"
KEEP_WORKDIR="${KEEP_WORKDIR:-0}"

fail() {
  echo "verify-standalone-quickstart: $*" >&2
  exit 1
}

run() {
  echo "+ $*" >&2
  "$@"
}

checksum_verify() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$1"
  else
    fail "sha256sum or shasum is required"
  fi
}

download() {
  local name="$1"
  local url="${RELEASE_BASE_URL}/${name}"
  echo "+ curl -fsSL -o ${name} ${url}" >&2
  curl -fsSL --retry 5 --retry-delay 2 --retry-all-errors -o "${name}" "${url}"
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v python3 >/dev/null 2>&1 || fail "python3 is required"

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  fail "the published prebuilt quickstart currently supports linux-x86_64 only"
fi

if [[ -z "${WORK_DIR}" ]]; then
  WORK_DIR="$(mktemp -d)"
  cleanup_work_dir=1
else
  mkdir -p "${WORK_DIR}"
  cleanup_work_dir=0
fi

cleanup() {
  if [[ "${cleanup_work_dir}" == "1" && "${KEEP_WORKDIR}" != "1" ]]; then
    rm -rf "${WORK_DIR}"
  else
    echo "quickstart work dir: ${WORK_DIR}" >&2
  fi
}
trap cleanup EXIT

echo "Verifying magazine-core standalone quickstart"
echo "release_tag=${RELEASE_TAG}"
echo "work_dir=${WORK_DIR}"

cd "${WORK_DIR}"

download "SHA256SUMS.txt"
mapfile -t checksum_assets < <(awk '{path=$2; sub(/^\.\//, "", path); if (path != "") print path}' SHA256SUMS.txt)
if [[ "${#checksum_assets[@]}" -eq 0 ]]; then
  fail "SHA256SUMS.txt did not list any assets"
fi

for asset in "${checksum_assets[@]}"; do
  case "${asset}" in
    *"/"* | *".."* | "" )
      fail "unsafe asset path in SHA256SUMS.txt: ${asset}"
      ;;
  esac
  download "${asset}"
done

run checksum_verify "SHA256SUMS.txt"

binary_package="magazine-core-mh-linux-x86_64.tar.gz"
[[ -f "${binary_package}" ]] || fail "missing linux binary asset: ${binary_package}"
run tar -xzf "${binary_package}"
[[ -x "./mh" ]] || fail "binary tarball did not produce executable ./mh"
run ./mh --help >/dev/null

shopt -s nullglob
wheel_files=(magazine_core_plugin_sdk-*.whl)
shopt -u nullglob
if [[ "${#wheel_files[@]}" -ne 1 ]]; then
  fail "expected exactly one SDK wheel, found ${#wheel_files[@]}"
fi
wheel_file="${wheel_files[0]}"

run python3 -m venv .venv
run .venv/bin/python -m pip install --no-index --no-deps "${wheel_file}"
run .venv/bin/python - <<'PY'
import magazine_core_plugin_sdk as sdk

assert sdk.PROTOCOL_VERSION == 1
assert sdk.RECORD_SCHEMA_VERSION == 1
PY

cat > quickstart_plugin.py <<'PY'
from __future__ import annotations

from magazine_core_plugin_sdk import ExternalLink, SourceRecord, run_plugin


def discover(context, _params):
    context.log("info", "standalone quickstart discover")
    context.send_record(
        SourceRecord(
            source_name="quickstart",
            source_url="synthetic://quickstart/1",
            title="Standalone Quickstart One",
            brand_raw="Synthetic Brand",
            performers_raw=["Alice Example"],
            cover_urls=["https://example.invalid/cover.jpg"],
            page_urls=["https://example.invalid/page/1"],
            issue_no="1",
            external_links=[
                ExternalLink(
                    url="https://example.invalid/retail/1",
                    provider="example",
                    label="Example Retail",
                    kind="retail",
                    external_id="retail-1",
                    metadata={"fixture": "standalone-quickstart"},
                )
            ],
            release_date="2026-07-02",
            extra={"fixture": "standalone-quickstart"},
        )
    )


if __name__ == "__main__":
    run_plugin(
        source_name="quickstart",
        display_label="Standalone Quickstart",
        discover=discover,
        allowed_domains=["example.invalid"],
        capabilities=["discover"],
    )
PY

mkdir -p plugins.d
.venv/bin/python - "${WORK_DIR}" <<'PY'
import json
import sys
from pathlib import Path

work_dir = Path(sys.argv[1])
manifest = {
    "id": "quickstart",
    "argv": [str(work_dir / ".venv/bin/python"), "quickstart_plugin.py"],
    "working_dir": str(work_dir),
}
(work_dir / "plugins.d/quickstart.json").write_text(
    json.dumps(manifest, indent=2) + "\n",
    encoding="utf-8",
)
PY

run ./mh init-db scratch.db > init-inspect.json
run ./mh inspect scratch.db > before-discover-inspect.json
run ./mh discover scratch.db ./plugins.d quickstart \
  --max-pages 1 \
  --per-page 30 \
  --max-records 30 \
  --timeout-seconds 60 > discover.json
run ./mh inspect scratch.db > final-inspect.json

.venv/bin/python - <<'PY'
import json
from pathlib import Path

before = json.loads(Path("before-discover-inspect.json").read_text(encoding="utf-8"))
discover = json.loads(Path("discover.json").read_text(encoding="utf-8"))
final = json.loads(Path("final-inspect.json").read_text(encoding="utf-8"))

expected_before = {
    "schema_version": 1,
    "source_posts": 0,
    "performers": 0,
    "covers": 0,
    "pages": 0,
    "external_links": 0,
}
expected_final = {
    "schema_version": 1,
    "source_posts": 1,
    "performers": 1,
    "covers": 1,
    "pages": 1,
    "external_links": 1,
}

for key, value in expected_before.items():
    actual = before.get(key)
    if actual != value:
        raise SystemExit(f"before inspect {key}: expected {value}, got {actual}")

for key, value in expected_final.items():
    actual = final.get(key)
    if actual != value:
        raise SystemExit(f"final inspect {key}: expected {value}, got {actual}")

expected_discover = {
    "plugin_id": "quickstart",
    "source_name": "quickstart",
    "discover_records": 1,
    "spooled_records": 1,
    "ingested_records": 1,
    "exit_status": 0,
}
for key, value in expected_discover.items():
    actual = discover.get(key)
    if actual != value:
        raise SystemExit(f"discover {key}: expected {value!r}, got {actual!r}")

print("standalone quickstart verified")
print(json.dumps({"discover": discover, "inspect": final}, indent=2))
PY
