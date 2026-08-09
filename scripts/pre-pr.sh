#!/usr/bin/env bash
# Record the one review cycle and the final verification against the current HEAD.
set -euo pipefail

usage() {
  echo "usage: scripts/pre-pr.sh [--docs-only | --reviewer ID --review-log FILE [--findings-fixed]] [--base REF] [--owner-test LABEL] [-- COMMAND ...]" >&2
  exit 2
}

base="origin/main"
reviewer=""
review_log=""
owner_test="none"
docs_only=false
findings_fixed=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --docs-only) docs_only=true; shift ;;
    --findings-fixed) findings_fixed=true; shift ;;
    --base) [[ $# -ge 2 ]] || usage; base="$2"; shift 2 ;;
    --reviewer) [[ $# -ge 2 ]] || usage; reviewer="$2"; shift 2 ;;
    --review-log) [[ $# -ge 2 ]] || usage; review_log="$2"; shift 2 ;;
    --owner-test) [[ $# -ge 2 ]] || usage; owner_test="$2"; shift 2 ;;
    --) shift; break ;;
    *) usage ;;
  esac
done

git rev-parse --is-inside-work-tree >/dev/null
base_sha="$(git rev-parse --verify "${base}^{commit}")"
head_sha="$(git rev-parse HEAD)"
branch="$(git branch --show-current)"
[[ -n "$branch" && "$branch" != "main" ]] || {
  echo "preflight requires a named non-main branch" >&2
  exit 1
}
[[ -z "$(git status --porcelain)" ]] || {
  echo "preflight requires a clean worktree" >&2
  exit 1
}
git diff --quiet "$base_sha"...HEAD && {
  echo "preflight found no change against $base" >&2
  exit 1
}

review_head="$head_sha"
review_state="docs-only"
if [[ "$docs_only" == true ]]; then
  [[ -z "$reviewer" && -z "$review_log" && "$findings_fixed" == false ]] || {
    echo "--docs-only does not accept review arguments" >&2
    exit 2
  }
  reviewer="docs-only"
else
  [[ "$reviewer" =~ ^[A-Za-z0-9._-]+$ && -f "$review_log" ]] || {
    echo "code preflight requires --reviewer ID and --review-log FILE" >&2
    exit 2
  }
  last_nonempty="$(awk 'NF { line=$0 } END { print line }' "$review_log")"
  case "$last_nonempty" in
    ALIGN_REVIEW_VERDICT=CLEAN) verdict="CLEAN" ;;
    ALIGN_REVIEW_VERDICT=FINDINGS) verdict="FINDINGS" ;;
    *)
      echo "review log must end with a CLEAN or FINDINGS verdict" >&2
      exit 1
      ;;
  esac
  review_head="$(sed -n 's/^ALIGN_REVIEW_HEAD=//p' "$review_log" | sed -n '1p')"
  review_base="$(sed -n 's/^ALIGN_REVIEW_BASE=//p' "$review_log" | sed -n '1p')"
  [[ "$review_head" =~ ^[0-9a-f]{40}$ && "$review_base" =~ ^[0-9a-f]{40}$ ]] || {
    echo "review log has no initial HEAD/base binding" >&2
    exit 1
  }
  [[ "$review_base" == "$base_sha" ]] || {
    echo "review log belongs to another base" >&2
    exit 1
  }
  case "$verdict:$findings_fixed" in
    CLEAN:false)
      [[ "$review_head" == "$head_sha" ]] || {
        echo "a clean review must belong to final HEAD" >&2
        exit 1
      }
      review_state="clean"
      ;;
    FINDINGS:true)
      [[ "$review_head" != "$head_sha" ]] || {
        echo "--findings-fixed requires a later fix commit" >&2
        exit 1
      }
      git merge-base --is-ancestor "$review_head" "$head_sha" || {
        echo "review HEAD is not an ancestor of final HEAD" >&2
        exit 1
      }
      review_state="fixed"
      ;;
    FINDINGS:false)
      echo "review findings are open; fix them once and pass --findings-fixed" >&2
      exit 1
      ;;
    CLEAN:true)
      echo "--findings-fixed does not apply to a clean review" >&2
      exit 2
      ;;
  esac
fi

rust_changed=false
non_documentation_changed=false
while IFS= read -r path; do
  case "$path" in *.md|docs/*) ;; *) non_documentation_changed=true ;; esac
  case "$path" in
    crates/*|Cargo.toml|Cargo.lock|rust-toolchain*|.cargo/*) rust_changed=true ;;
  esac
  case "$path" in *.sh) [[ ! -f "$path" ]] || bash -n "$path" ;; esac
done < <(git diff --no-renames --name-only "$base_sha"...HEAD)
[[ "$docs_only" == false || "$non_documentation_changed" == false ]] || {
  echo "--docs-only requires Markdown/documentation changes only" >&2
  exit 1
}

git diff --check "$base_sha"...HEAD
if [[ "$docs_only" == false && $# -eq 0 ]]; then
  echo "code preflight requires one focused owner verification after --" >&2
  exit 1
fi
# Fail cheaply at the changed boundary before paying for the cumulative gate.
if [[ $# -gt 0 ]]; then
  "$@"
fi
if [[ "$rust_changed" == true ]]; then
  scripts/lint-ratchet.sh
  scripts/test-pr.sh
  # Clippy keeps its own target dir: clippy and build/test record incompatible
  # fingerprints in a shared dir, so alternating them forces a near-full
  # rebuild in both directions. A dedicated dir makes the warm clippy pass
  # incremental and stops it from invalidating the test build cache.
  CARGO_TARGET_DIR=target/clippy \
    scripts/cargo.sh clippy --workspace --lib --bins --locked -- -D warnings
fi
[[ "$(git rev-parse HEAD)" == "$head_sha" && -z "$(git status --porcelain)" ]] || {
  echo "HEAD or worktree changed during preflight" >&2
  exit 1
}

stamp_dir="$(git rev-parse --git-path align-preflight)"
mkdir -p "$stamp_dir"
stamp="$stamp_dir/$head_sha"
{
  printf 'version=1\n'
  printf 'kind=%s\n' "$([[ "$docs_only" == true ]] && echo docs-only || echo code)"
  printf 'head=%s\n' "$head_sha"
  printf 'base_ref=%s\n' "$base"
  printf 'base_sha=%s\n' "$base_sha"
  printf 'review_head=%s\n' "$review_head"
  printf 'review_state=%s\n' "$review_state"
  printf 'reviewer=%s\n' "$reviewer"
  printf 'owner_test=%s\n' "$owner_test"
  printf 'created_at=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} >"$stamp"

echo "preflight recorded for $head_sha ($review_state; reviewer $reviewer; owner $owner_test)"
