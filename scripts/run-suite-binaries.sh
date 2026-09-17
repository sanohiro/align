#!/usr/bin/env bash
# Run the ENTIRE workspace test suite concurrently and judge it against the
# known-failure manifest. This is what the nightly job runs, and running it
# with no arguments reproduces that judgement locally.
#
#   usage: scripts/run-suite-binaries.sh [ARTIFACT_JSON]
#
# With no argument the script builds the workspace (for the runtime staticlib
# the driver tests link against) and then performs the `cargo test --no-run`
# build itself. ARTIFACT_JSON, the stdout of such a test build, is accepted so
# scripts/test-pr-workflow.sh can exercise every branch against fixtures
# without compiling anything.
#
# Three environment knobs shape the run. All three default to the whole suite
# on one host with no budget, so a bare local invocation still reproduces the
# nightly's complete judgement even though the nightly itself is sharded:
#
#   ALIGN_SUITE_SHARDS     how many hosts share the suite (default 1)
#   ALIGN_SUITE_SHARD      which one this is, 1..ALIGN_SUITE_SHARDS (default 1)
#   ALIGN_SUITE_DEADLINE   whole-run wall-clock budget in seconds (default 0,
#                          meaning unbounded)
#
# Sharding is a partition of the RUN set only. Discovery, the target-identity
# collision check, and the "manifest names a target the workspace does not
# build" check all still see every workspace binary in every shard, so no
# manifest invariant weakens when the suite is spread out. Only the two-way
# failure diff narrows, to the targets this shard actually ran.
#
# Why not `cargo test --workspace`: cargo runs test binaries one at a time and
# stops the whole run at the first failing binary, so the previous nightly job
# died 4-10 minutes in on the first known failure and reported nothing about
# the other ~220 targets. Every night was red and nothing was being detected.
# Building once and running the compiled binaries here (the shape
# scripts/run-gate-binaries.sh already shipped) turns the serial sum into
# roughly the longest binary, and the manifest below turns "known red" into a
# green baseline that a NEW failure can stand out against.
#
# The judgement is a two-way diff against scripts/known-failures.txt:
#
#   a failure that is not in the manifest   -> red, named
#   a manifest failure that now passes      -> red ("delete the line")
#   exactly the manifest                    -> green
#
# The second direction is deliberate ratchet pressure: a fixed test may not sit
# in the manifest quietly re-earning its exemption.
#
# Any test run that needs more than 30 minutes is worthless as a detector, so
# the nightly job carries `timeout-minutes: 30` and this script caps each
# individual binary (ALIGN_SUITE_BINARY_TIMEOUT, default 900s) — one hung
# binary must not cost the report on all the others.
#
# The per-binary cap alone was not enough. Between 2026-08-29 and 2026-09-17
# the workspace grew past what one four-core runner can execute in that budget
# (262 binaries; 7 of them completed in the 25 minutes the job had left after
# building), the job-level `timeout-minutes: 30` cancelled it, and a cancelled
# job reports nothing — nineteen consecutive nights with no verdict at all. So
# the budget is now enforced from inside as well: ALIGN_SUITE_DEADLINE stops
# the run, prints every binary's elapsed time, and exits non-zero with a named
# overrun BEFORE the job-level cap can cancel the job and take the report with
# it. The budget itself is not the thing to raise; the shard count is.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

manifest="${ALIGN_KNOWN_FAILURES:-$repo_root/scripts/known-failures.txt}"
[ -f "$manifest" ] || {
  echo "known-failure manifest not found: $manifest" >&2
  exit 2
}

# shellcheck source=scripts/dyld-env.sh
. "$script_dir/dyld-env.sh"
# shellcheck source=scripts/test-binaries-lib.sh
. "$script_dir/test-binaries-lib.sh"
align_use_private_dyld_region

suite_started_at="$(date +%s)"

ALIGN_TB_TIMEOUT="${ALIGN_SUITE_BINARY_TIMEOUT:-900}"
case "$ALIGN_TB_TIMEOUT" in
  '' | *[!0-9]*)
    echo "ALIGN_SUITE_BINARY_TIMEOUT must be a whole number of seconds" >&2
    exit 2
    ;;
esac

# Every malformed knob is a configuration error (exit 2), not a verdict, and is
# rejected before anything is built.
suite_shards="${ALIGN_SUITE_SHARDS:-1}"
case "$suite_shards" in
  '' | *[!0-9]* | 0)
    echo "ALIGN_SUITE_SHARDS must be a positive whole number" >&2
    exit 2
    ;;
