#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: bench/prebuilt_cache/run.sh PACKAGE_ROOT ARCHIVE SUMMARY" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PACKAGE_ROOT="$(cd "$1" && pwd)"
ARCHIVE="$2"
SUMMARY="$3"
ALIGNC_BIN="$PACKAGE_ROOT/alignc"
CACHE_ROOT="$PACKAGE_ROOT/share/align/cache/1"
if [[ ! -x "$ALIGNC_BIN" || ! -f "$ARCHIVE" || ! -d "$CACHE_ROOT" ]]; then
  echo "prebuilt cache benchmark: package or archive is incomplete" >&2
  exit 1
fi

WORK_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-prebuilt-bench.XXXXXX")"
cleanup() {
  local status=$?
  if [[ $status -eq 0 ]]; then
    rm -rf "$WORK_ROOT"
  else
    echo "prebuilt cache benchmark: preserved failed work at $WORK_ROOT" >&2
  fi
}
trap cleanup EXIT

PROJECT="$WORK_ROOT/project"
mkdir -p "$PROJECT/pkg" "$WORK_ROOT/off" "$WORK_ROOT/hit" "$WORK_ROOT/emit-off" \
  "$WORK_ROOT/emit-hit" "$WORK_ROOT/timing-off" "$WORK_ROOT/timing-hit"
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

run_build() {
  local directory="$1"
  local cache="$2"
  local diagnostics="$3"
  local xdg="$4"
  (
    cd "$directory"
    ALIGNC_CACHE="$cache" XDG_CACHE_HOME="$xdg" \
      "$ALIGNC_BIN" build "$PROJECT/main.align" --profile release \
      --target-cpu baseline >/dev/null 2> "$diagnostics"
  )
}

# Correctness is established before timing. The same source and options must produce identical
# diagnostics, object bytes, executable bytes, and stdout with the cache disabled or packaged-hit.
run_build "$WORK_ROOT/off" off "$WORK_ROOT/off.stderr" "$WORK_ROOT/unused-off"
run_build "$WORK_ROOT/hit" on "$WORK_ROOT/hit.stderr" "$WORK_ROOT/xdg-correctness"
cmp "$WORK_ROOT/off.stderr" "$WORK_ROOT/hit.stderr"
cmp "$WORK_ROOT/off/main" "$WORK_ROOT/hit/main"
"$WORK_ROOT/off/main" > "$WORK_ROOT/off.stdout"
"$WORK_ROOT/hit/main" > "$WORK_ROOT/hit.stdout"
cmp "$WORK_ROOT/off.stdout" "$WORK_ROOT/hit.stdout"
(
  cd "$WORK_ROOT/emit-off"
  ALIGNC_CACHE=off "$ALIGNC_BIN" emit-obj "$PROJECT/main.align" \
    --profile release --target-cpu baseline >/dev/null 2> "$WORK_ROOT/emit-off.stderr"
)
(
  cd "$WORK_ROOT/emit-hit"
  ALIGNC_CACHE=on XDG_CACHE_HOME="$WORK_ROOT/xdg-correctness" \
    "$ALIGNC_BIN" emit-obj "$PROJECT/main.align" \
    --profile release --target-cpu baseline >/dev/null 2> "$WORK_ROOT/emit-hit.stderr"
)
cmp "$WORK_ROOT/emit-off.stderr" "$WORK_ROOT/emit-hit.stderr"
diff -qr "$WORK_ROOT/emit-off" "$WORK_ROOT/emit-hit"
if [[ -e "$WORK_ROOT/xdg-correctness/alignc/1/actions" ]]; then
  echo "prebuilt cache benchmark: packaged hit published into writable storage" >&2
  exit 1
fi

measure() {
  python3 - "$@" <<'PY'
import subprocess
import sys
import time

started = time.perf_counter()
result = subprocess.run(
    sys.argv[2:], cwd=sys.argv[1], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
)
if result.returncode != 0:
    raise SystemExit(result.returncode)
print(f"{time.perf_counter() - started:.6f}")
PY
}

CSV="$WORK_ROOT/times.csv"
echo "pair,mode,seconds" > "$CSV"
pair=1
while [[ $pair -le 4 ]]; do
  if [[ $((pair % 2)) -eq 1 ]]; then
    modes="off hit"
  else
    modes="hit off"
  fi
  for mode in $modes; do
    if [[ "$mode" = off ]]; then
      seconds="$(measure "$WORK_ROOT/timing-off" env ALIGNC_CACHE=off "$ALIGNC_BIN" build "$PROJECT/main.align" \
        --profile release --target-cpu baseline)"
    else
      seconds="$(measure "$WORK_ROOT/timing-hit" env ALIGNC_CACHE=on XDG_CACHE_HOME="$WORK_ROOT/xdg-timing" \
        "$ALIGNC_BIN" build "$PROJECT/main.align" --profile release --target-cpu baseline)"
    fi
    echo "$pair,$mode,$seconds" >> "$CSV"
  done
  pair=$((pair + 1))
done

NO_CACHE_ARCHIVE="$WORK_ROOT/no-cache.tar.gz"
tar -C "$PACKAGE_ROOT" -czf "$NO_CACHE_ARCHIVE" \
  alignc align-repl libalign_runtime.a \
  LICENSE-APACHE LICENSE-MIT README.md README.ja.md
python3 - "$CSV" "$ARCHIVE" "$NO_CACHE_ARCHIVE" "$CACHE_ROOT" "$SUMMARY" <<'PY'
import csv
from pathlib import Path
import statistics
import sys

csv_path, archive, no_cache, cache_root, summary = map(Path, sys.argv[1:])
rows = list(csv.DictReader(csv_path.open(encoding="utf-8")))
off = [float(row["seconds"]) for row in rows if row["mode"] == "off"]
hit = [float(row["seconds"]) for row in rows if row["mode"] == "hit"]
cache_bytes = sum(path.stat().st_size for path in cache_root.rglob("*") if path.is_file())
archive_bytes = archive.stat().st_size
no_cache_bytes = no_cache.stat().st_size
summary.write_text(
    "# Prebuilt cache release evidence\n\n"
    f"- cache-off median: {statistics.median(off):.6f} s ({len(off)} samples)\n"
    f"- packaged-hit median: {statistics.median(hit):.6f} s ({len(hit)} samples)\n"
    f"- cache payload bytes: {cache_bytes}\n"
    f"- archive bytes with cache: {archive_bytes}\n"
    f"- archive bytes without cache: {no_cache_bytes}\n"
    f"- compressed archive delta: {archive_bytes - no_cache_bytes}\n"
    "- correctness: diagnostics, object, executable, and stdout are byte-identical\n",
    encoding="utf-8",
)
PY

echo "prebuilt cache benchmark: wrote $SUMMARY"
