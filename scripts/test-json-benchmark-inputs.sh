#!/usr/bin/env bash
# Deterministic owner for locked/offline JSON benchmark inputs and work-directory cleanup.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
fixture_root="$(mktemp -d)"
script_pid=""
descendant_pid=""
cleanup() {
  if [[ -n "$script_pid" ]]; then
    kill -TERM "$script_pid" 2>/dev/null || true
    wait "$script_pid" 2>/dev/null || true
  fi
  if [[ -n "$descendant_pid" ]]; then
    kill -KILL "$descendant_pid" 2>/dev/null || true
  fi
  chmod -R u+w "$fixture_root" 2>/dev/null || true
  rm -rf -- "$fixture_root"
}
trap cleanup EXIT

fail() {
  echo "json benchmark input test: $*" >&2
  exit 1
}

for benchmark in json_decode json_soa; do
  lock="$repo_root/bench/$benchmark/Cargo.lock"
  [[ -f "$lock" ]] || fail "$benchmark detached Cargo.lock is missing"
  git -C "$repo_root" ls-files --error-unmatch "bench/$benchmark/Cargo.lock" >/dev/null 2>&1 ||
    fail "$benchmark detached Cargo.lock is not tracked"
  if git -C "$repo_root" check-ignore -q "bench/$benchmark/Cargo.lock"; then
    fail "$benchmark detached Cargo.lock is still ignored"
  fi
  cargo metadata --locked --offline --format-version 1 \
    --manifest-path "$repo_root/bench/$benchmark/Cargo.toml" >/dev/null
done

for document in \
  "$repo_root/docs/impl/core-design/json-escape-benchmark-evidence.md" \
  "$repo_root/docs/impl/core-design/ja/json-escape-benchmark-evidence.md"; do
  grep -Fxq 'scripts/cargo.sh' "$document" || fail "$(basename "$document") does not protect scripts/cargo.sh"
  grep -Fxq 'scripts/dyld-env.sh' "$document" || fail "$(basename "$document") does not protect scripts/dyld-env.sh"
done

missing_lock="$fixture_root/missing-lock"
stale_lock="$fixture_root/stale-lock"
mkdir -p "$missing_lock/src" "$stale_lock/src"
cp "$repo_root/bench/json_decode/Cargo.toml" "$missing_lock/Cargo.toml"
: >"$missing_lock/src/main.rs"
sed 's/serde_json = "1"/serde_json = "=1.0.0"/' \
  "$repo_root/bench/json_decode/Cargo.toml" >"$stale_lock/Cargo.toml"
cp "$repo_root/bench/json_decode/Cargo.lock" "$stale_lock/Cargo.lock"
: >"$stale_lock/src/main.rs"
if cargo metadata --locked --offline --format-version 1 \
  --manifest-path "$stale_lock/Cargo.toml" >/dev/null 2>&1; then
  fail "Cargo accepted a stale detached lock under --locked"
fi

fake_bin="$fixture_root/bin"
fake_llvm="$fixture_root/llvm"
fake_cargo_home="$fixture_root/cargo-home"
cargo_log="$fixture_root/cargo.log"
mkdir -p "$fake_bin" "$fake_llvm/lib" "$fake_cargo_home"
: >"$cargo_log"
: >"$fake_cargo_home/.sentinel"
chmod 0555 "$fake_cargo_home"

cat >"$fake_bin/llvm-config-22" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  --version) printf '22.0.0\n' ;;
  --prefix) printf '%s\n' "$FAKE_LLVM_PREFIX" ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$fake_bin/llvm-config-22"

cat >"$fake_bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

args=" $* "
[[ "$args" == *" --locked "* ]] || { echo "cargo fixture: missing --locked" >&2; exit 80; }
[[ "$args" == *" --offline "* ]] || { echo "cargo fixture: missing --offline" >&2; exit 81; }
[[ "${CARGO_NET_OFFLINE:-}" == true ]] || { echo "cargo fixture: CARGO_NET_OFFLINE is not true" >&2; exit 82; }
[[ -n "${CARGO_HOME:-}" && ! -w "$CARGO_HOME" ]] || exit 87
[[ -n "${CARGO_TARGET_DIR:-}" && -n "${TMPDIR:-}" ]] || exit 83
[[ "$CARGO_TARGET_DIR" == "$ALIGN_BENCH_WORK_DIR"/align-json-bench.*/*-target ]] || exit 84
[[ "$TMPDIR" == "$ALIGN_BENCH_WORK_DIR"/align-json-bench.*/tmp ]] || exit 85

