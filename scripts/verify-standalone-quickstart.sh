#!/usr/bin/env bash
set -euo pipefail

REPO="${REPO:-oudouusa/magazine-core}"
RELEASE_TAG="${RELEASE_TAG:-0.1.0-beta.2}"
RELEASE_BASE_URL="${RELEASE_BASE_URL:-https://github.com/${REPO}/releases/download/${RELEASE_TAG}}"
WORK_DIR="${WORK_DIR:-}"
KEEP_WORKDIR="${KEEP_WORKDIR:-0}"
VERIFY_UI="${VERIFY_UI:-auto}"
VERIFY_TRUSTED_UI="${VERIFY_TRUSTED_UI:-auto}"
TRUSTED_UI_GATE="${TRUSTED_UI_GATE:-}"
UI_PID=""
UI_PORT=""

fail() {
  echo "verify-standalone-quickstart: $*" >&2
  exit 1
}

run() {
  echo "+ $*" >&2
  "$@"
}

stop_ui() {
  if [[ -n "${UI_PID}" ]] && kill -0 "${UI_PID}" >/dev/null 2>&1; then
    kill "${UI_PID}" >/dev/null 2>&1 || true
    wait "${UI_PID}" >/dev/null 2>&1 || true
  fi
  UI_PID=""
  UI_PORT=""
}

start_ui() {
  local log_file="$1"
  shift
  stop_ui
  echo "+ ./mh ui --db scratch.db --plugins-dir ./plugins.d --port 0 $*" >&2
  ./mh ui --db scratch.db --plugins-dir ./plugins.d --port 0 "$@" >"${log_file}" 2>&1 &
  UI_PID=$!
  for _ in $(seq 1 100); do
    if ! kill -0 "${UI_PID}" >/dev/null 2>&1; then
      cat "${log_file}" >&2 || true
      fail "mh ui exited before reporting a listening port"
    fi
    UI_PORT="$(sed -n 's/.*http:\/\/127\.0\.0\.1:\([0-9][0-9]*\).*/\1/p' "${log_file}" | tail -n 1)"
    if [[ -n "${UI_PORT}" ]]; then
      return 0
    fi
    sleep 0.05
  done
  cat "${log_file}" >&2 || true
  fail "timed out waiting for mh ui to listen"
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
  stop_ui
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
run ./mh --help > mh-help.txt

trusted_ui_supported=0
if [[ -x "./mh-ui-ext" ]]; then
  trusted_ui_supported=1
fi
case "${VERIFY_TRUSTED_UI}" in
  1 | true | yes) trusted_ui_required=1 ;;
  0 | false | no) trusted_ui_required=0 ;;
  auto) trusted_ui_required="${trusted_ui_supported}" ;;
  *) fail "VERIFY_TRUSTED_UI must be auto, 1, or 0" ;;
esac

if [[ "${trusted_ui_required}" == "1" && "${trusted_ui_supported}" != "1" ]]; then
  fail "VERIFY_TRUSTED_UI=1 but the binary tarball has no executable ./mh-ui-ext"
fi

if [[ "${trusted_ui_required}" == "1" ]]; then
  run ./mh-ui-ext --help > mh-ui-ext-help.txt
fi

if [[ "${trusted_ui_required}" == "1" && -n "${TRUSTED_UI_GATE}" ]]; then
  [[ -f "${TRUSTED_UI_GATE}" ]] || fail "trusted UI gate does not exist: ${TRUSTED_UI_GATE}"
  run env \
    MH_BIN="${WORK_DIR}/mh" \
    MH_UI_EXT_BIN="${WORK_DIR}/mh-ui-ext" \
    python3 "${TRUSTED_UI_GATE}"
fi

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

ui_supported=0
if grep -q 'mh ui --db' mh-help.txt; then
  ui_supported=1
fi
case "${VERIFY_UI}" in
  1 | true | yes) ui_required=1 ;;
  0 | false | no) ui_required=0 ;;
  auto) ui_required="${ui_supported}" ;;
  *) fail "VERIFY_UI must be auto, 1, or 0" ;;
esac

if [[ "${ui_required}" == "1" && "${ui_supported}" != "1" ]]; then
  fail "VERIFY_UI=1 but the release binary help does not advertise mh ui"
fi

if [[ "${ui_required}" == "1" ]]; then
  start_ui ui-readonly.log
  .venv/bin/python - "${UI_PORT}" "${WORK_DIR}" <<'PY'
import json
import sys
import urllib.error
import urllib.request

port = sys.argv[1]
work_dir = sys.argv[2]
base = f"http://127.0.0.1:{port}"


