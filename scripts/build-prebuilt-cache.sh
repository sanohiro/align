#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: scripts/build-prebuilt-cache.sh ALIGNC OUTPUT_ROOT" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ALIGNC_ARG="$1"
OUTPUT_ROOT="$2"
if [[ ! -x "$ALIGNC_ARG" ]]; then
  echo "prebuilt cache: compiler is not executable: $ALIGNC_ARG" >&2
  exit 1
fi
ALIGNC_BIN="$(cd "$(dirname "$ALIGNC_ARG")" && pwd)/$(basename "$ALIGNC_ARG")"
# Both compiler invocations run from WORK_ROOT below. Preserve the caller's
# meaning for a relative release destination before changing directories.
case "$OUTPUT_ROOT" in
  /*) ;;
  *) OUTPUT_ROOT="$PWD/$OUTPUT_ROOT" ;;
esac

WORK_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-prebuilt-cache.XXXXXX")"
cleanup() {
  local status=$?
  if [[ $status -eq 0 ]]; then
    rm -rf "$WORK_ROOT"
  else
    echo "prebuilt cache: preserved failed work at $WORK_ROOT" >&2
  fi
}
trap cleanup EXIT
PROJECT="$WORK_ROOT/project"
WARM_ROOT="$WORK_ROOT/writable"
mkdir -p "$PROJECT/pkg"

for TREE in apps/web/pkg apps/frame/pkg apps/auth/pkg apps/db/pkg apps/kv/pkg apps/csv/pkg apps/ws/pkg; do
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
import pkg.frame
import pkg.auth
import pkg.kv
import pkg.csv
import pkg.ws

fn main() -> i32 = 0
ALIGN

EXPECTED="$WORK_ROOT/expected-units.txt"
find "$PROJECT" -name '*.align' -type f -exec \
  awk '$1 == "module" { print $2; found = 1 } END { if (!found) exit 1 }' '{}' \; \
  | LC_ALL=C sort -u > "$EXPECTED"
SOURCE_COUNT="$(find "$PROJECT" -name '*.align' -type f | wc -l | tr -d ' ')"
EXPECTED_COUNT="$(wc -l < "$EXPECTED" | tr -d ' ')"
if [[ "$SOURCE_COUNT" != "$EXPECTED_COUNT" ]]; then
  echo "prebuilt cache: source/module inventory differs ($SOURCE_COUNT files, $EXPECTED_COUNT modules)" >&2
  exit 1
fi

OUTCOMES="$WORK_ROOT/warm.outcomes"
(
  cd "$WORK_ROOT"
  ALIGNC_CACHE="$WARM_ROOT" "$ALIGNC_BIN" build "$PROJECT/main.align" \
    --profile release --target-cpu baseline --cache-stats >/dev/null 2> "$OUTCOMES"
)

python3 "$REPO_ROOT/scripts/prebuilt-cache-inventory.py" bundle \
  --source "$WARM_ROOT" \
  --destination "$OUTPUT_ROOT" \
  --expected "$EXPECTED" \
  --outcomes "$OUTCOMES"

BEFORE="$WORK_ROOT/before.sha256"
AFTER="$WORK_ROOT/after.sha256"
hash_files() {
  if command -v sha256sum >/dev/null 2>&1; then
    find "$1" -type f -exec sha256sum '{}' \;
  else
    find "$1" -type f -exec shasum -a 256 '{}' \;
  fi
}
hash_files "$OUTPUT_ROOT" | LC_ALL=C sort > "$BEFORE"
HOT_OUTCOMES="$WORK_ROOT/hot.outcomes"
(
  cd "$WORK_ROOT"
  ALIGNC_CACHE="$OUTPUT_ROOT" "$ALIGNC_BIN" build "$PROJECT/main.align" \
    --profile release --target-cpu baseline --cache-stats >/dev/null 2> "$HOT_OUTCOMES"
)
python3 "$REPO_ROOT/scripts/prebuilt-cache-inventory.py" verify-outcomes \
  --expected "$EXPECTED" --outcomes "$HOT_OUTCOMES" --require hit
hash_files "$OUTPUT_ROOT" | LC_ALL=C sort > "$AFTER"
cmp "$BEFORE" "$AFTER"

echo "prebuilt cache: warmed and verified $EXPECTED_COUNT units"
