#!/usr/bin/env bash
# Deterministic owner for the benchmark-input work-directory contract.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-benchmark-input.XXXXXX")"
FAKE_BIN="$TEST_ROOT/bin"
FAKE_LOG="$TEST_ROOT/cargo.log"
ORIGINAL_PATH="$PATH"
ORIGINAL_PYTHON3="$(command -v python3)"
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

assert_cleared_private_tree() {
  [[ -d "$1/prepared" && ! -L "$1/prepared" ]] || fail "missing cleared private child: $1"
  if find "$1/prepared" ! -type d -print -quit | grep -q .; then
    fail "cleared private child contains a non-directory entry: $1"
  fi
  local entry
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    if [[ ( -e "$entry" || -L "$entry" ) && "$entry" != "$1/prepared" ]]; then
      fail "foreign entry remains beside cleared private child: $entry"
    fi
  done
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
FAKE_WORK_DIR_PHYSICAL="$(cd "$FAKE_WORK_DIR" && pwd -P)"
[[ -f Cargo.lock ]] || { echo "detached or root Cargo.lock is missing" >&2; exit 87; }
resolve_path() {
  "$ORIGINAL_PYTHON3" -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"
}
resolved_target="$(resolve_path "$CARGO_TARGET_DIR")"
resolved_tmp="$(resolve_path "$TMPDIR")"
case "$resolved_target" in
  "$FAKE_WORK_DIR_PHYSICAL"/prepared/*) ;;
  *) echo "cargo target escaped the private child: $CARGO_TARGET_DIR" >&2; exit 81 ;;
esac
case "$resolved_tmp" in
  "$FAKE_WORK_DIR_PHYSICAL"/prepared/*) ;;
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
  if [[ "${FAKE_SURVIVOR_MODE:-}" == root ]]; then
    : "${FAKE_SURVIVOR_MARKER:?}"
    trap '' TERM INT
    (
      trap '' TERM INT
      while :; do sleep 1; done
    ) &
    printf '%s\n' "$!" > "$FAKE_SURVIVOR_MARKER"
    exit 0
  fi
  mkdir -p "$CARGO_TARGET_DIR/release"
  cat > "$CARGO_TARGET_DIR/release/alignc" <<'FAKE_ALIGNC'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_WORK_DIR:?}"
: "${ALIGNC_CACHE:?}"
FAKE_WORK_DIR_PHYSICAL="$(cd "$FAKE_WORK_DIR" && pwd -P)"
if [[ "${FAKE_FAIL_MODE:-}" == alignc ]]; then
  exit 71
fi
if [[ "${1:-}" != emit-obj || "$#" -lt 3 ]]; then
  exit 72
fi
resolved_output="$($ORIGINAL_PYTHON3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$3")"
case "$resolved_output" in
  "$FAKE_WORK_DIR_PHYSICAL"/prepared/*) ;;
  *) echo "kernel object escaped the private child" >&2; exit 83 ;;
esac
resolved_cache="$($ORIGINAL_PYTHON3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$ALIGNC_CACHE")"
case "$resolved_cache" in
  "$FAKE_WORK_DIR_PHYSICAL"/prepared/*) ;;
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
  printf 'int align_fake_runtime(void) { return 0; }\n' > "$CARGO_TARGET_DIR/release/fake-runtime.c"
  case "$(uname -s)" in
    Darwin)
      cc -dynamiclib -Wl,-install_name,@rpath/libalign_runtime.dylib \
        "$CARGO_TARGET_DIR/release/fake-runtime.c" \
        -o "$CARGO_TARGET_DIR/release/libalign_runtime.dylib"
      ;;
    Linux)
      cc -shared -fPIC -Wl,-soname,libalign_runtime.so \
        "$CARGO_TARGET_DIR/release/fake-runtime.c" \
        -o "$CARGO_TARGET_DIR/release/libalign_runtime.so"
      ;;
    *) exit 90 ;;
  esac
elif has_arg build "$@"; then
  if [[ "${FAKE_ADD_FOREIGN:-0}" == 1 && ! -e "$FAKE_WORK_DIR/foreign" ]]; then
    printf 'caller-owned residue\n' > "$FAKE_WORK_DIR/foreign"
  fi
  resolved_kernel="$($ORIGINAL_PYTHON3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "${ALIGN_KERNEL_OBJ:-}")"
  case "$resolved_kernel" in
    "$FAKE_WORK_DIR_PHYSICAL"/prepared/*) ;;
    *) echo "detached kernel object escaped the private child" >&2; exit 84 ;;
  esac
  resolved_runtime="$($ORIGINAL_PYTHON3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "${ALIGN_RUNTIME_DIR:-}")"
  case "$resolved_runtime" in
    "$FAKE_WORK_DIR_PHYSICAL"/prepared/*) ;;
    *) echo "runtime directory escaped the private child" >&2; exit 85 ;;
  esac
  if [[ "${FAKE_FAIL_MODE:-}" == detached ]]; then
    exit 74
  fi
  if [[ "${FAKE_SWAP_PREPARED:-0}" == 1 ]]; then
    mv "$FAKE_WORK_DIR_PHYSICAL/prepared" "$FAKE_WORK_DIR_PHYSICAL/original-prepared"
    mkdir "$FAKE_WORK_DIR_PHYSICAL/prepared"
    printf 'foreign\n' > "$FAKE_WORK_DIR_PHYSICAL/prepared/foreign"
    exit 75
  fi
  if [[ "${FAKE_SWAP_ARTIFACTS:-0}" == 1 ]]; then
    mv "$FAKE_WORK_DIR_PHYSICAL/prepared/artifacts" \
      "$FAKE_WORK_DIR_PHYSICAL/prepared/original-artifacts"
    ln -s "${FAKE_ARTIFACT_ESCAPE_DIR:?}" "$FAKE_WORK_DIR_PHYSICAL/prepared/artifacts"
  fi
  if [[ -n "${FAKE_SLEEP:-}" ]]; then
    sleep "$FAKE_SLEEP"
  fi
  case "$PWD" in
    */bench/json_decode) binary=json-decode-bench ;;
    */bench/json_soa) binary=json-soa-bench ;;
    *) echo "unexpected detached Cargo directory: $PWD" >&2; exit 89 ;;
  esac
  mkdir -p "$CARGO_TARGET_DIR/release"
  cat > "$CARGO_TARGET_DIR/release/$binary" <<'FAKE_BENCHMARK'
