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
# Discovery, the concurrency cap, and the run loop live in
# scripts/test-binaries-lib.sh, shared with scripts/run-suite-binaries.sh; this
# script owns the gate's declared-set check and its full-output report.
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

script_dir="$(dirname "$0")"
# shellcheck source=scripts/dyld-env.sh
. "$script_dir/dyld-env.sh"
# shellcheck source=scripts/test-binaries-lib.sh
. "$script_dir/test-binaries-lib.sh"
align_use_private_dyld_region

ALIGN_TB_LOGS="$(mktemp -d)"
trap 'align_tb_cleanup' EXIT
trap 'align_tb_cleanup; exit 130' INT
trap 'align_tb_cleanup; exit 143' TERM

align_tb_discover "$artifacts" || {
  echo "the gate build produced no test binaries (cargo artifact format changed?)" >&2
  exit 1
}

# Fail closed in BOTH directions, and on a multiset rather than a set: a
# missing binary means the gate silently stopped covering something, an extra
# one means a `-p`/`--test` selection quietly widened the gate (two packages
# each holding, say, a tests/summary.rs would appear here as a duplicate), and
# either way the declared list in scripts/test-pr.sh must be updated
# deliberately.
found_names="$(align_tb_names | LC_ALL=C sort)"
expected_names="$(printf '%s\n' $expected | LC_ALL=C sort)"
if [ "$found_names" != "$expected_names" ]; then
  echo "the compiled gate binaries do not match the declared gate set" >&2
  echo "  expected: $(printf '%s ' $expected_names)" >&2
  echo "  compiled: $(printf '%s ' $found_names)" >&2
  echo "  update the gate selection and its declared list in scripts/test-pr.sh together" >&2
  exit 1
fi

align_tb_export_dylib_path "$artifacts"
align_tb_configure_jobs

printf 'gate: %s binaries, %s parallel, %s test thread(s) each\n' \
  "$(printf '%s\n' "$found_names" | wc -l | tr -d '[:space:]')" \
  "$ALIGN_TB_JOBS" "$ALIGN_TB_THREADS"
printf 'gate: %s\n' "$(printf '%s ' $found_names)"

align_tb_run gate

# Every binary's output is reported, in one deterministic C-collation order,
# whichever of them failed. The gate's own exit code is 1 for any failure; a
# binary's real exit code (and a child that died before recording one) is in
# its header line.
status=0
align_tb_report_all || status=1
exit "$status"
