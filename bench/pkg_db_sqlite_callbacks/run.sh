#!/usr/bin/env bash
# D14 SQLite scalar-callback registration and invocation measurement.
set -euo pipefail

cd "$(dirname "$0")"
repo="$(cd ../.. && pwd)"

( cd "$repo" && scripts/cargo.sh build -q --release --bin alignc )

build_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$build_dir"
}
trap cleanup EXIT

if ! command -v pg_config >/dev/null 2>&1; then
  libpq_runtime="$(ldconfig -p 2>/dev/null | awk '$1 == "libpq.so.5" { print $NF; exit }')"
  if [ -z "$libpq_runtime" ]; then
    echo "libpq development linker name is unavailable and no libpq.so.5 runtime was found" >&2
    exit 1
  fi
  mkdir -p "$build_dir/lib"
  ln -s "$libpq_runtime" "$build_dir/lib/libpq.so"
  export LIBRARY_PATH="$build_dir/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
fi

mkdir -p "$build_dir/pkg"
cp -R "$repo/apps/db/pkg/." "$build_dir/pkg/"
cp kernel.align "$build_dir/"

(
  cd "$build_dir"
  "$repo/target/release/alignc" build kernel.align \
    --profile fast --target-cpu native --no-rt-lto >/dev/null
)

echo "rows=10000 registration_iterations=10000"
echo "record order: registration, scalar-arity-0, scalar-arity-1, scalar-arity-127"
echo "fields per record: elapsed_ns, operations, checksum"
"$build_dir/kernel"