#!/usr/bin/env bash
set -euo pipefail
[[ "${CARGO_NET_OFFLINE:-}" == true ]]
[[ "${HOME+x}" == x ]]
[[ -z "$HOME" ]]
[[ "${LC_ALL:-}" == C ]]
[[ "${PATH:-}" == /usr/bin:/bin ]]
[[ "${TZ:-}" == UTC ]]
[[ -z "${ALIGN_BENCH_AMBIENT_SENTINEL+x}" ]]
printf 'fake prepared benchmark\n'
FAKE_BENCHMARK
  chmod 700 "$CARGO_TARGET_DIR/release/$binary"
else
  echo "unexpected cargo invocation: $*" >&2
  exit 86
fi
FAKE_CARGO
  chmod 700 "$FAKE_BIN/cargo"

  cat > "$FAKE_BIN/mkdir" <<'FAKE_MKDIR'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${FAKE_MKDIR_COLLIDE:-0}" == 1 && "$#" -eq 1 && "$1" == "$(cd "$FAKE_WORK_DIR" && pwd -P)/prepared" ]]; then
  /bin/mkdir "$1"
  printf 'foreign\n' > "$1/foreign"
  exit 1
fi
exec /bin/mkdir "$@"
FAKE_MKDIR
  chmod 700 "$FAKE_BIN/mkdir"

  cat > "$FAKE_BIN/python3" <<'FAKE_PYTHON'
#!/usr/bin/env bash
set -euo pipefail
: "${ORIGINAL_PYTHON3:?}"
if [[ "${FAKE_SWAP_BEFORE_BOUND_EXEC:-0}" == 1 && "${1:-}" == */bound_exec.py ]]; then
  mv "$FAKE_WORK_DIR/prepared" "$FAKE_WORK_DIR/original-prepared-run"
  mv "${FAKE_REPLACEMENT_DIR:?}" "$FAKE_WORK_DIR/prepared"
fi
if [[ "${FAKE_PREPARED_TREE_CLEAR_FAIL:-0}" == 1 && \
      "${1:-}" == */prepared_tree.py && "${2:-}" == clear ]]; then
  exit 91
