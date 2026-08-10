#!/usr/bin/env bash
# Shared verification-tier classifier for the PR machinery.
#
# One definition, sourced by both the local gate (scripts/pre-pr.sh) and the
# CI attestation checker (scripts/check-pr-preflight.sh), so the enforced tier
# and the recorded tier cannot drift — the duplicated-validator failure class
# CLAUDE.md calls out.
#
# `pr_tier_library_changed <base_sha> <head_sha>` succeeds when the diff touches
# anything whose blast radius exceeds one focused owner check. It FAILS CLOSED:
# a path that matches no known tooling shape counts as library.
#
# Tooling (light) is deliberately narrow — only inputs that cannot change
# compiled behavior beyond their own owner test:
#   * leaf owner tests            crates/<crate>/tests/<name>.rs
#   * prose                       *.md, docs/**
# Everything else is library (full gate), including the verification machinery
# itself (scripts/, .github/): those files gate every future PR, so they keep
# the independent review even though they compile nothing.

pr_tier_path_is_library() {
  case "$1" in
    *.md | docs/*) return 1 ;;
    crates/*)
      local rest="${1#crates/}"
      rest="${rest#*/}"
      case "$rest" in
        # Shared harnesses, fixtures, and golden baselines reach every suite.
        tests/common* | tests/helpers/* | tests/fixtures/* | tests/golden/*) return 0 ;;
        # A nested module under tests/ is shared infrastructure, not a leaf owner.
        tests/*/*) return 0 ;;
        tests/*.rs) return 1 ;;
        *) return 0 ;;
      esac
      ;;
    *) return 0 ;;
  esac
}

pr_tier_library_changed() {
  local base_sha="$1" head_sha="$2" path
  # An uncomputable diff cannot be proven light: fail closed.
  git diff --no-renames --name-only "$base_sha...$head_sha" >/dev/null 2>&1 || return 0
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    if pr_tier_path_is_library "$path"; then
      return 0
    fi
  done < <(git diff --no-renames --name-only "$base_sha...$head_sha")
  return 1
}
