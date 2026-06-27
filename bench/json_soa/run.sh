#!/usr/bin/env bash
# JSON → SoA analytics benchmark: Align `json.decode → soa<Row> → where(.active).pay.sum()` vs
# idiomatic Rust `serde_json → Vec<Row> → filter/sum`. Unlike the flat `bench/`, the kernel pulls in
# the Align runtime (JSON parser / arena), so the harness links `libalign_runtime.a` too.
#
#   bench/json_soa/run.sh [baseline|v3|native]   (default: native — both sides at the host's best CPU)
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

# Build alignc + the runtime staticlib (release). Two invocations: the staticlib crate-type of
# `align_runtime` is what produces `libalign_runtime.a`.
( cd ../.. && cargo build -q --release --bin alignc && cargo build -q --release -p align_runtime )
ALIGNC="../../target/release/alignc"
RT_DIR="$(cd ../.. && pwd)/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library (.so/.dylib) in $RT_DIR" >&2; exit 1; }

KOBJ="$PWD/kernel.o"
trap 'rm -f "$KOBJ"' EXIT  # always clean up the temp object, even on failure/interrupt
"$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt"

echo "target: $mode"
ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
