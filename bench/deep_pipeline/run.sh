#!/usr/bin/env bash
# Deep-stage pipeline scaling benchmark. Align and clang controls use the same CPU target and O2.
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:-native}"
CLANG="${CLANG:-clang-22}"
arch="$(uname -m)"
case "$mode" in
  native)
    align_cpu="native"
    rust_cpu="native"
    case "$arch" in
      x86_64|amd64) clang_cpu=(-march=native) ;;
      arm64|aarch64) clang_cpu=(-mcpu=native) ;;
      *) echo "error: unsupported benchmark host architecture '$arch'" >&2; exit 1 ;;
    esac
    ;;
  v3)
    case "$(uname -m)" in
      x86_64|amd64)
        align_cpu="x86-64-v3"
        rust_cpu="x86-64-v3"
        clang_cpu=(-march=x86-64-v3)
        ;;
      *)
        echo "error: v3 mode is x86_64-only; use native or baseline" >&2
        exit 1
        ;;
    esac
    ;;
  baseline)
    align_cpu="baseline"
    case "$arch" in
      x86_64|amd64) rust_cpu="x86-64-v2"; clang_cpu=(-march=x86-64-v2) ;;
      arm64|aarch64) rust_cpu="generic"; clang_cpu=(-march=armv8-a) ;;
      *) echo "error: unsupported benchmark host architecture '$arch'" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "usage: run.sh [baseline|v3|native]" >&2
    exit 2
    ;;
esac

if ! command -v "$CLANG" >/dev/null 2>&1; then
  echo "error: '$CLANG' is required for the equal-LLVM control" >&2
  exit 1
fi

if [ -z "${ALIGNC:-}" ]; then
  (cd ../.. && cargo build -q --release --bin alignc)
  ALIGNC="$(cd ../.. && pwd)/target/release/alignc"
fi
if [ ! -x "$ALIGNC" ]; then
  echo "error: alignc not found at '$ALIGNC'" >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

exports=()
for family in named masked capture guarded; do
  for depth in 1 2 4 8 16 32; do
    exports+=(--export "${family}_${depth}")
  done
done

"$ALIGNC" emit-llvm kernels.align --stage optimized \
  --target-cpu "$align_cpu" "${exports[@]}" >"$tmp/align.ll"
"$CLANG" -O2 "${clang_cpu[@]}" -S -emit-llvm controls.c -o "$tmp/control.ll"

function_ir() {
  local file="$1" name="$2"
  sed -n "/^define .*@${name}(/,/^}/p" "$file"
}

reduce_shape() {
  grep -oE 'llvm\.vector\.reduce\.add\.v[0-9]+i64' | sort -u | paste -sd, - || true
}

checked=0
for family in named masked capture guarded; do
  for depth in 1 2 4 8 16 32; do
    align_name="${family}_${depth}"
    control_name="c_${family}_${depth}"
    align_ir="$(function_ir "$tmp/align.ll" "$align_name")"
    control_ir="$(function_ir "$tmp/control.ll" "$control_name")"
    if [ -z "$align_ir" ] || [ -z "$control_ir" ]; then
      echo "error: missing optimized IR for $align_name / $control_name" >&2
      exit 1
    fi
    align_reduce="$(printf '%s\n' "$align_ir" | reduce_shape)"
    control_reduce="$(printf '%s\n' "$control_ir" | reduce_shape)"
    if [ "$align_reduce" != "$control_reduce" ]; then
      echo "error: vector-reduction shape differs for $align_name" >&2
      echo "  Align: ${align_reduce:--}" >&2
      echo "  clang: ${control_reduce:--}" >&2
      exit 1
    fi
    checked=$((checked + 1))
  done
done
echo "optimized-IR reduction parity: $checked/24"

"$ALIGNC" emit-obj kernels.align "$tmp/kernels.o" \
  --target-cpu "$align_cpu" --profile release "${exports[@]}"
"$CLANG" -O2 "${clang_cpu[@]}" -c controls.c -o "$tmp/controls.o"
rustc --edition=2021 harness.rs -o "$tmp/deep-pipeline" \
  -C opt-level=2 -C codegen-units=1 -C target-cpu="$rust_cpu" \
  -C link-arg="$tmp/kernels.o" -C link-arg="$tmp/controls.o"

echo "target: $mode (Align/clang O2, LLVM 22)"
"$tmp/deep-pipeline"
