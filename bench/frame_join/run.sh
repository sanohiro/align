#!/usr/bin/env bash
# Non-gating local implementation-class probe for the two pkg.frame v1 joins.
set -euo pipefail
cd "$(dirname "$0")"
repo="$(cd ../.. && pwd)"

( cd "$repo" && scripts/cargo.sh build -q --release -p align_runtime )
runtime_dir="$repo/target/release"
if [[ ! -f "$runtime_dir/libalign_runtime.so" && ! -f "$runtime_dir/libalign_runtime.dylib" ]]; then
  echo "missing align_runtime dynamic library in $runtime_dir" >&2
  exit 1
fi

ALIGN_RUNTIME_DIR="$runtime_dir" cargo run -q --release
