#!/usr/bin/env bash
# doc-10 §8.1 / doc-13 §8.5 unique-buffer donation — REAL leak/double-free gate + AB/BA timing probe.
# Compiles kernel.align TWICE with the current alignc: once with donation ON (default) and once with
# `ALIGN_BUFFER_DONATE=off`, the OFF build with its entry symbols renamed `*_off` (a byte rewrite of
# the assembly-visible export names, done here on the source) so both variants coexist in one process.
# Links both against the Align runtime cdylib built with `--features alloc-count` and runs the
# harness, which asserts alloc==free for both, that ON allocates exactly `reps` fewer buffers, and
# reports the balanced AB/BA timing sweep. Exits non-zero on any leak / double free / result mismatch.
#
#   bench/buffer_donate/run.sh [baseline|native]   (default: native)
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

ON_O="$PWD/kernel_on.o"
OFF_O="$PWD/kernel_off.o"
OFF_SRC="$PWD/kernel_off.align"
trap 'rm -f "$ON_O" "$OFF_O" "$OFF_SRC"' EXIT

# ON build: pub entries alloc_probe / time_probe.
"$ALIGNC" emit-obj kernel.align "$ON_O" --target-cpu "$align_tgt" \
  --export alloc_probe --export time_probe

# OFF build: same source with the two exported entry names suffixed `_off`, compiled with the toggle
# off. Renaming at the Align source level keeps the two objects' public symbols disjoint.
sed -e 's/\balloc_probe\b/alloc_probe_off/g' -e 's/\btime_probe\b/time_probe_off/g' kernel.align > "$OFF_SRC"
ALIGN_BUFFER_DONATE=off "$ALIGNC" emit-obj "$OFF_SRC" "$OFF_O" --target-cpu "$align_tgt" \
  --export alloc_probe_off --export time_probe_off

echo "target: $mode"
ALIGN_KERNEL_ON="$ON_O" ALIGN_KERNEL_OFF="$OFF_O" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