esac
suite_shard="${ALIGN_SUITE_SHARD:-1}"
case "$suite_shard" in
  '' | *[!0-9]* | 0)
    echo "ALIGN_SUITE_SHARD must be a positive whole number" >&2
    exit 2
    ;;
esac
[ "$suite_shard" -le "$suite_shards" ] || {
  echo "ALIGN_SUITE_SHARD ($suite_shard) exceeds ALIGN_SUITE_SHARDS ($suite_shards)" >&2
  exit 2
}

# The deadline covers the builds too. A night that spends its whole budget
# compiling has also failed to detect anything, and saying so with the build
# timings is more useful than letting the run loop start with no time left.
suite_deadline="${ALIGN_SUITE_DEADLINE:-0}"
case "$suite_deadline" in
  '' | *[!0-9]*)
    echo "ALIGN_SUITE_DEADLINE must be a whole number of seconds" >&2
    exit 2
    ;;
esac
if [ "$suite_deadline" -gt 0 ]; then
  ALIGN_TB_DEADLINE=$((suite_started_at + suite_deadline))
else
  ALIGN_TB_DEADLINE=0
fi

work="$(mktemp -d)"
ALIGN_TB_LOGS="$work/logs"
mkdir -p "$ALIGN_TB_LOGS"
suite_cleanup() {
  align_tb_cleanup
  rm -rf "$work" 2>/dev/null || true
}
trap 'suite_cleanup' EXIT
trap 'align_tb_report_interrupted suite INT; suite_cleanup; exit 130' INT
trap 'align_tb_report_interrupted suite TERM; suite_cleanup; exit 143' TERM

tab="$align_tb_tab"

# Parse the manifest before spending anything on a build: every malformed-
# manifest outcome is a configuration error (exit 2) either way, and it should
# cost seconds, not a workspace build.
#
# Manifest lines are "<package-dir>::<kind>::<target><TAB><test>" with an optional
# third field. The only third field is "env": a test whose outcome depends on
# an environment variable, a network, or a service that the nightly runner
# does not provide. Those run, and pass or fail without changing the verdict.
# Everything else is strict in both directions.
expected="$work/expected"
env_dependent="$work/env"
: >"$expected"
: >"$env_dependent"
line_number=0
while IFS= read -r line || [ -n "$line" ]; do
  line_number=$((line_number + 1))
  case "$line" in
    '' | '#'*) continue ;;
  esac
  case "$line" in
    *"$tab"*) ;;
    *)
      echo "$manifest:$line_number: fields must be TAB-separated: $line" >&2
      exit 2
      ;;
  esac
  target="${line%%"$tab"*}"
  rest="${line#*"$tab"}"
  test_name="${rest%%"$tab"*}"
  case "$target" in
    ?*::?*::?*) ;;
    *)
      echo "$manifest:$line_number: target must be package-dir::kind::target: $target" >&2
      exit 2
      ;;
  esac
  if [ "$rest" = "$test_name" ]; then
    kind=fail
  else
    kind="${rest#*"$tab"}"
  fi
  case "$kind" in
    fail) printf '%s%s%s\n' "$target" "$tab" "$test_name" >>"$expected" ;;
    env) printf '%s%s%s\n' "$target" "$tab" "$test_name" >>"$env_dependent" ;;
    *)
      echo "$manifest:$line_number: unknown third field '$kind' (expected 'env')" >&2
      exit 2
      ;;
  esac
done <"$manifest"
LC_ALL=C sort -o "$expected" "$expected"
LC_ALL=C sort -o "$env_dependent" "$env_dependent"

# One key cannot be both strict and tolerated: the two rules contradict each
# other, and whichever won would be an accident of ordering. That is a broken
# manifest, not a verdict.
conflicting="$(LC_ALL=C comm -12 "$expected" "$env_dependent")"
[ -z "$conflicting" ] || {
  echo "$manifest: entries listed both as a known failure and as 'env':" >&2
  printf '%s\n' "$conflicting" | while IFS= read -r entry; do
    printf '  %s\n' "$entry" >&2
  done
  echo "  keep one line per test: strict, or environment-dependent" >&2
  exit 2
}

