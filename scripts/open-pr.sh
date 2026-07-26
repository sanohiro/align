#!/usr/bin/env bash
# Open a draft PR only when the current HEAD has passed scripts/pre-pr.sh.
set -euo pipefail

usage() {
  echo "usage: scripts/open-pr.sh --title TITLE --body-file FILE [--base BRANCH]" >&2
  exit 2
}

base="main"
title=""
body_file=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ $# -ge 2 ]] || usage
      base="$2"
      shift 2
      ;;
    --title)
      [[ $# -ge 2 ]] || usage
      title="$2"
      shift 2
      ;;
    --body-file)
      [[ $# -ge 2 ]] || usage
      body_file="$2"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done
[[ -n "$title" && -f "$body_file" ]] || usage
command -v gh >/dev/null 2>&1 || {
  echo "gh is required to open the PR" >&2
  exit 2
}

head_sha="$(git rev-parse HEAD)"
stamp="$(git rev-parse --git-path "align-preflight/$head_sha")"
[[ -f "$stamp" ]] || {
  echo "no preflight stamp for HEAD $head_sha; run scripts/pre-pr.sh first" >&2
  exit 1
}
if [[ -n "$(git status --porcelain)" ]]; then
  echo "PR creation requires a clean worktree" >&2
  exit 1
fi
upstream_sha="$(git rev-parse '@{upstream}' 2>/dev/null || true)"
if [[ "$upstream_sha" != "$head_sha" ]]; then
  echo "HEAD must be pushed to its upstream before PR creation" >&2
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
requested_base_ref="$base"
if git rev-parse --verify --quiet "refs/remotes/origin/${base}^{commit}" >/dev/null; then
  requested_base_ref="origin/$base"
fi
requested_base_sha="$(git rev-parse --verify "${requested_base_ref}^{commit}")"
if [[ "$requested_base_sha" != "$base_sha" ]]; then
  echo "PR base $base is not the reviewed preflight base $base_ref" >&2
  exit 1
fi

tmp_body="$(mktemp)"
cleanup() {
  rm -f "$tmp_body"
}
trap cleanup EXIT
sed '/<!-- align-preflight-/d' "$body_file" >"$tmp_body"
{
  printf '\n<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$head_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$base"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:clean -->\n'
  printf '<!-- align-preflight-reviewer:%s -->\n' "$reviewer"
} >>"$tmp_body"

gh pr create --draft --base "$base" --title "$title" --body-file "$tmp_body"
