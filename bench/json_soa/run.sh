#!/usr/bin/env bash
# JSON → SoA analytics benchmark: Align `json.decode → soa<Row> → where(.active).pay.sum()` vs
# idiomatic Rust `serde_json → Vec<Row> → filter/sum`. Unlike the flat `bench/`, the kernel pulls in
# the Align runtime (JSON parser / arena), so the harness links `libalign_runtime.a` too.
#
#   ALIGN_BENCH_WORK_DIR=/absolute/empty/outside/repo bench/json_soa/run.sh [baseline|v3|native]
#     (default: native — both sides at the host's best CPU)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd -P)"
BENCH_WORK_DIR=""
BENCH_PRIVATE_DIR=""
BENCH_ROOT_TARGET_DIR=""
BENCH_DETACHED_TARGET_DIR=""
BENCH_TMP_DIR=""
BENCH_ALIGNC_CACHE_DIR=""
BENCH_CHILD_PID=""
BENCH_WORK_DIR_VALIDATED=0
BENCH_CLEANED=0

directory_has_entries() {
  local entry
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    if [[ -e "$entry" || -L "$entry" ]]; then
      return 0
    fi
  done
  return 1
}

stop_child_group() {
  local pid="${BENCH_CHILD_PID:-}"
  local attempt

  [[ -n "$pid" ]] || return 0
  kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  for ((attempt = 0; attempt < 20; attempt++)); do
    kill -0 "-$pid" 2>/dev/null || break
    sleep 0.1
  done
  if kill -0 "-$pid" 2>/dev/null; then
    kill -KILL "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
  BENCH_CHILD_PID=""
}

run_child_group() {
  local monitor_was_set=0
  local pid
  local status

  case "$-" in
    *m*) monitor_was_set=1 ;;
  esac
  set -m
  "$@" &
  BENCH_CHILD_PID=$!
  pid="$BENCH_CHILD_PID"
  if [[ "$monitor_was_set" -eq 0 ]]; then
    set +m
  fi
  set +e
  wait "$pid"
  status=$?
  set -e
  if kill -0 "-$pid" 2>/dev/null; then
    stop_child_group
    echo "error: benchmark child process group survived its owning command" >&2
    return 1
  fi
  BENCH_CHILD_PID=""
  return "$status"
}

cleanup() {
  local status=$?
  local cleanup_failed=0
  if [[ "$BENCH_CLEANED" -eq 1 ]]; then
    return "$status"
  fi
  BENCH_CLEANED=1
  trap - EXIT HUP INT TERM

  stop_child_group

  if [[ -n "$BENCH_PRIVATE_DIR" && ( -e "$BENCH_PRIVATE_DIR" || -L "$BENCH_PRIVATE_DIR" ) ]]; then
    if ! rm -rf "$BENCH_PRIVATE_DIR" || [[ -e "$BENCH_PRIVATE_DIR" || -L "$BENCH_PRIVATE_DIR" ]]; then
      echo "error: failed to remove the benchmark work child" >&2
      cleanup_failed=1
    fi
  fi

  if [[ "$BENCH_WORK_DIR_VALIDATED" -eq 1 ]]; then
    if [[ ! -d "$BENCH_WORK_DIR" || -L "$BENCH_WORK_DIR" ]]; then
      echo "error: benchmark work directory disappeared during cleanup" >&2
      cleanup_failed=1
    elif directory_has_entries "$BENCH_WORK_DIR"; then
      echo "error: foreign residue remains in the benchmark work directory" >&2
      cleanup_failed=1
    fi
  fi

  if [[ "$cleanup_failed" -ne 0 ]]; then
    status=1
  fi
  exit "$status"
}

on_signal() {
  stop_child_group
  exit "$1"
}

trap cleanup EXIT
trap 'on_signal 129' HUP
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

umask 077

if [[ -z "${ALIGN_BENCH_WORK_DIR:-}" ]]; then
  echo "error: ALIGN_BENCH_WORK_DIR is required" >&2
  exit 2
fi

requested_work_dir="$ALIGN_BENCH_WORK_DIR"
while [[ "$requested_work_dir" != "/" ]]; do
  case "$requested_work_dir" in
    */) requested_work_dir="${requested_work_dir%/}" ;;
    */.) requested_work_dir="${requested_work_dir%/.}" ;;
    *) break ;;
  esac
