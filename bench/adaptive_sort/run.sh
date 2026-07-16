#!/usr/bin/env bash
# Adaptive total-order stable-sort probe (doc-12 §4.1). Builds the kernel with the CURRENT (post-
# change) alignc for the `after` symbols and with a main-worktree alignc for the `before` symbols
# (exports sed-renamed with a `_before` suffix so both link into one harness), links both against the
# runtime cdylib built with `--features alloc-count`, and runs the in-process AB/BA harness.
#
#   bench/adaptive_sort/run.sh [baseline|v3|native]   (default: native)
#
# The `before` alignc lives in a cached git worktree at `../../.adaptive-sort-before-wt`; delete it to
# force a rebuild. Building it recompiles the compiler once (slow), then it is reused.
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

# 1. Current alignc + runtime (with alloc-count) from this working tree.
( cd "$REPO" && cargo build -q --release --bin alignc && cargo build -q --release -p align_runtime --features alloc-count )
ALIGNC_AFTER="$REPO/target/release/alignc"
RT_DIR="$REPO/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library in $RT_DIR" >&2; exit 1; }

# 2. main-worktree alignc for the `before` build (pre-change baseline).
WT="$REPO/../.adaptive-sort-before-wt"
if [ ! -x "$WT/target/release/alignc" ]; then
  echo "building the pre-change (main) alignc in a worktree — one-time, slow…" >&2
  git -C "$REPO" worktree add --force "$WT" main >/dev/null 2>&1 || git -C "$REPO" worktree add "$WT" main
  ( cd "$WT" && cargo build -q --release --bin alignc )
fi
ALIGNC_BEFORE="$WT/target/release/alignc"

# 3. Compile both kernels. The `before` kernel is kernel.align with each export renamed `_before`.
AFTER_O="$PWD/kernel_after.o"
BEFORE_O="$PWD/kernel_before.o"
BEFORE_ALIGN="$PWD/kernel_before.align"
trap 'rm -f "$AFTER_O" "$BEFORE_O" "$BEFORE_ALIGN"' EXIT
sed -E 's/pub fn (sort_u64|sort_by_key_u64|sort_str)\b/pub fn \1_before/' kernel.align > "$BEFORE_ALIGN"

"$ALIGNC_AFTER" emit-obj kernel.align "$AFTER_O" --target-cpu "$align_tgt" \
  --export sort_u64 --export sort_by_key_u64 --export sort_str
"$ALIGNC_BEFORE" emit-obj "$BEFORE_ALIGN" "$BEFORE_O" --target-cpu "$align_tgt" \
  --export sort_u64_before --export sort_by_key_u64_before --export sort_str_before

echo "target: $mode"
ALIGN_KERNEL_AFTER="$AFTER_O" ALIGN_KERNEL_BEFORE="$BEFORE_O" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
