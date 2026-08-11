#!/usr/bin/env bash
# Run the bounded gate's already-compiled test binaries concurrently.
#
#   usage: scripts/run-gate-binaries.sh ARTIFACT_JSON EXPECTED_NAME...
#
# ARTIFACT_JSON is the stdout of a `cargo test --no-run --message-format=json*`
# build; EXPECTED_NAME... is the exact set of binaries that build must have
# produced. The split from scripts/test-pr.sh is what lets
# scripts/test-pr-workflow.sh exercise discovery, mismatch, failure, and
# killed-child handling against fixtures without compiling anything.
#
# Cargo runs test binaries one at a time, and separate `cargo test` invocations
# cannot overlap either: cargo holds the target-directory lock across the test
# RUN as well as the build, so a second invocation blocks until the first has
# finished running its tests (measured: two ~60s crates launched together took
# 121s, each reporting "Blocking waiting for file lock on build directory").
# Running the compiled binaries here instead turns the gate's serial sum into
# roughly its longest binary.
#
# This reproduces exactly two things cargo does when it runs a test binary: the
# working directory (the package directory) and the dynamic-library search path
# (the build scripts' link paths plus the profile output directories). It does
# not reproduce cargo's other runtime environment — notably the CARGO_* runtime
# variables, which no gate test reads, since env!() bakes those in at compile
# time.
set -euo pipefail

[ $# -ge 2 ] || {
  echo "usage: scripts/run-gate-binaries.sh ARTIFACT_JSON EXPECTED_NAME..." >&2
  exit 2
}
artifacts="$1"
shift
# Deliberately unquoted below: the expected set is a whitespace-separated list
# of target names, all of them [a-z0-9_] identifiers.
expected="$*"

# shellcheck source=scripts/dyld-env.sh
. "$(dirname "$0")/dyld-env.sh"
align_use_private_dyld_region

logs="$(mktemp -d)"
running_jobs=""
# On any exit, including an interrupted run, stop what is still running before
# removing the logs. The test binary is the tracked subshell's own child, so it
# has to be killed first — killing only the subshell would orphan it and leave
# it holding the machine. Anything the test binary itself spawned is beyond
# reach here; `pkill -P` is best-effort and absent hosts just skip it.
cleanup() {
  local entry pid
  for entry in $running_jobs; do
    pid="${entry%%:*}"
    pkill -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
  done
  rm -rf "$logs" 2>/dev/null || true
}
trap 'cleanup' EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

# Cargo emits one JSON object per built unit, fresh or not. A test binary is an
# artifact whose *profile* carries "test":true — the target-level flag of the
# same name is true for an ordinary library too, and the alignc binary the
# driver tests depend on is an executable with "test":false — and whose
# `executable` is not null. `manifest_path` gives the package directory, the
# working directory cargo runs a test binary in.
#
# A tab separates the two fields, because a repository checked out under a path
# containing a space is otherwise silently mis-split.
tab="$(printf '\t')"
extract='s/.*"manifest_path":"\([^"]*\)".*"profile":{[^}]*"test":true}.*"executable":"\([^"]*\)".*/\1'"$tab"'\2/p'
binaries="$(sed -n "$extract" "$artifacts")"
[ -n "$binaries" ] || {
  echo "the gate build produced no test binaries (cargo artifact format changed?)" >&2
  exit 1
}

# Cargo's own name for a test binary, which is its file name without the
# disambiguating -<hash> suffix.
binary_names() {
  printf '%s\n' "$binaries" | while IFS="$tab" read -r manifest executable; do
    [ -n "$executable" ] || continue
    name="$(basename "$executable")"
    printf '%s\n' "${name%-*}"
  done
}

# Fail closed in BOTH directions, and on a multiset rather than a set: a
# missing binary means the gate silently stopped covering something, an extra
# one means a `-p`/`--test` selection quietly widened the gate (two packages
# each holding, say, a tests/summary.rs would appear here as a duplicate), and
# either way the declared list in scripts/test-pr.sh must be updated
# deliberately.
found_names="$(binary_names | LC_ALL=C sort)"
expected_names="$(printf '%s\n' $expected | LC_ALL=C sort)"
if [ "$found_names" != "$expected_names" ]; then
  echo "the compiled gate binaries do not match the declared gate set" >&2
  echo "  expected: $(printf '%s ' $expected_names)" >&2
  echo "  compiled: $(printf '%s ' $found_names)" >&2
  echo "  update the gate selection and its declared list in scripts/test-pr.sh together" >&2
  exit 1
fi

