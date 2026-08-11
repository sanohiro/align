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

# The attestation binds the branch's MERGE BASE with the base branch, not the
# base branch tip: `github.event.pull_request.base.sha` moves whenever another
# PR lands on main, which would otherwise invalidate a stamp for a branch whose
# own content never changed. Normalize here so this trusted-base checker owns
# the definition, whichever of the two the caller passed: the merge base of HEAD
# and an ancestor merge base is that same merge base, so this is idempotent.
# Fails closed — a base whose merge base cannot be computed is not attestable.
merge_base="$(git merge-base "$head_sha" "$base_sha" 2>/dev/null || true)"
[[ "$merge_base" =~ ^[0-9a-f]{40}$ ]] || {
  echo "cannot compute the merge base of $head_sha and $base_sha" >&2
  exit 1
}
base_sha="$merge_base"

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
    # The reviewed candidate must be a strict descendant of base and an
    # ancestor of the final HEAD: a log claiming a review of the branch point
    # itself is a review of nothing.
    [[ "$review_head" != "$head_sha" && "$review_head" != "$base_sha" && "$reviewer" != docs-only ]] &&
      git merge-base --is-ancestor "$base_sha" "$review_head" &&
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
