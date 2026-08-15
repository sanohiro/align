#!/usr/bin/env bash
# Deterministic owner for the benchmark-input work-directory contract.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-benchmark-input.XXXXXX")"
FAKE_BIN="$TEST_ROOT/bin"
FAKE_LOG="$TEST_ROOT/cargo.log"
ORIGINAL_PATH="$PATH"
SIGNAL_SCRIPT_PID=""
SIGNAL_DESCENDANT_PID=""

cleanup_test_root() {
  if [[ -n "$SIGNAL_SCRIPT_PID" ]]; then
    kill -TERM "$SIGNAL_SCRIPT_PID" 2>/dev/null || true
    wait "$SIGNAL_SCRIPT_PID" 2>/dev/null || true
  fi
  if [[ -n "$SIGNAL_DESCENDANT_PID" ]]; then
    kill -KILL "$SIGNAL_DESCENDANT_PID" 2>/dev/null || true
  fi
  rm -rf "$TEST_ROOT"
}
trap cleanup_test_root EXIT HUP INT TERM

fail() {
  echo "benchmark-input test failed: $*" >&2
  exit 1
}

directory_has_entries() {
  local entry
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    if [[ -e "$entry" || -L "$entry" ]]; then
      return 0
    fi
  done
  return 1
}

assert_empty() {
  if directory_has_entries "$1"; then
    fail "expected empty directory: $1"
  fi
}

assert_rejected() {
  local label="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    fail "$label was accepted"
  fi
}

make_fake_tools() {
  mkdir -p "$FAKE_BIN"
  cat > "$FAKE_BIN/cargo" <<'FAKE_CARGO'
#!/usr/bin/env bash
set -euo pipefail

: "${FAKE_LOG:?}"
: "${FAKE_WORK_DIR:?}"
: "${CARGO_TARGET_DIR:?}"
: "${TMPDIR:?}"
[[ -f Cargo.lock ]] || { echo "detached or root Cargo.lock is missing" >&2; exit 87; }
case "$CARGO_TARGET_DIR" in
  "$FAKE_WORK_DIR"/.align-bench.*/*) ;;
  *) echo "cargo target escaped the private child" >&2; exit 81 ;;
esac
case "$TMPDIR" in
  "$FAKE_WORK_DIR"/.align-bench.*/*) ;;
  *) echo "cargo temporary directory escaped the private child" >&2; exit 82 ;;
esac
printf 'cargo %s\n' "$*" >> "$FAKE_LOG"

has_arg() {
  local wanted="$1"
  shift
  local arg
  for arg in "$@"; do
    if [[ "$arg" == "$wanted" ]]; then
      return 0
    fi
  done
  return 1
}

if has_arg --bin "$@" && has_arg alignc "$@"; then
  if [[ "${FAKE_FAIL_MODE:-}" == root ]]; then
    exit 70
  fi
  if [[ "${FAKE_BLOCK_MODE:-}" == root ]]; then
    : "${FAKE_BLOCK_MARKER:?}"
    : "${FAKE_DESCENDANT_MARKER:?}"
    : > "$FAKE_BLOCK_MARKER"
    trap '' TERM INT
    (
      trap '' TERM INT
      while :; do sleep 1; done
    ) &
    printf '%s\n' "$!" > "$FAKE_DESCENDANT_MARKER"
    while :; do sleep 1; done
  fi
  mkdir -p "$CARGO_TARGET_DIR/release"
  cat > "$CARGO_TARGET_DIR/release/alignc" <<'FAKE_ALIGNC'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_WORK_DIR:?}"
: "${ALIGNC_CACHE:?}"
if [[ "${FAKE_FAIL_MODE:-}" == alignc ]]; then
  exit 71
fi
if [[ "${1:-}" != emit-obj || "$#" -lt 3 ]]; then
  exit 72
