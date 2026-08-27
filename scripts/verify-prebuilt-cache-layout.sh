#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/verify-prebuilt-cache-layout.sh PACKAGE_ROOT" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_ROOT="$(cd "$1" && pwd)"
ALIGNC_BIN="$PACKAGE_ROOT/alignc"
CACHE_ROOT="$PACKAGE_ROOT/share/align/cache/1"
if [[ ! -x "$ALIGNC_BIN" || ! -f "$PACKAGE_ROOT/libalign_runtime.a" ]]; then
  echo "prebuilt cache layout: compiler/runtime payload is incomplete" >&2
  exit 1
fi

WORK_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-prebuilt-layout.XXXXXX")"
cleanup() {
  local status=$?
  if [[ $status -eq 0 ]]; then
    rm -rf "$WORK_ROOT"
  else
    echo "prebuilt cache layout: preserved failed work at $WORK_ROOT" >&2
  fi
}
trap cleanup EXIT
PROJECT="$WORK_ROOT/project"
mkdir -p "$PROJECT/pkg"
for TREE in apps/web/pkg apps/jwt/pkg apps/db/pkg; do
  cp -R "$REPO_ROOT/$TREE/." "$PROJECT/pkg/"
done
cat > "$PROJECT/main.align" <<'ALIGN'
module align_release_cache_warm

import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import pkg.db.pool
import pkg.web
import pkg.web.types
import pkg.web.cookie
import pkg.web.cors
import pkg.web.multipart
import pkg.jwt

fn main() -> i32 = 0
ALIGN

EXPECTED="$WORK_ROOT/expected-units.txt"
find "$PROJECT" -name '*.align' -type f -exec \
  awk '$1 == "module" { print $2; found = 1 } END { if (!found) exit 1 }' '{}' \; \
  | LC_ALL=C sort -u > "$EXPECTED"

hash_one() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

hash_tree() {
  if command -v sha256sum >/dev/null 2>&1; then
    find "$1" -type f -exec sha256sum '{}' \;
  else
    find "$1" -type f -exec shasum -a 256 '{}' \;
  fi
}

COMPILER_BEFORE="$(hash_one "$ALIGNC_BIN")"
hash_tree "$CACHE_ROOT" | LC_ALL=C sort > "$WORK_ROOT/cache.before"

run_build() {
  local cache_value="$1"
  local output="$2"
  local xdg="$3"
  (
    cd "$WORK_ROOT"
    ALIGNC_CACHE="$cache_value" XDG_CACHE_HOME="$xdg" \
      "$ALIGNC_BIN" build "$PROJECT/main.align" --profile release \
      --target-cpu baseline --cache-stats >/dev/null 2> "$output"
  )
}

run_build on "$WORK_ROOT/hit.outcomes" "$WORK_ROOT/xdg"
python3 "$REPO_ROOT/scripts/prebuilt-cache-inventory.py" verify-outcomes \
  --expected "$EXPECTED" --outcomes "$WORK_ROOT/hit.outcomes" --require hit
if [[ -e "$WORK_ROOT/xdg/alignc/1/actions" ]]; then
  echo "prebuilt cache layout: packaged hit was promoted into writable storage" >&2
  exit 1
fi

ALIGNC_CACHE=on XDG_CACHE_HOME="$WORK_ROOT/xdg" "$ALIGNC_BIN" cache clear >/dev/null
run_build on "$WORK_ROOT/after-clear.outcomes" "$WORK_ROOT/xdg"
python3 "$REPO_ROOT/scripts/prebuilt-cache-inventory.py" verify-outcomes \
  --expected "$EXPECTED" --outcomes "$WORK_ROOT/after-clear.outcomes" --require hit

run_build "$WORK_ROOT/custom" "$WORK_ROOT/custom.outcomes" "$WORK_ROOT/unused-xdg"
python3 "$REPO_ROOT/scripts/prebuilt-cache-inventory.py" verify-outcomes \
  --expected "$EXPECTED" --outcomes "$WORK_ROOT/custom.outcomes" --require miss

mkdir -p "$WORK_ROOT/absent-source"
cat > "$WORK_ROOT/absent-source/main.align" <<'ALIGN'
import pkg.web
fn main() -> i32 = 0
ALIGN
if ALIGNC_CACHE=on XDG_CACHE_HOME="$WORK_ROOT/absent-xdg" \
  "$ALIGNC_BIN" build "$WORK_ROOT/absent-source/main.align" \
  >"$WORK_ROOT/absent.stdout" 2>"$WORK_ROOT/absent.stderr"; then
  echo "prebuilt cache layout: absent package source unexpectedly built" >&2
  exit 1
fi
grep -q "cannot find module" "$WORK_ROOT/absent.stderr"

COMPILER_AFTER="$(hash_one "$ALIGNC_BIN")"
test "$COMPILER_BEFORE" = "$COMPILER_AFTER"
hash_tree "$CACHE_ROOT" | LC_ALL=C sort > "$WORK_ROOT/cache.after"
cmp "$WORK_ROOT/cache.before" "$WORK_ROOT/cache.after"

echo "prebuilt cache layout: PASS"
