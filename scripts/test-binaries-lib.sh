#!/usr/bin/env bash
# Shared engine for running already-compiled cargo test binaries concurrently.
# Sourced, never executed: scripts/run-gate-binaries.sh runs the bounded gate's
# declared set with it, scripts/run-suite-binaries.sh runs the whole workspace.
#
# Cargo runs test binaries one at a time, and separate `cargo test` invocations
# cannot overlap either: cargo holds the target-directory lock across the test
# RUN as well as the build, so a second invocation blocks until the first has
# finished running its tests (measured: two ~60s crates launched together took
# 121s, each reporting "Blocking waiting for file lock on build directory").
# Running the compiled binaries here instead turns a serial sum into roughly
# the longest binary.
#
# This reproduces exactly two things cargo does when it runs a test binary: the
# working directory (the package directory) and the dynamic-library search path
# (the build scripts' link paths plus the profile output directories). It does
# not reproduce cargo's other runtime environment — notably the CARGO_* runtime
# variables, which no test here reads, since env!() bakes those in at compile
# time.

# A tab separates the discovered manifest from the executable, because a
# repository checked out under a path containing a space is otherwise silently
# mis-split.
align_tb_tab="$(printf '\t')"

ALIGN_TB_BINARIES=""
ALIGN_TB_LOGS=""
ALIGN_TB_RUNNING=""
ALIGN_TB_JOBS=1
ALIGN_TB_THREADS=1
# Per-binary wall-clock budget in seconds; 0 disables it. A hung binary
# otherwise consumes the whole job budget and reports nothing at all, which is
# the one failure mode a nightly detector cannot afford.
ALIGN_TB_TIMEOUT="${ALIGN_TB_TIMEOUT:-0}"

# Cargo emits one JSON object per built unit, fresh or not. A test binary is an
# artifact whose *profile* carries "test":true — the target-level flag of the
# same name is true for an ordinary library too, and the alignc binary the
# driver tests depend on is an executable with "test":false — and whose
# `executable` is not null. Each discovered row retains the manifest, target
# kind, Cargo target name, and executable. The first gives the package working
# directory; the middle pair lets the nightly form a stable identity even when
# Cargo normalizes two distinct target names to the same executable basename.
align_tb_discover() {
  local artifacts="$1" extract
  extract='s/.*"manifest_path":"\([^"]*\)".*"target":{"kind":\["\([^"]*\)"[^]]*][^}]*"name":"\([^"]*\)".*"profile":{[^}]*"test":true}.*"executable":"\([^"]*\)".*/\1'"$align_tb_tab"'\2'"$align_tb_tab"'\3'"$align_tb_tab"'\4/p'
  ALIGN_TB_BINARIES="$(sed -n "$extract" "$artifacts")"
  [ -n "$ALIGN_TB_BINARIES" ]
}

# Cargo's own name for a test binary, which is its file name without the
# disambiguating -<hash> suffix.
align_tb_names() {
  local manifest target_kind target_name executable name
  printf '%s\n' "$ALIGN_TB_BINARIES" | while IFS="$align_tb_tab" read -r manifest target_kind target_name executable; do
    [ -n "$executable" ] || continue
    name="$(basename "$executable")"
    printf '%s\n' "${name%-*}"
  done
}

# Stable full-suite identity. Package directory, target kind, and Cargo target
# name are all required: separate packages commonly reuse integration-test
# names, and one package may have a same-named lib/bin pair. Package directory
# names are unique in this workspace; if that ever stops being true, the suite
# collision check fails closed instead of binding a manifest line ambiguously.
align_tb_qualified_names() {
  local manifest target_kind target_name executable package
  printf '%s\n' "$ALIGN_TB_BINARIES" | while IFS="$align_tb_tab" read -r manifest target_kind target_name executable; do
    [ -n "$executable" ] || continue
    package="$(basename "$(dirname "$manifest")")"
    printf '%s::%s::%s\n' "$package" "$target_kind" "$target_name"
  done
}

