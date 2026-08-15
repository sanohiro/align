#!/usr/bin/env bash
# Shared work-directory and child cleanup boundary for the JSON benchmark inputs.

benchmark_input_work_root=""
benchmark_input_private_dir=""
benchmark_input_child_pid=""

benchmark_input_fail() {
  echo "benchmark input: $*" >&2
  return 1
}

benchmark_input_entries() {
  local directory="$1"
  local -a entries=()
  shopt -s nullglob dotglob
  entries=("$directory"/*)
  shopt -u nullglob dotglob
  ((${#entries[@]} != 0))
}

benchmark_input_cleanup() {
  local status=$?
  local cleanup_status=0
  local -a entries=()

  trap - EXIT HUP INT TERM
  if [[ -n "$benchmark_input_child_pid" ]]; then
    kill -TERM "$benchmark_input_child_pid" 2>/dev/null || true
    wait "$benchmark_input_child_pid" 2>/dev/null || true
    benchmark_input_child_pid=""
  fi
  if [[ -n "$benchmark_input_private_dir" && -d "$benchmark_input_private_dir" ]]; then
    rm -rf -- "$benchmark_input_private_dir" || cleanup_status=1
  fi
  if [[ -n "$benchmark_input_work_root" && -d "$benchmark_input_work_root" ]]; then
    shopt -s nullglob dotglob
    entries=("$benchmark_input_work_root"/*)
    shopt -u nullglob dotglob
    if ((${#entries[@]} != 0)); then
      echo "benchmark input: work directory contains foreign residue after cleanup" >&2
      cleanup_status=1
    fi
  fi
  if ((cleanup_status != 0 && status == 0)); then
    status=1
  fi
  exit "$status"
}

benchmark_input_signal() {
  local status="$1"
  if [[ -n "$benchmark_input_child_pid" ]]; then
    kill -TERM "$benchmark_input_child_pid" 2>/dev/null || true
  fi
  exit "$status"
}

benchmark_input_begin() {
  local repository_root="$1"
  local benchmark_root="$2"
  local requested="${ALIGN_BENCH_WORK_DIR:-}"
  local physical_repository
  local physical_work

  [[ -f "$benchmark_root/Cargo.lock" && ! -L "$benchmark_root/Cargo.lock" ]] ||
    benchmark_input_fail "the detached Cargo.lock must exist and must not be a symbolic link"
  [[ -n "$requested" ]] || benchmark_input_fail "ALIGN_BENCH_WORK_DIR is required"
  [[ "$requested" == /* ]] || benchmark_input_fail "ALIGN_BENCH_WORK_DIR must be absolute"
  if [[ "$requested" != / ]]; then
    requested="${requested%/}"
  fi
  [[ -d "$requested" ]] || benchmark_input_fail "ALIGN_BENCH_WORK_DIR must be an existing directory"
  [[ ! -L "$requested" ]] || benchmark_input_fail "ALIGN_BENCH_WORK_DIR must not be a symbolic link"

  physical_repository="$(cd "$repository_root" && pwd -P)"
  physical_work="$(cd "$requested" && pwd -P)"
  [[ "$physical_work" != / ]] || benchmark_input_fail "ALIGN_BENCH_WORK_DIR must not be the filesystem root"
  [[ "$physical_work" != "$physical_repository" && "$physical_work" != "$physical_repository/"* ]] ||
    benchmark_input_fail "ALIGN_BENCH_WORK_DIR must be outside the repository"
  ! benchmark_input_entries "$physical_work" || benchmark_input_fail "ALIGN_BENCH_WORK_DIR must be empty"

  umask 077
  benchmark_input_work_root="$physical_work"
  benchmark_input_private_dir="$(mktemp -d "$physical_work/align-json-bench.XXXXXXXX")"
  trap benchmark_input_cleanup EXIT
  trap 'benchmark_input_signal 129' HUP
  trap 'benchmark_input_signal 130' INT
  trap 'benchmark_input_signal 143' TERM
  export ALIGN_BENCH_WORK_DIR="$physical_work"
  export ALIGN_BENCH_PRIVATE_DIR="$benchmark_input_private_dir"
  export ALIGN_BENCH_ROOT_TARGET_DIR="$benchmark_input_private_dir/root-target"
  export ALIGN_BENCH_DETACHED_TARGET_DIR="$benchmark_input_private_dir/detached-target"
  export TMPDIR="$benchmark_input_private_dir/tmp"
  mkdir -p "$ALIGN_BENCH_ROOT_TARGET_DIR" "$ALIGN_BENCH_DETACHED_TARGET_DIR" "$TMPDIR"
}

benchmark_input_run() {
  local status

  "$@" &
  benchmark_input_child_pid=$!
  set +e
  wait "$benchmark_input_child_pid"
  status=$?
  set -e
  benchmark_input_child_pid=""
  return "$status"
}
