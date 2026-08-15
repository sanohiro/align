#!/usr/bin/env bash
# JSON decode-throughput benchmark: Align `json.decode` vs idiomatic Rust `serde_json`, regression
# tracker for the parser rewrite. The kernel pulls in the Align runtime (JSON parser / arena), so
# the harness links the runtime cdylib.
#
#   bench/json_decode/run.sh [baseline|v3|native]   (default: native — both sides at the host's best CPU)
set -euo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd -P)"
repo_root="$(cd "$script_dir/../.." && pwd -P)"
# shellcheck source=bench/json_escape/evidence/benchmark-input.sh
. "$repo_root/bench/json_escape/evidence/benchmark-input.sh"
benchmark_input_begin "$repo_root" "$script_dir"

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

# Build alignc and the runtime cdylib in the private root target. The detached harness has its own
# checked-in lock and target directory.
cargo_wrapper="$repo_root/scripts/cargo.sh"
benchmark_input_run env CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$ALIGN_BENCH_ROOT_TARGET_DIR" \
  "$cargo_wrapper" build -q --release --locked --offline --manifest-path "$repo_root/Cargo.toml" \
  --bin alignc
benchmark_input_run env CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$ALIGN_BENCH_ROOT_TARGET_DIR" \
  "$cargo_wrapper" build -q --release --locked --offline --manifest-path "$repo_root/Cargo.toml" \
  -p align_runtime
ALIGNC="$ALIGN_BENCH_ROOT_TARGET_DIR/release/alignc"
RT_DIR="$ALIGN_BENCH_ROOT_TARGET_DIR/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library (.so/.dylib) in $RT_DIR" >&2; exit 1; }

KOBJ="$ALIGN_BENCH_PRIVATE_DIR/kernel.o"
benchmark_input_run "$ALIGNC" emit-obj "$script_dir/kernel.align" "$KOBJ" --target-cpu "$align_tgt" \
  --export decode_full --export decode_full_len --export decode_proj --export decode_proj_len

echo "target: $mode"
benchmark_input_run env CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$ALIGN_BENCH_DETACHED_TARGET_DIR" \
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" \
  "$cargo_wrapper" run -q --release --locked --offline --manifest-path "$script_dir/Cargo.toml"