# Reproduce cargo's runtime dynamic-library search path: every build script's
# `linked_paths` (this is how a dynamically linked libLLVM is found) plus the
# two profile output directories. Replayed for fresh units too, so it does not
# depend on anything actually recompiling.
align_tb_export_dylib_path() {
  local artifacts="$1" deps_dir dylib_path
  deps_dir="$(dirname "$(printf '%s\n' "$ALIGN_TB_BINARIES" | sed -n "1s/.*$align_tb_tab//p")")"
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
}

# One process per binary, capped so a run does not oversubscribe a small CI
# runner, and libtest's own thread pool divided by that cap: without this each
# of N concurrent binaries would start N threads of its own, multiplying both
# the runnable thread count and peak RSS by the core count.
align_tb_configure_jobs() {
  local ncpu
  ncpu="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"
  case "$ncpu" in
    '' | *[!0-9]*) ncpu=4 ;;
  esac
  [ "$ncpu" -ge 1 ] || ncpu=1
  ALIGN_TB_JOBS="${ALIGN_GATE_JOBS:-$ncpu}"
  case "$ALIGN_TB_JOBS" in
    '' | *[!0-9]*) ALIGN_TB_JOBS="$ncpu" ;;
  esac
  [ "$ALIGN_TB_JOBS" -ge 1 ] || ALIGN_TB_JOBS=1
  ALIGN_TB_THREADS=$((ncpu / ALIGN_TB_JOBS))
  [ "$ALIGN_TB_THREADS" -ge 1 ] || ALIGN_TB_THREADS=1
  export RUST_TEST_THREADS="$ALIGN_TB_THREADS"
}

# On any exit, including an interrupted run, stop what is still running before
# removing the logs. The test binary is the tracked subshell's own child, so it
# has to be killed first — killing only the subshell would orphan it and leave
# it holding the machine. Anything the test binary itself spawned is beyond
# reach here; `pkill -P` is best-effort and absent hosts just skip it.
align_tb_cleanup() {
  local entry pid
  for entry in $ALIGN_TB_RUNNING; do
    pid="${entry%%:*}"
    pkill -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
  done
  [ -z "$ALIGN_TB_LOGS" ] || rm -rf "$ALIGN_TB_LOGS" 2>/dev/null || true
}

# Slot bookkeeping is "pid:slot" entries in one string; Bash 3.2 has no
# `wait -n`, and `jobs` is unspecified once this shell has run a command
# substitution, so the parent counts its own children explicitly. A child is
# done when it has recorded its result, and waiting on such a pid returns
# immediately — which reaps it without blocking behind a slower sibling.
#
# The liveness fallback matters: a child killed outright (the OOM killer, a
# test that signals its own parent) never records anything, and without it the
# admission loop below would wait on a slot that can never be freed. `kill -0`
# is a builtin, so this costs nothing per poll, and it can only ever conclude
# "gone" for a pid that really is gone — a still-running child always answers.
align_tb_reap_completed() {
  local remaining="" entry pid slot
  for entry in $ALIGN_TB_RUNNING; do
    pid="${entry%%:*}"
    slot="${entry#*:}"
    if [ -e "$ALIGN_TB_LOGS/$slot.status" ] || ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" 2>/dev/null || true
    else
      remaining="$remaining $entry"
    fi
  done
  ALIGN_TB_RUNNING="$remaining"
}

align_tb_count_running() {
  # shellcheck disable=SC2086
  set -- $ALIGN_TB_RUNNING
  echo $#
}

