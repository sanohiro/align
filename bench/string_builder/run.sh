#!/usr/bin/env bash
# string_builder benchmark: Align's `builder` write path (the reduce-append pattern) vs idiomatic
# Rust string building (naive `String` + `to_string`, and capacity-preallocated + manual itoa). The
# kernel pulls in the Align runtime (the builder), so the harness links `libalign_runtime.so`
# (a cdylib — dynamic, over the C-ABI, so its std doesn't collide with the harness's std).
#
#   bench/string_builder/run.sh [baseline|v3|native]   (default: native)
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native" ;;
  v3)
    case "$(uname -m)" in
      x86_64|amd64) align_tgt="x86-64-v3" ;;
      *) echo "error: v3 is x86_64-only (host is $(uname -m))" >&2; exit 1 ;;
    esac ;;
  baseline) align_tgt="baseline" ;;
  *) echo "usage: run.sh [baseline|v3|native]" >&2; exit 2 ;;
esac

( cd ../.. && cargo build -q --release --bin alignc && cargo build -q --release -p align_runtime )
ALIGNC="../../target/release/alignc"
RT_DIR="$(cd ../.. && pwd)/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library (.so/.dylib) in $RT_DIR" >&2; exit 1; }

KOBJ="$PWD/kernel.o"
trap 'rm -f "$KOBJ"' EXIT  # always clean up the temp object, even on failure/interrupt
"$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt" \
  --export build --export build_cap --export build_static_one --export build_static_two --export build_int_only

echo "target: $mode"
ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
