#!/usr/bin/env bash
# doc-13 §6.6 repeated-needle plan hoisting — REAL leak / double-free gate (finding 5).
# Compiles kernel.align with the CURRENT alignc, links it against the Align runtime cdylib built with
# `--features alloc-count` (so the `align_rt_str_finder_{new,free}_count` counters resolve), and runs
# the harness, which asserts finder_new_count == finder_free_count after a reps loop and after an
# early `?` error exit. Exits non-zero on any leak / double free.
#
#   bench/needle_hoist/run.sh [baseline|native]   (default: native)
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(cd ../.. && pwd)"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native" ;;
  baseline) align_tgt="baseline" ;;
  *) echo "usage: run.sh [baseline|native]" >&2; exit 2 ;;
esac

( cd "$REPO" && cargo build -q --release --bin alignc && cargo build -q --release -p align_runtime --features alloc-count )
ALIGNC="$REPO/target/release/alignc"
RT_DIR="$REPO/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library in $RT_DIR" >&2; exit 1; }

KERNEL_O="$PWD/kernel.o"
trap 'rm -f "$KERNEL_O"' EXIT
"$ALIGNC" emit-obj kernel.align "$KERNEL_O" --target-cpu "$align_tgt" \
  --export count_reps --export count_try

echo "target: $mode"
ALIGN_KERNEL="$KERNEL_O" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
