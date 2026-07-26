#!/usr/bin/env bash
# Publish the required SHA-bound status after both post-open reviews are clean.
set -euo pipefail

if [[ $# -ne 3 || ! "$1" =~ ^[0-9]+$ ]]; then
  echo "usage: scripts/record-post-review.sh PR_NUMBER HOST_REVIEW_LOG INDEPENDENT_REVIEW_LOG" >&2
  exit 2
fi
pr_number="$1"
host_log="$2"
independent_log="$3"
[[ "$host_log" != "$independent_log" ]] || {
  echo "host and independent reviews require distinct logs" >&2
  exit 2
}

head_sha="$(git rev-parse HEAD)"
pr_json="$(gh pr view "$pr_number" --json headRefOid,baseRefName,baseRefOid)"
remote_head="$(printf '%s' "$pr_json" | jq -r .headRefOid)"
base_ref="$(printf '%s' "$pr_json" | jq -r .baseRefName)"
base_sha="$(printf '%s' "$pr_json" | jq -r .baseRefOid)"
[[ "$head_sha" == "$remote_head" ]] || {
  echo "PR #$pr_number is not at local HEAD $head_sha" >&2
  exit 1
}
for review_log in "$host_log" "$independent_log"; do
  [[ -f "$review_log" ]] || {
    echo "review log does not exist: $review_log" >&2
    exit 2
  }
  marker_count="$(grep -Ec '^ALIGN_REVIEW_VERDICT=(CLEAN|FINDINGS)$' "$review_log" || true)"
  last_nonempty="$(awk 'NF { line = $0 } END { print line }' "$review_log")"
  [[ "$marker_count" -eq 1 && "$last_nonempty" == "ALIGN_REVIEW_VERDICT=CLEAN" ]] || {
    echo "review is not clean: $review_log" >&2
    exit 1
  }
  grep -Fqx "ALIGN_REVIEW_HEAD=$head_sha" "$review_log" || {
    echo "review log belongs to another HEAD: $review_log" >&2
    exit 1
  }
  grep -Fqx "ALIGN_REVIEW_BASE=$base_sha" "$review_log" || {
    echo "review log belongs to another base: $review_log" >&2
    exit 1
  }
done
grep -Fqx 'ALIGN_REVIEW_KIND=HOST' "$host_log" || {
  echo "host review log has the wrong kind" >&2
  exit 1
}
grep -Fqx 'ALIGN_REVIEW_KIND=INDEPENDENT' "$independent_log" || {
  echo "independent review log has the wrong kind" >&2
  exit 1
}
grep -Eq '^ALIGN_REVIEW_REVIEWER=[A-Za-z0-9._-]+$' "$independent_log" || {
  echo "independent review identity is missing" >&2
  exit 1
}
repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
gh api --method POST "repos/$repo/statuses/$head_sha" \
  -f state=success \
  -f context='Post-open review' \
  -f description='bounded host review + independent adversarial review'

tmp_body="$(mktemp)"
trap 'rm -f "$tmp_body"' EXIT
gh pr view "$pr_number" --json body --jq '.body // ""' |
  sed '/<!-- align-post-review-/d' >"$tmp_body"
{
  printf '\n<!-- align-post-review-head:%s -->\n' "$head_sha"
  printf '<!-- align-post-review-base-sha:%s -->\n' "$base_sha"
} >>"$tmp_body"
gh pr edit "$pr_number" --body-file "$tmp_body"