manifest=""
previous=""
for argument in "$@"; do
  if [[ "$previous" == --manifest-path ]]; then
    manifest="$argument"
    break
  fi
  previous="$argument"
done
[[ -n "$manifest" && -f "$(dirname "$manifest")/Cargo.lock" ]] || exit 86
printf '%s\t%s\t%s\n' "$*" "$CARGO_TARGET_DIR" "$TMPDIR" >>"$FAKE_CARGO_LOG"

mkdir -p "$CARGO_TARGET_DIR/release"
if [[ "$args" == *" --bin alignc "* ]]; then
  cat >"$CARGO_TARGET_DIR/release/alignc" <<'ALIGNC'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == emit-obj && $# -ge 3 ]] || exit 90
[[ "${ALIGNC_CACHE:-}" == off ]] || exit 94
printf 'fixture object\n' >"$3"
ALIGNC
  chmod +x "$CARGO_TARGET_DIR/release/alignc"
elif [[ "$args" == *" -p align_runtime "* ]]; then
  if [[ "${FAKE_CARGO_FAIL_ON:-}" == runtime ]]; then
    exit 91
  fi
  if [[ "${FAKE_CARGO_BLOCK_ON:-}" == runtime ]]; then
    : >"$FAKE_BLOCK_MARKER"
    trap '' TERM INT
    (
      trap '' TERM INT
      while :; do sleep 1; done
    ) &
    printf '%s\n' "$!" >"$FAKE_DESCENDANT_MARKER"
    while :; do sleep 1; done
  fi
  : >"$CARGO_TARGET_DIR/release/libalign_runtime.dylib"
elif [[ "${1:-}" == run ]]; then
  [[ -f "${ALIGN_KERNEL_OBJ:-}" && -d "${ALIGN_RUNTIME_DIR:-}" ]] || exit 92
  if [[ "${FAKE_FOREIGN_RESIDUE:-}" == 1 ]]; then
    : >"$ALIGN_BENCH_WORK_DIR/foreign-residue"
  fi
  printf 'fixture benchmark\n'
else
  exit 93
fi
EOF
chmod +x "$fake_bin/cargo"

fixture_env=(
  env
  "PATH=$fake_bin:/usr/bin:/bin"
  "LLVM_CONFIG=$fake_bin/llvm-config-22"
  "FAKE_LLVM_PREFIX=$fake_llvm"
  "FAKE_CARGO_LOG=$cargo_log"
  "CARGO_HOME=$fake_cargo_home"
)

new_work_root() {
  mktemp -d "$fixture_root/work.XXXXXXXX"
}

