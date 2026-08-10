#!/usr/bin/env bash
# Validate the minimal HEAD/base/review attestation in a PR body.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: scripts/check-pr-preflight.sh HEAD_SHA BASE_REF BASE_SHA BODY_FILE" >&2
  exit 2
fi
head_sha="$1"
base_ref="$2"
base_sha="$3"
body_file="$4"
[[ "$head_sha" =~ ^[0-9a-f]{40}$ && "$base_sha" =~ ^[0-9a-f]{40}$ && -f "$body_file" ]] || exit 2

required=(
  "<!-- align-preflight-version:1 -->"
  "<!-- align-preflight-head:$head_sha -->"
  "<!-- align-preflight-base-ref:$base_ref -->"
  "<!-- align-preflight-base-sha:$base_sha -->"
)
for marker in "${required[@]}"; do
  grep -Fqx "$marker" "$body_file" || { echo "missing or stale preflight marker: $marker" >&2; exit 1; }
done

state="$(sed -n 's/^<!-- align-preflight-review:\([^[:space:]<>]*\) -->$/\1/p' "$body_file")"
review_head="$(sed -n 's/^<!-- align-preflight-review-head:\([0-9a-f]*\) -->$/\1/p' "$body_file")"
reviewer="$(sed -n 's/^<!-- align-preflight-reviewer:\([^[:space:]<>]*\) -->$/\1/p' "$body_file")"
[[ "$review_head" =~ ^[0-9a-f]{40}$ && "$reviewer" =~ ^[A-Za-z0-9._-]+$ ]] || {
  echo "invalid review attestation" >&2
  exit 1
}
case "$state" in
  clean) [[ "$review_head" == "$head_sha" && "$reviewer" != docs-only ]] ;;
  fixed)
    [[ "$review_head" != "$head_sha" && "$reviewer" != docs-only ]] &&
      git merge-base --is-ancestor "$review_head" "$head_sha"
    ;;
  docs-only) [[ "$review_head" == "$head_sha" && "$reviewer" == docs-only ]] ;;
  tooling) [[ "$review_head" == "$head_sha" && "$reviewer" == tooling ]] ;;
  *) false ;;
esac || { echo "invalid review state: $state" >&2; exit 1; }

# The attestation states what tier the author claimed; recompute it here so a
# hand-written body cannot carry a library change under the light attestation.
# Both this checker and the classifier it sources come from the trusted base.
case "$state" in
  docs-only|tooling)
    tier_lib="$(dirname "$0")/pr-tier.sh"
    [[ -r "$tier_lib" ]] || { echo "missing tier classifier" >&2; exit 1; }
    # shellcheck source=scripts/pr-tier.sh
    . "$tier_lib"
    if pr_tier_library_changed "$base_sha" "$head_sha"; then
      echo "attestation claims '$state' but the diff changes library-tier paths" >&2
      exit 1
    fi
    ;;
esac
