#!/usr/bin/env bash
# Scaffold a review-log file with the required HEAD/base binding and a
# FINDINGS verdict placeholder, then print the scripts/pre-pr.sh invocations
# to run once the review is recorded.
#
# scripts/pre-pr.sh requires the review log to live where an untracked file
# does not fail its clean-worktree check. `git status --porcelain` never
# reports paths under .git/, so .git/ and any path entirely outside the
# repository both qualify; this script defaults to .git/, matching
# scripts/review-bounded.sh's own default location and its .log extension.
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: scripts/new-review-log.sh [--base REF] [OUTPUT_PATH]

--base REF   ref whose tip becomes ALIGN_REVIEW_BASE (default: origin/main).
OUTPUT_PATH  where to write the log (default: .git/align-review-<shortsha>.log).
             Refuses to overwrite an existing file at OUTPUT_PATH.
USAGE
  exit 2
}

base="origin/main"
output=""
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage ;;
    --base)
      [ $# -ge 2 ] || usage
      base="$2"
      shift 2
      ;;
    -*) usage ;;
    *)
      [ -z "$output" ] || usage
      output="$1"
      shift
      ;;
  esac
done

git rev-parse --is-inside-work-tree >/dev/null

head_sha="$(git rev-parse HEAD)"
base_sha="$(git rev-parse "${base}^{commit}")"
short_sha="$(git rev-parse --short HEAD)"

if [ -z "$output" ]; then
  output="$(git rev-parse --git-path "align-review-${short_sha}.log")"
fi

case "$output" in
  /*) ;;
  *) output="$(pwd)/$output" ;;
esac

if [ -e "$output" ]; then
  echo "refusing to overwrite existing review log: $output" >&2
  exit 1
fi

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
echo "next, once the verdict line reads CLEAN and belongs to the final HEAD:"
echo "  scripts/pre-pr.sh --reviewer YOUR_ID --review-log $output --base $base --owner-test LABEL -- COMMAND ..."
echo "next, once findings were fixed in a later commit and the verdict stays FINDINGS:"
echo "  scripts/pre-pr.sh --reviewer YOUR_ID --review-log $output --findings-fixed --base $base --owner-test LABEL -- COMMAND ..."
