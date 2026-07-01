#!/usr/bin/env bash
# Regenerate golden fixtures with the Python oracle and assert zero diff against
# the committed crates/mh-protocol/golden/*.hex. Fails (non-zero) on mismatch.
set -euo pipefail
cd "$(dirname "$0")/.."
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
python3 conformance/golden.py "$tmp"
fail=0
for n in initialize discover record record_batch fetch_request state_query; do
  if ! diff -u "crates/mh-protocol/golden/$n.hex" "$tmp/$n.hex"; then
    echo "MISMATCH: $n.hex" >&2
    fail=1
  fi
done
if [ "$fail" -eq 0 ]; then
  echo "golden fixtures match the python oracle"
fi
exit "$fail"
