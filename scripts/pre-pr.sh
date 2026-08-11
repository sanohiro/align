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
  ALIGN_REVIEW_BASE=<40-hex merge base of HEAD and --base (git merge-base HEAD
                     origin/main), not the base branch tip>
  ...free-form review record...
  ALIGN_REVIEW_VERDICT=CLEAN | ALIGN_REVIEW_VERDICT=FINDINGS   (last non-empty line)

A CLEAN verdict must belong to the exact final HEAD. FINDINGS requires
--findings-fixed and a later fix commit descending from the reviewed HEAD.
The recorded stamp binds the final HEAD: push and open the PR immediately,
without amending, rebasing, or committing in between. It binds the branch's
merge base, not the base branch tip, so another PR landing on main does not
invalidate it; rebasing or merging main in does, because that changes the
branch.
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
# Every base binding below is the MERGE BASE of HEAD and the base branch, never
# the base branch tip. The tip is not a property of this branch: an unrelated PR
# merging into main would otherwise invalidate an attestation and a review for a
# branch whose own content never changed, forcing a rebase and a full CI rerun.
# The merge base only moves when the branch itself moves (rebase, amend, or a
# merge of main into the branch), which is exactly when the evidence really is
# stale. It is also the basis of the `base...HEAD` diffs used throughout, so the
# reviewed diff, the classified diff, and the recorded base now share one
# definition.
base_tip="$(git rev-parse --verify "${base}^{commit}")"
base_sha="$(git merge-base HEAD "$base_tip" 2>/dev/null || true)"
[[ "$base_sha" =~ ^[0-9a-f]{40}$ ]] || {
  echo "preflight cannot compute the merge base of HEAD and $base" >&2
  exit 1
}
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

# Review-round tripwire. The first commit in base..HEAD is always the
# implementation, uncounted regardless of its subject — a branch whose every
# commit happens to be fix-titled still has that first commit excluded, so it
# cannot dodge the tripwire by simply never having a non-fix commit at all.
# Every commit after that whose subject is fix-titled is a review round;
# three of them means the review has become the discovery loop for one
# root-cause class, which CLAUDE.md requires closing by re-opening the
# closure matrix instead of patching the next reported cell. The count spans
# the whole range and an interleaved non-fix commit (a docs tweak, a rename)
# never resets it: the discovery-loop pattern is about how many fix rounds a
# root cause absorbed in total, not whether they happened to stay
# consecutive. Titling a review-round commit outside the `fix` prefix
# excludes only that one commit from the count — it does not reset any other
# counted commit — and an author who never uses `fix` for a review round
# escapes the tripwire entirely; both are visible, auditable choices in the
# history, not hidden ones. Releasing the gate takes a separate deliberate
# act: ANY commit after the implementation (fix-titled or not) that both
# changes an authoritative docs/impl plan or audit (not a translated mirror)
# and records a `Closure-Matrix-Reopened:` trailer naming the axis.
review_streak=()
post_implementation_commits=()
seen_implementation=false
while IFS= read -r entry; do
  if [[ "$seen_implementation" == false ]]; then
    seen_implementation=true
    continue
  fi
  post_implementation_commits+=("${entry%% *}")
  case "${entry#* }" in
    fix*|Fix*) review_streak+=("${entry%% *}") ;;
  esac
done < <(git log --reverse --format='%H %s' "$base_sha"..HEAD)
if [[ "$docs_only" == false && "${#review_streak[@]}" -ge 3 ]]; then
  matrix_reopened=false
  for commit in "${post_implementation_commits[@]}"; do
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
    echo "preflight: ${#review_streak[@]} post-implementation review-fix commits without re-opening a closure matrix" >&2
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
    echo "review log belongs to another base: it records $review_base, and this" >&2
    echo "  branch's merge base with $base is $base_sha. Main advancing does not" >&2
    echo "  cause this; rebasing or merging main into the branch does, and that" >&2
    echo "  really does change the reviewed diff." >&2
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
if [[ "$docs_only" == true ]]; then
  [[ "$non_documentation_changed" == false ]] || {
    echo "--docs-only requires Markdown/documentation changes only" >&2
    exit 1
  }
  # The per-path *.md check alone misses two things the shared classifier
  # already knows: a compiled-prose document (PR_TIER_COMPILED_PROSE, e.g.
  # docs/impl/pkg-design/web.md) is a source input to a test binary despite
  # its extension, and any deletion is library tier regardless of path shape.
  # Require its full verdict too, so a --docs-only PR that would fail CI's
  # recomputed tier cannot pass here first.
  [[ "$library_changed" == false ]] || {
    echo "--docs-only requires Markdown/documentation changes only (scripts/pr-tier.sh still classifies this diff as library tier — a compiled-prose path or a deletion)" >&2
    exit 1
  }
fi

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
  # Trap-based so the logs are removed on any exit from here on (success,
  # gate failure, or an interrupted run), not only the two explicit paths
  # below.
  trap 'rm -f "$test_pr_log" "$clippy_log"' EXIT
  echo "preflight: scripts/test-pr.sh log: $test_pr_log" >&2
  echo "preflight: Clippy log: $clippy_log" >&2
  scripts/test-pr.sh >"$test_pr_log" 2>&1 &
  test_pr_pid=$!
  # Clippy keeps its own target dir: clippy and build/test record incompatible
  # fingerprints in a shared dir, so alternating them forces a near-full
  # rebuild in both directions. A dedicated dir makes the warm clippy pass
  # incremental and stops it from invalidating the test build cache. Cap its
  # job count so it does not fight scripts/test-pr.sh for every core on the
  # same machine while both run concurrently.
  ncpu="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"
  [[ "$ncpu" =~ ^[0-9]+$ && "$ncpu" -ge 1 ]] || ncpu=4
  clippy_jobs=$(( ncpu / 2 ))
  [[ "$clippy_jobs" -ge 1 ]] || clippy_jobs=1
  CARGO_TARGET_DIR=target/clippy CARGO_BUILD_JOBS="$clippy_jobs" \
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
    echo "preflight: scripts/test-pr.sh exited $test_pr_status, Clippy exited $clippy_status" >&2
    exit 1
  fi
  cat "$test_pr_log"
  cat "$clippy_log"
  trap - EXIT
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
