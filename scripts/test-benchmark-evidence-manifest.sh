#!/usr/bin/env bash
# Deterministic owner for the evidence installation manifest boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-evidence-manifest.XXXXXX")"
MANIFEST_TOOL="$REPO_ROOT/scripts/benchmark_evidence/manifest.py"
export PYTHONDONTWRITEBYTECODE=1
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

fail() {
  echo "evidence manifest test failed: $*" >&2
  exit 1
}

assert_rejected() {
  local label="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    fail "$label was accepted"
  fi
}

tree="$TEST_ROOT/install"
mkdir -m 700 "$tree"
mkdir -m 755 "$tree/bin" "$tree/lib"
mkdir -m 700 "$tree/lib/private"
printf '#!/bin/sh\n' > "$tree/bin/launcher"
chmod 755 "$tree/bin/launcher"
printf 'module\n' > "$tree/lib/module.py"
chmod 644 "$tree/lib/module.py"
printf 'private\n' > "$tree/lib/private/data"
chmod 644 "$tree/lib/private/data"

digest="$(python3 "$MANIFEST_TOOL" write --root "$tree")"
[[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail "write did not return a SHA-256"
[[ "$(python3 "$MANIFEST_TOOL" verify --root "$tree")" == "$digest" ]] || fail "fresh tree did not verify"
[[ "$(sha256sum "$tree/manifest.json" | cut -d' ' -f1)" == "$digest" ]] || fail "manifest digest mismatch"
tail -c 1 "$tree/manifest.json" | od -An -t x1 | grep -q '0a' || fail "manifest lacks final LF"
grep -q '"schema":"align.json_escape_benchmark_install_manifest/v1"' "$tree/manifest.json" || fail "schema is not canonical"
assert_rejected "manifest overwrite" python3 "$MANIFEST_TOOL" write --root "$tree"

printf 'extra\n' > "$tree/lib/extra"
chmod 644 "$tree/lib/extra"
assert_rejected "extra file" python3 "$MANIFEST_TOOL" verify --root "$tree"
rm "$tree/lib/extra"

printf 'changed\n' > "$tree/lib/module.py"
assert_rejected "changed file" python3 "$MANIFEST_TOOL" verify --root "$tree"
printf 'module\n' > "$tree/lib/module.py"
chmod 644 "$tree/lib/module.py"

chmod 755 "$tree/lib/private/data"
assert_rejected "changed mode" python3 "$MANIFEST_TOOL" verify --root "$tree"
chmod 644 "$tree/lib/private/data"

ln -s module.py "$tree/lib/link"
assert_rejected "installed symlink" python3 "$MANIFEST_TOOL" verify --root "$tree"
rm "$tree/lib/link"

ln -s "$tree" "$TEST_ROOT/root-link"
assert_rejected "symlink root" python3 "$MANIFEST_TOOL" verify --root "$TEST_ROOT/root-link"
rm "$TEST_ROOT/root-link"

mv "$tree/manifest.json" "$tree/manifest.saved"
ln -s manifest.saved "$tree/manifest.json"
assert_rejected "symlink manifest" python3 "$MANIFEST_TOOL" verify --root "$tree"
rm "$tree/manifest.json"
mv "$tree/manifest.saved" "$tree/manifest.json"

python3 - "$tree/manifest.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as stream:
    value = json.load(stream)
value["entries"].reverse()
with open(path, "w", encoding="utf-8") as stream:
    json.dump(value, stream, separators=(",", ":"))
    stream.write("\n")
PY
assert_rejected "wrong entry order" python3 "$MANIFEST_TOOL" verify --root "$tree"

python3 - "$tree/manifest.json" <<'PY'
import sys

path = sys.argv[1]
with open(path, "rb") as stream:
    data = stream.read()
with open(path, "wb") as stream:
    stream.write(data.replace(b'"schema":', b'"schema": "', 1))
PY
assert_rejected "noncanonical JSON" python3 "$MANIFEST_TOOL" verify --root "$tree"

echo "evidence manifest checks passed"
