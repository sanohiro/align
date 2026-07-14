#!/usr/bin/env bash
# M14 Slice 2 --rt-lto benchmark (docs/impl/07-roadmap.md). Builds ONE kernel object twice through
# the REAL driver — once without `--rt-lto`, once with — links each into the harness, and reports the
# OFF/ON ratio on the `eq_count` (str_eq fast-path) kernel plus the `sum_sq_pos` numeric control.
# Also reports the compile-time delta of the emit-obj step (target: <= ~100 ms over flag-off).
#
#   bench/rt_lto/run.sh [baseline|v3|native]   (default: native)
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
trap 'rm -f "$KOBJ"' EXIT

EXPORTS=(--export eq_count --export sum_sq_pos)

# Time the emit-obj compile step (median of 5) for a flag on/off delta.
compile_ms() {  # $1... = extra alignc args
  local best=999999 i t0 t1 ms
  for i in 1 2 3 4 5; do
    t0=$(date +%s%N)
    "$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt" "${EXPORTS[@]}" "$@" >/dev/null
    t1=$(date +%s%N)
    ms=$(( (t1 - t0) / 1000000 ))
    (( ms < best )) && best=$ms
  done
  echo "$best"
}

run_pass() {  # $1 = label, $2... = extra alignc args
  local label="$1"; shift
  "$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt" "${EXPORTS[@]}" "$@"
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
}

echo "target: $mode"
echo "== OFF (no --rt-lto) =="
OFF=$(run_pass off)
echo "$OFF"
echo "== ON (--rt-lto) =="
ON=$(run_pass on --rt-lto)
echo "$ON"

echo "== compile-time (emit-obj, best of 5) =="
OFF_MS=$(compile_ms)
ON_MS=$(compile_ms --rt-lto)
echo "off=${OFF_MS}ms on=${ON_MS}ms delta=$(( ON_MS - OFF_MS ))ms"

echo "== ratios (OFF/ON, >1 = --rt-lto faster) =="
for k in eq_count sum_sq_pos; do
  o=$(echo "$OFF" | awk -v k="$k" '$1==k{print $2}')
  n=$(echo "$ON"  | awk -v k="$k" '$1==k{print $2}')
  awk -v k="$k" -v o="$o" -v n="$n" 'BEGIN{ if(n>0) printf "%-12s off=%d ns  on=%d ns  ratio=%.3f\n", k, o, n, o/n }'
done
