#!/usr/bin/env bash
# par_map benchmark: Align `s.par_map(work).sum()` (persistent worker pool) vs Rust sequential and
# Rust `rayon` (work-stealing pool). The kernel pulls in the Align runtime, so the harness links
# `libalign_runtime.so` (cdylib — dynamic, over the C-ABI, so its std doesn't collide with ours).
#
#   bench/par_map/run.sh [baseline|v3|native|threshold]   (default: native)
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native"; rust_tgt="native" ;;
  threshold) align_tgt="native"; rust_tgt="native" ;;
  v3) case "$(uname -m)" in x86_64|amd64) align_tgt="x86-64-v3"; rust_tgt="x86-64-v3" ;; *) echo "v3 is x86_64-only" >&2; exit 1 ;; esac ;;
  baseline)
    align_tgt="baseline"
    case "$(uname -m)" in
      x86_64|amd64) rust_tgt="x86-64-v2" ;;
      *) rust_tgt="generic" ;;
    esac
    ;;
  *) echo "usage: run.sh [baseline|v3|native|threshold]" >&2; exit 2 ;;
esac

( cd ../.. && cargo build -q --release --bin alignc && cargo build -q --release -p align_runtime --features par-map-probe )
ALIGNC="../../target/release/alignc"
RT_DIR="$(cd ../.. && pwd)/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic lib in $RT_DIR" >&2; exit 1; }

KOBJ="$PWD/kernel.o"
trap 'rm -f "$KOBJ"' EXIT
"$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt" \
  --export pmap_cheap --export smap_cheap --export pmap --export smap

echo "target: $mode (Align=$align_tgt, Rust=$rust_tgt)"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=$rust_tgt"
if [ "$mode" = threshold ]; then
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release -- threshold
else
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
fi
