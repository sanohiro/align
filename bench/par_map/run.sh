#!/usr/bin/env bash
# par_map benchmark: Align `s.par_map(work).sum()` (persistent worker pool) vs Rust sequential and
# Rust `rayon` (work-stealing pool). The kernel pulls in the Align runtime, so the harness links
# `libalign_runtime.so` (cdylib — dynamic, over the C-ABI, so its std doesn't collide with ours).
#
#   bench/par_map/run.sh [baseline|v3|native|threshold|filter|width|aggregate|chunks|source-consumer]
#   (default: native)
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:-native}"
case "$mode" in
  native) align_tgt="native"; rust_tgt="native" ;;
  threshold|filter|width|aggregate|chunks|source-consumer) align_tgt="native"; rust_tgt="native" ;;
  v3) case "$(uname -m)" in x86_64|amd64) align_tgt="x86-64-v3"; rust_tgt="x86-64-v3" ;; *) echo "v3 is x86_64-only" >&2; exit 1 ;; esac ;;
  baseline)
    align_tgt="baseline"
    case "$(uname -m)" in
      x86_64|amd64) rust_tgt="x86-64-v2" ;;
      *) rust_tgt="generic" ;;
    esac
    ;;
  *) echo "usage: run.sh [baseline|v3|native|threshold|filter|width|aggregate|chunks|source-consumer]" >&2; exit 2 ;;
esac

( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="../../target/release/alignc"
RT_DIR="$(cd ../.. && pwd)/target/release"

if [ "$mode" = source-consumer ]; then
  REPO="$(cd ../.. && pwd)"
  BASELINE_DIR="/home/hiro/prj/align-v1-20260907/o0-baseline"
  BASELINE_ALIGNC="$BASELINE_DIR/alignc"
  BASELINE_HASH="926a1f29f5d837c587cef764edfc3632c00e0a79243523b918a43386bf1a457a"
  RUNTIME_HASH="44c00aa01a65caaec203e3f33d55c0f57537e8a1be08c094f84734ca47bb7ed0"
  [ -x "$BASELINE_ALIGNC" ] || { echo "missing preserved O0 baseline compiler" >&2; exit 1; }
  [ -f "$BASELINE_DIR/libalign_runtime.a" ] || { echo "missing preserved O0 baseline runtime" >&2; exit 1; }
  [ "$(sha256sum "$BASELINE_ALIGNC" | cut -d ' ' -f 1)" = "$BASELINE_HASH" ] || { echo "O0 baseline compiler hash mismatch" >&2; exit 1; }
  [ "$(sha256sum "$BASELINE_DIR/libalign_runtime.a" | cut -d ' ' -f 1)" = "$RUNTIME_HASH" ] || { echo "O0 baseline runtime hash mismatch" >&2; exit 1; }

  SOURCE_CONSUMER_TMP="$(mktemp -d "$PWD/.source-consumer.XXXXXX")"
  case "$SOURCE_CONSUMER_TMP" in "$PWD"/.source-consumer.*) ;; *) echo "invalid source-consumer temp directory" >&2; exit 1 ;; esac
  BASELINE_OBJ="$SOURCE_CONSUMER_TMP/base.o"
  CANDIDATE_OBJ="$SOURCE_CONSUMER_TMP/candidate.o"
  STANDARD_OBJ="$SOURCE_CONSUMER_TMP/standard.o"
  CANDIDATE_SRC="$SOURCE_CONSUMER_TMP/candidate.align"
  BASELINE_PREFIX="base"
  CANDIDATE_PREFIX="cand"
  trap 'rm -f "$BASELINE_OBJ" "$CANDIDATE_OBJ" "$STANDARD_OBJ" "$CANDIDATE_SRC"; rmdir "$SOURCE_CONSUMER_TMP"' EXIT
  [ "${#BASELINE_PREFIX}" -eq "${#CANDIDATE_PREFIX}" ] || { echo "source-consumer prefixes must have equal length" >&2; exit 1; }
  sed "s/${BASELINE_PREFIX}_/${CANDIDATE_PREFIX}_/g" source_consumer.align > "$CANDIDATE_SRC"
  "$BASELINE_ALIGNC" emit-obj source_consumer.align "$BASELINE_OBJ" --target-cpu native \
    --export base_materialize --export base_reduce
  "$ALIGNC" emit-obj "$CANDIDATE_SRC" "$CANDIDATE_OBJ" --target-cpu native \
    --export cand_materialize --export cand_reduce
  "$ALIGNC" emit-obj kernel.align "$STANDARD_OBJ" --target-cpu native \
    --export pmap --export smap --export pfilter

  baseline_size="$(wc -c < "$BASELINE_OBJ")"
  candidate_size="$(wc -c < "$CANDIDATE_OBJ")"
  allowed_growth="$((baseline_size / 20))"
  if [ "$allowed_growth" -lt 65536 ]; then allowed_growth=65536; fi
  echo "O0 object bytes: baseline=$baseline_size candidate=$candidate_size allowed_growth=$allowed_growth"
  if [ "$candidate_size" -gt "$((baseline_size + allowed_growth))" ]; then
    echo "O0 object-size guard failed" >&2
    exit 1
  fi

  export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=$rust_tgt"
  # Timing must use the production runtime. Even an inactive alloc-count probe adds one-sided
  # atomic/mutex work to the baseline's deliberately retained chunk-header allocation.
  ( cd "$REPO" && scripts/cargo.sh build -q --release -p align_runtime )
  [ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing production runtime dynamic library in $RT_DIR" >&2; exit 1; }
  ALIGN_KERNEL_BASELINE="$BASELINE_OBJ" ALIGN_KERNEL_CANDIDATE="$CANDIDATE_OBJ" ALIGN_KERNEL_OBJ="$STANDARD_OBJ" \
    ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release --features source-consumer -- source-consumer timing

  # Resource accounting runs in its own binary after replacing the dynamic runtime with the
  # instrumented build. The feature split keeps counter symbols out of the production timing link.
  ( cd "$REPO" && scripts/cargo.sh build -q --release -p align_runtime --features alloc-count )
  [ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing alloc-count runtime dynamic library in $RT_DIR" >&2; exit 1; }
  ALIGN_KERNEL_BASELINE="$BASELINE_OBJ" ALIGN_KERNEL_CANDIDATE="$CANDIDATE_OBJ" ALIGN_KERNEL_OBJ="$STANDARD_OBJ" \
    ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release --features source-consumer-resource -- source-consumer resource
  exit 0
fi

KOBJ="$PWD/kernel.o"
trap 'rm -f "$KOBJ"' EXIT
"$ALIGNC" emit-obj kernel.align "$KOBJ" --target-cpu "$align_tgt" \
  --export pmap_cheap --export smap_cheap --export pmap --export smap --export pfilter \
  --export pwidth_i8 --export swidth_i8 --export pwidth_i32 --export swidth_i32 \
  --export pwidth_i64 --export swidth_i64 --export pwidth_i8_to_i64 --export swidth_i8_to_i64 \
  --export pwidth_i64_to_i8 --export swidth_i64_to_i8 \
  --export pwidth_materialize_i8_to_i64 --export swidth_materialize_i8_to_i64 \
  --export pwidth_materialize_i64_to_i8 --export swidth_materialize_i64_to_i8

echo "target: $mode (Align=$align_tgt, Rust=$rust_tgt)"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=$rust_tgt"
if [ "$mode" = threshold ] || [ "$mode" = width ] || [ "$mode" = aggregate ] || [ "$mode" = chunks ]; then
  ( cd ../.. && cargo build -q --release -p align_runtime --features par-map-probe )
  [ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic lib in $RT_DIR" >&2; exit 1; }
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release --features probe -- "$mode"
else
  ( cd ../.. && cargo build -q --release -p align_runtime )
  [ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic lib in $RT_DIR" >&2; exit 1; }
  if [ "$mode" = filter ]; then
    ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release -- filter
  else
    ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" cargo run -q --release
  fi
fi
