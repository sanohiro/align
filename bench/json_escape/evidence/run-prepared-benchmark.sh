#!/usr/bin/env bash
# Shared prepare/direct-exec boundary for the protected JSON evidence benchmarks.
# The benchmark wrapper sets BENCH_NAME, BENCH_BINARY, and BENCH_EXPORTS before sourcing this file.
set -euo pipefail

: "${BENCH_NAME:?}"
: "${BENCH_BINARY:?}"
: "${BENCH_EXPORTS:?}"

REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd -P)"
BENCH_WORK_DIR=""
BENCH_PRIVATE_DIR=""
BENCH_BOUND_PRIVATE_DIR=""
BENCH_PREPARED_DIR=""
BENCH_ROOT_TARGET_DIR=""
BENCH_DETACHED_TARGET_DIR=""
BENCH_TMP_DIR=""
BENCH_ALIGNC_CACHE_DIR=""
BENCH_ARTIFACT_DIR=""
BENCH_BOUND_ARTIFACT_DIR=""
BENCH_CHILD_PID=""
BENCH_PENDING_SIGNAL=""
BENCH_WORK_DIR_VALIDATED=0
BENCH_KEEP_PRIVATE=0
BENCH_EXPECT_PREPARED=0
BENCH_PRIVATE_IDENTITY=""
BENCH_PRIVATE_FD_OPEN=0
BENCH_ARTIFACT_FD_OPEN=0
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

directory_identity() {
  python3 - "$1" <<'PY'
import os
import stat
import sys

value = os.lstat(sys.argv[1])
if not stat.S_ISDIR(value.st_mode):
    raise SystemExit(1)
print(f"{value.st_dev}:{value.st_ino}")
PY
}

