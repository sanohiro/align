#!/usr/bin/env bash
# Local item-6 measurement. This is a performance acceptance harness, not a CI gate.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
target_dir="${ALIGN_FUNCTION_TARGET_DIR:-$repo/target/function-incremental}"
binary="$target_dir/release/align-function-incremental-bench"

# Runtime-only Linux images may expose libpq.so.5 without the development linker name. Parity
# linking needs no headers or server, so provide that name only in a disposable benchmark root.
work="$(mktemp -d)"
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT
if [ "$(uname -s)" = Linux ] && command -v ldconfig >/dev/null 2>&1 \
  && ! cc -print-file-name=libpq.so | grep -q '^/'; then
  pq_runtime="$(ldconfig -p 2>/dev/null | awk '/libpq\.so\.[0-9]+ / { print $NF; exit }')"
  if [ -n "$pq_runtime" ]; then
    mkdir -p "$work/lib"
    ln -s "$pq_runtime" "$work/lib/libpq.so"
    export LIBRARY_PATH="$work/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
  fi
fi

CARGO_TARGET_DIR="$target_dir" "$repo/scripts/cargo.sh" build \
  --release --manifest-path "$here/Cargo.toml"
CARGO_TARGET_DIR="$target_dir" "$repo/scripts/cargo.sh" build \
  --release --manifest-path "$repo/Cargo.toml" -p align_runtime

"$binary" edit
python3 "$here/measure.py" "$binary" cold-unit >"$work/cold-unit.txt"
python3 "$here/measure.py" "$binary" cold-function >"$work/cold-function.txt"
cat "$work/cold-unit.txt"
cat "$work/cold-function.txt"

unit_wall="$(sed -n 's/^wall_seconds=//p' "$work/cold-unit.txt")"
function_wall="$(sed -n 's/^wall_seconds=//p' "$work/cold-function.txt")"
unit_rss="$(sed -n 's/^peak_rss_kb=//p' "$work/cold-unit.txt")"
function_rss="$(sed -n 's/^peak_rss_kb=//p' "$work/cold-function.txt")"
awk -v unit="$unit_wall" -v candidate="$function_wall" \
  'BEGIN { ratio = candidate / unit; printf "cold wall function/unit ratio=%.4f\n", ratio; exit !(ratio <= 1.25) }'
awk -v unit="$unit_rss" -v candidate="$function_rss" \
  'BEGIN { ratio = candidate / unit; printf "cold RSS function/unit ratio=%.4f\n", ratio; exit !(ratio <= 1.25) }'

"$binary" parity
