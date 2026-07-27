#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(cd ../.. && pwd)"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native" ;;
  baseline) align_tgt="baseline" ;;
  *) echo "usage: run.sh [baseline|native]" >&2; exit 2 ;;
esac

(
  cd "$REPO"
  cargo build -q --release --bin alignc
  cargo build -q --release -p align_runtime --features alloc-count
)
ALIGNC="$REPO/target/release/alignc"
RT_DIR="$REPO/target/release"
KERNEL_O="$PWD/kernel.o"
KERNEL_LL="$PWD/kernel.ll"
trap 'rm -f "$KERNEL_O" "$KERNEL_LL"' EXIT

"$ALIGNC" emit-obj kernel.align "$KERNEL_O" --target-cpu "$align_tgt" \
  --export scalar_rows --export tagged_none --export tagged_some --export tagged_sparse \
  --export tagged_replace --export tagged_conditional_replace --export tagged_match_loop_replace \
  --export tagged_early_try
"$ALIGNC" emit-llvm kernel.align --target-cpu "$align_tgt" > "$KERNEL_LL"

echo "target: $mode"
echo "raw LLVM Option Drop tag branches: $(grep -c 'dropoptissome' "$KERNEL_LL" || true)"
ALIGN_KERNEL="$KERNEL_O" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
