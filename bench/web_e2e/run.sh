#!/usr/bin/env bash
# web_e2e benchmark (pkg.web W5): the framework's request path end-to-end against the same responses
# written directly on the `std.http` server primitive. Both are real compiled Align servers driven
# over loopback with keep-alive'd connections; the difference is what `pkg.web` costs.
#
#   bench/web_e2e/run.sh [baseline|v3|native]     (default: native)
#   SECS=10 CONNS=64 THREADS=8 bench/web_e2e/run.sh
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

( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="$(cd ../.. && pwd)/target/release/alignc"

# Assemble a build tree: the SHIPPED pkg.web sources + this bench's two server programs. The raw
# control needs no pkg tree, but it shares the directory so both are built the same way.
BUILD="$(mktemp -d)"
trap 'rm -rf "$BUILD"' EXIT
mkdir -p "$BUILD/pkg"
cp -r ../../apps/web/pkg/. "$BUILD/pkg/"
cp align/framework.align align/raw.align "$BUILD/"

( cd "$BUILD" && "$ALIGNC" build framework.align --target-cpu "$align_tgt" >/dev/null \
             && "$ALIGNC" build raw.align --target-cpu "$align_tgt" >/dev/null )

echo "target: $mode"
ALIGN_FRAMEWORK_EXE="$BUILD/framework" ALIGN_RAW_EXE="$BUILD/raw" cargo run -q --release
