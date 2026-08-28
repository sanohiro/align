#!/usr/bin/env bash
# Run Codex review with a progress-based stall guard and a machine-readable verdict.
# Review is inspection-only: tests belong to the selected verification gate.
set -euo pipefail

usage() {
  echo "usage: scripts/review-bounded.sh [--base REF] [--output FILE] [--changed-since REF] [--reopen-axis AXIS]" >&2
  exit 2
}

base="origin/main"
output=""
changed_since=""
reopen_axis=""
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
    --changed-since)
      [[ $# -ge 2 && -n "$2" ]] || usage
      changed_since="$2"
      shift 2
      ;;
    --reopen-axis)
      [[ $# -ge 2 && -n "$2" ]] || usage
      reopen_axis="$2"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

stall_seconds="${ALIGN_REVIEW_STALL_SECONDS:-900}"
progress_interval_seconds="${ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS:-30}"
# There is no default wall-clock maximum. ALIGN_REVIEW_TIMEOUT_SECONDS remains
# a compatibility spelling for an explicitly requested one-invocation maximum.
max_seconds="${ALIGN_REVIEW_MAX_SECONDS:-${ALIGN_REVIEW_TIMEOUT_SECONDS:-0}}"
if [[ ! "$stall_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "ALIGN_REVIEW_STALL_SECONDS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$progress_interval_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$max_seconds" =~ ^[0-9]+$ ]]; then
  echo "ALIGN_REVIEW_MAX_SECONDS must be zero or a positive integer" >&2
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
stop_reason="$tmp_dir/stop-reason"
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
progress_signature() {
  local pgid="$1"
  local log_file="$2"
  printf 'bytes=%s\n' "$(wc -c <"$log_file" | tr -d ' ')"
  # PID creation/exit and accumulated CPU time are progress even when the
  # review model buffers prose until its final answer.
  ps -axo pgid=,pid=,time= 2>/dev/null |
    awk -v wanted="$pgid" '$1 == wanted { print $2 ":" $3 }' |
    sort
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

head_sha="$(git rev-parse HEAD)"
# Bind the review to the merge base of HEAD and the base branch, matching
# scripts/pre-pr.sh's ALIGN_REVIEW_BASE check. Recording the base branch tip
# would orphan this log the moment an unrelated PR merges into main, even
# though the reviewed `base...HEAD` diff is unchanged.
base_tip="$(git rev-parse "${base}^{commit}")"
base_sha="$(git merge-base HEAD "$base_tip" 2>/dev/null || true)"
[[ "$base_sha" =~ ^[0-9a-f]{40}$ ]] || {
  echo "cannot compute the merge base of HEAD and $base" >&2
  exit 1
}

# A second full-diff review on a descendant commit is the review-as-design
# loop. Continue against the changed slice instead. A genuinely redesigned
# contract may reopen the full review only through the same authoritative
# closure-matrix commit and trailer that release the preflight tripwire.
prior_review_head=""
prior_review_distance=""
for prior_log in "$git_dir"/align-review-*.log "$git_dir"/align-review-cycle-*; do
  [[ -f "$prior_log" ]] || continue
  [[ "$(sed -n 's/^ALIGN_REVIEW_KIND=//p' "$prior_log" | head -1)" == "HOST" ]] || continue
  candidate_base="$(sed -n 's/^ALIGN_REVIEW_BASE=//p' "$prior_log" | head -1)"
  candidate_head="$(sed -n 's/^ALIGN_REVIEW_HEAD=//p' "$prior_log" | head -1)"
  [[ "$candidate_base" == "$base_sha" && "$candidate_head" != "$head_sha" ]] || continue
  [[ "$candidate_head" =~ ^[0-9a-f]{40}$ ]] || continue
  git merge-base --is-ancestor "$candidate_head" "$head_sha" 2>/dev/null || continue
  candidate_distance="$(git rev-list --count "$candidate_head".."$head_sha")"
  if [[ -z "$prior_review_distance" || "$candidate_distance" -lt "$prior_review_distance" ]]; then
    prior_review_head="$candidate_head"
    prior_review_distance="$candidate_distance"
  fi
done

review_range="${base_sha}...${head_sha}"
review_scope="FULL"
if [[ -n "$prior_review_head" ]]; then
  matrix_reopened=false
  if [[ -n "$reopen_axis" ]]; then
    while IFS= read -r commit; do
      git log -1 --format='%B' "$commit" |
        grep -Fqx "Closure-Matrix-Reopened: $reopen_axis" || continue
      while IFS= read -r path; do
        case "$path" in
          docs/impl/*/ja/*|docs/impl/ja/*) ;;
          docs/impl/*|CLAUDE.md) matrix_reopened=true ;;
        esac
      done < <(git show --no-renames --name-only --format= "$commit")
    done < <(git rev-list --reverse "$prior_review_head".."$head_sha")
  fi
  if [[ "$matrix_reopened" == true ]]; then
    [[ -z "$changed_since" ]] || {
      echo "--changed-since and --reopen-axis are mutually exclusive" >&2
      exit 2
    }
  elif [[ -n "$changed_since" ]]; then
    changed_since_sha="$(git rev-parse "${changed_since}^{commit}" 2>/dev/null || true)"
    [[ "$changed_since_sha" == "$prior_review_head" ]] || {
      echo "--changed-since must name the nearest reviewed ancestor $prior_review_head" >&2
      exit 1
    }
    review_range="${prior_review_head}..${head_sha}"
    review_scope="CHANGED_SLICE"
  else
    echo "full-diff review already exists for ancestor $prior_review_head" >&2
    echo "continue with --changed-since $prior_review_head; do not restart complete-diff discovery" >&2
    echo "a high-risk redesign requires --reopen-axis AXIS and a matching" >&2
    echo "'Closure-Matrix-Reopened: AXIS' commit that changes CLAUDE.md or docs/impl" >&2
    exit 1
  fi
elif [[ -n "$changed_since" ]]; then
  echo "--changed-since requires an existing reviewed ancestor" >&2
  exit 1
fi
cycle_record="$git_dir/align-review-cycle-$head_sha"
{
  printf 'ALIGN_REVIEW_KIND=HOST\n'
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$head_sha"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$base_sha"
  printf 'ALIGN_REVIEW_SCOPE=%s\n' "$review_scope"
} >"$cycle_record"
if [[ "$review_scope" == "CHANGED_SLICE" ]]; then
  prompt="An ancestor full-diff review already covered ${base_sha}...${prior_review_head}. Review only the changed slice git diff ${review_range} and compose its result with that checkpoint. Inspect only: do not modify files and do not run cargo, tests, builds, benchmarks, or network commands. Use read-only git/rg/sed inspection as needed. Report actionable findings first. End with exactly one line: ALIGN_REVIEW_VERDICT=CLEAN when there are no actionable findings, or ALIGN_REVIEW_VERDICT=FINDINGS when there are any."
else
  prompt="Review git diff ${review_range} for soundness and regression risks. Inspect only: do not modify files and do not run cargo, tests, builds, benchmarks, or network commands. Use read-only git/rg/sed inspection as needed. Report actionable findings first. End with exactly one line: ALIGN_REVIEW_VERDICT=CLEAN when there are no actionable findings, or ALIGN_REVIEW_VERDICT=FINDINGS when there are any."
fi
{
  printf 'ALIGN_REVIEW_KIND=HOST\n'
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$head_sha"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$base_sha"
  printf 'ALIGN_REVIEW_SCOPE=%s\n' "$review_scope"
} >"$output"

# Job control gives the review its own process group, so the stall guard
# terminates Codex and every helper it spawned instead of leaving an orphaned
# review.
set -m
# codex-cli 0.145 rejects a custom PROMPT together with --base even though its
# help text displays both. Keep code-review mode and put the explicit base in
# the inspection-only prompt so the watchdog can require a bounded verdict.
PATH="$review_path" DEVELOPER_DIR="$review_developer_dir" GIT_OPTIONAL_LOCKS=0 \
codex review -c 'sandbox_mode="read-only"' -c 'approval_policy="never"' \
  "$prompt" >>"$output" 2>&1 &
review_pid=$!

(
  started_at="$(date +%s)"
  last_progress_at="$started_at"
  last_signature="$(progress_signature "$review_pid" "$output")"
  while kill -0 "$review_pid" 2>/dev/null; do
    now="$(date +%s)"
    if (( max_seconds > 0 && now - started_at >= max_seconds )); then
      printf 'MAX:%s\n' "$max_seconds" >"$stop_reason"
      terminate_group "$review_pid"
      sleep 2
      kill -KILL "-$review_pid" 2>/dev/null || kill -KILL "$review_pid" 2>/dev/null || true
      break
    fi
    if (( now - last_progress_at >= stall_seconds )); then
      printf 'STALL:%s\n' "$stall_seconds" >"$stop_reason"
      terminate_group "$review_pid"
      sleep 2
      kill -KILL "-$review_pid" 2>/dev/null || kill -KILL "$review_pid" 2>/dev/null || true
      break
    fi
    sleep_seconds="$progress_interval_seconds"
    if (( max_seconds > 0 && max_seconds - (now - started_at) < sleep_seconds )); then
      sleep_seconds=$((max_seconds - (now - started_at)))
    fi
    if (( stall_seconds - (now - last_progress_at) < sleep_seconds )); then
      sleep_seconds=$((stall_seconds - (now - last_progress_at)))
    fi
    sleep "$sleep_seconds"
    now="$(date +%s)"
    signature="$(progress_signature "$review_pid" "$output")"
    if [[ "$signature" != "$last_signature" ]]; then
      last_signature="$signature"
      last_progress_at="$now"
    fi
    if (( max_seconds > 0 && now - started_at >= max_seconds )); then
      printf 'MAX:%s\n' "$max_seconds" >"$stop_reason"
      terminate_group "$review_pid"
      sleep 2
      kill -KILL "-$review_pid" 2>/dev/null || kill -KILL "$review_pid" 2>/dev/null || true
      break
    fi
    if (( now - last_progress_at >= stall_seconds )); then
      printf 'STALL:%s\n' "$stall_seconds" >"$stop_reason"
      terminate_group "$review_pid"
      sleep 2
      kill -KILL "-$review_pid" 2>/dev/null || kill -KILL "$review_pid" 2>/dev/null || true
      break
    fi
  done
) &
watchdog_pid=$!
set +m

set +e
wait "$review_pid"
review_status=$?
set -e
if [[ -f "$stop_reason" ]]; then
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
if [[ -f "$stop_reason" ]]; then
  reason="$(cat "$stop_reason")"
  case "$reason" in
    STALL:*)
      echo "review stalled with no observed log or process progress for ${reason#STALL:}s" >&2
      ;;
    MAX:*)
      echo "review reached the explicit one-invocation maximum of ${reason#MAX:}s" >&2
      ;;
    *)
      echo "review stopped without a recognized reason" >&2
      ;;
  esac
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
  elif awk '
    BEGIN { accepted = 0; invalid = 0 }
    /^[[:space:]]*$/ { next }
    $0 == "No findings." ||
    $0 == "Read-only inspection found no actionable soundness or regression risks in the diff." ||
    $0 ~ /^No actionable soundness or regression (issues|risks) were found in the inspected diff[.]$/ ||
    $0 ~ /^No actionable soundness or regression (issues|risks) were found( in the inspected diff)?[.] ALIGN_REVIEW_VERDICT=CLEAN$/ ||
    $0 == "No actionable issues were found in the changed files." ||
    $0 ~ /^[A-Z][[:print:]]* without introducing an actionable (soundness or regression )?(risk|risks|issue|issues|regression)[.]$/ {
      accepted = 1
      next
    }
    { invalid = 1 }
    END { exit !(accepted && !invalid) }
  ' "$native_result"
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