work_dir_contains_only_prepared() {
  local entry
  [[ -d "$BENCH_PREPARED_DIR" && ! -L "$BENCH_PREPARED_DIR" ]] || return 1
  for entry in "$BENCH_WORK_DIR"/* "$BENCH_WORK_DIR"/.[!.]* "$BENCH_WORK_DIR"/..?*; do
    if [[ ( -e "$entry" || -L "$entry" ) && "$entry" != "$BENCH_PREPARED_DIR" ]]; then
      return 1
    fi
  done
}

work_dir_contains_only_private() {
  local entry
  [[ -d "$BENCH_PRIVATE_DIR" && ! -L "$BENCH_PRIVATE_DIR" ]] || return 1
  for entry in "$BENCH_WORK_DIR"/* "$BENCH_WORK_DIR"/.[!.]* "$BENCH_WORK_DIR"/..?*; do
    if [[ ( -e "$entry" || -L "$entry" ) && "$entry" != "$BENCH_PRIVATE_DIR" ]]; then
      return 1
    fi
  done
}

work_dir_contains_only_cleared_private() {
  work_dir_contains_only_private || return 1
  PYTHONDONTWRITEBYTECODE=1 python3 \
    "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" verify-cleared
}

private_identity_matches() {
  local current_identity
  current_identity="$(directory_identity "$BENCH_PRIVATE_DIR" 2>/dev/null || true)"
  [[ -n "$BENCH_PRIVATE_IDENTITY" && "$current_identity" == "$BENCH_PRIVATE_IDENTITY" ]]
}

bind_private_directory() {
  if ! exec 9<"$BENCH_PRIVATE_DIR"; then
    return 1
  fi
  BENCH_PRIVATE_FD_OPEN=1
  case "$(uname -s)" in
    Linux) BENCH_BOUND_PRIVATE_DIR="/proc/$$/fd/9" ;;
    Darwin) BENCH_BOUND_PRIVATE_DIR="$BENCH_PRIVATE_DIR" ;;
    *) return 1 ;;
  esac
}

clear_bound_private_directory() {
  PYTHONDONTWRITEBYTECODE=1 python3 \
    "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" clear
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

defer_signal() {
  if [[ -z "$BENCH_PENDING_SIGNAL" ]]; then
    BENCH_PENDING_SIGNAL="$1"
  fi
}

install_signal_traps() {
  trap 'on_signal 129' HUP
  trap 'on_signal 130' INT
  trap 'on_signal 143' TERM
}

run_child_group() {
  local monitor_was_set=0
  local pid
  local status

  case "$-" in
    *m*) monitor_was_set=1 ;;
  esac
  set -m
  BENCH_PENDING_SIGNAL=""
  trap 'defer_signal 129' HUP
  trap 'defer_signal 130' INT
  trap 'defer_signal 143' TERM
  "$@" &
  BENCH_CHILD_PID=$!
  pid="$BENCH_CHILD_PID"
  install_signal_traps
  if [[ -n "$BENCH_PENDING_SIGNAL" ]]; then
    on_signal "$BENCH_PENDING_SIGNAL"
  fi
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

  if [[ "$BENCH_KEEP_PRIVATE" -eq 0 && "$BENCH_PRIVATE_FD_OPEN" -eq 1 ]]; then
    if ! clear_bound_private_directory; then
      echo "error: failed to clear the bound benchmark work child" >&2
      cleanup_failed=1
    fi
    if ! private_identity_matches; then
      echo "error: benchmark work child was replaced; cleared retained tree only" >&2
      cleanup_failed=1
    elif ! work_dir_contains_only_cleared_private; then
      echo "error: cleared benchmark work child is not the sole directory-only residue" >&2
      cleanup_failed=1
    fi
  fi

  if [[ "$BENCH_WORK_DIR_VALIDATED" -eq 1 ]]; then
    if [[ ! -d "$BENCH_WORK_DIR" || -L "$BENCH_WORK_DIR" ]]; then
      echo "error: benchmark work directory disappeared during cleanup" >&2
      cleanup_failed=1
    elif [[ "$BENCH_EXPECT_PREPARED" -eq 1 ]]; then
      if ! work_dir_contains_only_prepared || ! private_identity_matches; then
        echo "error: benchmark work directory does not contain exactly the prepared artifact" >&2
        cleanup_failed=1
      fi
    elif [[ "$BENCH_PRIVATE_FD_OPEN" -eq 1 ]] && work_dir_contains_only_cleared_private; then
      :
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
install_signal_traps
umask 077

if [[ "$#" -eq 2 && "$1" == prepare && "$2" == native ]]; then
  BENCH_ACTION=prepare
elif [[ "$#" -eq 1 && "$1" == native ]]; then
  BENCH_ACTION=run
else
  echo "usage: run.sh prepare native | run.sh native" >&2
  exit 2
fi

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
BENCH_WORK_DIR_VALIDATED=1
BENCH_PREPARED_DIR="$BENCH_WORK_DIR/prepared"

if [[ ! -f "$SCRIPT_DIR/Cargo.lock" || -L "$SCRIPT_DIR/Cargo.lock" ]]; then
  echo "error: detached Cargo.lock must exist and must not be a symbolic link" >&2
  exit 2
fi

verify_prepared() {
  PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/manifest.py" \
    verify --root "$BENCH_PREPARED_DIR" --manifest artifact-manifest.json >/dev/null
}

if [[ "$BENCH_ACTION" == run ]]; then
  BENCH_PRIVATE_DIR="$BENCH_PREPARED_DIR"
  BENCH_KEEP_PRIVATE=1
  BENCH_EXPECT_PREPARED=1
  if ! work_dir_contains_only_prepared; then
    echo "error: benchmark work directory does not contain exactly one prepared artifact" >&2
    exit 2
  fi
  if ! BENCH_PRIVATE_IDENTITY="$(directory_identity "$BENCH_PRIVATE_DIR")"; then
    echo "error: cannot bind the prepared benchmark directory identity" >&2
    exit 2
  fi
  if ! bind_private_directory; then
    echo "error: cannot retain the prepared benchmark directory" >&2
    exit 2
  fi
  manifest_sha256="${ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256:-}"
  if [[ "${#manifest_sha256}" -ne 64 || "$manifest_sha256" == *[!0-9a-f]* ]]; then
    echo "error: ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256 must be lowercase SHA-256" >&2
    exit 2
  fi
  printf 'target: native\n'
  exec env PYTHONDONTWRITEBYTECODE=1 python3 \
    "$REPO_ROOT/scripts/benchmark_evidence/bound_exec.py" \
    --root "$BENCH_PREPARED_DIR" --root-fd 9 --root-identity "$BENCH_PRIVATE_IDENTITY" \
    --manifest-sha256 "$ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256" \
    --executable "$BENCH_BINARY"
fi

if directory_has_entries "$BENCH_WORK_DIR"; then
  echo "error: ALIGN_BENCH_WORK_DIR must initially be empty for prepare" >&2
  exit 2
fi
if ! mkdir "$BENCH_PREPARED_DIR"; then
  echo "error: cannot create the prepared benchmark directory" >&2
  exit 1
fi
BENCH_PRIVATE_DIR="$BENCH_PREPARED_DIR"
if ! BENCH_PRIVATE_IDENTITY="$(directory_identity "$BENCH_PRIVATE_DIR")"; then
  echo "error: cannot bind the prepared benchmark directory identity" >&2
  exit 1
fi
if ! chmod 700 "$BENCH_PRIVATE_DIR"; then
  echo "error: cannot secure the private benchmark work child" >&2
  exit 1
fi
if ! bind_private_directory; then
  echo "error: cannot retain the prepared benchmark directory" >&2
  exit 1
fi

BENCH_ROOT_TARGET_DIR="$BENCH_BOUND_PRIVATE_DIR/root-target"
BENCH_DETACHED_TARGET_DIR="$BENCH_BOUND_PRIVATE_DIR/detached-target"
BENCH_TMP_DIR="$BENCH_BOUND_PRIVATE_DIR/tmp"
BENCH_ALIGNC_CACHE_DIR="$BENCH_BOUND_PRIVATE_DIR/alignc-cache"
BENCH_ARTIFACT_DIR="$BENCH_BOUND_PRIVATE_DIR/artifacts"
mkdir -p "$BENCH_ROOT_TARGET_DIR" "$BENCH_DETACHED_TARGET_DIR" "$BENCH_TMP_DIR" \
  "$BENCH_ALIGNC_CACHE_DIR" "$BENCH_ARTIFACT_DIR"
if ! exec 8<"$BENCH_PRIVATE_DIR/artifacts"; then
  echo "error: cannot retain the prepared artifacts directory" >&2
  exit 1
fi
BENCH_ARTIFACT_FD_OPEN=1
case "$(uname -s)" in
  Linux) BENCH_BOUND_ARTIFACT_DIR="/proc/$$/fd/8" ;;
  Darwin) BENCH_BOUND_ARTIFACT_DIR="$BENCH_PRIVATE_DIR/artifacts" ;;
  *) echo "error: unsupported native benchmark host" >&2; exit 1 ;;
esac
BENCH_ARTIFACT_DIR="$BENCH_BOUND_ARTIFACT_DIR"

build_alignc() {
  cd "$REPO_ROOT"
  CARGO_TARGET_DIR="$BENCH_ROOT_TARGET_DIR" TMPDIR="$BENCH_TMP_DIR" \
    "$REPO_ROOT/scripts/cargo.sh" build -q --release --locked --offline --bin alignc
}

build_runtime() {
  cd "$REPO_ROOT"
  CARGO_TARGET_DIR="$BENCH_ROOT_TARGET_DIR" TMPDIR="$BENCH_TMP_DIR" \
    "$REPO_ROOT/scripts/cargo.sh" build -q --release --locked --offline -p align_runtime
}

emit_kernel() {
  cd "$BENCH_BOUND_PRIVATE_DIR"
  # BENCH_EXPORTS is a fixed, wrapper-owned word list.
  # shellcheck disable=SC2086
  ALIGNC_CACHE="$BENCH_ALIGNC_CACHE_DIR" TMPDIR="$BENCH_TMP_DIR" \
    "$BENCH_ROOT_TARGET_DIR/release/alignc" emit-obj "$SCRIPT_DIR/kernel.align" \
    kernel.o --target-cpu native $BENCH_EXPORTS
}

build_harness() {
  cd "$SCRIPT_DIR"
  CARGO_TARGET_DIR="$BENCH_DETACHED_TARGET_DIR" TMPDIR="$BENCH_TMP_DIR" \
    ALIGN_KERNEL_OBJ="$BENCH_BOUND_PRIVATE_DIR/kernel.o" \
    ALIGN_RUNTIME_DIR="$BENCH_ROOT_TARGET_DIR/release" \
    "$REPO_ROOT/scripts/cargo.sh" build -q --release --locked --offline
}

run_child_group build_alignc
run_child_group build_runtime
run_child_group emit_kernel
run_child_group build_harness

# No final artifact exists while candidate-controlled build/compiler work can still run. Only the
# descriptor-relative helper publishes the complete set after every child group has exited.
PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
  copy --source root-target/release/alignc --destination alignc --mode 0755
runtime_name="$(PYTHONDONTWRITEBYTECODE=1 python3 \
  "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" copy-runtime)"
if [[ "$runtime_name" == *.dylib ]]; then
  install_name_tool -id "@rpath/$runtime_name" "$BENCH_ARTIFACT_DIR/$runtime_name"
  PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
    chmod-artifact --name "$runtime_name" --mode 0755
fi
PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
  copy --source kernel.o --destination kernel.o --mode 0644
PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
  copy --source "detached-target/release/$BENCH_BINARY" \
  --destination "$BENCH_BINARY" --mode 0755
PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
  write-configuration --benchmark "$BENCH_NAME"
PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
  clear-build-trees root-target detached-target tmp alignc-cache kernel.o
PYTHONDONTWRITEBYTECODE=1 python3 "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" \
  verify-artifacts
manifest_sha256="$(PYTHONDONTWRITEBYTECODE=1 python3 \
  "$REPO_ROOT/scripts/benchmark_evidence/prepared_tree.py" write-manifest)"

if ! work_dir_contains_only_private; then
  echo "error: foreign residue appeared in the benchmark work directory" >&2
  exit 1
fi
if ! private_identity_matches; then
  echo "error: prepared benchmark directory identity changed before publication" >&2
  exit 1
fi

BENCH_KEEP_PRIVATE=1
BENCH_EXPECT_PREPARED=1
printf '%s\n' "$BENCH_PREPARED_DIR/artifact-manifest.json"
printf 'artifact-manifest-sha256: %s\n' "$manifest_sha256"
