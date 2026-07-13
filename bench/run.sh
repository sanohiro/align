#!/usr/bin/env bash
# Reproducible Align-vs-Rust benchmark. Compiles the Align kernels (`kernels.align`) to an object
# with `alignc emit-obj`, links them into the Rust harness, and runs the head-to-head.
#
#   bench/run.sh [baseline|native]   (default: native — both sides at the host's best CPU)
#
# Both `alignc` and `rustc` are pinned to the SAME target so the comparison is fair.
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native"; rust_tgt="native" ;;
  v3) # portable AVX2/FMA tier (server/container) — x86_64 only
    case "$(uname -m)" in
      x86_64|amd64) align_tgt="x86-64-v3"; rust_tgt="x86-64-v3" ;;
      *) echo "error: v3 mode is x86_64-only (host is $(uname -m)); use native or baseline" >&2; exit 1 ;;
    esac
    ;;
  baseline)
    align_tgt="baseline"
    # Match alignc's per-arch baseline: x86-64-v2 on amd64, the generic (armv8-a) floor elsewhere.
    case "$(uname -m)" in
      x86_64|amd64) rust_tgt="x86-64-v2" ;;
      *)            rust_tgt="generic" ;;
    esac
    ;;
  *) echo "usage: run.sh [baseline|v3|native]" >&2; exit 2 ;;
esac

# Build alignc (release for realistic codegen speed; the produced code is the same either way).
( cd .. && cargo build -q --release --bin alignc )
ALIGNC="../target/release/alignc"

"$ALIGNC" emit-obj kernels.align kernels.o --target-cpu "$align_tgt" \
  --export sum_sq_pos --export col_sum --export total_pay
rustc -O -C target-cpu="$rust_tgt" -C link-arg="$PWD/kernels.o" harness.rs -o harness
echo "target: $mode"
./harness

rm -f kernels.o harness
