#!/usr/bin/env bash
# par_map benchmark: Align `s.par_map(work).sum()` (persistent worker pool) vs Rust sequential and
# Rust `rayon` (work-stealing pool). The kernel pulls in the Align runtime, so the harness links
# `libalign_runtime.so` (cdylib — dynamic, over the C-ABI, so its std doesn't collide with ours).
#
#   bench/par_map/run.sh [baseline|v3|native|threshold|filter|width|aggregate]   (default: native)
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native"; rust_tgt="native" ;;
  threshold|filter|width|aggregate) align_tgt="native"; rust_tgt="native" ;;
  v3) case "$(uname -m)" in x86_64|amd64) align_tgt="x86-64-v3"; rust_tgt="x86-64-v3" ;; *) echo "v3 is x86_64-only" >&2; exit 1 ;; esac ;;
  baseline)
    align_tgt="baseline"
    case "$(uname -m)" in
      x86_64|amd64) rust_tgt="x86-64-v2" ;;
      *) rust_tgt="generic" ;;
    esac
    ;;
  *) echo "usage: run.sh [baseline|v3|native|threshold|filter|width|aggregate]" >&2; exit 2 ;;
esac

( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="../../target/release/alignc"
RT_DIR="$(cd ../.. && pwd)/target/release"

KOBJ="$PWD/kernel.o"
trap 'rm -f "$KOBJ"' EXIT
"$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt" \
  --export pmap_cheap --export smap_cheap --export pmap --export smap --export pfilter \
  --export pwidth_i8 --export swidth_i8 --export pwidth_i32 --export swidth_i32 \
  --export pwidth_i64 --export swidth_i64 --export pwidth_i8_to_i64 --export swidth_i8_to_i64 \
  --export pwidth_i64_to_i8 --export swidth_i64_to_i8 \
  --export pwidth_materialize_i8_to_i64 --export swidth_materialize_i8_to_i64 \
  --export pwidth_materialize_i64_to_i8 --export swidth_materialize_i64_to_i8

echo "target: $mode (Align=$align_tgt, Rust=$rust_tgt)"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=$rust_tgt"
if [ "$mode" = threshold ] || [ "$mode" = width ] || [ "$mode" = aggregate ]; then
  ( cd ../.. && cargo build -q --release -p align_runtime --features par-map-probe )
  [ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic lib in $RT_DIR" >&2; exit 1; }
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release --features probe -- "$mode"
else
  ( cd ../.. && cargo build -q --release -p align_runtime )
  [ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic lib in $RT_DIR" >&2; exit 1; }
  if [ "$mode" = filter ]; then
    ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release -- filter
  else
    ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
  fi
fi
