#!/usr/bin/env bash
# Run Codex review with a hard wall-clock limit and a machine-readable verdict.
# Review is inspection-only: tests belong to the selected verification gate.
set -euo pipefail

usage() {
  echo "usage: scripts/review-bounded.sh [--base REF] [--output FILE]" >&2
  exit 2
}

base="origin/main"
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
review_path="$PATH"
review_developer_dir="${DEVELOPER_DIR:-}"
if [[ "$(uname -s)" == "Darwin" && -x "/Applications/Xcode.app/Contents/Developer/usr/bin/git" ]]; then
  # /usr/bin/git is an xcrun shim on macOS. A read-only review sandbox cannot
  # populate xcrun's cache, so use Xcode's real Git binary for review helpers.
  review_path="/Applications/Xcode.app/Contents/Developer/usr/bin:$review_path"
  if [[ -z "$review_developer_dir" ]]; then
    review_developer_dir="/Applications/Xcode.app/Contents/Developer"
  fi
fi
[[ -z "$(git status --porcelain)" ]] || {
  echo "review requires a clean worktree" >&2
  exit 1
}

tmp_dir="$(mktemp -d)"
timed_out="$tmp_dir/timed-out"
review_pid=""
watchdog_pid=""
if [[ -z "$output" ]]; then
  output="$(git rev-parse --git-path "align-review-$(git rev-parse HEAD).log")"
fi
repo_root="$(git rev-parse --show-toplevel)"
git_dir="$(cd "$(dirname "$(git rev-parse --git-dir)")" && pwd)/$(basename "$(git rev-parse --git-dir)")"
output_dir="$(cd "$(dirname "$output")" && pwd)"
output_abs="$output_dir/$(basename "$output")"
case "$output_abs" in
  "$repo_root"/*)
    case "$output_abs" in
      "$git_dir"/*) ;;
      *)
        echo "--output inside the worktree must be under .git" >&2
        exit 2
        ;;
    esac
    ;;
esac
terminate_group() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
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

prompt="Review git diff ${base}...HEAD for soundness and regression risks. Inspect only: do not modify files and do not run cargo, tests, builds, benchmarks, network commands, or scripts/test-full.sh. Use read-only git/rg/sed inspection as needed. Report actionable findings first. End with exactly one line: ALIGN_REVIEW_VERDICT=CLEAN when there are no actionable findings, or ALIGN_REVIEW_VERDICT=FINDINGS when there are any."
head_sha="$(git rev-parse HEAD)"
base_sha="$(git rev-parse "${base}^{commit}")"
{
  printf 'ALIGN_REVIEW_KIND=HOST\n'
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$head_sha"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$base_sha"
} >"$output"

# Job control gives the review its own process group, so the watchdog terminates
# Codex and every helper it spawned instead of leaving an orphaned review.
set -m
# codex-cli 0.145 rejects a custom PROMPT together with --base even though its
# help text displays both. Keep code-review mode and put the explicit base in
# the inspection-only prompt so the watchdog can require a bounded verdict.
PATH="$review_path" DEVELOPER_DIR="$review_developer_dir" GIT_OPTIONAL_LOCKS=0 \
codex review -c 'sandbox_mode="read-only"' -c 'approval_policy="never"' \
  "$prompt" >>"$output" 2>&1 &
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
if [[ -f "$timed_out" ]]; then
  wait "$watchdog_pid" 2>/dev/null || true
  watchdog_pid=""
  kill -KILL "-$review_pid" 2>/dev/null || true
  review_pid=""
else
  terminate_group "$review_pid"
  review_pid=""
  terminate_group "$watchdog_pid"
  wait "$watchdog_pid" 2>/dev/null || true
  watchdog_pid=""
fi

sed -n '1,$p' "$output"
if [[ -f "$timed_out" ]]; then
  echo "review timed out after ${timeout_seconds}s" >&2
  exit 124
fi
if [[ $review_status -ne 0 ]]; then
  echo "review process failed with status $review_status" >&2
  exit "$review_status"
fi
if [[ "$(git rev-parse HEAD)" != "$head_sha" || -n "$(git status --porcelain)" ]]; then
  echo "HEAD or worktree changed during review" >&2
  exit 1
fi

native_result="$tmp_dir/native-result"
awk '
  $0 == "codex" { capture = 1; result = ""; next }
  capture { result = result $0 ORS }
  END { printf "%s", result }
' "$output" >"$native_result"
marker_count="$(grep -Ec '^ALIGN_REVIEW_VERDICT=(CLEAN|FINDINGS)$' "$native_result" || true)"
if [[ "$marker_count" -eq 0 ]]; then
  if grep -Eq '^- \[P[0-3]\]' "$native_result"; then
    printf 'ALIGN_REVIEW_VERDICT=FINDINGS\n' | tee -a "$output"
  elif grep -Fqx 'No findings.' "$native_result" ||
    grep -Fqx 'Read-only inspection found no actionable soundness or regression risks in the diff.' "$native_result"
  then
    printf 'ALIGN_REVIEW_VERDICT=CLEAN\n' | tee -a "$output"
  else
    echo "review returned an unrecognized native result" >&2
    exit 3
  fi
  marker_count=1
fi
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
