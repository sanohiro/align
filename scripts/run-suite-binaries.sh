#!/usr/bin/env bash
# Run the ENTIRE workspace test suite concurrently and judge it against the
# known-failure manifest. This is what the nightly job runs, and running it
# with no arguments reproduces that judgement locally.
#
#   usage: scripts/run-suite-binaries.sh [ARTIFACT_JSON]
#
# With no argument the script performs the `cargo test --no-run` build itself.
# ARTIFACT_JSON, the stdout of such a build, is accepted so
# scripts/test-pr-workflow.sh can exercise every branch against fixtures
# without compiling anything.
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

ALIGN_TB_TIMEOUT="${ALIGN_SUITE_BINARY_TIMEOUT:-900}"
case "$ALIGN_TB_TIMEOUT" in
  '' | *[!0-9]*)
    echo "ALIGN_SUITE_BINARY_TIMEOUT must be a whole number of seconds" >&2
    exit 2
    ;;
esac

work="$(mktemp -d)"
ALIGN_TB_LOGS="$work/logs"
mkdir -p "$ALIGN_TB_LOGS"
suite_cleanup() {
  align_tb_cleanup
  rm -rf "$work" 2>/dev/null || true
}
trap 'suite_cleanup' EXIT
trap 'suite_cleanup; exit 130' INT
trap 'suite_cleanup; exit 143' TERM

if [ $# -ge 1 ]; then
  artifacts="$1"
else
  artifacts="$work/artifacts.json"
  echo "suite: building every workspace test binary"
  "$script_dir/cargo.sh" test --no-run --workspace --locked \
    --message-format=json-render-diagnostics >"$artifacts"
fi

align_tb_discover "$artifacts" || {
  echo "the suite build produced no test binaries (cargo artifact format changed?)" >&2
  exit 1
}

# The manifest keys failures by target name, so two packages holding a
# same-named target would make a manifest line ambiguous. Cargo's own -<hash>
# disambiguation is not stable across runs, so reject the collision instead of
# silently binding the line to whichever one ran first.
found_names="$(align_tb_names | LC_ALL=C sort)"
duplicates="$(printf '%s\n' "$found_names" | LC_ALL=C uniq -d)"
[ -z "$duplicates" ] || {
  echo "two workspace test targets share a name, so the manifest cannot key on it:" >&2
  printf '  %s\n' $duplicates >&2
  echo "  rename one of them, or teach the manifest a package-qualified key" >&2
  exit 1
}

align_tb_export_dylib_path "$artifacts"
align_tb_configure_jobs

tab="$align_tb_tab"

# Manifest lines are "<target><TAB><test>" with an optional third field. The
# only third field is "env": a test whose outcome depends on an environment
# variable, a network, or a service that the nightly runner does not provide.
# Those run, and pass or fail without changing the verdict. Everything else is
# strict in both directions.
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

printf 'suite: %s binaries, %s parallel, %s test thread(s) each, %ss per-binary cap\n' \
  "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" \
  "$ALIGN_TB_JOBS" "$ALIGN_TB_THREADS" "$ALIGN_TB_TIMEOUT"
printf 'suite: manifest %s (%s known, %s environment-dependent)\n' \
  "$manifest" "$(wc -l <"$expected" | tr -d '[:space:]')" \
  "$(wc -l <"$env_dependent" | tr -d '[:space:]')"

suite_started="$(date +%s)"
align_tb_run suite
suite_elapsed="$(($(date +%s) - suite_started))"

# One "<target><TAB><test>" line per observed failure. A binary that never
# printed libtest's own summary (it crashed, aborted, or hit the per-binary
# cap) contributes a synthetic entry instead of silently looking clean, and a
# binary that exited non-zero without naming a test contributes one too.
actual="$work/actual"
: >"$actual"
for slot in $(align_tb_slots); do
  record="$(align_tb_slot_record "$slot")"
  binary_status="${record%% *}"
  names="$(align_tb_failed_tests "$slot")"
  if ! grep -q '^test result:' "$ALIGN_TB_LOGS/$slot.log"; then
    names="$names${names:+
}<binary-did-not-report>"
  elif [ "$binary_status" != 0 ] && [ -z "$names" ]; then
    names="$names<binary-exit-$binary_status>"
  fi
  [ -z "$names" ] || printf '%s\n' "$names" |
    while IFS= read -r name; do
      [ -n "$name" ] || continue
      printf '%s%s%s\n' "$slot" "$tab" "$name" >>"$actual"
    done
done
LC_ALL=C sort -o "$actual" "$actual"

# Environment-dependent entries are removed from the comparison in both
# directions before the strict diff.
tolerated="$work/tolerated"
LC_ALL=C comm -23 "$actual" "$env_dependent" >"$tolerated"
new_failures="$(LC_ALL=C comm -23 "$tolerated" "$expected")"
fixed="$(LC_ALL=C comm -13 "$actual" "$expected")"

# Full output for a binary that produced an unexpected failure; one line for
# everything else, so the whole run stays auditable without burying the signal.
noisy="$work/noisy"
printf '%s\n' "$new_failures" | LC_ALL=C sed -n "s/$tab.*//p" | LC_ALL=C sort -u >"$noisy"
for slot in $(align_tb_slots); do
  record="$(align_tb_slot_record "$slot")"
  binary_status="${record%% *}"
  elapsed="${record#* }"
  printf -- '--- %s (exit %s, %ss) ---\n' "$slot" "$binary_status" "$elapsed"
  if LC_ALL=C grep -qxF "$slot" "$noisy"; then
    cat "$ALIGN_TB_LOGS/$slot.log"
  else
    LC_ALL=C grep -E '^test result:' "$ALIGN_TB_LOGS/$slot.log" || true
  fi
done

printf 'suite: ran %s binaries in %ss\n' \
  "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" "$suite_elapsed"

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
    if printf '%s\n' "$found_names" | LC_ALL=C grep -qxF "${entry%%"$tab"*}"; then
      printf '  %s (passed)\n' "$entry" >&2
    else
      printf '  %s (no such target in the workspace)\n' "$entry" >&2
    fi
  done
  echo "  a fixed test must lose its manifest line in the same change that fixes it" >&2
fi
if [ "$status" -eq 0 ]; then
  printf 'suite: matches %s exactly\n' "$manifest"
fi
exit "$status"