# Run one build phase under the whole-run deadline.
#
# The deadline is useless if it can only be observed between phases: a cold
# cache or a stalled build would consume the entire job budget, the job-level
# cap would cancel the job, and the night would end in exactly the silent
# cancellation this deadline exists to replace — with the cache save skipped
# too, guaranteeing the next night starts just as cold. So the budget is
# checked WHILE a build runs, not only after it returns.
#
# The child records its own status through a marker file because Bash 3.2 has
# no `wait -n` and `kill -0` cannot distinguish a live child from a zombie one.
# `set +e` inside the subshell is required: the whole point is to capture a
# failing status rather than let `set -e` abort before the marker is written.
#
# The kill reaches one level, the same reach and the same deliberate limit as
# the per-binary watchdog: run-quiet.sh dies and cargo below it may not. On CI
# the runner is destroyed with the job, and locally the named failure and a
# stray compiler are still strictly better than a cancelled job that reports
# nothing.
suite_run_phase() {
  local phase="$1" marker="$work/phase.status" child status
  shift
  if [ "$ALIGN_TB_DEADLINE" -le 0 ]; then
    "$@"
    return
  fi
  rm -f "$marker"
  (
    set +e
    "$@"
    printf '%s\n' "$?" >"$marker"
  ) &
  child=$!
  while [ ! -e "$marker" ]; do
    if [ "$(date +%s)" -ge "$ALIGN_TB_DEADLINE" ]; then
      pkill -P "$child" 2>/dev/null || true
      kill -KILL "$child" 2>/dev/null || true
      wait "$child" 2>/dev/null || true
      echo "suite: BUDGET EXCEEDED during $phase" >&2
      printf 'suite:   %ss elapsed of a %ss whole-run budget; no binary has run\n' \
        "$(($(date +%s) - suite_started_at))" "$suite_deadline" >&2
      echo "  cut build cost or raise ALIGN_SUITE_SHARDS; the budget is not the number to raise" >&2
      exit 1
    fi
    sleep 1
  done
  wait "$child" 2>/dev/null || true
  status="$(cat "$marker")"
  rm -f "$marker"
  return "$status"
}

