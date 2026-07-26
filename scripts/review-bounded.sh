#!/usr/bin/env bash
# Run Codex review with a hard wall-clock limit and a machine-readable verdict.
# Review is inspection-only: tests belong to the selected verification gate.
set -euo pipefail

usage() {
  echo "usage: scripts/review-bounded.sh [--base REF] [--output FILE]" >&2
  exit 2
}

base="main"
output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ $# -ge 2 ]] || usage
      base="$2"
      shift 2
      ;;
    --output)
      [[ $# -ge 2 ]] || usage
      output="$2"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

timeout_seconds="${ALIGN_REVIEW_TIMEOUT_SECONDS:-900}"
if [[ ! "$timeout_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "ALIGN_REVIEW_TIMEOUT_SECONDS must be a positive integer" >&2
  exit 2
fi
command -v codex >/dev/null 2>&1 || {
  echo "codex is required for the bounded host-native review" >&2
  exit 2
}

tmp_dir="$(mktemp -d)"
timed_out="$tmp_dir/timed-out"
review_pid=""
watchdog_pid=""
if [[ -z "$output" ]]; then
  output="$tmp_dir/review.log"
fi
terminate_group() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  fi
}
cleanup() {
  trap - EXIT INT TERM
  terminate_group "$watchdog_pid"
  terminate_group "$review_pid"
  [[ -z "$watchdog_pid" ]] || wait "$watchdog_pid" 2>/dev/null || true
  [[ -z "$review_pid" ]] || wait "$review_pid" 2>/dev/null || true
  rm -rf "$tmp_dir"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

prompt=$'Review the diff for soundness and regression risks. Inspect only: do not modify files and do not run cargo, tests, builds, benchmarks, network commands, or scripts/test-full.sh. Use read-only git/rg/sed inspection as needed. Report actionable findings first. End with exactly one line: ALIGN_REVIEW_VERDICT=CLEAN when there are no actionable findings, or ALIGN_REVIEW_VERDICT=FINDINGS when there are any.'

# Job control gives the review its own process group, so the watchdog terminates
# Codex and every helper it spawned instead of leaving an orphaned review.
set -m
codex review --base "$base" "$prompt" >"$output" 2>&1 &
review_pid=$!

(
  sleep "$timeout_seconds"
  if kill -0 "$review_pid" 2>/dev/null; then
    : >"$timed_out"
    kill -TERM "-$review_pid" 2>/dev/null || kill -TERM "$review_pid" 2>/dev/null || true
    sleep 2
    kill -KILL "-$review_pid" 2>/dev/null || kill -KILL "$review_pid" 2>/dev/null || true
  fi
) &
watchdog_pid=$!
set +m

set +e
wait "$review_pid"
review_status=$?
set -e
review_pid=""
terminate_group "$watchdog_pid"
wait "$watchdog_pid" 2>/dev/null || true
watchdog_pid=""

sed -n '1,$p' "$output"
if [[ -f "$timed_out" ]]; then
  echo "review timed out after ${timeout_seconds}s" >&2
  exit 124
fi
if [[ $review_status -ne 0 ]]; then
  echo "review process failed with status $review_status" >&2
  exit "$review_status"
fi

marker_count="$(grep -Ec '^ALIGN_REVIEW_VERDICT=(CLEAN|FINDINGS)$' "$output" || true)"
last_nonempty="$(awk 'NF { line = $0 } END { print line }' "$output")"
if [[ "$marker_count" -ne 1 || ! "$last_nonempty" =~ ^ALIGN_REVIEW_VERDICT=(CLEAN|FINDINGS)$ ]]; then
  echo "review must end with exactly one machine-readable verdict" >&2
  exit 3
fi
verdict="${last_nonempty#ALIGN_REVIEW_VERDICT=}"
case "$verdict" in
  CLEAN) exit 0 ;;
  FINDINGS) exit 2 ;;
  *)
    echo "invalid review verdict: $verdict" >&2
    exit 3
    ;;
esac
