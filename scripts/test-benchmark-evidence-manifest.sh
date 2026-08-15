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
[[ "$(python3 -c 'import os,sys; print(format(os.stat(sys.argv[1]).st_mode & 0o777, "o"))' "$tree/manifest.json")" == 644 ]] ||
  fail "manifest mode is not 0644"
[[ "$(sha256sum "$tree/manifest.json" | cut -d' ' -f1)" == "$digest" ]] || fail "manifest digest mismatch"
tail -c 1 "$tree/manifest.json" | od -An -t x1 | grep -q '0a' || fail "manifest lacks final LF"
grep -q '"schema":"align.json_escape_benchmark_install_manifest/v1"' "$tree/manifest.json" || fail "schema is not canonical"
assert_rejected "manifest overwrite" python3 "$MANIFEST_TOOL" write --root "$tree"

fd_tree="$TEST_ROOT/fd-root"
fd_tree_moved="$TEST_ROOT/fd-root-moved"
mkdir -m 700 "$fd_tree"
printf 'bound root\n' > "$fd_tree/payload"
chmod 644 "$fd_tree/payload"
python3 "$MANIFEST_TOOL" write --root "$fd_tree" >/dev/null
PYTHONDONTWRITEBYTECODE=1 python3 - "$fd_tree" "$fd_tree_moved" <<'PY'
import os
import sys

from scripts.benchmark_evidence import manifest

root, moved = sys.argv[1:]
root_fd = manifest._open_root(root)
os.rename(root, moved)
os.mkdir(root, 0o700)
try:
    manifest.verify_manifest_fd(root_fd)
finally:
    os.close(root_fd)
PY

chmod 600 "$tree/manifest.json"
assert_rejected "changed manifest mode" python3 "$MANIFEST_TOOL" verify --root "$tree"
chmod 644 "$tree/manifest.json"

good_manifest="$TEST_ROOT/manifest.good"
cp "$tree/manifest.json" "$good_manifest"
restore_manifest() {
  cp "$good_manifest" "$tree/manifest.json"
}

printf '{"schema":"x","schema":"y"}\n' > "$tree/manifest.json"
assert_rejected "duplicate manifest key" python3 "$MANIFEST_TOOL" verify --root "$tree"
restore_manifest

python3 - "$tree/manifest.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as stream:
    value = json.load(stream)
value["root"]["uid"] = "wrong-type"
with open(path, "w", encoding="utf-8") as stream:
    json.dump(value, stream, separators=(",", ":"))
    stream.write("\n")
PY
assert_rejected "wrong manifest field type" python3 "$MANIFEST_TOOL" verify --root "$tree"
restore_manifest

python3 - "$tree/manifest.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as stream:
    value = json.load(stream)
del value["entries"]
with open(path, "w", encoding="utf-8") as stream:
    json.dump(value, stream, separators=(",", ":"))
    stream.write("\n")
PY
assert_rejected "missing manifest member" python3 "$MANIFEST_TOOL" verify --root "$tree"
restore_manifest

python3 - "$tree/manifest.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as stream:
    value = json.load(stream)
value["entries"].insert(1, value["entries"][0])
with open(path, "w", encoding="utf-8") as stream:
    json.dump(value, stream, separators=(",", ":"))
    stream.write("\n")
PY
assert_rejected "duplicate manifest entry" python3 "$MANIFEST_TOOL" verify --root "$tree"
restore_manifest

assert_rejected "manifest path traversal" python3 "$MANIFEST_TOOL" verify --root "$tree" --manifest ../manifest.json

chmod 755 "$tree"
assert_rejected "changed root mode" python3 "$MANIFEST_TOOL" verify --root "$tree"
chmod 700 "$tree"

python3 - "$tree/manifest.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as stream:
    value = json.load(stream)
value["root"]["uid"] = (value["root"]["uid"] + 1) % (2**32)
value["entries"][0]["gid"] = (value["entries"][0]["gid"] + 1) % (2**32)
with open(path, "w", encoding="utf-8") as stream:
    json.dump(value, stream, separators=(",", ":"))
    stream.write("\n")
PY
assert_rejected "changed manifest owner" python3 "$MANIFEST_TOOL" verify --root "$tree"
restore_manifest

printf 'module with a changed size\n' > "$tree/lib/module.py"
assert_rejected "changed file size" python3 "$MANIFEST_TOOL" verify --root "$tree"
printf 'module\n' > "$tree/lib/module.py"
chmod 644 "$tree/lib/module.py"

hard_tree="$TEST_ROOT/hard-links"
mkdir -m 700 "$hard_tree"
printf 'same inode\n' > "$hard_tree/first"
chmod 644 "$hard_tree/first"
ln "$hard_tree/first" "$hard_tree/second"
assert_rejected "hard-linked files" python3 "$MANIFEST_TOOL" write --root "$hard_tree"

race_tree="$TEST_ROOT/race"
mkdir -m 700 "$race_tree" "$race_tree/child" "$race_tree/replacement"
printf 'child\n' > "$race_tree/child/value"
printf 'replacement\n' > "$race_tree/replacement/value"
chmod 644 "$race_tree/child/value" "$race_tree/replacement/value"
PYTHONDONTWRITEBYTECODE=1 python3 - "$race_tree" <<'PY'
import sys

from scripts.benchmark_evidence import manifest

root = sys.argv[1]
original = manifest._open_directory


def swapped(parent_fd, name):
    if name == "child":
        return original(parent_fd, "replacement")
    return original(parent_fd, name)


manifest._open_directory = swapped
try:
    manifest.build_manifest(root)
except manifest.ManifestError:
    pass
else:
    raise SystemExit("directory identity swap was accepted")
PY

nested_tree="$TEST_ROOT/nested"
outside="$TEST_ROOT/outside"
mkdir -m 700 "$nested_tree" "$outside"
printf 'nested\n' > "$nested_tree/payload"
chmod 644 "$nested_tree/payload"
python3 "$MANIFEST_TOOL" write --root "$nested_tree" --manifest metadata/manifest.json >/dev/null
cp "$nested_tree/metadata/manifest.json" "$outside/manifest.json"
mv "$nested_tree/metadata" "$nested_tree/metadata.real"
ln -s "$outside" "$nested_tree/metadata"
assert_rejected "nested manifest parent symlink" python3 "$MANIFEST_TOOL" verify --root "$nested_tree" --manifest metadata/manifest.json

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
