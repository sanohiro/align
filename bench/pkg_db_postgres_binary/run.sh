#!/usr/bin/env bash
# D13 PostgreSQL Text-versus-Binary parameter/result measurement.
set -euo pipefail

cd "$(dirname "$0")"
repo="$(cd ../.. && pwd)"
url="${ALIGN_DB_POSTGRES_URL:-}"
if [ -z "$url" ]; then
  echo "set ALIGN_DB_POSTGRES_URL to a disposable PostgreSQL database" >&2
  exit 2
fi

( cd "$repo" && scripts/cargo.sh build -q --release --bin alignc )
( cd "$repo" && scripts/cargo.sh build -q --release -p align_runtime --features alloc-count )

build_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$build_dir"
}
trap cleanup EXIT
mkdir -p "$build_dir/pkg"
cp -R "$repo/apps/db/pkg/." "$build_dir/pkg/"
cp kernel.align "$build_dir/"

(
  cd "$build_dir"
  "$repo/target/release/alignc" build kernel.align \
    --profile fast --target-cpu native --no-rt-lto >/dev/null
)

echo "rows=100000"
echo "record order: text-first, binary-first, text-scan, binary-scan"
echo "fields per record: elapsed_ns, allocations, frees, checksum"
"$build_dir/kernel" "$url"