if [ $# -ge 1 ]; then
  artifacts="$1"
else
  artifacts="$work/artifacts.json"
  # `cargo test --no-run` builds only what the test binaries link against
  # directly; it does not produce the alignc runtime staticlib that driver
  # tests hand to the linker at RUN time. The first nightly proved the hole:
  # 33 targets failed together solely because target/debug/libalign_runtime.a
  # did not exist. Build the workspace first, and fail closed — as a
  # configuration error, not a verdict — if the runtime library still is not
  # there afterwards.
  #
  # Two invocations resolve features twice under resolver 2. Deliberately left
  # that way for now: whether merging them into one `--all-targets`-style
  # build is worth it gets decided from the first fixed nightly's wall-clock,
  # not guessed here.
  suite_run_phase "the workspace build" \
    "$script_dir/run-quiet.sh" "suite build: workspace" -- \
    "$script_dir/cargo.sh" build --workspace --locked
  # Where the artifacts landed comes from cargo itself: CARGO_TARGET_DIR alone
  # would miss CARGO_BUILD_TARGET_DIR and any .cargo/config override.
  target_dir="$("$script_dir/cargo.sh" metadata --format-version 1 --no-deps \
    --manifest-path "$repo_root/Cargo.toml" |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
  [ -n "$target_dir" ] || {
    echo "suite: cargo metadata did not report a target directory" >&2
    exit 2
  }
  # Existence only, deliberately: freshness is the compile driver's own
  # digest check downstream.
  runtime_lib="$target_dir/debug/libalign_runtime.a"
  [ -f "$runtime_lib" ] || {
    echo "suite: workspace build did not produce $runtime_lib" >&2
    exit 2
  }
  suite_run_phase "the test-binary build" \
    "$script_dir/run-quiet.sh" --stdout "$artifacts" \
    "suite build: test binaries" -- \
    "$script_dir/cargo.sh" test --no-run --workspace --locked \
    --message-format=json-render-diagnostics
fi

align_tb_discover "$artifacts" || {
  echo "the suite build produced no test binaries (cargo artifact format changed?)" >&2
  exit 1
}

# The four-core nightly runner has enough aggregate CPU for the full suite but
# can still miss its 30-minute budget when Cargo's artifact order leaves large
# generated-program owners at the tail. Give the measured long runners a
# coarse longest-first rank before admission. Every discovered binary still
# runs exactly once, and verdict identity remains package/kind/target below.
#
# Equal ranks break by the stable package/kind/name identity, NOT by Cargo's
# artifact ordinal. Cargo emits artifacts in completion order, which varies
# with parallel compilation, so the ordinal is not reproducible between two
# builds of the same commit. That is only a cosmetic difference for a single
# host, but each shard builds on its own runner and partitions this list
# independently: a tie broken differently on two runners moves a binary from
# one shard to another, so some targets would run twice and others not at all,
# with every shard still green. The identity is a total order (the collision
# check below proves it), so every runner computes the same partition.
#
# Ranks come from the 2026-08-29 four-core constrained full-suite run, with
# pkg_db_vc1 and pkg_db_q5a added from the required db-postgres shard timings
# of run 34833186128 — they are as expensive as their shard-mates and were the
# only pkg.db owners left unranked, so they were landing in the unranked bulk
# and unbalancing whichever shard drew them. Only relative order matters;
# refresh the list when the nightly timing table shows a different tail, rather
# than raising either timeout.
align_suite_priority_score() {
  case "$1" in
    align_driver::test::pkg_db_a1) echo 100 ;;
    align_driver::test::pkg_db_q4b) echo 99 ;;
    align_driver::test::pkg_db_vc1) echo 96 ;;
    align_driver::test::pkg_db_q5b2) echo 95 ;;
    align_driver::test::pkg_db_q3) echo 94 ;;
    align_driver::test::pkg_db_q1) echo 93 ;;
    align_driver::test::deep_type_graphs) echo 92 ;;
    align_driver::test::pkg_db_q4a) echo 85 ;;
    align_driver::test::pkg_db_q2) echo 80 ;;
    align_driver::test::pkg_db_callbacks) echo 75 ;;
    align_driver::test::pkg_db_pool) echo 72 ;;
    align_driver::test::pkg_db_q5a) echo 71 ;;
    align_driver::test::apps_web_validate) echo 70 ;;
    align_driver::test::apps_ws) echo 70 ;;
    align_driver::test::pkg_db_a2) echo 60 ;;
    align_driver::test::pkg_db_q6) echo 59 ;;
    align_driver::test::pkg_db_q5b1) echo 55 ;;
    align_driver::test::fuzz_differential) echo 52 ;;
    align_mir::lib::align_mir) echo 50 ;;
    align_driver::test::inprocess_memo) echo 45 ;;
    align_driver::test::apps_web_router) echo 43 ;;
    align_driver::test::apps_web_multipart) echo 40 ;;
    align_codegen_llvm::lib::align_codegen_llvm) echo 38 ;;
    align_driver::test::m5) echo 35 ;;
    *) echo 0 ;;
  esac
}

align_suite_order_binaries() {
  local manifest target_kind target_name executable package identity score
  printf '%s\n' "$ALIGN_TB_BINARIES" |
    while IFS="$align_tb_tab" read -r manifest target_kind target_name executable; do
      [ -n "$executable" ] || continue
      package="$(basename "$(dirname "$manifest")")"
      identity="$package::$target_kind::$target_name"
      score="$(align_suite_priority_score "$identity")"
      printf '%03d%s%s%s%s%s%s%s%s%s%s\n' \
        "$score" "$align_tb_tab" "$identity" "$align_tb_tab" \
        "$manifest" "$align_tb_tab" "$target_kind" "$align_tb_tab" \
        "$target_name" "$align_tb_tab" "$executable"
    done |
    LC_ALL=C sort -t "$align_tb_tab" -k1,1nr -k2,2 |
    cut -f3-
}

ALIGN_TB_BINARIES="$(align_suite_order_binaries)"

# Round-robin over that longest-first order. Taking every ALIGN_SUITE_SHARDS-th
# entry hands each shard a comparable share of the measured long runners and of
# the unranked bulk behind them, which is what keeps the shard makespans close
# without inventing per-target durations the suite does not measure. It is a
# pure function of the ordered list, so the partition is identical on every
# shard and every night, and each binary belongs to exactly one shard.
align_suite_select_shard() {
  local ordinal=0 line
  printf '%s\n' "$ALIGN_TB_BINARIES" |
    while IFS= read -r line; do
      [ -n "$line" ] || continue
      if [ "$((ordinal % suite_shards + 1))" -eq "$suite_shard" ]; then
        printf '%s\n' "$line"
      fi
      ordinal=$((ordinal + 1))
    done
}