fi
"$ORIGINAL_PYTHON3" "$@"
status=$?
if [[ "$status" -eq 0 && "${FAKE_SWAP_AFTER_VERIFY:-0}" == 1 ]]; then
  if [[ ( "${1:-}" == */manifest.py && "${2:-}" == verify ) || \
        ( "${1:-}" == */prepared_tree.py && "${2:-}" == write-manifest ) ]]; then
    mv "$FAKE_WORK_DIR/prepared" "$FAKE_WORK_DIR/original-prepared-publish"
    mkdir "$FAKE_WORK_DIR/prepared"
    printf 'foreign\n' > "$FAKE_WORK_DIR/prepared/foreign"
  fi
fi
if [[ "$status" -eq 0 && "${FAKE_SWAP_AFTER_VERIFY_EMPTY:-0}" == 1 && \
      "${1:-}" == */prepared_tree.py && "${2:-}" == write-manifest ]]; then
  mv "$FAKE_WORK_DIR/prepared" "$FAKE_WORK_DIR/original-prepared-empty-publish"
  mkdir "$FAKE_WORK_DIR/prepared"
fi
exit "$status"
FAKE_PYTHON
  chmod 700 "$FAKE_BIN/python3"
}

run_benchmark() {
  local bench="$1"
  shift
  PATH="$FAKE_BIN:$ORIGINAL_PATH" \
    FAKE_LOG="$FAKE_LOG" FAKE_WORK_DIR="$FAKE_WORK_DIR" ORIGINAL_PYTHON3="$ORIGINAL_PYTHON3" \
    ALIGN_BENCH_WORK_DIR="$FAKE_WORK_DIR" "$@" \
    "$REPO_ROOT/bench/$bench/run.sh" prepare native
}

run_prepared_benchmark() {
  local bench="$1"
  local digest
  shift
  digest="$("$ORIGINAL_PYTHON3" "$REPO_ROOT/scripts/benchmark_evidence/manifest.py" \
    verify --root "$FAKE_WORK_DIR/prepared" --manifest artifact-manifest.json)"
  PATH="$FAKE_BIN:$ORIGINAL_PATH" ALIGN_BENCH_WORK_DIR="$FAKE_WORK_DIR" \
    ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256="$digest" \
    FAKE_WORK_DIR="$FAKE_WORK_DIR" ORIGINAL_PYTHON3="$ORIGINAL_PYTHON3" "$@" \
    "$REPO_ROOT/bench/$bench/run.sh" native
}

