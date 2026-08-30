#!/usr/bin/env bash
# Capture a command's routine output, print one success summary, and replay the
# captured detail only when the command fails.
#
#   usage: scripts/run-quiet.sh [--stdout FILE] [--expect-failure] LABEL -- COMMAND [ARG...]
#
# --stdout keeps machine-readable stdout in FILE while capturing stderr. On a
# failure both streams are replayed; on success FILE remains available to the
# caller without flooding the terminal. ALIGN_QUIET_VERBOSE=1 replays captured
# human output after a successful command too. --expect-failure inverts only
# the command verdict: a non-zero child is a terse wrapper success, while an
# unexpected zero replays the output and fails.
set -uo pipefail

usage() {
  echo "usage: scripts/run-quiet.sh [--stdout FILE] [--expect-failure] LABEL -- COMMAND [ARG...]" >&2
  exit 2
}

stdout_file=""
expect_failure=0
while [ $# -gt 0 ]; do
  case "$1" in
    --stdout)
      [ $# -ge 2 ] || usage
      stdout_file="$2"
      [ -n "$stdout_file" ] || {
        echo "--stdout requires a non-empty file path" >&2
        exit 2
      }
      shift 2
      ;;
    --expect-failure)
      expect_failure=1
      shift
      ;;
    *) break ;;
  esac
done

[ $# -ge 3 ] && [ "$2" = -- ] || {
  usage
}
label="$1"
shift 2

verbose="${ALIGN_QUIET_VERBOSE:-0}"
case "$verbose" in
  0 | 1) ;;
  *)
    echo "ALIGN_QUIET_VERBOSE must be 0 or 1" >&2
    exit 2
    ;;
esac

log="$(mktemp)" || {
  echo "could not create a temporary command log" >&2
  exit 2
}
quiet_cleanup() {
  rm -f "$log"
}
quiet_interrupted() {
  local code="$1" signal_name="$2" interrupted_elapsed
  trap - INT TERM
  interrupted_elapsed="$(($(date +%s) - started))"
  printf '%s: INTERRUPTED by %s (%ss)\n' \
    "$label" "$signal_name" "$interrupted_elapsed" >&2
  [ ! -s "$log" ] || cat "$log" >&2
  if [ -n "$stdout_file" ] && [ -s "$stdout_file" ]; then
    cat "$stdout_file" >&2
  fi
  exit "$code"
}
trap quiet_cleanup EXIT
started="$(date +%s)"
trap 'quiet_interrupted 130 INT' INT
trap 'quiet_interrupted 143 TERM' TERM
status=0
if [ -n "$stdout_file" ]; then
  "$@" >"$stdout_file" 2>"$log" || status=$?
else
  "$@" >"$log" 2>&1 || status=$?
fi
elapsed="$(($(date +%s) - started))"

if [ "$expect_failure" -eq 1 ] && [ "$status" -ne 0 ]; then
  printf '%s: expected failure observed (exit %s, %ss)\n' \
    "$label" "$status" "$elapsed"
  if [ "$verbose" -eq 1 ]; then
    [ ! -s "$log" ] || cat "$log"
    if [ -n "$stdout_file" ] && [ -s "$stdout_file" ]; then
      cat "$stdout_file"
    fi
  fi
  exit 0
elif [ "$expect_failure" -eq 1 ]; then
  printf '%s: FAILED (expected a non-zero exit, got 0; %ss)\n' \
    "$label" "$elapsed" >&2
  [ ! -s "$log" ] || cat "$log" >&2
  if [ -n "$stdout_file" ] && [ -s "$stdout_file" ]; then
    cat "$stdout_file" >&2
  fi
  exit 1
elif [ "$status" -eq 0 ]; then
  printf '%s: ok (%ss)\n' "$label" "$elapsed"
  if [ "$verbose" -eq 1 ] && [ -s "$log" ]; then
    cat "$log"
  fi
else
  printf '%s: FAILED (exit %s, %ss)\n' "$label" "$status" "$elapsed" >&2
  [ ! -s "$log" ] || cat "$log" >&2
  if [ -n "$stdout_file" ] && [ -s "$stdout_file" ]; then
    cat "$stdout_file" >&2
  fi
fi

exit "$status"
