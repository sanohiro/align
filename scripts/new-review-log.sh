#!/usr/bin/env bash
# Scaffold a review-log file with the required HEAD/base binding and a
# FINDINGS verdict placeholder, then print the scripts/pre-pr.sh invocation
# to run once the review is recorded.
#
# scripts/pre-pr.sh requires the review log to live where an untracked file
# does not fail its clean-worktree check. `git status --porcelain` never
# reports paths under .git/, so .git/ and any path entirely outside the
# repository both qualify; this script defaults to .git/, matching
# scripts/review-bounded.sh's own default location.
set -euo pipefail

usage() {
  echo "usage: scripts/new-review-log.sh [OUTPUT_PATH]" >&2
  exit 2
}

[ $# -le 1 ] || usage

git rev-parse --is-inside-work-tree >/dev/null

head_sha="$(git rev-parse HEAD)"
base_sha="$(git rev-parse origin/main)"
short_sha="$(git rev-parse --short HEAD)"

output="${1:-}"
if [ -z "$output" ]; then
  output="$(git rev-parse --git-path "align-review-${short_sha}.md")"
fi

case "$output" in
  /*) ;;
  *) output="$(pwd)/$output" ;;
esac

mkdir -p "$(dirname "$output")"

{
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$head_sha"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$base_sha"
  printf '\n'
  printf '# Reviewer: <name or ID>\n'
  printf '# Scope: git diff %s...%s\n' "$base_sha" "$head_sha"
  printf '#\n'
  printf '# Record findings below, one per item. When every finding is fixed or was\n'
  printf '# never valid, change the trailing verdict line to CLEAN. Until then, leave\n'
  printf '# it as FINDINGS and pass --findings-fixed to scripts/pre-pr.sh once the fix\n'
  printf '# commit lands. The verdict line below must stay the last non-empty line.\n'
  printf '\n'
  printf 'ALIGN_REVIEW_VERDICT=FINDINGS\n'
} >"$output"

echo "review log scaffolded at $output"
echo "next: scripts/pre-pr.sh --reviewer YOUR_ID --review-log $output --base origin/main --owner-test LABEL -- COMMAND ..."