done
case "$requested_work_dir" in
  /*) ;;
  *) echo "error: ALIGN_BENCH_WORK_DIR must be absolute" >&2; exit 2 ;;
esac
if [[ -L "$requested_work_dir" ]]; then
  echo "error: ALIGN_BENCH_WORK_DIR must not have a symbolic-link final component" >&2
  exit 2
fi
if [[ ! -d "$requested_work_dir" ]]; then
  echo "error: ALIGN_BENCH_WORK_DIR must be an existing directory" >&2
  exit 2
fi
if ! BENCH_WORK_DIR="$(cd "$requested_work_dir" && pwd -P)"; then
  echo "error: cannot resolve ALIGN_BENCH_WORK_DIR" >&2
  exit 2
fi
if [[ "$BENCH_WORK_DIR" == "/" ]]; then
  echo "error: ALIGN_BENCH_WORK_DIR must not be the filesystem root" >&2
  exit 2
fi
case "$BENCH_WORK_DIR" in
  "$REPO_ROOT"|"$REPO_ROOT"/*)
    echo "error: ALIGN_BENCH_WORK_DIR must be outside the repository" >&2
    exit 2
    ;;
esac
if directory_has_entries "$BENCH_WORK_DIR"; then
  echo "error: ALIGN_BENCH_WORK_DIR must initially be empty" >&2
  exit 2
fi
BENCH_WORK_DIR_VALIDATED=1
if [[ ! -f "$SCRIPT_DIR/Cargo.lock" || -L "$SCRIPT_DIR/Cargo.lock" ]]; then
  echo "error: detached Cargo.lock must exist and must not be a symbolic link" >&2
  exit 2
fi

if ! BENCH_PRIVATE_DIR="$(mktemp -d "$BENCH_WORK_DIR/.align-bench.XXXXXX")"; then
  echo "error: cannot create the private benchmark work child" >&2
  exit 1
fi
if ! chmod 700 "$BENCH_PRIVATE_DIR"; then
  echo "error: cannot secure the private benchmark work child" >&2
  exit 1
fi
BENCH_ROOT_TARGET_DIR="$BENCH_PRIVATE_DIR/root-target"
BENCH_DETACHED_TARGET_DIR="$BENCH_PRIVATE_DIR/detached-target"
BENCH_TMP_DIR="$BENCH_PRIVATE_DIR/tmp"
BENCH_ALIGNC_CACHE_DIR="$BENCH_PRIVATE_DIR/alignc-cache"
mkdir -p "$BENCH_ROOT_TARGET_DIR" "$BENCH_DETACHED_TARGET_DIR" "$BENCH_TMP_DIR" "$BENCH_ALIGNC_CACHE_DIR"

cd "$SCRIPT_DIR"

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

build_root() {
  cd "$REPO_ROOT"
  CARGO_TARGET_DIR="$BENCH_ROOT_TARGET_DIR" TMPDIR="$BENCH_TMP_DIR" \
    cargo build -q --release --locked --offline --bin alignc
  CARGO_TARGET_DIR="$BENCH_ROOT_TARGET_DIR" TMPDIR="$BENCH_TMP_DIR" \
    cargo build -q --release --locked --offline -p align_runtime
}

emit_kernel() {
  cd "$BENCH_PRIVATE_DIR"
  ALIGNC_CACHE="$BENCH_ALIGNC_CACHE_DIR" TMPDIR="$BENCH_TMP_DIR" \
    "$ALIGNC" emit-obj "$SCRIPT_DIR/kernel.align" "$KOBJ" --target-cpu "$align_tgt" \
    --export agg --export agg_len --export agg_aos --export agg_aos_len --export agg_proj --export agg_proj_len
}

# Build alignc + the runtime staticlib (release). Two invocations: the staticlib crate-type of
# `align_runtime` is what produces `libalign_runtime.a`.
run_child_group build_root
ALIGNC="$BENCH_ROOT_TARGET_DIR/release/alignc"
RT_DIR="$BENCH_ROOT_TARGET_DIR/release"
[ -f "$RT_DIR/libalign_runtime.so" ] || [ -f "$RT_DIR/libalign_runtime.dylib" ] || { echo "missing libalign_runtime dynamic library (.so/.dylib) in $RT_DIR" >&2; exit 1; }

KOBJ="$BENCH_PRIVATE_DIR/kernel.o"
run_child_group emit_kernel

echo "target: $mode"
run_child_group env CARGO_TARGET_DIR="$BENCH_DETACHED_TARGET_DIR" TMPDIR="$BENCH_TMP_DIR" \
  ALIGN_KERNEL_OBJ="$KOBJ" ALIGN_RUNTIME_DIR="$RT_DIR" \
  cargo run -q --release --locked --offline
