#!/usr/bin/env bash
# web_router benchmark (pkg.web W5): the shipped framework router's per-request dispatch against a
# hand-written match over the same six-route table. The contract (`docs/impl/pkg-design/web.md`
# item 3) is that dispatch is O(path segments) — flat in table size — and within noise of what an
# app would write by hand.
#
# The router lives in `pkg.web.internal.*`, which the pkg-foundation D7 rule makes importable only
# from within `pkg.web`. So the build ASSEMBLES a tree: the shipped `apps/web/pkg/**` sources plus
# `align/bench_window.align` (a `pkg.web.bench` window that forwards to the internal router) and
# `align/kernel.align` (the entry unit, which holds every `--export`ed function). Nothing here is
# copied into the shipped package.
#
#   bench/web_router/run.sh [baseline|v3|native]   (default: native)
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
ALIGNC="$(cd ../.. && pwd)/target/release/alignc"
RT_DIR="$(cd ../.. && pwd)/target/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || {
  echo "missing libalign_runtime dynamic library (.so/.dylib) in $RT_DIR" >&2; exit 1; }

# Assemble the module tree in a temp dir: the SHIPPED pkg.web sources + the bench-only window.
BUILD="$(mktemp -d)"
trap 'rm -rf "$BUILD"' EXIT
mkdir -p "$BUILD/pkg/web/internal"
cp ../../apps/web/pkg/web.align "$BUILD/pkg/"
cp ../../apps/web/pkg/web/types.align "$BUILD/pkg/web/"
cp ../../apps/web/pkg/web/internal/router.align ../../apps/web/pkg/web/internal/query.align "$BUILD/pkg/web/internal/"
cp align/bench_window.align "$BUILD/pkg/web/bench.align"
cp align/kernel.align "$BUILD/kernel.align"

# `emit-obj` writes its objects into the CURRENT directory, one per module — so emit from inside
# the assembled tree and collect them there.
( cd "$BUILD" && "$ALIGNC" emit-obj kernel.align --target-cpu "$align_tgt" \
    --export fw --export hw --export fw_big >/dev/null )

# One object per module; link them all.
OBJS="$(ls "$BUILD"/*.o | tr '\n' ':')"

echo "target: $mode"
ALIGN_KERNEL_OBJS="$OBJS" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