# Cargo's executable basenames are not identities: `align-repl` and
# `align_repl` normalize to the same `align_repl-<hash>`, and separate packages
# may reuse a target name. Key the manifest by package directory, target kind,
# and Cargo target name. None contains Cargo's unstable artifact hash.
#
# This set is the WHOLE workspace even when this process runs one shard: the
# collision check and the unknown-entry check below are manifest-wide
# invariants, and a shard that could only see its own targets would report a
# perfectly good manifest line as naming a target that does not exist.
ALIGN_TB_QUALIFIED_NAMES=1
all_names="$(align_tb_qualified_names | LC_ALL=C sort)"
duplicates="$(printf '%s\n' "$all_names" | LC_ALL=C uniq -d)"
[ -z "$duplicates" ] || {
  echo "two workspace test targets share a package/kind/name identity:" >&2
  printf '  %s\n' $duplicates >&2
  echo "  give the targets distinct Cargo identities" >&2
  exit 1
}

if [ "$suite_shards" -gt 1 ]; then
  ALIGN_TB_BINARIES="$(align_suite_select_shard)"
  [ -n "$ALIGN_TB_BINARIES" ] || {
    echo "suite: shard $suite_shard of $suite_shards selected no binaries" >&2
    echo "  more shards than the workspace has test targets" >&2
    exit 2
  }
  found_names="$(align_tb_qualified_names | LC_ALL=C sort)"
else
  # Unsharded, the run set IS the workspace. Deriving it again would spend two
  # more subshells per binary to arrive at the same list.
  found_names="$all_names"
fi

align_tb_export_dylib_path "$artifacts"
align_tb_configure_jobs

# A manifest line naming a target the workspace no longer builds is red for
# both kinds. A strict line can never be satisfied, and an env line is worse:
# it would sit there indefinitely excusing a test that does not exist.
unknown="$work/unknown"
: >"$unknown"
cat "$expected" "$env_dependent" | while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  printf '%s\n' "$all_names" | LC_ALL=C grep -qxF "${entry%%"$tab"*}" ||
    printf '%s\n' "$entry" >>"$unknown"
done
LC_ALL=C sort -o "$unknown" "$unknown"

# Now narrow both expectations to what this shard actually runs. Without this a
# shard would report every OTHER shard's known failures as repaired, which is
# the "delete the line" ratchet firing on a test it never executed. Entries
# removed here are not lost: the shard that owns the target applies them, and
# an entry naming no target at all was already collected into `unknown` above,
# from the whole-workspace set, by every shard.
align_suite_restrict_to_shard() {
  local source="$1" entry
  : >"$source.shard"
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    if printf '%s\n' "$found_names" | LC_ALL=C grep -qxF "${entry%%"$tab"*}"; then
      printf '%s\n' "$entry" >>"$source.shard"
    fi
  done <"$source"
  mv "$source.shard" "$source"
}
if [ "$suite_shards" -gt 1 ]; then
  align_suite_restrict_to_shard "$expected"
  align_suite_restrict_to_shard "$env_dependent"
fi

printf 'suite: %s binaries, %s parallel, %s test thread(s) each, %ss per-binary cap\n' \
  "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" \
  "$ALIGN_TB_JOBS" "$ALIGN_TB_THREADS" "$ALIGN_TB_TIMEOUT"
if [ "$suite_shards" -gt 1 ]; then
  if [ "$suite_deadline" -gt 0 ]; then
    suite_budget_note="${suite_deadline}s whole-run budget"
  else
    suite_budget_note="no whole-run budget"
  fi
  printf 'suite: shard %s of %s of %s workspace binaries, %s\n' \
    "$suite_shard" "$suite_shards" \
    "$(printf '%s\n' "$all_names" | wc -l | tr -d '[:space:]')" "$suite_budget_note"
fi
printf 'suite: manifest %s (%s known, %s environment-dependent)\n' \
  "$manifest" "$(wc -l <"$expected" | tr -d '[:space:]')" \
  "$(wc -l <"$env_dependent" | tr -d '[:space:]')"

# A budget that is already spent before the first binary starts is the same
# failure as one spent halfway through, and the build timings run-quiet.sh
# already printed are the whole diagnosis. Say so rather than starting a run
# loop that would be abandoned on its first poll.
if [ "$ALIGN_TB_DEADLINE" -gt 0 ] && [ "$(date +%s)" -ge "$ALIGN_TB_DEADLINE" ]; then
  echo "suite: BUDGET EXCEEDED before any binary ran" >&2
  printf 'suite:   the %ss budget was consumed by the build phase alone\n' \
    "$suite_deadline" >&2
  echo "  cut build cost or raise ALIGN_SUITE_SHARDS; the budget is not the number to raise" >&2
  exit 1