fi
case "$3" in
  "$FAKE_WORK_DIR"/.align-bench.*/*) ;;
  *) echo "kernel object escaped the private child" >&2; exit 83 ;;
esac
case "$ALIGNC_CACHE" in
  "$FAKE_WORK_DIR"/.align-bench.*/*) ;;
  *) echo "alignc cache escaped the private child" >&2; exit 88 ;;
esac
printf 'fake kernel object\n' > "$3"
FAKE_ALIGNC
  chmod 700 "$CARGO_TARGET_DIR/release/alignc"
  if [[ -n "${FAKE_SLEEP:-}" ]]; then
    sleep "$FAKE_SLEEP"
  fi
elif has_arg -p "$@" && has_arg align_runtime "$@"; then
  if [[ "${FAKE_FAIL_MODE:-}" == root ]]; then
    exit 73
  fi
  mkdir -p "$CARGO_TARGET_DIR/release"
  printf 'fake runtime\n' > "$CARGO_TARGET_DIR/release/libalign_runtime.so"
elif has_arg run "$@"; then
  if [[ "${FAKE_ADD_FOREIGN:-0}" == 1 && ! -e "$FAKE_WORK_DIR/foreign" ]]; then
    printf 'caller-owned residue\n' > "$FAKE_WORK_DIR/foreign"
  fi
  case "${ALIGN_KERNEL_OBJ:-}" in
    "$FAKE_WORK_DIR"/.align-bench.*/*) ;;
    *) echo "detached kernel object escaped the private child" >&2; exit 84 ;;
  esac
  case "${ALIGN_RUNTIME_DIR:-}" in
    "$FAKE_WORK_DIR"/.align-bench.*/*) ;;
    *) echo "runtime directory escaped the private child" >&2; exit 85 ;;
  esac
  if [[ "${FAKE_FAIL_MODE:-}" == detached ]]; then
    exit 74
  fi
  if [[ -n "${FAKE_SLEEP:-}" ]]; then
    sleep "$FAKE_SLEEP"
  fi
else
  echo "unexpected cargo invocation: $*" >&2
  exit 86
fi
FAKE_CARGO
  chmod 700 "$FAKE_BIN/cargo"

  cat > "$FAKE_BIN/rm" <<'FAKE_RM'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${FAKE_RM_MODE:-}" == fail-child && "$*" == *"/.align-bench."* ]]; then
  exit 1
fi
exec /bin/rm "$@"
FAKE_RM
  chmod 700 "$FAKE_BIN/rm"
}

run_benchmark() {
  local bench="$1"
  shift
  PATH="$FAKE_BIN:$ORIGINAL_PATH" \
    FAKE_LOG="$FAKE_LOG" FAKE_WORK_DIR="$FAKE_WORK_DIR" \
    ALIGN_BENCH_WORK_DIR="$FAKE_WORK_DIR" "$@" \
    "$REPO_ROOT/bench/$bench/run.sh" native
}

expect_invalid_workdirs() {
  local empty_dir="$TEST_ROOT/empty"
  local nonempty_dir="$TEST_ROOT/nonempty"
  local target_dir="$TEST_ROOT/symlink-target"
  local symlink_dir="$TEST_ROOT/final-symlink"
  local file_path="$TEST_ROOT/not-a-directory"
  mkdir -p "$empty_dir" "$nonempty_dir" "$target_dir"
  printf 'foreign\n' > "$nonempty_dir/.hidden"
  printf 'not a directory\n' > "$file_path"
  ln -s "$target_dir" "$symlink_dir"

  assert_rejected "missing work directory" env ALIGN_BENCH_WORK_DIR="$TEST_ROOT/missing" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "relative work directory" env ALIGN_BENCH_WORK_DIR=relative \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "non-directory work path" env ALIGN_BENCH_WORK_DIR="$file_path" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "final symlink work directory" env ALIGN_BENCH_WORK_DIR="$symlink_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "final symlink with repeated separators" env ALIGN_BENCH_WORK_DIR="$symlink_dir//" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "final symlink with dot suffix" env ALIGN_BENCH_WORK_DIR="$symlink_dir/." \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "root work directory" env ALIGN_BENCH_WORK_DIR=/ \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "repository work directory" env ALIGN_BENCH_WORK_DIR="$REPO_ROOT" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "in-repository work directory" env ALIGN_BENCH_WORK_DIR="$REPO_ROOT/bench" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "nonempty work directory" env ALIGN_BENCH_WORK_DIR="$nonempty_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_rejected "missing work-dir environment" env -u ALIGN_BENCH_WORK_DIR \
    "$REPO_ROOT/bench/json_decode/run.sh" native
  assert_empty "$empty_dir"
  assert_empty "$target_dir"
  [[ -L "$symlink_dir" ]] || fail "invalid final symlink was removed"
  [[ -e "$nonempty_dir/.hidden" ]] || fail "foreign entry was removed"
}

