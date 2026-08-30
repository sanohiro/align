#!/usr/bin/env bash
# Capture a command's routine output, print one success summary, and replay the
# captured detail only when the command fails.
#
#   usage: scripts/run-quiet.sh [--stdout FILE] LABEL -- COMMAND [ARG...]
#
# --stdout keeps machine-readable stdout in FILE while capturing stderr. On a
# failure both streams are replayed; on success FILE remains available to the
# caller without flooding the terminal. ALIGN_QUIET_VERBOSE=1 replays captured
# human output after a successful command too.
set -uo pipefail

stdout_file=""
if [ "${1:-}" = --stdout ]; then
  [ $# -ge 3 ] || {
    echo "usage: scripts/run-quiet.sh [--stdout FILE] LABEL -- COMMAND [ARG...]" >&2
    exit 2
  }
  stdout_file="$2"
  [ -n "$stdout_file" ] || {
    echo "--stdout requires a non-empty file path" >&2
    exit 2
  }
  shift 2
fi

[ $# -ge 3 ] && [ "$2" = -- ] || {
  echo "usage: scripts/run-quiet.sh [--stdout FILE] LABEL -- COMMAND [ARG...]" >&2
  exit 2
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
trap 'rm -f "$log"' EXIT
started="$(date +%s)"
status=0
if [ -n "$stdout_file" ]; then
  "$@" >"$stdout_file" 2>"$log" || status=$?
else
  "$@" >"$log" 2>&1 || status=$?
fi
elapsed="$(($(date +%s) - started))"

if [ "$status" -eq 0 ]; then
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