fi

suite_started="$(date +%s)"
align_tb_run suite
suite_elapsed="$(($(date +%s) - suite_started))"

# Slowest-first, one line per binary. Nothing printed this before, which is
# exactly why nineteen cancelled nights could not be diagnosed from their own
# transcripts. LIMIT 0 prints every binary.
align_suite_timing_table() {
  local limit="$1" slot record binary_status elapsed
  for slot in $(align_tb_slots); do
    record="$(align_tb_slot_record "$slot")"
    binary_status="${record%% *}"
    elapsed="${record#* }"
    case "$elapsed" in
      '' | *[!0-9]*) elapsed=0 ;;
    esac
    printf '%s%s%s%s%s\n' "$elapsed" "$tab" "$slot" "$tab" "$binary_status"
  done | LC_ALL=C sort -t "$tab" -k1,1nr |
    LC_ALL=C awk -F"$tab" -v n="$limit" \
      'n == 0 || NR <= n { printf "suite:   %6ds  %s (exit %s)\n", $1, $2, $3 }'
}

# The slowest binary as "<target> <elapsed>s", for the one-line summary.
align_suite_slowest() {
  align_suite_timing_table 1 |
    LC_ALL=C awk '{ printf "%s %s", $3, $2 }'
}

# The overrun report is the point of the internal deadline: a cancelled job
# prints nothing at all, so the elapsed table for EVERY binary — including the
# ones still running when the budget ran out, which the parent timed from its
# own launch record — goes out before the non-zero exit.
#
# No manifest verdict is produced. Most of the shard never ran, so the two-way
# diff would report every unreached known failure as repaired: a "delete the
# line" ratchet firing on tests nothing executed. An incomplete run has one
# honest result, and this is it.
if [ "$ALIGN_TB_DEADLINE_HIT" -eq 1 ]; then
  launched="$(align_tb_slots | LC_ALL=C wc -l | tr -d '[:space:]')"
  abandoned=0
  for slot in $(align_tb_slots); do
    record="$(align_tb_slot_record "$slot")"
    [ "${record%% *}" != deadline ] || abandoned=$((abandoned + 1))
  done
  echo "suite: BUDGET EXCEEDED after ${suite_elapsed}s of a ${suite_deadline}s whole-run budget" >&2
  printf 'suite:   %s of %s binaries launched, %s finished, %s abandoned mid-run\n' \
    "$launched" "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" \
    "$((launched - abandoned))" "$abandoned" >&2
  align_suite_timing_table 0 >&2
  echo "  raise ALIGN_SUITE_SHARDS or cut test cost; the budget is not the number to raise" >&2
  exit 1
fi

# One "<package-dir>::<kind>::<target><TAB><test>" line per observed failure. A binary that never
# printed libtest's own summary (it crashed, aborted, or hit the per-binary
# cap) contributes a synthetic entry instead of silently looking clean, and so
# does one whose exit code cannot be explained by the tests it named.
#
# 101 is libtest's own "some test failed" code and 0 is success; anything else
# after a printed summary means the process died some other way — a segfault
# or abort in a destructor, an exit() in a leaked thread — and it is added
# ALONGSIDE the named failures, not only when there are none. Those names may
# all be in the manifest, and without this line a crash that happens to strike
# a known-failing target would be absorbed into the baseline and never seen.
actual="$work/actual"
synthetic="$work/synthetic-targets"
: >"$actual"
: >"$synthetic"
for slot in $(align_tb_slots); do
  record="$(align_tb_slot_record "$slot")"
  binary_status="${record%% *}"
  names="$(align_tb_failed_tests "$slot")"
  if ! grep -q '^test result:' "$ALIGN_TB_LOGS/$slot.log"; then
    names="$names${names:+
}<binary-did-not-report>"
  else
    case "$binary_status" in
      0 | timeout) ;;
      101) [ -n "$names" ] || names="<binary-exit-101>" ;;
      *)
        names="$names${names:+
}<binary-exit-$binary_status>"
        ;;
    esac
  fi
  [ -z "$names" ] || printf '%s\n' "$names" |
    while IFS= read -r name; do
      [ -n "$name" ] || continue
      printf '%s%s%s\n' "$slot" "$tab" "$name" >>"$actual"
      # Any synthetic marker means this target's real outcome is unknown: it
      # crashed, hung, or its failure list could not be parsed.
      case "$name" in
        '<'*) printf '%s\n' "$slot" >>"$synthetic" ;;
      esac
    done