assert_lock_inputs() {
  local bench
  local lock
  for bench in json_decode json_soa; do
    lock="$REPO_ROOT/bench/$bench/Cargo.lock"
    [[ -f "$lock" && ! -L "$lock" ]] || fail "$bench Cargo.lock is missing or symbolic"
    git -C "$REPO_ROOT" ls-files --error-unmatch "bench/$bench/Cargo.lock" >/dev/null 2>&1 ||
      fail "$bench Cargo.lock is not tracked"
    if git -C "$REPO_ROOT" check-ignore -q "bench/$bench/Cargo.lock"; then
      fail "$bench Cargo.lock is ignored"
    fi
    cargo metadata --locked --offline --format-version 1 \
      --manifest-path "$REPO_ROOT/bench/$bench/Cargo.toml" >/dev/null
  done

  local missing_repo="$TEST_ROOT/missing-lock-repo"
  local missing_work="$TEST_ROOT/missing-lock-work"
  mkdir -p "$missing_repo/bench/json_decode" "$missing_work"
  cp "$REPO_ROOT/bench/json_decode/run.sh" "$missing_repo/bench/json_decode/run.sh"
  if ALIGN_BENCH_WORK_DIR="$missing_work" "$missing_repo/bench/json_decode/run.sh" native \
    >"$TEST_ROOT/missing-lock.out" 2>"$TEST_ROOT/missing-lock.err"; then
    fail "missing detached Cargo.lock was accepted"
  fi
  grep -Fq 'detached Cargo.lock must exist and must not be a symbolic link' \
    "$TEST_ROOT/missing-lock.err" || fail "missing detached Cargo.lock reached another failure"
  assert_empty "$missing_work"

  local stale="$TEST_ROOT/stale-lock"
  mkdir -p "$stale/src"
  sed 's/serde_json = "1"/serde_json = "=1.0.0"/' \
    "$REPO_ROOT/bench/json_decode/Cargo.toml" > "$stale/Cargo.toml"
  cp "$REPO_ROOT/bench/json_decode/Cargo.lock" "$stale/Cargo.lock"
  : > "$stale/src/main.rs"
  if cargo metadata --locked --offline --format-version 1 \
    --manifest-path "$stale/Cargo.toml" >/dev/null 2>&1; then
    fail "stale detached Cargo.lock was accepted"
  fi
}

assert_locked_offline_invocations() {
  local count
  count="$(wc -l < "$FAKE_LOG" | tr -d ' ')"
  [[ "$count" -eq 6 ]] || fail "expected six Cargo invocations, got $count"
  if grep -v -- '--locked --offline' "$FAKE_LOG" >/dev/null; then
    fail "a Cargo invocation was not locked and offline"
  fi
}