assert_empty() {
  local directory="$1"
  local -a entries=()
  shopt -s nullglob dotglob
  entries=("$directory"/*)
  shopt -u nullglob dotglob
  ((${#entries[@]} == 0)) || fail "$directory was not empty after cleanup"
}

expect_fail() {
  local label="$1"
  shift
  if "$@" >"$fixture_root/$label.out" 2>"$fixture_root/$label.err"; then
    fail "$label unexpectedly succeeded"
  fi
}

missing_lock_work="$(new_work_root)"
expect_fail missing_lock env ALIGN_BENCH_WORK_DIR="$missing_lock_work" bash -euo pipefail -c \
  '. "$1"; benchmark_input_begin "$2" "$3"' _ \
  "$repo_root/bench/json_escape/evidence/benchmark-input.sh" "$repo_root" "$missing_lock"
assert_empty "$missing_lock_work"

before_status="$(git -C "$repo_root" status --porcelain --untracked-files=all)"
for benchmark in json_decode json_soa; do
  work_root="$(new_work_root)"
  "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$work_root" \
    "$repo_root/bench/$benchmark/run.sh" native >"$fixture_root/$benchmark.out"
  grep -Fxq 'target: native' "$fixture_root/$benchmark.out" || fail "$benchmark lost target output"
  grep -Fxq 'fixture benchmark' "$fixture_root/$benchmark.out" || fail "$benchmark did not exec the harness"
  assert_empty "$work_root"
done
after_status="$(git -C "$repo_root" status --porcelain --untracked-files=all)"
[[ "$after_status" == "$before_status" ]] || fail "a nominal run changed the repository"
[[ "$(wc -l <"$cargo_log" | tr -d ' ')" == 6 ]] || fail "the two scripts did not make exactly six Cargo invocations"
[[ -f "$fake_cargo_home/.sentinel" ]] || fail "the read-only Cargo home was changed"
shopt -s nullglob dotglob
cargo_home_entries=("$fake_cargo_home"/*)
shopt -u nullglob dotglob
[[ ${#cargo_home_entries[@]} == 1 && "${cargo_home_entries[0]}" == "$fake_cargo_home/.sentinel" ]] ||
  fail "the benchmark wrote into the read-only Cargo home"

probe="$repo_root/bench/json_decode/run.sh"
expect_fail absent env -u ALIGN_BENCH_WORK_DIR \
  "PATH=$fake_bin:/usr/bin:/bin" "LLVM_CONFIG=$fake_bin/llvm-config-22" \
  "FAKE_LLVM_PREFIX=$fake_llvm" "FAKE_CARGO_LOG=$cargo_log" "CARGO_HOME=$fake_cargo_home" \
  "$probe" native
expect_fail relative "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR=relative "$probe" native
expect_fail missing "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$fixture_root/missing" "$probe" native
plain_file="$fixture_root/plain-file"
: >"$plain_file"
expect_fail nondirectory "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$plain_file" "$probe" native
symlink_target="$(new_work_root)"
symlink_path="$fixture_root/work-link"
ln -s "$symlink_target" "$symlink_path"
expect_fail symlink "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$symlink_path" "$probe" native
expect_fail symlink_slashes "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$symlink_path//" "$probe" native
expect_fail symlink_dot "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$symlink_path/." "$probe" native
expect_fail root "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR=/ "$probe" native
expect_fail repository "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$repo_root" "$probe" native
expect_fail in_repository "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$repo_root/bench" "$probe" native
nonempty="$(new_work_root)"
: >"$nonempty/.hidden"
expect_fail nonempty "${fixture_env[@]}" ALIGN_BENCH_WORK_DIR="$nonempty" "$probe" native
[[ -f "$nonempty/.hidden" ]] || fail "nonempty rejection deleted a foreign entry"

error_root="$(new_work_root)"
expect_fail child_error "${fixture_env[@]}" FAKE_CARGO_FAIL_ON=runtime \
  ALIGN_BENCH_WORK_DIR="$error_root" "$probe" native
assert_empty "$error_root"

residue_root="$(new_work_root)"
expect_fail foreign_residue "${fixture_env[@]}" FAKE_FOREIGN_RESIDUE=1 \
  ALIGN_BENCH_WORK_DIR="$residue_root" "$probe" native
[[ -f "$residue_root/foreign-residue" ]] || fail "cleanup deleted foreign residue"

signal_root="$(new_work_root)"
block_marker="$fixture_root/block-marker"
descendant_marker="$fixture_root/descendant-marker"
"${fixture_env[@]}" FAKE_CARGO_BLOCK_ON=runtime FAKE_BLOCK_MARKER="$block_marker" \
  FAKE_DESCENDANT_MARKER="$descendant_marker" \
  ALIGN_BENCH_WORK_DIR="$signal_root" "$probe" native >"$fixture_root/signal.out" 2>"$fixture_root/signal.err" &
script_pid=$!
for _ in $(seq 1 100); do
  [[ -f "$block_marker" ]] && break
  sleep 0.05
done
[[ -f "$block_marker" ]] || fail "signal fixture did not reach the blocking child"
[[ -f "$descendant_marker" ]] || fail "signal fixture did not create a descendant"
descendant_pid="$(cat "$descendant_marker")"
kill -TERM "$script_pid"
if wait "$script_pid"; then
  fail "signalled benchmark unexpectedly succeeded"
fi
script_pid=""
for _ in $(seq 1 100); do
  kill -0 "$descendant_pid" 2>/dev/null || break
  sleep 0.05
done
if kill -0 "$descendant_pid" 2>/dev/null; then
  fail "signalled benchmark left a child-process-group descendant"
fi
descendant_pid=""
assert_empty "$signal_root"

echo "json benchmark input matrix: PASS"