assert_linux_sealed_copy() {
  [[ "$(uname -s)" == Linux ]] || return 0
  PYTHONDONTWRITEBYTECODE=1 python3 - "$REPO_ROOT" "$TEST_ROOT" <<'PY'
import hashlib
import os
import sys

sys.path.insert(0, os.path.join(sys.argv[1], "scripts", "benchmark_evidence"))
import bound_exec

root = os.path.join(sys.argv[2], "sealed-copy")
os.mkdir(root, 0o700)
path = os.path.join(root, "payload")
payload = b"verified bytes\n"
with open(path, "wb") as stream:
    stream.write(payload)
os.chmod(path, 0o755)
value = os.stat(path, follow_symlinks=False)
expected = {
    "kind": "file",
    "mode": "100755",
    "uid": value.st_uid,
    "gid": value.st_gid,
    "size": len(payload),
    "sha256": hashlib.sha256(payload).hexdigest(),
}
parent_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
sealed_fd = bound_exec._open_bound_file(parent_fd, "payload", expected)
try:
    with open(path, "wb") as stream:
        stream.write(b"changed source\n")
    os.lseek(sealed_fd, 0, os.SEEK_SET)
    if os.read(sealed_fd, len(payload) + 1) != payload:
        raise SystemExit("sealed copy changed with its source")
    try:
        os.write(sealed_fd, b"x")
    except OSError:
        pass
    else:
        raise SystemExit("sealed copy remained writable")
finally:
    os.close(sealed_fd)
    os.close(parent_fd)
PY
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
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "relative work directory" env ALIGN_BENCH_WORK_DIR=relative \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "non-directory work path" env ALIGN_BENCH_WORK_DIR="$file_path" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "final symlink work directory" env ALIGN_BENCH_WORK_DIR="$symlink_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "final symlink with repeated separators" env ALIGN_BENCH_WORK_DIR="$symlink_dir//" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "final symlink with dot suffix" env ALIGN_BENCH_WORK_DIR="$symlink_dir/." \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "root work directory" env ALIGN_BENCH_WORK_DIR=/ \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "repository work directory" env ALIGN_BENCH_WORK_DIR="$REPO_ROOT" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "in-repository work directory" env ALIGN_BENCH_WORK_DIR="$REPO_ROOT/bench" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "nonempty work directory" env ALIGN_BENCH_WORK_DIR="$nonempty_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "missing work-dir environment" env -u ALIGN_BENCH_WORK_DIR \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native
  assert_rejected "missing phase" env ALIGN_BENCH_WORK_DIR="$empty_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh"
  assert_rejected "prepare without native target" env ALIGN_BENCH_WORK_DIR="$empty_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare
  assert_rejected "removed baseline selector" env ALIGN_BENCH_WORK_DIR="$empty_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare baseline
  assert_rejected "extra selector" env ALIGN_BENCH_WORK_DIR="$empty_dir" \
    "$REPO_ROOT/bench/json_decode/run.sh" prepare native extra
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
  mkdir -p "$missing_repo/bench/json_decode" "$missing_repo/bench/json_escape/evidence" "$missing_work"
  cp "$REPO_ROOT/bench/json_decode/run.sh" "$missing_repo/bench/json_decode/run.sh"
  cp "$REPO_ROOT/bench/json_escape/evidence/run-prepared-benchmark.sh" \
    "$missing_repo/bench/json_escape/evidence/run-prepared-benchmark.sh"
  if ALIGN_BENCH_WORK_DIR="$missing_work" "$missing_repo/bench/json_decode/run.sh" prepare native \
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
  assert_linux_sealed_copy
  [[ -f "$REPO_ROOT/bench/json_decode/Cargo.lock" ]] || fail "json_decode Cargo.lock is missing"
  [[ -f "$REPO_ROOT/bench/json_soa/Cargo.lock" ]] || fail "json_soa Cargo.lock is missing"

  local bench
  local cargo_count_before
  local cargo_count_after
  local benchmark_output
  for bench in json_decode json_soa; do
    FAKE_WORK_DIR="$TEST_ROOT/work-$bench"
    mkdir -p "$FAKE_WORK_DIR"
    run_benchmark "$bench" env
    [[ -f "$FAKE_WORK_DIR/prepared/artifact-manifest.json" ]] || fail "$bench did not seal artifacts"
    cargo_count_before="$(wc -l < "$FAKE_LOG" | tr -d ' ')"
    benchmark_output="$(run_prepared_benchmark "$bench" env ALIGN_BENCH_AMBIENT_SENTINEL=leak)"
    [[ "$benchmark_output" == $'target: native\nfake prepared benchmark' ]] ||
      fail "$bench measurement output did not use the fixed native target prelude"
    cargo_count_after="$(wc -l < "$FAKE_LOG" | tr -d ' ')"
    [[ "$cargo_count_after" -eq "$cargo_count_before" ]] || fail "$bench measurement invoked Cargo"
    [[ -f "$FAKE_WORK_DIR/prepared/artifact-manifest.json" ]] || fail "$bench measurement removed artifacts"
    [[ -d "$FAKE_WORK_DIR/prepared/root-target" ]] ||
      fail "$bench removed a build directory before candidate teardown"
    [[ ! -e "$REPO_ROOT/bench/$bench/kernel.o" ]] || fail "$bench wrote kernel.o beside its source"
  done
  assert_locked_offline_invocations

  FAKE_WORK_DIR="$TEST_ROOT/mutated-work"
  mkdir -p "$FAKE_WORK_DIR"
  run_benchmark json_decode env
  printf 'mutation\n' >> "$FAKE_WORK_DIR/prepared/artifacts/json-decode-bench"
  if run_prepared_benchmark json_decode >/dev/null 2>&1; then
    fail "mutated prepared executable was accepted"
  fi
  [[ -d "$FAKE_WORK_DIR/prepared" ]] || fail "failed verification deleted prepared evidence"

  FAKE_WORK_DIR="$TEST_ROOT/empty-native-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_prepared_benchmark json_decode >/dev/null 2>&1; then
    fail "native measurement without prepare was accepted"
  fi
  assert_empty "$FAKE_WORK_DIR"

  FAKE_WORK_DIR="$TEST_ROOT/missing-digest-work"
  mkdir -p "$FAKE_WORK_DIR"
  run_benchmark json_decode env
  if PATH="$FAKE_BIN:$ORIGINAL_PATH" ALIGN_BENCH_WORK_DIR="$FAKE_WORK_DIR" \
    FAKE_WORK_DIR="$FAKE_WORK_DIR" ORIGINAL_PYTHON3="$ORIGINAL_PYTHON3" \
    "$REPO_ROOT/bench/json_decode/run.sh" native >/dev/null 2>&1; then
    fail "native measurement without the prepare-time digest was accepted"
  fi

  FAKE_WORK_DIR="$TEST_ROOT/mkdir-collision-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_MKDIR_COLLIDE=1; then
    fail "prepared-directory mkdir collision was accepted"
  fi
  [[ -f "$FAKE_WORK_DIR/prepared/foreign" ]] || fail "mkdir collision deleted the foreign directory"

  FAKE_WORK_DIR="$TEST_ROOT/replaced-prepared-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_SWAP_PREPARED=1; then
    fail "prepared-directory identity replacement was accepted"
  fi
  [[ -f "$FAKE_WORK_DIR/prepared/foreign" ]] || fail "cleanup deleted a replaced prepared directory"
  [[ -d "$FAKE_WORK_DIR/original-prepared" ]] || fail "identity replacement lost the owned directory"

  FAKE_WORK_DIR="$TEST_ROOT/replaced-after-verify-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_SWAP_AFTER_VERIFY=1; then
    fail "prepared-directory replacement after verification was accepted"
  fi
  [[ -f "$FAKE_WORK_DIR/prepared/foreign" ]] || fail "post-verification replacement was deleted"
  [[ -d "$FAKE_WORK_DIR/original-prepared-publish" ]] ||
    fail "post-verification replacement lost the owned directory"

  FAKE_WORK_DIR="$TEST_ROOT/empty-replacement-after-verify-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_SWAP_AFTER_VERIFY_EMPTY=1; then
    fail "empty prepared-directory replacement after verification was accepted"
  fi
  [[ -d "$FAKE_WORK_DIR/prepared" ]] || fail "empty replacement was removed"
  directory_has_entries "$FAKE_WORK_DIR/prepared" && fail "empty replacement gained content"
  [[ -d "$FAKE_WORK_DIR/original-prepared-empty-publish" ]] ||
    fail "empty replacement lost the retained owned directory"

  FAKE_WORK_DIR="$TEST_ROOT/replaced-artifacts-work"
  artifact_escape="$TEST_ROOT/artifact-escape"
  mkdir -p "$FAKE_WORK_DIR" "$artifact_escape"
  printf 'caller owned\n' > "$artifact_escape/sentinel"
  if run_benchmark json_decode env FAKE_SWAP_ARTIFACTS=1 \
    FAKE_ARTIFACT_ESCAPE_DIR="$artifact_escape"; then
    fail "prepared artifacts-directory replacement was accepted"
  fi
  [[ "$(find "$artifact_escape" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" -eq 1 ]] ||
    fail "trusted preparation wrote through the replaced artifacts path"
  [[ -f "$artifact_escape/sentinel" ]] || fail "artifact replacement removed caller data"

  FAKE_WORK_DIR="$TEST_ROOT/replaced-before-exec-work"
  replacement_dir="$TEST_ROOT/replacement-prepared"
  mkdir -p "$FAKE_WORK_DIR"
  run_benchmark json_decode env
  cp -R "$FAKE_WORK_DIR/prepared" "$replacement_dir"
  benchmark_output="$(run_prepared_benchmark json_decode env FAKE_SWAP_BEFORE_BOUND_EXEC=1 \
    FAKE_REPLACEMENT_DIR="$replacement_dir")"
  [[ "$benchmark_output" == $'target: native\nfake prepared benchmark' ]] ||
    fail "descriptor-bound execution did not retain the shell-opened prepared root"
  [[ -d "$FAKE_WORK_DIR/original-prepared-run" ]] ||
    fail "pre-exec replacement lost the originally bound directory"

  FAKE_WORK_DIR="$TEST_ROOT/replaced-between-phases-work"
  phase_replacement="$TEST_ROOT/phase-replacement-prepared"
  mkdir -p "$FAKE_WORK_DIR"
  run_benchmark json_decode env
  prepare_digest="$("$ORIGINAL_PYTHON3" "$REPO_ROOT/scripts/benchmark_evidence/manifest.py" \
    verify --root "$FAKE_WORK_DIR/prepared" --manifest artifact-manifest.json)"
  cp -R "$FAKE_WORK_DIR/prepared" "$phase_replacement"
  printf 'changed configuration\n' > "$phase_replacement/configuration.json"
  unlink "$phase_replacement/artifact-manifest.json"
  "$ORIGINAL_PYTHON3" "$REPO_ROOT/scripts/benchmark_evidence/manifest.py" \
    write --root "$phase_replacement" --manifest artifact-manifest.json >/dev/null
  mv "$FAKE_WORK_DIR/prepared" "$FAKE_WORK_DIR/original-prepared-phase"
  mv "$phase_replacement" "$FAKE_WORK_DIR/prepared"
  if run_prepared_benchmark json_decode env \
    ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256="$prepare_digest" >/dev/null 2>&1; then
    fail "self-consistent prepared replacement between phases was accepted"
  fi

  local survivor_marker
  local survivor_pid
  local attempt
  for bench in json_decode json_soa; do
    FAKE_WORK_DIR="$TEST_ROOT/survivor-work-$bench"
    survivor_marker="$TEST_ROOT/survivor-marker-$bench"
    mkdir -p "$FAKE_WORK_DIR"
    if run_benchmark "$bench" env FAKE_SURVIVOR_MODE=root \
      FAKE_SURVIVOR_MARKER="$survivor_marker"; then
      fail "$bench accepted a surviving descendant from the first root build"
    fi
    [[ -f "$survivor_marker" ]] || fail "$bench survivor fixture did not create a descendant"
    survivor_pid="$(sed -n '1p' "$survivor_marker")"
    for attempt in $(seq 1 100); do
      kill -0 "$survivor_pid" 2>/dev/null || break
      sleep 0.05
    done
    if kill -0 "$survivor_pid" 2>/dev/null; then
      fail "$bench first root build left its surviving descendant"
    fi
    assert_cleared_private_tree "$FAKE_WORK_DIR"
  done

  FAKE_WORK_DIR="$TEST_ROOT/foreign-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_ADD_FOREIGN=1; then
    fail "foreign residue was accepted"
  fi
  [[ -e "$FAKE_WORK_DIR/foreign" ]] || fail "foreign residue was deleted"
  [[ -d "$FAKE_WORK_DIR/prepared" ]] || fail "cleared private child was removed"
  if find "$FAKE_WORK_DIR/prepared" ! -type d -print -quit | grep -q .; then
    fail "cleared private child retained a non-directory entry"
  fi

  FAKE_WORK_DIR="$TEST_ROOT/error-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_soa env FAKE_FAIL_MODE=detached; then
    fail "detached benchmark failure was accepted"
  fi
  assert_cleared_private_tree "$FAKE_WORK_DIR"

  FAKE_WORK_DIR="$TEST_ROOT/cleanup-failure-work"
  mkdir -p "$FAKE_WORK_DIR"
  if run_benchmark json_decode env FAKE_ADD_FOREIGN=1 FAKE_PREPARED_TREE_CLEAR_FAIL=1; then
    fail "cleanup failure was accepted"
  fi
  directory_has_entries "$FAKE_WORK_DIR" || fail "cleanup failure did not preserve evidence of failure"
  PATH="$ORIGINAL_PATH" rm -rf "$FAKE_WORK_DIR"

  local block_marker
  local descendant_marker
  local ready
  local signal_status
  for bench in json_decode json_soa; do
    FAKE_WORK_DIR="$TEST_ROOT/signal-work-$bench"
    block_marker="$TEST_ROOT/block-marker-$bench"
    descendant_marker="$TEST_ROOT/descendant-marker-$bench"
    mkdir -p "$FAKE_WORK_DIR"
    set +e
    PATH="$FAKE_BIN:$ORIGINAL_PATH" FAKE_LOG="$FAKE_LOG" FAKE_WORK_DIR="$FAKE_WORK_DIR" \
      ORIGINAL_PYTHON3="$ORIGINAL_PYTHON3" \
      ALIGN_BENCH_WORK_DIR="$FAKE_WORK_DIR" \
      FAKE_BLOCK_MODE=root FAKE_BLOCK_MARKER="$block_marker" \
      FAKE_DESCENDANT_MARKER="$descendant_marker" \
      "$REPO_ROOT/bench/$bench/run.sh" prepare native &
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
    assert_cleared_private_tree "$FAKE_WORK_DIR"
  done
}

benchmark_input_workdir_matrix
echo "benchmark input checks passed"