benchmark_input_workdir_matrix() {
  assert_lock_inputs
  expect_invalid_workdirs
  make_fake_tools
  [[ -f "$REPO_ROOT/bench/json_decode/Cargo.lock" ]] || fail "json_decode Cargo.lock is missing"
  [[ -f "$REPO_ROOT/bench/json_soa/Cargo.lock" ]] || fail "json_soa Cargo.lock is missing"

  local bench
  for bench in json_decode json_soa; do
    FAKE_WORK_DIR="$TEST_ROOT/work-$bench"
    mkdir -p "$FAKE_WORK_DIR"
    run_benchmark "$bench" env
    assert_empty "$FAKE_WORK_DIR"
    [[ ! -e "$REPO_ROOT/bench/$bench/kernel.o" ]] || fail "$bench wrote kernel.o beside its source"
  done
  assert_locked_offline_invocations

  FAKE_WORK_DIR="$TEST_ROOT/foreign-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_ADD_FOREIGN=1; then
    fail "foreign residue was accepted"
  fi
  [[ -e "$FAKE_WORK_DIR/foreign" ]] || fail "foreign residue was deleted"
  for entry in "$FAKE_WORK_DIR"/.align-bench.*; do
    if [[ -e "$entry" || -L "$entry" ]]; then
      fail "owned private child survived foreign-residue cleanup"
    fi
  done

  FAKE_WORK_DIR="$TEST_ROOT/error-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_soa env FAKE_FAIL_MODE=detached; then
    fail "detached benchmark failure was accepted"
  fi
  assert_empty "$FAKE_WORK_DIR"

  FAKE_WORK_DIR="$TEST_ROOT/cleanup-failure-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_RM_MODE=fail-child; then
    fail "cleanup failure was accepted"
  fi
  directory_has_entries "$FAKE_WORK_DIR" || fail "cleanup failure did not preserve evidence of failure"
  PATH="$ORIGINAL_PATH" rm -rf "$FAKE_WORK_DIR"

  local block_marker
  local descendant_marker
  local ready
  local attempt
  local signal_status
  for bench in json_decode json_soa; do
    FAKE_WORK_DIR="$TEST_ROOT/signal-work-$bench"
    block_marker="$TEST_ROOT/block-marker-$bench"
    descendant_marker="$TEST_ROOT/descendant-marker-$bench"
    mkdir -p "$FAKE_WORK_DIR"
    set +e
    PATH="$FAKE_BIN:$ORIGINAL_PATH" FAKE_LOG="$FAKE_LOG" FAKE_WORK_DIR="$FAKE_WORK_DIR" \
      ALIGN_BENCH_WORK_DIR="$FAKE_WORK_DIR" \
      FAKE_BLOCK_MODE=root FAKE_BLOCK_MARKER="$block_marker" \
      FAKE_DESCENDANT_MARKER="$descendant_marker" \
      "$REPO_ROOT/bench/$bench/run.sh" native &
    SIGNAL_SCRIPT_PID=$!
    set -e
    ready=0
    for attempt in $(seq 1 100); do
      if [[ -f "$block_marker" && -f "$descendant_marker" ]]; then
        ready=1
        break
      fi
      sleep 0.05
    done
    [[ "$ready" -eq 1 ]] || fail "$bench signal fixture never created the blocking descendant"
    SIGNAL_DESCENDANT_PID="$(sed -n '1p' "$descendant_marker")"
    kill -TERM "$SIGNAL_SCRIPT_PID"
    set +e
    wait "$SIGNAL_SCRIPT_PID"
    signal_status=$?
    set -e
    SIGNAL_SCRIPT_PID=""
    [[ "$signal_status" -ne 0 ]] || fail "$bench signal was accepted"
    for attempt in $(seq 1 100); do
      kill -0 "$SIGNAL_DESCENDANT_PID" 2>/dev/null || break
      sleep 0.05
    done
    if kill -0 "$SIGNAL_DESCENDANT_PID" 2>/dev/null; then
      fail "$bench signal cleanup left a child-process-group descendant"
    fi
    SIGNAL_DESCENDANT_PID=""
    assert_empty "$FAKE_WORK_DIR"
  done
}

benchmark_input_workdir_matrix
echo "benchmark input checks passed"