# Reproduce cargo's runtime dynamic-library search path: every build script's
# `linked_paths` (this is how a dynamically linked libLLVM is found) plus the
# two profile output directories. Replayed for fresh units too, so it does not
# depend on anything actually recompiling.
deps_dir="$(dirname "$(printf '%s\n' "$binaries" | sed -n "1s/^[^$tab]*$tab//p")")"
dylib_path="$(
  {
    sed -n 's/.*"linked_paths":\[\([^]]*\)\].*/\1/p' "$artifacts" |
      tr ',' '\n' |
      sed -n 's/^"\([a-z-]*=\)\{0,1\}\(\/[^"]*\)"$/\2/p'
    printf '%s\n' "$deps_dir" "$(dirname "$deps_dir")"
  } | awk '!seen[$0]++' | tr '\n' ':' | sed 's/:$//'
)"
if [ "$(uname -s)" = Darwin ]; then
  export DYLD_FALLBACK_LIBRARY_PATH="$dylib_path:${DYLD_FALLBACK_LIBRARY_PATH:-$HOME/lib:/usr/local/lib:/usr/lib}"
else
  export LD_LIBRARY_PATH="$dylib_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# One process per binary, capped so the gate does not oversubscribe a small CI
# runner, and libtest's own thread pool divided by that cap: without this each
# of N concurrent binaries would start N threads of its own, multiplying both
# the runnable thread count and peak RSS by the core count.
ncpu="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"
case "$ncpu" in
  '' | *[!0-9]*) ncpu=4 ;;
esac
[ "$ncpu" -ge 1 ] || ncpu=1
gate_jobs="${ALIGN_GATE_JOBS:-$ncpu}"
case "$gate_jobs" in
  '' | *[!0-9]*) gate_jobs="$ncpu" ;;
esac
[ "$gate_jobs" -ge 1 ] || gate_jobs=1
test_threads=$((ncpu / gate_jobs))
[ "$test_threads" -ge 1 ] || test_threads=1
export RUST_TEST_THREADS="$test_threads"

# Slot bookkeeping is "pid:slot" entries in one string; Bash 3.2 has no
# `wait -n`, and `jobs` is unspecified once this shell has run a command
# substitution, so the parent counts its own children explicitly. A child is
# done when it has recorded its result, and waiting on such a pid returns
# immediately — which reaps it without blocking behind a slower sibling.
reap_completed() {
  local remaining="" entry
  for entry in $running_jobs; do
    if [ -e "$logs/${entry#*:}.status" ]; then
      wait "${entry%%:*}" 2>/dev/null || true
    else
      remaining="$remaining $entry"
    fi
  done
  running_jobs="$remaining"
}
count_running() {
  # shellcheck disable=SC2086
  set -- $running_jobs
  echo $#
}

printf 'gate: %s binaries, %s parallel, %s test thread(s) each\n' \
  "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" "$gate_jobs" "$test_threads"
printf 'gate: %s\n' "$(printf '%s ' $found_names)"

while IFS="$tab" read -r manifest executable; do
  [ -n "$executable" ] || continue
  name="$(basename "$executable")"
  name="${name%-*}"
  # Claim the log slot in the parent so a collision cannot silently drop a
  # binary's result. Which of two same-named binaries takes the "-2" label
  # follows cargo's artifact order and is therefore not stable across runs —
  # but a collision already fails the declared-set check above, so this only
  # has to stay lossless, not deterministic.
  slot="$name"
  suffix=2
  while [ -e "$logs/$slot.log" ]; do
    slot="$name-$suffix"
    suffix=$((suffix + 1))
  done
  : >"$logs/$slot.log"
  reap_completed
  while [ "$(count_running)" -ge "$gate_jobs" ]; do
    sleep 0.2
    reap_completed
  done
  # Announce before launching: if a binary hangs, the last "start" line without
  # a matching result names the suspect.
  printf 'gate: start %s\n' "$slot"
  (
    cd "$(dirname "$manifest")"
    binary_status=0
    started="$(date +%s)"
    "$executable" >"$logs/$slot.log" 2>&1 || binary_status=$?
    printf '%s %s\n' "$binary_status" "$(($(date +%s) - started))" >"$logs/$slot.status"
  ) </dev/null &
  running_jobs="$running_jobs $!:$slot"
done <<<"$binaries"
wait
running_jobs=""

# Every binary's output is reported, in one deterministic C-collation order,
# whichever of them failed. The gate's own exit code is 1 for any failure; a
# binary's real exit code (and a child that died before recording one) is in
# its header line.
status=0
for slot in $(cd "$logs" && LC_ALL=C ls | LC_ALL=C sed -n 's/\.log$//p' | LC_ALL=C sort); do
  record="$(cat "$logs/$slot.status" 2>/dev/null || true)"
  binary_status="${record%% *}"
  elapsed="${record#* }"
  if [ -z "$binary_status" ]; then
    binary_status="killed"
    elapsed="?"
  fi
  printf -- '--- %s (exit %s, %ss) ---\n' "$slot" "$binary_status" "$elapsed"
  cat "$logs/$slot.log"
  [ "$binary_status" = 0 ] || status=1
done
exit "$status"