done
LC_ALL=C sort -o "$actual" "$actual"

# Environment-dependent entries are removed from the comparison in both
# directions before the strict diff.
tolerated="$work/tolerated"
LC_ALL=C comm -23 "$actual" "$env_dependent" >"$tolerated"
new_failures="$(LC_ALL=C comm -23 "$tolerated" "$expected")"
# A strict entry whose target does not exist is also, trivially, an entry that
# did not fail. Report it once, under the heading that explains it.
#
# A target that contributed a synthetic marker never reported a trustworthy
# outcome, so its manifest entries are not declared repaired: "delete the
# line" for a test that crashed, hung, or lost its failure list would be the
# ratchet pointing the wrong way. Those entries stay, and the synthetic
# marker itself already turned the run red.
fixed_all="$work/fixed"
LC_ALL=C comm -13 "$actual" "$expected" >"$fixed_all"
fixed="$(LC_ALL=C comm -23 "$fixed_all" "$unknown" |
  while IFS= read -r entry; do
    LC_ALL=C grep -qxF "${entry%%"$tab"*}" "$synthetic" ||
      printf '%s\n' "$entry"
  done)"

# Full output for every binary that produced an unexpected failure. Successful,
# known-failing, and environment-dependent binaries stay captured but silent;
# ALIGN_TB_VERBOSE=1 restores the per-binary headers and summaries.
noisy="$work/noisy"
printf '%s\n' "$new_failures" | LC_ALL=C sed -n "s/$tab.*//p" | LC_ALL=C sort -u >"$noisy"
for slot in $(align_tb_slots); do
  record="$(align_tb_slot_record "$slot")"
  binary_status="${record%% *}"
  elapsed="${record#* }"
  if [ "$ALIGN_TB_VERBOSE" -eq 1 ]; then
    printf -- '--- %s (exit %s, %ss) ---\n' "$slot" "$binary_status" "$elapsed"
    if LC_ALL=C grep -qxF "$slot" "$noisy"; then
      cat "$ALIGN_TB_LOGS/$slot.log"
    else
      LC_ALL=C grep -E '^test result:' "$ALIGN_TB_LOGS/$slot.log" || true
    fi
  elif LC_ALL=C grep -qxF "$slot" "$noisy"; then
    printf -- '--- %s (exit %s, %ss) ---\n' "$slot" "$binary_status" "$elapsed" >&2
    cat "$ALIGN_TB_LOGS/$slot.log" >&2
  fi
done

# The makespan of a run this shape is its slowest binary, so naming that one
# binary on the summary line is the whole tuning signal, and it costs no extra
# line in a routine green transcript. ALIGN_TB_VERBOSE=1 prints the full table.
printf 'suite: ran %s binaries in %ss (slowest %s)\n' \
  "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" "$suite_elapsed" \
  "$(align_suite_slowest)"
if [ "$ALIGN_TB_VERBOSE" -eq 1 ]; then
  align_suite_timing_table 0
fi

status=0
if [ -n "$new_failures" ]; then
  status=1
  echo "suite: NEW failures, absent from $manifest:" >&2
  printf '%s\n' "$new_failures" | while IFS= read -r entry; do
    printf '  %s\n' "$entry" >&2
  done
fi
if [ -n "$fixed" ]; then
  status=1
  echo "suite: manifest entries that did NOT fail — fix the manifest, not the test:" >&2
  printf '%s\n' "$fixed" | while IFS= read -r entry; do
    printf '  %s (passed)\n' "$entry" >&2
  done
  echo "  a fixed test must lose its manifest line in the same change that fixes it" >&2
fi
if [ -s "$unknown" ]; then
  status=1
  echo "suite: manifest entries naming a target the workspace does not build:" >&2
  while IFS= read -r entry; do
    printf '  %s (no such target in the workspace)\n' "$entry" >&2
  done <"$unknown"
  echo "  this holds for an 'env' line too: it cannot excuse a test that is gone" >&2
fi
if [ "$status" -eq 0 ]; then
  printf 'suite: matches %s exactly\n' "$manifest"
fi
exit "$status"