# Per-binary watchdog: wait out the budget, then mark and kill. It polls rather
# than sleeping the whole budget in one call so that cancelling it (the normal
# case, on every binary that finishes in time) leaves at most a few seconds of
# orphaned `sleep` behind even where `pkill` is unavailable.
#
# The kill reaches one level: the test binary and its immediate children. A
# driver test that spawned `alignc`, which in turn spawned a linker, can leave
# that grandchild running. On CI that is harmless — the runner is destroyed
# with the job — but a local run that actually hits the 900 s cap can leave an
# orphaned compiler process behind, which is worth knowing before reaching for
# a smaller cap on a laptop. Reaching further would mean a process group per
# binary, and killing a group risks the runner's own shell; the shallow kill is
# the deliberate trade, matching align_tb_cleanup.
align_tb_watchdog() {
  local child="$1" marker="$2" budget="$3" interval=5 waited=0
  [ "$budget" -ge "$interval" ] || interval="$budget"
  [ "$interval" -ge 1 ] || interval=1
  while [ "$waited" -lt "$budget" ]; do
    sleep "$interval"
    kill -0 "$child" 2>/dev/null || return 0
    waited=$((waited + interval))
  done
  : >"$marker"
  pkill -P "$child" 2>/dev/null || true
  kill -KILL "$child" 2>/dev/null || true
}

# Launch every discovered binary, at most ALIGN_TB_JOBS at a time, and return
# once all of them have recorded a result. $1 labels the progress lines.
align_tb_run() {
  local label="$1" manifest target_kind target_name executable name slot suffix
  while IFS="$align_tb_tab" read -r manifest target_kind target_name executable; do
    [ -n "$executable" ] || continue
    if [ "${ALIGN_TB_QUALIFIED_NAMES:-0}" = 1 ]; then
      name="$(basename "$(dirname "$manifest")")::$target_kind::$target_name"
    else
      name="$(basename "$executable")"
      name="${name%-*}"
    fi
    # Claim the log slot in the parent so a collision cannot silently drop a
    # binary's result. Which of two same-named binaries takes the "-2" label
    # follows cargo's artifact order and is therefore not stable across runs —
    # every caller rejects a duplicate name up front, so this only has to stay
    # lossless, not deterministic.
    slot="$name"
    suffix=2
    while [ -e "$ALIGN_TB_LOGS/$slot.log" ]; do
      slot="$name-$suffix"
      suffix=$((suffix + 1))
    done
    : >"$ALIGN_TB_LOGS/$slot.log"
    align_tb_reap_completed
    while [ "$(align_tb_count_running)" -ge "$ALIGN_TB_JOBS" ]; do
      sleep 0.2
      align_tb_reap_completed
    done
    # Announce before launching: if a binary hangs, the last "start" line
    # without a matching result names the suspect.
    printf '%s: start %s\n' "$label" "$slot"
    (
      cd "$(dirname "$manifest")"
      binary_status=0
      started="$(date +%s)"
      if [ "$ALIGN_TB_TIMEOUT" -gt 0 ]; then
        "$executable" >"$ALIGN_TB_LOGS/$slot.log" 2>&1 &
        child=$!
        # The watchdog's stderr is discarded, and so is this shell's own job
        # bookkeeping while the watchdog is torn down: bash announces a
        # signalled child ("Terminated: 15  sleep 5") on stderr, and cancelling
        # a watchdog is the normal case on every binary that finishes in time.
        align_tb_watchdog "$child" "$ALIGN_TB_LOGS/$slot.timeout" "$ALIGN_TB_TIMEOUT" 2>/dev/null &
        watchdog=$!
        wait "$child" 2>/dev/null || binary_status=$?
        {
          pkill -P "$watchdog" 2>/dev/null || true
          kill "$watchdog" 2>/dev/null || true
          wait "$watchdog" 2>/dev/null || true
        } 2>/dev/null
        # The watchdog decides to fire before it kills, so a binary that
        # finished in the same instant can end up with both a marker and a
        # clean exit. The exit code wins: a binary that reported success is
        # never turned red by the race.
        if [ "$binary_status" != 0 ] && [ -e "$ALIGN_TB_LOGS/$slot.timeout" ]; then
          binary_status=timeout
        fi
      else
        "$executable" >"$ALIGN_TB_LOGS/$slot.log" 2>&1 || binary_status=$?
      fi
      printf '%s %s\n' "$binary_status" "$(($(date +%s) - started))" >"$ALIGN_TB_LOGS/$slot.status"
    ) </dev/null &
    ALIGN_TB_RUNNING="$ALIGN_TB_RUNNING $!:$slot"
  done <<<"$ALIGN_TB_BINARIES"
  wait
  ALIGN_TB_RUNNING=""
}