def request(path, method="GET", token=None, payload=None):
    data = None
    headers = {}
    if payload is not None:
        data = json.dumps(payload).encode()
        headers["Content-Type"] = "application/json"
    if token is not None:
        headers["X-MH-UI-Token"] = token
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, resp.reason, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as err:
        return err.code, err.reason, dict(err.headers), err.read()


status, reason, _headers, body = request("/api/summary")
assert (status, reason) == (200, "OK"), (status, reason, body)
summary = json.loads(body)
assert summary["inspection"]["source_posts"] == 1, summary

status, reason, _headers, body = request("/api/records?limit=10")
assert (status, reason) == (200, "OK"), (status, reason, body)
records = json.loads(body)
assert records["records"][0]["source_url"] == "synthetic://quickstart/1", records

status, reason, _headers, body = request("/api/plugins")
assert (status, reason) == (200, "OK"), (status, reason, body)
plugins = json.loads(body)
assert plugins["plugins"][0]["id"] == "quickstart", plugins
assert work_dir not in body.decode("utf-8"), body.decode("utf-8")

status, reason, headers, body = request("/api/summary", method="POST")
assert (status, reason) == (405, "Method Not Allowed"), (status, reason, body)
assert headers.get("Allow") == "GET, HEAD", headers
assert not any(name.lower().startswith("access-control-") for name in headers), headers

status, reason, _headers, body = request("/api/manage/status")
assert (status, reason) == (200, "OK"), (status, reason, body)
status_body = json.loads(body)
assert status_body["manage"] is False, status_body

status, reason, _headers, body = request(
    "/api/manage/discover",
    method="POST",
    payload={
        "plugin_id": "quickstart",
        "max_pages": 1,
        "per_page": 30,
        "max_records": 30,
        "timeout_seconds": 60,
    },
)
assert (status, reason) == (403, "Forbidden"), (status, reason, body)
PY
  stop_ui

  start_ui ui-manage.log --manage
  .venv/bin/python - "${UI_PORT}" <<'PY'
import json
import re
import sys
import time
import urllib.error
import urllib.request

port = sys.argv[1]
base = f"http://127.0.0.1:{port}"


def request(path, method="GET", token=None, payload=None):
    data = None
    headers = {}
    if payload is not None:
        data = json.dumps(payload).encode()
        headers["Content-Type"] = "application/json"
    if token is not None:
        headers["X-MH-UI-Token"] = token
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, resp.reason, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as err:
        return err.code, err.reason, dict(err.headers), err.read()


status, reason, _headers, body = request("/")
assert (status, reason) == (200, "OK"), (status, reason, body)
match = re.search(rb"window\.MH_UI = (\{.*?\});", body)
assert match, body[:200]
config = json.loads(match.group(1))
assert config["manage"] is True, config
assert len(config["token"]) == 64, config
token = config["token"]

status, reason, headers, body = request("/api/manage/init-db", method="POST", token="bad-token", payload={})
assert (status, reason) == (403, "Forbidden"), (status, reason, body)
assert not any(name.lower().startswith("access-control-") for name in headers), headers

status, reason, headers, body = request("/api/manage/discover")
assert (status, reason) == (405, "Method Not Allowed"), (status, reason, body)
assert headers.get("Allow") == "POST", headers

status, reason, _headers, body = request(
    "/api/manage/discover",
    method="POST",
    token=token,
    payload={
        "plugin_id": "quickstart",
        "max_pages": 1001,
        "per_page": 30,
        "max_records": 30,
        "timeout_seconds": 60,
    },
)
assert (status, reason) == (400, "Bad Request"), (status, reason, body)

status, reason, _headers, body = request(
    "/api/manage/discover",
    method="POST",
    token=token,
    payload={
        "plugin_id": "quickstart",
        "max_pages": 1,
        "per_page": 30,
        "max_records": 30,
        "timeout_seconds": 60,
    },
)
assert (status, reason) == (202, "Accepted"), (status, reason, body)
run_id = json.loads(body)["run_id"]

last = None
for _ in range(100):
    status, reason, _headers, body = request("/api/manage/status")
    assert (status, reason) == (200, "OK"), (status, reason, body)
    state = json.loads(body)
    last = state.get("last")
    if last and last.get("status") == "succeeded":
        break
    time.sleep(0.1)
else:
    raise AssertionError(last)

assert last["run_id"] == run_id, last
assert last["result"]["discover_records"] == 1, last
assert last["result"]["spooled_records"] == 1, last
assert last["result"]["ingested_records"] == 1, last

status, reason, _headers, body = request("/api/summary")
assert (status, reason) == (200, "OK"), (status, reason, body)
summary = json.loads(body)
assert summary["inspection"]["source_posts"] == 1, summary
PY
  stop_ui
  echo "standalone UI quickstart verified"
else
  echo "standalone UI quickstart skipped (VERIFY_UI=${VERIFY_UI}, ui_supported=${ui_supported})"
fi
