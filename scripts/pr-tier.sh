#!/usr/bin/env bash
# Shared verification-tier classifier for the PR machinery.
#
# One definition, sourced by both the local gate (scripts/pre-pr.sh) and the CI
# attestation checker (scripts/check-pr-preflight.sh), so the enforced tier and
# the recorded tier cannot drift — the duplicated-validator failure class
# CLAUDE.md calls out.
#
# `pr_tier_library_changed <base_sha> <head_sha>` succeeds when the diff touches
# anything whose blast radius exceeds one focused owner check. It FAILS CLOSED
# in every direction: an unknown path shape, an uncomputable diff, and any
# deletion all count as library.
#
# Path shape alone is not sufficient evidence that a file is light. Two classes
# of file look inert and are not:
#
#   * gate content — the test targets scripts/test-pr.sh names ARE the bounded
#     gate, so weakening one is strictly worse than editing the runner;
#   * compiled prose — a document reached by include_str!/include_bytes! is a
#     source input to a test binary, not documentation.
#
# Both are listed below and pinned by scripts/test-pr-workflow.sh, which
# recomputes the two sets from the repository and fails when a list goes stale.
# Adding a gate target or embedding a new document therefore forces this file
# to be updated in the same change.

# Test targets named in scripts/test-pr.sh (bounded-gate content).
PR_TIER_GATE_TESTS="m0 examples summary effect_fail_closed"

# Documents embedded into compiled test binaries.
PR_TIER_COMPILED_PROSE="docs/impl/pkg-design/web.md"

pr_tier_path_is_library() {
  local path="$1" candidate rest base
  for candidate in $PR_TIER_COMPILED_PROSE; do
    [[ "$path" == "$candidate" ]] && return 0
  done
  case "$path" in
    *.md | docs/*.md) return 1 ;;
    crates/*)
      rest="${path#crates/}"
      rest="${rest#*/}"
      case "$rest" in
        # Shared harnesses, fixtures, and golden baselines reach every suite.
        tests/common* | tests/helpers/* | tests/fixtures/* | tests/golden/*) return 0 ;;
        # A nested module under tests/ is shared infrastructure, not a leaf owner.
        tests/*/*) return 0 ;;
        tests/*.rs)
          base="${rest#tests/}"
          base="${base%.rs}"
          for candidate in $PR_TIER_GATE_TESTS; do
            [[ "$base" == "$candidate" ]] && return 0
          done
          return 1
          ;;
        *) return 0 ;;
      esac
      ;;
    *) return 0 ;;
  esac
}

pr_tier_library_changed() {
  local base_sha="$1" head_sha="$2" path status
  # An uncomputable diff cannot be proven light.
  git diff --no-renames --name-only "$base_sha...$head_sha" >/dev/null 2>&1 || return 0
  # A deletion removes coverage rather than adding it; never light.
  while IFS= read -r path; do
    [[ -z "$path" ]] || return 0
  done < <(git diff --no-renames --diff-filter=D --name-only "$base_sha...$head_sha")
  status=1
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    if pr_tier_path_is_library "$path"; then
      status=0
      break
    fi
  done < <(git diff --no-renames --name-only "$base_sha...$head_sha")
  return "$status"
}