# Slot names in one deterministic C-collation order.
align_tb_slots() {
  (cd "$ALIGN_TB_LOGS" && LC_ALL=C ls | LC_ALL=C sed -n 's/\.log$//p' | LC_ALL=C sort)
}

# "<status> <elapsed>" for a slot. A child that died before recording one
# reports "killed", never a pass.
align_tb_slot_record() {
  local record
  record="$(cat "$ALIGN_TB_LOGS/$1.status" 2>/dev/null || true)"
  if [ -z "$record" ]; then
    printf 'killed ?\n'
  else
    printf '%s\n' "$record"
  fi
}

# libtest names every failed test in the indented list under the final
# "failures:" heading, printed together with the summary once every test has
# finished. That list — not the per-test progress lines — is what gets parsed.
#
# The progress lines ("test name ... FAILED") are written while tests are
# still running, and the binary's stdout and stderr share one log: a stderr
# write from a concurrent test can splice into the middle of a progress line,
# losing the name and turning a named failure into a phantom <binary-exit-101>
# (observed on the first nightly). The trailing list is written after every
# test has finished, so a splice there takes a straggler thread — much rarer,
# but not impossible, and a lost name must not let a failure hide. libtest's
# own summary line carries the failed count, so a list that does not account
# for every counted failure contributes the synthetic <failure-list-unparsed>,
# which no manifest line can match: a mangled list can only turn the run red,
# never absorb a failure. A count-preserving splice (one that garbles a name
# without changing the line count) still yields a name no manifest lists —
# red again, from the other direction.
#
# Each "failures:" heading resets the collected block, so the first heading
# (the per-test detail sections) is superseded by the final name list, and
# only a block confirmed by a "test result:" summary counts. Detail-section
# content staying out of the names rests on the shape libtest prints (the
# sections open with an unindented "---- name ----" banner, which stops
# collection); a test's own replayed output could still fabricate an indented
# line under a fabricated heading just before the summary. That replay exists
# only when something already failed, so the error is a spurious extra name
# inside an already-red run, never a missed failure. Parsing libtest's JSON
# output would remove even that, but it is nightly-only, so the fail-closed
# direction is the right trade here.
align_tb_failed_tests() {
  LC_ALL=C awk '
    /^failures:$/ { block = ""; count = 0; collecting = 1; next }
    collecting && /^    [^ ]/ { block = block substr($0, 5) "\n"; count++; next }
    collecting && /^$/ { next }
    collecting { collecting = 0 }
    /^test result:/ {
      names = block
      named = count
      failed = 0
      if (match($0, /[0-9]+ failed/)) failed = substr($0, RSTART, RLENGTH) + 0
      confirmed = 1
      collecting = 0
    }
    END {
      printf "%s", names
      if (confirmed && failed != named) print "<failure-list-unparsed>"
    }
  ' "$ALIGN_TB_LOGS/$1.log"
}

# Every binary's output, whichever of them failed. Returns 1 if any did.
align_tb_report_all() {
  local status=0 slot record binary_status elapsed
  for slot in $(align_tb_slots); do
    record="$(align_tb_slot_record "$slot")"
    binary_status="${record%% *}"
    elapsed="${record#* }"
    printf -- '--- %s (exit %s, %ss) ---\n' "$slot" "$binary_status" "$elapsed"
    cat "$ALIGN_TB_LOGS/$slot.log"
    [ "$binary_status" = 0 ] || status=1
  done
  return "$status"
}
