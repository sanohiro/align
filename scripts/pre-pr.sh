#!/usr/bin/env bash
# Record the one review cycle and the final verification against the current HEAD.
set -euo pipefail

usage() {
  cat >&2 << 'USAGE'
usage: scripts/pre-pr.sh [--docs-only | --reviewer ID --review-log FILE [--findings-fixed]] [--base REF] [--owner-test LABEL] [-- COMMAND ...]

The tier follows the changed paths (scripts/pr-tier.sh, fail-closed):
  docs-only  *.md files only               pass --docs-only; no owner command
  tooling    leaf crates/*/tests/*.rs      review optional; one owner command
             that is not gate content,     (plus the ratchet when Rust changed)
             and uncompiled prose
  code       everything else, including    review required; owner command, then
             scripts/, .github/, shared    the bounded gate and Clippy
             test harnesses and fixtures

A code PR needs all of:
  --reviewer ID            reviewer identity ([A-Za-z0-9._-]+)
  --review-log FILE        review evidence file, kept OUTSIDE the repository
                           (an untracked file here fails the clean-worktree check).
                           .git/ (e.g. .git/align-review-<sha>.log, the
                           scripts/new-review-log.sh default) or a path outside
                           the repository entirely both qualify: `git status
                           --porcelain` never reports files under .git/.
  -- COMMAND ...           one focused owner verification command, e.g.
                           -- scripts/cargo.sh test -p align_driver --test pkg_db_q6

The review log must contain, in this order:
  ALIGN_REVIEW_HEAD=<40-hex reviewed candidate SHA>
  ALIGN_REVIEW_BASE=<40-hex SHA of --base's TIP (git rev-parse origin/main), not the merge-base>
  ...free-form review record...
  ALIGN_REVIEW_VERDICT=CLEAN | ALIGN_REVIEW_VERDICT=FINDINGS   (last non-empty line)

A CLEAN verdict must belong to the exact final HEAD. FINDINGS requires
--findings-fixed and a later fix commit descending from the reviewed HEAD.
The recorded stamp binds the final HEAD: push and open the PR immediately,
without amending, rebasing, or committing in between.
USAGE
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
rust_changed=false
non_documentation_changed=false
# shellcheck source=scripts/pr-tier.sh
. "$(dirname "$0")/pr-tier.sh"
library_changed=false
pr_tier_library_changed "$base_sha" "$head_sha" && library_changed=true
while IFS= read -r path; do
  # Match scripts/pr-tier.sh's documentation classification exactly: only
  # *.md files are documentation. A non-Markdown file under docs/ (a fixture,
  # a script, a generated asset) is not.
  case "$path" in *.md | docs/*.md) ;; *) non_documentation_changed=true ;; esac
  case "$path" in
    crates/*|Cargo.toml|Cargo.lock|rust-toolchain*|.cargo/*) rust_changed=true ;;
  esac
  case "$path" in *.sh) [[ ! -f "$path" ]] || bash -n "$path" ;; esac
done < <(git diff --no-renames --name-only "$base_sha"...HEAD)

# Review-round tripwire. Every `fix` commit after the implementation is a
# review round; three of them means the review has become the discovery loop
# for one root-cause class, which CLAUDE.md requires closing by re-opening the
# closure matrix instead of patching the next reported cell. The count spans
# the whole base..HEAD range once the implementation (the first non-fix
# commit) has been seen — an interleaved non-fix commit (a docs tweak, a
# rename) does not reset it, since the discovery-loop pattern is about how
# many fix rounds a root cause absorbed in total, not whether they happened to
# stay consecutive. Releasing the gate takes a deliberate, auditable act: a
# commit anywhere in the counted set that both changes an authoritative
# docs/impl plan or audit (not a translated mirror) and records a
# `Closure-Matrix-Reopened:` trailer naming the axis. Renaming a commit away
# from `fix` also silences the counter — that is a visible choice in the
# history, not a hidden one.
review_streak=()
seen_implementation=false
while IFS= read -r entry; do
  case "${entry#* }" in
    fix*|Fix*)
      if [[ "$seen_implementation" == true ]]; then
        review_streak+=("${entry%% *}")
      fi
      ;;
    *) seen_implementation=true ;;
  esac
done < <(git log --reverse --format='%H %s' "$base_sha"..HEAD)
if [[ "$docs_only" == false && "${#review_streak[@]}" -ge 3 ]]; then
  matrix_reopened=false
  for commit in "${review_streak[@]}"; do
    git log -1 --format='%B' "$commit" | grep -q '^Closure-Matrix-Reopened:' || continue
    while IFS= read -r path; do
      case "$path" in
        docs/impl/*/ja/*|docs/impl/ja/*) ;;
        # Authoritative contracts live in docs/impl for compiler and package
        # capabilities and in CLAUDE.md for the process machinery itself.
        docs/impl/*|CLAUDE.md) matrix_reopened=true ;;
      esac
    done < <(git show --no-renames --name-only --format= "$commit")
  done
  [[ "$matrix_reopened" == true ]] || {
    echo "preflight: ${#review_streak[@]} consecutive review-fix commits without re-opening a closure matrix" >&2
    echo "  Repeatedly patching individually reported cells means the matrix missed an axis." >&2
    echo "  Enumerate that axis in the owning docs/impl plan, audit, or CLAUDE.md, fix the class" >&2
    echo "  in one pass," >&2
    echo "  and commit it with a 'Closure-Matrix-Reopened: <axis>' trailer." >&2
    exit 1
  }
fi


if [[ "$docs_only" == true ]]; then
  [[ -z "$reviewer" && -z "$review_log" && "$findings_fixed" == false ]] || {
    echo "--docs-only does not accept review arguments" >&2
    exit 2
  }
  reviewer="docs-only"
elif [[ "$library_changed" == false && -z "$reviewer" && -z "$review_log" ]]; then
  # Tooling tier: a leaf owner test that is not bounded-gate content, or prose
  # that is not compiled into a test binary. Its blast radius is the target the
  # focused owner check already compiles and runs, so an independent review is
  # optional here. Passing --reviewer with a log still records one exactly as
  # the library tier does.
  [[ "$findings_fixed" == false ]] || {
    echo "--findings-fixed requires a review log" >&2
    exit 2
  }
  reviewer="tooling"
  review_state="tooling"
else
  [[ "$reviewer" =~ ^[A-Za-z0-9._-]+$ && -f "$review_log" ]] || {
    echo "preflight requires --reviewer ID and --review-log FILE for a library change" >&2
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
      # The reviewed candidate must be a strict descendant of base: a log
      # claiming a review of the branch point itself is a review of nothing.
      [[ "$review_head" != "$base_sha" ]] && git merge-base --is-ancestor "$base_sha" "$review_head" || {
        echo "review HEAD is not a strict descendant of base" >&2
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
fi
# The cumulative Rust gate is worth its cost only when the diff can change
# compiled behavior: library tier AND actual Rust build inputs.
if [[ "$library_changed" == true && "$rust_changed" == true ]]; then
  # scripts/test-pr.sh and Clippy already use disjoint target dirs by design
  # (see the CARGO_TARGET_DIR comment below), so nothing stops them from
  # running concurrently instead of paying their wall-clock cost twice.
  test_pr_log="$(mktemp)"
  clippy_log="$(mktemp)"
  scripts/test-pr.sh >"$test_pr_log" 2>&1 &
  test_pr_pid=$!
  # Clippy keeps its own target dir: clippy and build/test record incompatible
  # fingerprints in a shared dir, so alternating them forces a near-full
  # rebuild in both directions. A dedicated dir makes the warm clippy pass
  # incremental and stops it from invalidating the test build cache.
  CARGO_TARGET_DIR=target/clippy \
    scripts/cargo.sh clippy --workspace --lib --bins --locked -- -D warnings \
    >"$clippy_log" 2>&1 &
  clippy_pid=$!
  test_pr_status=0
  clippy_status=0
  wait "$test_pr_pid" || test_pr_status=$?
  wait "$clippy_pid" || clippy_status=$?
  if [[ "$test_pr_status" -ne 0 || "$clippy_status" -ne 0 ]]; then
    echo "--- scripts/test-pr.sh output (exit $test_pr_status) ---" >&2
    cat "$test_pr_log" >&2
    echo "--- Clippy output (exit $clippy_status) ---" >&2
    cat "$clippy_log" >&2
    rm -f "$test_pr_log" "$clippy_log"
    echo "preflight: scripts/test-pr.sh exited $test_pr_status, Clippy exited $clippy_status" >&2
    exit 1
  fi
  cat "$test_pr_log"
  cat "$clippy_log"
  rm -f "$test_pr_log" "$clippy_log"
fi
[[ "$(git rev-parse HEAD)" == "$head_sha" && -z "$(git status --porcelain)" ]] || {
  echo "HEAD or worktree changed during preflight" >&2
  exit 1
}

if [[ "$docs_only" == true ]]; then
  tier=docs-only
elif [[ "$library_changed" == true ]]; then
  tier=code
else
  tier=tooling
fi
stamp_dir="$(git rev-parse --git-path align-preflight)"
mkdir -p "$stamp_dir"
stamp="$stamp_dir/$head_sha"
{
  printf 'version=1\n'
  printf 'kind=%s\n' "$tier"
  printf 'head=%s\n' "$head_sha"
  printf 'base_ref=%s\n' "$base"
  printf 'base_sha=%s\n' "$base_sha"
  printf 'review_head=%s\n' "$review_head"
  printf 'review_state=%s\n' "$review_state"
  printf 'reviewer=%s\n' "$reviewer"
  printf 'owner_test=%s\n' "$owner_test"
  printf 'created_at=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} >"$stamp"

echo "preflight recorded for $head_sha ($tier tier; $review_state; reviewer $reviewer; owner $owner_test)"
