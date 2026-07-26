#!/usr/bin/env bash
# Refresh a PR's HEAD-bound preflight attestation after a reviewed follow-up.
set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^[0-9]+$ ]]; then
  echo "usage: scripts/update-pr-preflight.sh PR_NUMBER" >&2
  exit 2
fi
pr_number="$1"
head_sha="$(git rev-parse HEAD)"
stamp="$(git rev-parse --git-path "align-preflight/$head_sha")"
[[ -f "$stamp" ]] || {
  echo "no preflight stamp for HEAD $head_sha; rerun scripts/pre-pr.sh" >&2
  exit 1
}
remote_head="$(gh pr view "$pr_number" --json headRefOid --jq .headRefOid)"
if [[ "$remote_head" != "$head_sha" ]]; then
  echo "PR #$pr_number head is $remote_head, not local HEAD $head_sha" >&2
  exit 1
fi

reviewer="$(sed -n 's/^reviewer=//p' "$stamp")"
stamp_version="$(sed -n 's/^version=//p' "$stamp")"
stamp_head="$(sed -n 's/^head=//p' "$stamp")"
base_ref="$(sed -n 's/^base_ref=//p' "$stamp")"
base_sha="$(sed -n 's/^base_sha=//p' "$stamp")"
[[ "$stamp_version" == "1" && "$stamp_head" == "$head_sha" ]] || {
  echo "preflight stamp does not belong to HEAD $head_sha" >&2
  exit 1
}
[[ "$reviewer" =~ ^[A-Za-z0-9._-]+$ ]] || {
  echo "preflight stamp has no adversarial reviewer" >&2
  exit 1
}
[[ -n "$base_ref" && "$(git rev-parse --verify "${base_ref}^{commit}")" == "$base_sha" ]] || {
  echo "preflight base moved after verification; rerun scripts/pre-pr.sh" >&2
  exit 1
}
remote_base="$(gh pr view "$pr_number" --json baseRefName --jq .baseRefName)"
remote_base_sha="$(git rev-parse --verify "origin/${remote_base}^{commit}")"
if [[ "$remote_base_sha" != "$base_sha" ]]; then
  echo "PR base $remote_base is not the reviewed preflight base $base_ref" >&2
  exit 1
fi
tmp_old="$(mktemp)"
tmp_new="$(mktemp)"
cleanup() {
  rm -f "$tmp_old" "$tmp_new"
}
trap cleanup EXIT
gh pr view "$pr_number" --json body --jq '.body // ""' >"$tmp_old"
sed '/<!-- align-preflight-/d' "$tmp_old" >"$tmp_new"
{
  printf '\n<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$head_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$remote_base"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:clean -->\n'
  printf '<!-- align-preflight-reviewer:%s -->\n' "$reviewer"
} >>"$tmp_new"
gh pr edit "$pr_number" --body-file "$tmp_new"
