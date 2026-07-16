#!/usr/bin/env bash
# Adaptive total-order stable-sort probe (doc-12 §4.1). Compiles kernel.align three times with the
# CURRENT alignc — `after` = shipped shape, `before` = `ALIGN_SORT_ADAPTIVE=off` (the pre-change
# baseline, from the same compiler, so before/after differ only in the sort shape), `ctrl` = shipped
# shape again under a third symbol set (an identical-code control that quantifies the cross-kernel
# i-cache/position measurement bias). Links all three + the runtime cdylib built with
# `--features alloc-count`, and runs the drift-immune (median-of-adjacent-ratios) harness.
#
#   bench/adaptive_sort/run.sh [baseline|v3|native]   (default: native)
#
# WSL2 has no CPU-frequency control, so use `taskset -c` and read the `corrected` (real / control)
# column — the block-sequential method is corrupted by ±25% between-block frequency drift.
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(cd ../.. && pwd)"

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

( cd "$REPO" && cargo build -q --release --bin alignc && cargo build -q --release -p align_runtime --features alloc-count )
ALIGNC="$REPO/target/release/alignc"
RT_DIR="$REPO/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library in $RT_DIR" >&2; exit 1; }

AFTER_O="$PWD/kernel_after.o"; BEFORE_O="$PWD/kernel_before.o"; CTRL_O="$PWD/kernel_ctrl.o"
BEFORE_ALIGN="$PWD/kernel_before.align"; CTRL_ALIGN="$PWD/kernel_ctrl.align"
trap 'rm -f "$AFTER_O" "$BEFORE_O" "$CTRL_O" "$BEFORE_ALIGN" "$CTRL_ALIGN"' EXIT
sed -E 's/pub fn (sort_u64|sort_by_key_u64|sort_str)\b/pub fn \1_before/' kernel.align > "$BEFORE_ALIGN"
sed -E 's/pub fn (sort_u64|sort_by_key_u64|sort_str)\b/pub fn \1_ctrl/'   kernel.align > "$CTRL_ALIGN"

"$ALIGNC" emit-obj kernel.align "$AFTER_O" --target-cpu "$align_tgt" \
  --export sort_u64 --export sort_by_key_u64 --export sort_str
ALIGN_SORT_ADAPTIVE=off "$ALIGNC" emit-obj "$BEFORE_ALIGN" "$BEFORE_O" --target-cpu "$align_tgt" \
  --export sort_u64_before --export sort_by_key_u64_before --export sort_str_before
"$ALIGNC" emit-obj "$CTRL_ALIGN" "$CTRL_O" --target-cpu "$align_tgt" \
  --export sort_u64_ctrl --export sort_by_key_u64_ctrl --export sort_str_ctrl

echo "target: $mode"
ALIGN_KERNEL_AFTER="$AFTER_O" ALIGN_KERNEL_BEFORE="$BEFORE_O" ALIGN_KERNEL_CTRL="$CTRL_O" \
  ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
