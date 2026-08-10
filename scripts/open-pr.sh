#!/usr/bin/env bash
# Publish the current HEAD-bound preflight while opening or refreshing a draft PR.
set -euo pipefail

usage() {
  echo "usage: scripts/open-pr.sh --title TITLE --body-file FILE [--base BRANCH]" >&2
  echo "       scripts/open-pr.sh --update PR_NUMBER" >&2
  exit 2
}

base="main"
title=""
body_file=""
update_pr=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base) [[ $# -ge 2 ]] || usage; base="$2"; shift 2 ;;
    --title) [[ $# -ge 2 ]] || usage; title="$2"; shift 2 ;;
    --body-file) [[ $# -ge 2 ]] || usage; body_file="$2"; shift 2 ;;
    --update) [[ $# -ge 2 ]] || usage; update_pr="$2"; shift 2 ;;
    *) usage ;;
  esac
done
if [[ -n "$update_pr" ]]; then
  [[ "$update_pr" =~ ^[0-9]+$ && -z "$title" && -z "$body_file" && "$base" == main ]] || usage
else
  [[ -n "$title" && -f "$body_file" ]] || usage
fi
command -v gh >/dev/null 2>&1 || { echo "gh is required" >&2; exit 2; }

head_sha="$(git rev-parse HEAD)"
stamp="$(git rev-parse --git-path "align-preflight/$head_sha")"
[[ -f "$stamp" && -z "$(git status --porcelain)" ]] || {
  echo "clean HEAD needs a matching scripts/pre-pr.sh stamp" >&2
  exit 1
}
upstream_sha="$(git rev-parse '@{upstream}' 2>/dev/null || true)"
[[ "$upstream_sha" == "$head_sha" ]] || {
  echo "HEAD must be pushed to its upstream first" >&2
  exit 1
}

value() { sed -n "s/^$1=//p" "$stamp"; }
stamp_head="$(value head)"
base_ref="$(value base_ref)"
base_sha="$(value base_sha)"
review_head="$(value review_head)"
review_state="$(value review_state)"
reviewer="$(value reviewer)"
[[ "$(value version)" == 1 && "$stamp_head" == "$head_sha" ]] || {
  echo "invalid preflight stamp" >&2
  exit 1
}
case "$review_state" in
  clean)
    [[ "$review_head" == "$head_sha" && "$reviewer" != docs-only ]] || exit 1
    ;;
  fixed)
    [[ "$review_head" =~ ^[0-9a-f]{40}$ && "$review_head" != "$head_sha" && "$reviewer" != docs-only ]] || exit 1
    git merge-base --is-ancestor "$review_head" "$head_sha" || exit 1
    ;;
  docs-only)
    [[ "$review_head" == "$head_sha" && "$reviewer" == docs-only ]] || exit 1
    ;;
  tooling)
    # Tooling tier: no library source changed, so an independent review is
    # optional and the focused owner check recorded in the stamp is the gate.
    [[ "$review_head" == "$head_sha" && "$reviewer" == tooling ]] || exit 1
    ;;
  *) echo "invalid review state in preflight stamp" >&2; exit 1 ;;
esac
[[ "$(git rev-parse --verify "${base_ref}^{commit}")" == "$base_sha" ]] || {
  echo "preflight base moved; rerun scripts/pre-pr.sh" >&2
  exit 1
}

tmp_old="$(mktemp)"
tmp_body="$(mktemp)"
cleanup() { rm -f "$tmp_old" "$tmp_body"; }
trap cleanup EXIT
if [[ -n "$update_pr" ]]; then
  pr_meta="$(gh pr view "$update_pr" --json headRefOid,baseRefName,baseRefOid \
    --jq '[.headRefOid, .baseRefName, .baseRefOid] | @tsv')"
  IFS=$'\t' read -r pr_head pr_base pr_base_sha <<<"$pr_meta"
  [[ "$pr_head" == "$head_sha" && "$pr_base_sha" == "$base_sha" ]] || {
    echo "PR head or base does not match the preflight stamp" >&2
    exit 1
  }
  base="$pr_base"
  gh pr view "$update_pr" --json body --jq '.body // ""' >"$tmp_old"
else
  requested_ref="$base"
  git rev-parse --verify --quiet "refs/remotes/origin/${base}^{commit}" >/dev/null && requested_ref="origin/$base"
  [[ "$(git rev-parse --verify "${requested_ref}^{commit}")" == "$base_sha" ]] || {
    echo "requested PR base does not match the preflight base" >&2
    exit 1
  }
  cp "$body_file" "$tmp_old"
fi

sed '/<!-- align-preflight-/d' "$tmp_old" >"$tmp_body"
{
  printf '\n<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$head_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$base"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:%s -->\n' "$review_state"
  printf '<!-- align-preflight-review-head:%s -->\n' "$review_head"
  printf '<!-- align-preflight-reviewer:%s -->\n' "$reviewer"
} >>"$tmp_body"

if [[ -n "$update_pr" ]]; then
  gh pr edit "$update_pr" --body-file "$tmp_body" >/dev/null
  pr_url="$(gh pr view "$update_pr" --json url --jq .url)"
else
  pr_url="$(gh pr create --draft --base "$base" --title "$title" --body-file "$tmp_body")"
fi
# Keep the existing branch-protection context; the review itself happened once,
# before publication, and is bound to this SHA by the PR-body attestation.
repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
gh api --method POST "repos/$repo/statuses/$head_sha" \
  -f state=success \
  -f context='Post-open review' \
  -f description="pre-open review recorded: $review_state" >/dev/null
printf '%s\n' "$pr_url"
