#!/usr/bin/env bash
# CI-side validation of the HEAD-bound attestation inserted by open-pr.sh.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: scripts/check-pr-preflight.sh HEAD_SHA BASE_REF BASE_SHA BODY_FILE" >&2
  exit 2
fi
head_sha="$1"
base_ref="$2"
base_sha="$3"
body_file="$4"
[[ "$head_sha" =~ ^[0-9a-f]{40}$ ]] || {
  echo "invalid PR head SHA: $head_sha" >&2
  exit 2
}
[[ "$base_ref" =~ ^[A-Za-z0-9._/-]+$ && "$base_sha" =~ ^[0-9a-f]{40}$ ]] || {
  echo "invalid PR base: $base_ref at $base_sha" >&2
  exit 2
}
[[ -f "$body_file" ]] || {
  echo "PR body file does not exist: $body_file" >&2
  exit 2
}

grep -Fqx '<!-- align-preflight-version:1 -->' "$body_file" || {
  echo "missing preflight version attestation; use scripts/open-pr.sh" >&2
  exit 1
}
grep -Fqx "<!-- align-preflight-head:$head_sha -->" "$body_file" || {
  echo "preflight attestation is missing or stale for HEAD $head_sha" >&2
  exit 1
}
grep -Fqx "<!-- align-preflight-base-ref:$base_ref -->" "$body_file" || {
  echo "preflight attestation does not belong to base $base_ref" >&2
  exit 1
}
grep -Fqx "<!-- align-preflight-base-sha:$base_sha -->" "$body_file" || {
  echo "preflight attestation is stale for base $base_sha" >&2
  exit 1
}
grep -Fqx '<!-- align-preflight-review:clean -->' "$body_file" || {
  echo "preflight adversarial review is not recorded as clean" >&2
  exit 1
}
grep -Eq '^<!-- align-preflight-reviewer:[^[:space:]<>]+ -->$' "$body_file" || {
  echo "preflight adversarial reviewer identity is missing" >&2
  exit 1
}
grep -Fqx "<!-- align-post-review-head:$head_sha -->" "$body_file" || {
  echo "post-open review is missing or stale for HEAD $head_sha" >&2
  exit 1
}
grep -Fqx "<!-- align-post-review-base-sha:$base_sha -->" "$body_file" || {
  echo "post-open review is missing or stale for base $base_sha" >&2
  exit 1
}
