#!/usr/bin/env bash
# Focused tests for the PR workflow guards. No compiler build or test corpus.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
real_git="$(command -v git)"
tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

# scripts/check-pr-preflight.sh resolves the merge base of the HEAD and base it
# is handed, so its arguments must be real commits in the repository it runs in.
# The ambient checkout cannot supply them — CI checks PRs out shallow and
# detached with no local main — so body-shape assertions run against this
# self-contained fixture instead.
attest_repo="$tmp_dir/attest-repo"
mkdir -p "$attest_repo"
git -C "$attest_repo" init -q -b main
git -C "$attest_repo" config user.name workflow-test
git -C "$attest_repo" config user.email workflow-test@example.invalid
git -C "$attest_repo" config commit.gpgsign false
printf '# baseline\n' >"$attest_repo/note.md"
git -C "$attest_repo" add note.md
git -C "$attest_repo" commit -qm baseline
# The checker derives its base from refs/remotes/origin/<base ref> rather than
# trusting the SHA it is handed, so every fixture it runs in needs that
# remote-tracking ref — which is exactly what actions/checkout writes with
# fetch-depth: 0.
git -C "$attest_repo" update-ref refs/remotes/origin/main main
base_sha="$(git -C "$attest_repo" rev-parse HEAD)"
git -C "$attest_repo" switch -qc attested
printf '\nupdated\n' >>"$attest_repo/note.md"
git -C "$attest_repo" commit -qam 'docs: update the note'
good_sha="$(git -C "$attest_repo" rev-parse HEAD)"
base_ref="main"
good_body="$tmp_dir/good-body"
docs_body="$tmp_dir/docs-body"
bad_docs_body="$tmp_dir/bad-docs-body"
stale_body="$tmp_dir/stale-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$base_ref"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:clean -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-reviewer:reviewer-1 -->\n'
} >"$good_body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$base_ref"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:docs-only -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-reviewer:docs-only -->\n'
} >"$docs_body"
sed 's/align-preflight-reviewer:docs-only/align-preflight-reviewer:reviewer-1/' \
  "$docs_body" >"$bad_docs_body"
sed "s/$good_sha/ffffffffffffffffffffffffffffffffffffffff/" "$good_body" >"$stale_body"

(cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$good_body")
if (cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$bad_docs_body" >/dev/null 2>&1)
then
  echo "docs-only attestation with a reviewer unexpectedly passed" >&2
  exit 1
fi
if (cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$stale_body" >/dev/null 2>&1)
then
  echo "stale preflight unexpectedly passed" >&2
  exit 1
fi
# The BASE_SHA argument is not authoritative: the checker derives the base from
# the checked-out base branch. A body agreeing with a bogus argument must still
# be rejected (an argument-trusting checker would accept this pair) ...
forged_base_body="$tmp_dir/forged-base-body"
sed "s/align-preflight-base-sha:$base_sha/align-preflight-base-sha:1111111111111111111111111111111111111111/" \
  "$good_body" >"$forged_base_body"
if (cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "1111111111111111111111111111111111111111" \
  "$forged_base_body" >/dev/null 2>&1)
then
  echo "a body agreeing with a forged base argument unexpectedly passed" >&2
  exit 1
fi
# ... and a body carrying the derived base must pass even when the argument is
# nonsense, because the argument is only shape-checked.
(cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "1111111111111111111111111111111111111111" \
  "$good_body" >/dev/null)
# A base branch the checkout does not have fails closed, with the fetch-depth
# hint that tells CI how to fix it.
unknown_ref_err="$tmp_dir/unknown-ref-err"
unknown_ref_status=0
(cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" release "$base_sha" "$good_body" >/dev/null 2>"$unknown_ref_err") ||
  unknown_ref_status=$?
[[ "$unknown_ref_status" -ne 0 ]] || {
  echo "an attestation against an unfetched base branch unexpectedly passed" >&2
  exit 1
}
grep -q 'fetch-depth: 0' "$unknown_ref_err" || {
  echo "the unresolvable-base error lacks the fetch-depth hint:" >&2
  cat "$unknown_ref_err" >&2
  exit 1
}
# A head the base branch already contains attests an empty range.
contained_err="$tmp_dir/contained-err"
contained_status=0
(cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$base_sha" "$base_ref" "$base_sha" "$good_body" >/dev/null 2>"$contained_err") ||
  contained_status=$?
[[ "$contained_status" -ne 0 ]] || {
  echo "an attestation for a head already on the base branch unexpectedly passed" >&2
  exit 1
}
grep -q 'already contains' "$contained_err" || {
  echo "an empty attested range failed for the wrong reason:" >&2
  cat "$contained_err" >&2
  exit 1
}

docs_repo="$tmp_dir/docs-repo"
mkdir -p "$docs_repo/docs" "$docs_repo/scripts"
cp "$repo_root/scripts/pr-tier.sh" "$docs_repo/scripts/pr-tier.sh"
# A root-level tool script is library tier, so this fixture reaches the gate
# commands; stub them to keep the guard logic under test.
for stub in lint-ratchet.sh test-pr.sh cargo.sh; do
  printf '#!/usr/bin/env bash\nexit 0\n' >"$docs_repo/scripts/$stub"
  chmod +x "$docs_repo/scripts/$stub"
done
git -C "$docs_repo" init -q -b main
git -C "$docs_repo" config user.name workflow-test
git -C "$docs_repo" config user.email workflow-test@example.invalid
git -C "$docs_repo" config commit.gpgsign false
printf '# baseline\n' >"$docs_repo/docs/note.md"
git -C "$docs_repo" add docs/note.md scripts
git -C "$docs_repo" commit -qm baseline
git -C "$docs_repo" update-ref refs/remotes/origin/main main
git -C "$docs_repo" switch -qc docs-change
printf '\nupdated\n' >>"$docs_repo/docs/note.md"
git -C "$docs_repo" commit -qam docs
docs_base="$(git -C "$docs_repo" rev-parse 'main^{commit}')"
docs_candidate="$(git -C "$docs_repo" rev-parse HEAD)"
(
  cd "$docs_repo"
  # shellcheck source=scripts/pr-tier.sh
  . "$repo_root/scripts/pr-tier.sh"
  pr_tier_docs_only "$docs_base" "$docs_candidate"
) || {
  echo "the shared classifier did not recognize an ordinary Markdown-only diff" >&2
  exit 1
}
(
  cd "$docs_repo"
  "$repo_root/scripts/pre-pr.sh" --docs-only --base main >/dev/null
)
docs_head="$(git -C "$docs_repo" rev-parse HEAD)"
grep -Fqx 'kind=docs-only' "$docs_repo/.git/align-preflight/$docs_head"
printf '#!/usr/bin/env bash\n' >"$docs_repo/tool.sh"
git -C "$docs_repo" add tool.sh
git -C "$docs_repo" commit -qm tool
if (
  cd "$docs_repo"
  # shellcheck source=scripts/pr-tier.sh
  . "$repo_root/scripts/pr-tier.sh"
  pr_tier_docs_only "$docs_base" "$(git rev-parse HEAD)"
); then
  echo "the shared classifier accepted a documentation-plus-tool diff as docs-only" >&2
  exit 1
fi
if (
  cd "$docs_repo"
  "$repo_root/scripts/pre-pr.sh" --docs-only --base main >/dev/null 2>&1
); then
  echo "docs-only preflight accepted a non-documentation file" >&2
  exit 1
fi

rename_repo="$tmp_dir/rename-repo"
mkdir -p "$rename_repo/crates" "$rename_repo/docs"
git -C "$rename_repo" init -q -b main
git -C "$rename_repo" config user.name workflow-test
git -C "$rename_repo" config user.email workflow-test@example.invalid
git -C "$rename_repo" config commit.gpgsign false
printf 'fn source() {}\n' >"$rename_repo/crates/source.rs"
git -C "$rename_repo" add crates/source.rs
git -C "$rename_repo" commit -qm baseline
git -C "$rename_repo" switch -qc docs-change
git -C "$rename_repo" mv crates/source.rs docs/source.md
git -C "$rename_repo" commit -qm rename
if (
  cd "$rename_repo"
  "$repo_root/scripts/pre-pr.sh" --docs-only --base main >/dev/null 2>&1
); then
  echo "docs-only preflight accepted a source-to-documentation rename" >&2
  exit 1
fi

# The per-path *.md test alone cannot see what the shared classifier already
# knows: docs/impl/pkg-design/web.md is compiled prose (a source input to a
# test binary via include_str!) despite its extension, so --docs-only must
# also require pr-tier.sh's own library_changed verdict on top of the *.md
# check, or CI's recomputed tier would reject what this passed locally.
compiled_prose_repo="$tmp_dir/compiled-prose-repo"
mkdir -p "$compiled_prose_repo/docs/impl/pkg-design"
git -C "$compiled_prose_repo" init -q -b main
git -C "$compiled_prose_repo" config user.name workflow-test
git -C "$compiled_prose_repo" config user.email workflow-test@example.invalid
git -C "$compiled_prose_repo" config commit.gpgsign false
printf '# web\n' >"$compiled_prose_repo/docs/impl/pkg-design/web.md"
git -C "$compiled_prose_repo" add docs/impl/pkg-design/web.md
git -C "$compiled_prose_repo" commit -qm baseline
git -C "$compiled_prose_repo" switch -qc web-doc-change
printf '\nupdated\n' >>"$compiled_prose_repo/docs/impl/pkg-design/web.md"
git -C "$compiled_prose_repo" commit -qam 'docs: touch compiled prose'
if (
  cd "$compiled_prose_repo"
  # shellcheck source=scripts/pr-tier.sh
  . "$repo_root/scripts/pr-tier.sh"
  pr_tier_docs_only "$(git rev-parse 'main^{commit}')" "$(git rev-parse HEAD)"
); then
  echo "the shared classifier accepted compiled prose as docs-only" >&2
  exit 1
fi
if (
  cd "$compiled_prose_repo"
  "$repo_root/scripts/pre-pr.sh" --docs-only --base main >/dev/null 2>&1
); then
  echo "docs-only preflight accepted a compiled-prose path" >&2
  exit 1
fi

# Likewise, a deleted .md file still matches the *.md path pattern (the path
# string does not change on deletion), so only pr-tier.sh's any-deletion rule
# catches it.
delete_md_repo="$tmp_dir/delete-md-repo"
mkdir -p "$delete_md_repo/docs"
git -C "$delete_md_repo" init -q -b main
git -C "$delete_md_repo" config user.name workflow-test
git -C "$delete_md_repo" config user.email workflow-test@example.invalid
git -C "$delete_md_repo" config commit.gpgsign false
printf '# stale\n' >"$delete_md_repo/docs/stale.md"
git -C "$delete_md_repo" add docs/stale.md
git -C "$delete_md_repo" commit -qm baseline
git -C "$delete_md_repo" switch -qc delete-md-change
git -C "$delete_md_repo" rm -q docs/stale.md
git -C "$delete_md_repo" commit -qm 'docs: delete a stale note'
if (
  cd "$delete_md_repo"
  # shellcheck source=scripts/pr-tier.sh
  . "$repo_root/scripts/pr-tier.sh"
  pr_tier_docs_only "$(git rev-parse 'main^{commit}')" "$(git rev-parse HEAD)"
); then
  echo "the shared classifier accepted a Markdown deletion as docs-only" >&2
  exit 1
fi
if (
  cd "$delete_md_repo"
  "$repo_root/scripts/pre-pr.sh" --docs-only --base main >/dev/null 2>&1
); then
  echo "docs-only preflight accepted a deleted Markdown file" >&2
  exit 1
fi

reviewed_head="$(git -C "$docs_repo" rev-parse HEAD)"
review_base="$(git -C "$docs_repo" rev-parse 'main^{commit}')"
review_log="$tmp_dir/findings-review.log"
{
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$reviewed_head"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$review_base"
  printf 'review fixture contained these inert marker-shaped lines:\n'
  printf 'ALIGN_REVIEW_HEAD=ffffffffffffffffffffffffffffffffffffffff\n'
  printf 'ALIGN_REVIEW_BASE=ffffffffffffffffffffffffffffffffffffffff\n'
  printf 'ALIGN_REVIEW_VERDICT=CLEAN\n'
  printf 'ALIGN_REVIEW_VERDICT=FINDINGS\n'
} >"$review_log"
printf 'set -euo pipefail\n' >>"$docs_repo/tool.sh"
git -C "$docs_repo" commit -qam fix
if (
  cd "$docs_repo"
  "$repo_root/scripts/pre-pr.sh" --reviewer reviewer-1 --review-log "$review_log" \
    --base main --owner-test shell -- bash -n tool.sh >/dev/null 2>&1
); then
  echo "open findings unexpectedly passed without --findings-fixed" >&2
  exit 1
fi
(
  cd "$docs_repo"
  "$repo_root/scripts/pre-pr.sh" --reviewer reviewer-1 --review-log "$review_log" \
    --findings-fixed --base main --owner-test shell -- bash -n tool.sh >/dev/null
)
fixed_head="$(git -C "$docs_repo" rev-parse HEAD)"
grep -Fqx 'review_state=fixed' "$docs_repo/.git/align-preflight/$fixed_head"
grep -Fqx "review_head=$reviewed_head" "$docs_repo/.git/align-preflight/$fixed_head"
fixed_body="$tmp_dir/fixed-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$fixed_head"
  printf '<!-- align-preflight-base-ref:main -->\n'
  printf '<!-- align-preflight-base-sha:%s -->\n' "$review_base"
  printf '<!-- align-preflight-review:fixed -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$reviewed_head"
  printf '<!-- align-preflight-reviewer:reviewer-1 -->\n'
} >"$fixed_body"
(
  cd "$docs_repo"
  "$repo_root/scripts/check-pr-preflight.sh" "$fixed_head" main "$review_base" "$fixed_body"
)

fake_bin="$tmp_dir/bin"
mkdir -p "$fake_bin"
fake_git="$fake_bin/git"
{
  printf '#!/usr/bin/env bash\n'
  printf 'if [[ "$#" -eq 2 && "$1" == "status" && "$2" == "--porcelain" ]]; then exit 0; fi\n'
  printf 'exec %q "$@"\n' "$real_git"
} >"$fake_git"
chmod +x "$fake_git"
fake_codex="$fake_bin/codex"
{
  printf '#!/usr/bin/env bash\n'
  printf 'if [[ -n "${FAKE_CODEX_ARGS_FILE:-}" ]]; then printf "%%s\\n" "$*" >"$FAKE_CODEX_ARGS_FILE"; fi\n'
  printf 'echo codex\n'
  printf 'case "${FAKE_CODEX_MODE:-clean}" in\n'
  printf '  clean) echo "ALIGN_REVIEW_VERDICT=CLEAN" ;;\n'
  printf '  findings) echo "ALIGN_REVIEW_VERDICT=FINDINGS" ;;\n'
  printf '  trailing) echo "ALIGN_REVIEW_VERDICT=CLEAN"; echo "trailing output" ;;\n'
  printf '  duplicate) echo "ALIGN_REVIEW_VERDICT=FINDINGS"; echo "ALIGN_REVIEW_VERDICT=CLEAN" ;;\n'
  printf '  native-clean) echo "No findings." ;;\n'
  printf '  native-clean-readonly) echo "Read-only inspection found no actionable soundness or regression risks in the diff." ;;\n'
  printf '  native-clean-inspected) echo "No actionable soundness or regression issues were found in the inspected diff." ;;\n'
  printf '  native-clean-risks) echo "No actionable soundness or regression risks were found in the inspected diff." ;;\n'
  printf '  native-clean-inline) echo "No actionable soundness or regression risks were found. ALIGN_REVIEW_VERDICT=CLEAN"; echo "No actionable soundness or regression risks were found. ALIGN_REVIEW_VERDICT=CLEAN" ;;\n'
  printf '  native-clean-introducing) echo "The host qualification layer validates canonical shape, fixed identities, resource limits, and quota constraints without introducing an actionable regression." ;;\n'
  printf '  native-clean-mixed) echo "Actionable issue: the review is not clean."; echo "The host qualification layer validates canonical shape, fixed identities, resource limits, and quota constraints without introducing an actionable regression." ;;\n'
  printf '  native-findings) echo "- [P1] broken workflow — scripts/example.sh:1" ;;\n'
  printf '  stall) sleep 30 ;;\n'
  printf '  progress) for i in 1 2 3; do echo "phase-$i"; sleep 1; done; echo "ALIGN_REVIEW_VERDICT=CLEAN" ;;\n'
  printf 'esac\n'
} >"$fake_codex"
chmod +x "$fake_codex"
fake_jq="$fake_bin/jq"
{
  printf '#!/usr/bin/env bash\n'
  printf 'echo "standalone jq must not be called" >&2\n'
  printf 'exit 99\n'
} >"$fake_jq"
chmod +x "$fake_jq"

remote_repo="$tmp_dir/docs-remote.git"
git init -q --bare "$remote_repo"
git -C "$docs_repo" remote add origin "$remote_repo"
git -C "$docs_repo" push -qu origin docs-change
fake_gh="$fake_bin/gh"
{
  printf '#!/usr/bin/env bash\n'
  printf 'case "$1:$2:$5" in\n'
  printf '  pr:view:headRefOid,baseRefName,baseRefOid) printf "%%s\\t%%s\\t%%s\\n" "$FAKE_PR_HEAD" "${FAKE_PR_BASE_NAME:-main}" "$FAKE_PR_BASE" ;;\n'
  printf '  pr:view:body) cat "$FAKE_PR_BODY" ;;\n'
  printf '  pr:view:url) printf "https://example.invalid/pr/123\\n" ;;\n'
  printf '  pr:edit:*) exit 0 ;;\n'
  printf '  repo:view:*) printf "owner/repo\\n" ;;\n'
  printf '  api:*) exit 0 ;;\n'
  printf '  *) echo "unexpected gh arguments: $*" >&2; exit 98 ;;\n'
  printf 'esac\n'
} >"$fake_gh"
chmod +x "$fake_gh"
(
  cd "$docs_repo"
  PATH="$fake_bin:$PATH" FAKE_PR_HEAD="$fixed_head" FAKE_PR_BASE="$review_base" \
    FAKE_PR_BODY="$fixed_body" "$repo_root/scripts/open-pr.sh" --update 123 >/dev/null
)

# review-bounded.sh's own git plumbing (git rev-parse HEAD, "$base"^{commit},
# git status --porcelain, git rev-parse --show-toplevel/--git-dir) must run
# against a repo that actually has the requested base ref and a clean tree —
# never the ambient checkout this test script happens to be invoked from. A
# CI PR checkout is detached with no local `main` ref (and a shallow fetch
# has no merge base), so exercising review-bounded.sh there fails before ever
# reaching the fake codex stub below. docs_repo is already a self-contained
# fixture inside tmp_dir with a real `main` branch and a clean tree (its last
# mutation above was a commit), so every invocation below runs inside it.
fake_args="$tmp_dir/codex-args"
review_base_sha="$(git -C "$docs_repo" rev-parse 'main^{commit}')"
review_head_sha="$(git -C "$docs_repo" rev-parse HEAD)"
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean FAKE_CODEX_ARGS_FILE="$fake_args" \
  ALIGN_REVIEW_STALL_SECONDS=5 ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null
grep -Fq "git diff $review_base_sha...$review_head_sha" "$fake_args" || {
  echo "review prompt did not bind the requested base" >&2
  exit 1
}
set +e
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=findings ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null
findings_status=$?
# +2s over the historical 1s/2s budgets: a shared/contended runner can delay
# scheduling enough to make real progress look like a stall at the original
# margins, which would flip these two toward a false 124.
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=stall ALIGN_REVIEW_STALL_SECONDS=3 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
stall_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=progress ALIGN_REVIEW_STALL_SECONDS=4 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
progress_status=$?
max_started=$SECONDS
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=progress ALIGN_REVIEW_STALL_SECONDS=60 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=30 ALIGN_REVIEW_MAX_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
max_status=$?
max_elapsed=$((SECONDS - max_started))
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=trailing ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
trailing_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=duplicate ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
duplicate_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_clean_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-findings ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_findings_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-readonly ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_readonly_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-inspected ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_inspected_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-risks ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_risks_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-inline ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_inline_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-introducing ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_introducing_status=$?
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-mixed ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
native_mixed_status=$?
set -e
[[ $findings_status -eq 2 ]] || {
  echo "findings review returned $findings_status, expected 2" >&2
  exit 1
}
[[ $stall_status -eq 124 ]] || {
  echo "stalled review returned $stall_status, expected 124" >&2
  exit 1
}
[[ $progress_status -eq 0 ]] || {
  echo "progressing review returned $progress_status, expected 0" >&2
  exit 1
}
[[ $max_status -eq 124 ]] || {
  echo "explicitly bounded review returned $max_status, expected 124" >&2
  exit 1
}
# Relaxed from the historical 10s: the terminate/sleep-2/kill-KILL teardown in
# review-bounded.sh's cleanup can itself take longer than 10s on a slow or
# contended shared runner without indicating an actual regression.
[[ $max_elapsed -lt 30 ]] || {
  echo "explicitly bounded review overshot to ${max_elapsed}s" >&2
  exit 1
}
[[ $trailing_status -eq 3 && $duplicate_status -eq 3 ]] || {
  echo "malformed review verdict was accepted" >&2
  exit 1
}
[[ $native_clean_status -eq 0 && $native_readonly_status -eq 0 &&
  $native_inspected_status -eq 0 && $native_risks_status -eq 0 &&
  $native_inline_status -eq 0 && $native_introducing_status -eq 0 && $native_mixed_status -eq 3 &&
  $native_findings_status -eq 2 ]] || {
  echo "native review output was classified incorrectly" >&2
  exit 1
}

# The malformed-output fixtures above deliberately overwrite this same-HEAD
# cycle record. Restore one completed checkpoint before exercising descendant
# review policy.
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean \
  ALIGN_REVIEW_STALL_SECONDS=5 ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null

# A completed full-diff review on an ancestor blocks another complete review
# after an ordinary fix. Re-opening is explicit and requires both the owning
# matrix change and the exact trailer named on the command line.
printf '\ndescendant review fix\n' >>"$docs_repo/docs/notes.md"
git -C "$docs_repo" add docs/notes.md
git -C "$docs_repo" commit -qm 'docs: close review findings'
descendant_review_head="$(git -C "$docs_repo" rev-parse HEAD)"
set +e
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean \
  ALIGN_REVIEW_STALL_SECONDS=5 ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main ) >/dev/null 2>&1
descendant_review_status=$?
set -e
[[ $descendant_review_status -eq 1 ]] || {
  echo "descendant full-diff review was not blocked" >&2
  exit 1
}
changed_slice_args="$tmp_dir/codex-changed-slice-args"
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean \
  FAKE_CODEX_ARGS_FILE="$changed_slice_args" ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main \
    --changed-since "$review_head_sha" ) >/dev/null
grep -Fq "git diff $review_head_sha..$descendant_review_head" "$changed_slice_args" || {
  echo "changed-slice review did not bind the nearest reviewed ancestor:" >&2
  cat "$changed_slice_args" >&2
  exit 1
}

# A nearer STARTED review is not a completed checkpoint. A stall, kill, or
# malformed native result leaves the cycle record without a verdict; a later
# HEAD must resume that unfinished scope rather than skipping over it with a
# changed-slice review.
printf '\nincomplete review ancestor\n' >>"$docs_repo/docs/notes.md"
git -C "$docs_repo" add docs/notes.md
git -C "$docs_repo" commit -qm 'docs: create incomplete review ancestor'
incomplete_review_head="$(git -C "$docs_repo" rev-parse HEAD)"
incomplete_cycle="$(git -C "$docs_repo" rev-parse --absolute-git-dir)/align-review-cycle-$incomplete_review_head"
{
  printf 'ALIGN_REVIEW_KIND=HOST\n'
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$incomplete_review_head"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$review_base_sha"
  printf 'ALIGN_REVIEW_SCOPE=CHANGED_SLICE\n'
} >"$incomplete_cycle"
# Raw output is not completion evidence: a process can print this marker and
# then stall before the wrapper validates its exit and appends to the cycle.
incomplete_raw="$(dirname "$incomplete_cycle")/align-review-$incomplete_review_head.log"
{
  printf 'ALIGN_REVIEW_KIND=HOST\n'
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$incomplete_review_head"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$review_base_sha"
  printf 'ALIGN_REVIEW_VERDICT=CLEAN\n'
} >"$incomplete_raw"
printf '\nlater unreviewed change\n' >>"$docs_repo/docs/notes.md"
git -C "$docs_repo" add docs/notes.md
git -C "$docs_repo" commit -qm 'docs: advance past incomplete review'
set +e
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean \
  ALIGN_REVIEW_STALL_SECONDS=5 ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main \
    --changed-since "$incomplete_review_head" ) >/dev/null 2>&1
incomplete_slice_status=$?
set -e
[[ $incomplete_slice_status -eq 1 ]] || {
  echo "an incomplete ancestor authorized a changed-slice review" >&2
  exit 1
}
mkdir -p "$docs_repo/docs/impl"
printf '\n## Reopened review axis\n' >>"$docs_repo/docs/impl/00-plan.md"
git -C "$docs_repo" add docs/impl/00-plan.md
git -C "$docs_repo" commit -qm 'docs(impl): reopen review axis

Closure-Matrix-Reopened: review topology'
( cd "$docs_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean \
  ALIGN_REVIEW_STALL_SECONDS=5 ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main --reopen-axis 'review topology' ) >/dev/null

# The classifier's two evidence lists must stay equal to what the machinery
# actually references; recompute both from the repository so adding a gate
# target or embedding a new document cannot silently widen the light tier.
# shellcheck source=scripts/pr-tier.sh
. "$repo_root/scripts/pr-tier.sh"
actual_gate_tests="$(grep -oE '\-\-test [a-z0-9_]+' "$repo_root/scripts/test-pr.sh" |
  awk '{print $2}' | sort -u | tr '\n' ' ')"
declared_gate_tests="$(printf '%s\n' $PR_TIER_GATE_TESTS | sort -u | tr '\n' ' ')"
[[ "$actual_gate_tests" == "$declared_gate_tests" ]] || {
  echo "PR_TIER_GATE_TESTS is stale" >&2
  echo "  scripts/test-pr.sh names: $actual_gate_tests" >&2
  echo "  scripts/pr-tier.sh lists: $declared_gate_tests" >&2
  exit 1
}
# scripts/test-pr.sh hands scripts/run-gate-binaries.sh the exact set of
# binaries its cargo selection must produce, and the runner fails when the
# compiled set differs either way. That check is only fail-closed while the
# declared list still equals the selection, so derive the list from the `-p`
# and `--test` flags and compare.
declared_gate_binaries="$(
  awk '/^gate_binaries="/ { inside = 1; next } inside && /^"/ { exit } inside { print }' \
    "$repo_root/scripts/test-pr.sh" |
    tr ' ' '\n' | grep -v '^$' | LC_ALL=C sort | tr '\n' ' '
)"
derived_gate_binaries="$(
  {
    grep -oE '^  -p [a-z0-9_]+' "$repo_root/scripts/test-pr.sh"
    grep -oE '^  --test [a-z0-9_]+' "$repo_root/scripts/test-pr.sh"
  } | awk '{ print $2 }' | LC_ALL=C sort | tr '\n' ' '
)"
[[ -n "$derived_gate_binaries" && "$declared_gate_binaries" == "$derived_gate_binaries" ]] || {
  echo "scripts/test-pr.sh's declared gate_binaries list is stale" >&2
  echo "  declared: $declared_gate_binaries" >&2
  echo "  selected: $derived_gate_binaries" >&2
  exit 1
}

actual_prose="$(
  cd "$repo_root" || exit 1
  grep -rhoE 'include_(str|bytes)!\("[^"]*\.md"\)' crates 2>/dev/null |
    sed -E 's/.*!\("([^"]+)"\)/\1/' |
    sed -E 's#^(\.\./)+##' |
    sort -u | tr '\n' ' '
)"
declared_prose="$(printf '%s\n' $PR_TIER_COMPILED_PROSE | sort -u | tr '\n' ' ')"
[[ "$actual_prose" == "$declared_prose" ]] || {
  echo "PR_TIER_COMPILED_PROSE is stale" >&2
  echo "  crates embed: $actual_prose" >&2
  echo "  pr-tier.sh lists: $declared_prose" >&2
  exit 1
}
for gate_test in $PR_TIER_GATE_TESTS; do
  pr_tier_path_is_library "crates/align_driver/tests/${gate_test}.rs" || {
    echo "bounded-gate test $gate_test classified as tooling" >&2
    exit 1
  }
done
for embedded_doc in $PR_TIER_COMPILED_PROSE; do
  pr_tier_path_is_library "$embedded_doc" || {
    echo "compiled document $embedded_doc classified as tooling" >&2
    exit 1
  }
done
pr_tier_path_is_library "crates/align_driver/tests/pkg_db_q6.rs" && {
  echo "an ordinary leaf owner test must stay in the tooling tier" >&2
  exit 1
}

# preflight.yml runs the PR's own copy of the workflow, so where it fetches the
# checker and the classifier from is itself a guard. Pin that source to the live
# default-branch tip: `github.event.pull_request.base.sha` is frozen at PR
# creation and no event the workflow listens to refreshes it, so machinery fixed
# on main would never reach an already-open PR (PR #746, 2026-08-11), and a
# stacked PR's own base branch is author-controlled. Recompute the assertion
# from the workflow rather than trusting a remembered string.
preflight_workflow="$repo_root/.github/workflows/preflight.yml"
grep -Fq 'TRUSTED_TIP_SHA="$(git rev-parse --verify --quiet "refs/remotes/origin/main^{commit}")"' \
  "$preflight_workflow" || {
  echo "preflight.yml no longer resolves the trusted tip from refs/remotes/origin/main" >&2
  exit 1
}
for trusted_machinery in check-pr-preflight.sh pr-tier.sh; do
  grep -Fq "git show \"\$TRUSTED_TIP_SHA:scripts/$trusted_machinery\"" "$preflight_workflow" || {
    echo "preflight.yml no longer fetches scripts/$trusted_machinery from the trusted tip" >&2
    exit 1
  }
done
if grep -qE '\$\{?PR_BASE_SHA\}?:scripts/' "$preflight_workflow"; then
  echo "preflight.yml fetches machinery from the frozen pull_request.base.sha again" >&2
  exit 1
fi
untrusted_fetch="$(grep -oE 'git show "\$\{?[A-Za-z_][A-Za-z0-9_]*\}?:' "$preflight_workflow" |
  grep -Fv 'git show "$TRUSTED_TIP_SHA:' || true)"
[[ -z "$untrusted_fetch" ]] || {
  echo "preflight.yml fetches machinery from a source other than the trusted tip:" >&2
  echo "  $untrusted_fetch" >&2
  exit 1
}

# True docs-only pull requests must not launch the compiler platform matrix.
# The scope step loads the one shared classifier from the live trusted base;
# the always-running aggregate preserves one stable branch-protection context
# and fails closed if classification or a required matrix run fails.
ci_workflow="$repo_root/.github/workflows/ci.yml"
grep -Fq 'git show "$TRUSTED_TIP:scripts/pr-tier.sh"' "$ci_workflow" || {
  echo "ci.yml no longer loads the platform classifier from the trusted base" >&2
  exit 1
}
grep -Fq 'elif [ "$BASE_REF" != main ]; then' "$ci_workflow" || {
  echo "ci.yml no longer fails closed for author-controlled stacked-PR bases" >&2
  exit 1
}
[[ "$(grep -Fc 'TRUSTED_TIP="$(git rev-parse refs/remotes/origin/main)"' "$ci_workflow")" -eq 2 ]] || {
  echo "both CI scope classifiers must pin their machinery to protected main" >&2
  exit 1
}
[[ "$(grep -Fc 'reason=non-main-base' "$ci_workflow")" -eq 2 ]] || {
  echo "both CI scope classifiers must fail closed for stacked-PR bases" >&2
  exit 1
}
if grep -Fq 'TRUSTED_TIP="$(git rev-parse "origin/$BASE_REF")"' "$ci_workflow"; then
  echo "ci.yml trusts an author-controlled PR base for platform classification" >&2
  exit 1
fi
grep -Fq 'pr_tier_docs_only "$BASE_SHA" "$HEAD_SHA"' "$ci_workflow" || {
  echo "ci.yml no longer uses the shared docs-only predicate" >&2
  exit 1
}
grep -Fq "if: needs.db-scope.outputs.platform_required == 'true'" "$ci_workflow" || {
  echo "ci.yml no longer skips the platform matrix for docs-only pull requests" >&2
  exit 1
}
[[ "$(grep -Fc 'name: Platform CI (required)' "$ci_workflow")" -eq 1 ]] || {
  echo "ci.yml must expose exactly one stable platform aggregate" >&2
  exit 1
}
grep -Fq 'false) test "$PLATFORM_RESULT" = skipped ;;' "$ci_workflow" || {
  echo "the platform aggregate no longer requires a classified skip" >&2
  exit 1
}
grep -Fq 'true) test "$PLATFORM_RESULT" = success ;;' "$ci_workflow" || {
  echo "the platform aggregate no longer requires a successful matrix" >&2
  exit 1
}

# Tier classification and the review-round tripwire.
tier_repo="$tmp_dir/tier-repo"
mkdir -p "$tier_repo/crates/thing/src" "$tier_repo/crates/thing/tests" \
  "$tier_repo/docs/impl" "$tier_repo/scripts"
cp "$repo_root/scripts/pr-tier.sh" "$tier_repo/scripts/pr-tier.sh"
# The fixture stands in for the real repository, whose crates/ changes always
# run the cheap ratchet; stub it so the tier logic is what is under test.
# test-pr.sh and cargo.sh read STUB_TEST_PR_EXIT/STUB_CLIPPY_EXIT (default 0)
# so the parallel-gate fixtures below can drive either side to failure
# without disturbing every other test in this file, which never sets them.
printf '#!/usr/bin/env bash\nexit 0\n' >"$tier_repo/scripts/lint-ratchet.sh"
chmod +x "$tier_repo/scripts/lint-ratchet.sh"
printf '#!/usr/bin/env bash\necho test-pr-success-payload\nexit "${STUB_TEST_PR_EXIT:-0}"\n' >"$tier_repo/scripts/test-pr.sh"
chmod +x "$tier_repo/scripts/test-pr.sh"
{
  printf '#!/usr/bin/env bash\n'
  printf 'case "$1" in\n'
  printf '  clippy) echo clippy-success-payload; exit "${STUB_CLIPPY_EXIT:-0}" ;;\n'
  printf '  *) exit 0 ;;\n'
  printf 'esac\n'
} >"$tier_repo/scripts/cargo.sh"
chmod +x "$tier_repo/scripts/cargo.sh"
git -C "$tier_repo" init -q -b main
git -C "$tier_repo" config user.name workflow-test
git -C "$tier_repo" config user.email workflow-test@example.invalid
git -C "$tier_repo" config commit.gpgsign false
printf 'fn main() {}\n' >"$tier_repo/crates/thing/src/lib.rs"
printf '#[test]\nfn baseline() {}\n' >"$tier_repo/crates/thing/tests/baseline_owner.rs"
printf '# plan\n' >"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" add .
git -C "$tier_repo" commit -qm baseline
git -C "$tier_repo" update-ref refs/remotes/origin/main main

tier_preflight() { (cd "$tier_repo" && "$repo_root/scripts/pre-pr.sh" "$@"); }
tier_branch() {
  git -C "$tier_repo" switch -q main
  git -C "$tier_repo" switch -qc "$1"
}

# A leaf owner test is the tooling tier: no reviewer required, and the stamp
# records the tier so the PR wrappers accept the lighter attestation.
tier_branch tooling-change
mkdir -p "$tier_repo/crates/thing/tests"
printf '#[test]\nfn t() {}\n' >"$tier_repo/crates/thing/tests/owner.rs"
git -C "$tier_repo" add crates/thing/tests/owner.rs
git -C "$tier_repo" commit -qm 'test: add owner'
tier_preflight --base main --owner-test tier -- true >/dev/null
tooling_head="$(git -C "$tier_repo" rev-parse HEAD)"
grep -Fqx 'kind=tooling' "$tier_repo/.git/align-preflight/$tooling_head"
grep -Fqx 'review_state=tooling' "$tier_repo/.git/align-preflight/$tooling_head"

# Everything the light tier must NOT swallow. Each of these reaches beyond one
# owner check, so each must demand the full review.
library_case() {
  local label="$1" path="$2"
  tier_branch "lib-${label}"
  mkdir -p "$tier_repo/$(dirname "$path")"
  printf 'shared\n' >"$tier_repo/$path"
  git -C "$tier_repo" add "$path"
  git -C "$tier_repo" commit -qm "add $label"
  local status=0
  tier_preflight --base main --owner-test tier -- true >/dev/null 2>"$tmp_dir/tier-err" || status=$?
  [[ "$status" -eq 2 ]] || {
    echo "$label unexpectedly took the tooling tier (status $status)" >&2
    exit 1
  }
  grep -q 'requires --reviewer' "$tmp_dir/tier-err" || {
    echo "$label failed for the wrong reason:" >&2
    cat "$tmp_dir/tier-err" >&2
    exit 1
  }
}
library_case harness crates/thing/tests/common/mod.rs
library_case harness-file crates/thing/tests/common.rs
library_case helpers crates/thing/tests/helpers/mod.rs
library_case fixture crates/thing/tests/fixtures/stub.c
library_case golden crates/thing/tests/golden/expected.ll
library_case nested-src crates/thing/src/tests/mod.rs
library_case script scripts/new-gate.sh
library_case workflow .github/workflows/new.yml
library_case unknown-tree newcrate/src/lib.rs

# Deleting a leaf owner test removes coverage: never the light tier.
tier_branch delete-change
git -C "$tier_repo" rm -q crates/thing/tests/baseline_owner.rs
git -C "$tier_repo" commit -qm 'test: remove a leaf owner'
delete_status=0
tier_preflight --base main --owner-test tier -- true >/dev/null 2>&1 || delete_status=$?
[[ "$delete_status" -ne 0 ]] || {
  echo "deleting a leaf owner test unexpectedly took the tooling tier" >&2
  exit 1
}

# A library source change classifies as code and keeps the full attestation.
tier_branch code-change
printf 'fn added() {}\n' >>"$tier_repo/crates/thing/src/lib.rs"
git -C "$tier_repo" commit -qam 'feat: extend the library'
code_log="$tmp_dir/code-review.log"
{
  printf 'ALIGN_REVIEW_HEAD=%s\n' "$(git -C "$tier_repo" rev-parse HEAD)"
  printf 'ALIGN_REVIEW_BASE=%s\n' "$(git -C "$tier_repo" rev-parse 'main^{commit}')"
  printf 'ALIGN_REVIEW_VERDICT=CLEAN\n'
} >"$code_log"
code_preflight_out="$tmp_dir/code-preflight-out"
tier_preflight --reviewer reviewer-1 --review-log "$code_log" --base main \
  --owner-test tier -- true >"$code_preflight_out"
grep -Fq 'preflight: scripts/test-pr.sh passed' "$code_preflight_out" &&
  grep -Fq 'preflight: Clippy passed' "$code_preflight_out" || {
  echo "a successful code preflight omitted its terse phase summaries:" >&2
  cat "$code_preflight_out" >&2
  exit 1
}
if grep -Eq 'test-pr-success-payload|clippy-success-payload' "$code_preflight_out"; then
  echo "a successful code preflight replayed captured command output:" >&2
  cat "$code_preflight_out" >&2
  exit 1
fi
code_preflight_verbose_out="$tmp_dir/code-preflight-verbose-out"
ALIGN_QUIET_VERBOSE=1 \
  tier_preflight --reviewer reviewer-1 --review-log "$code_log" --base main \
  --owner-test tier -- true >"$code_preflight_verbose_out" 2>&1
grep -Fq 'test-pr-success-payload' "$code_preflight_verbose_out" &&
  grep -Fq 'clippy-success-payload' "$code_preflight_verbose_out" || {
  echo "verbose code preflight did not replay both successful command logs:" >&2
  cat "$code_preflight_verbose_out" >&2
  exit 1
}
code_head="$(git -C "$tier_repo" rev-parse HEAD)"
grep -Fqx 'kind=code' "$tier_repo/.git/align-preflight/$code_head"

# The parallel scripts/test-pr.sh/Clippy block in the code tier must fail no
# matter which side fails, replaying only the failed side while summarizing the
# successful peer.
parallel_case() {
  local label="$1" test_pr_exit="$2" clippy_exit="$3"
  tier_branch "parallel-${label}"
  printf 'fn %s() {}\n' "$label" >>"$tier_repo/crates/thing/src/lib.rs"
  git -C "$tier_repo" commit -qam "feat: trigger the parallel gate ($label)"
  local log="$tmp_dir/parallel-review-${label}.log"
  {
    printf 'ALIGN_REVIEW_HEAD=%s\n' "$(git -C "$tier_repo" rev-parse HEAD)"
    printf 'ALIGN_REVIEW_BASE=%s\n' "$(git -C "$tier_repo" rev-parse 'main^{commit}')"
    printf 'ALIGN_REVIEW_VERDICT=CLEAN\n'
  } >"$log"
  local out="$tmp_dir/parallel-out-${label}"
  local status=0
  STUB_TEST_PR_EXIT="$test_pr_exit" STUB_CLIPPY_EXIT="$clippy_exit" \
    tier_preflight --reviewer reviewer-1 --review-log "$log" --base main \
    --owner-test tier -- true >"$out" 2>&1 || status=$?
  [[ "$status" -ne 0 ]] || {
    echo "parallel gate ($label) unexpectedly passed" >&2
    cat "$out" >&2
    exit 1
  }
  if [[ "$test_pr_exit" -ne 0 ]]; then
    grep -q 'scripts/test-pr.sh output' "$out" &&
      grep -q 'test-pr-success-payload' "$out" || {
      echo "parallel gate ($label) did not show the failed scripts/test-pr.sh output:" >&2
      cat "$out" >&2
      exit 1
    }
  else
    grep -q 'scripts/test-pr.sh passed' "$out" &&
      ! grep -q 'test-pr-success-payload' "$out" || {
      echo "parallel gate ($label) replayed the successful scripts/test-pr.sh log:" >&2
      cat "$out" >&2
      exit 1
    }
  fi
  if [[ "$clippy_exit" -ne 0 ]]; then
    grep -q 'Clippy output' "$out" &&
      grep -q 'clippy-success-payload' "$out" || {
      echo "parallel gate ($label) did not show the failed Clippy output:" >&2
      cat "$out" >&2
      exit 1
    }
  else
    grep -q 'Clippy passed' "$out" &&
      ! grep -q 'clippy-success-payload' "$out" || {
      echo "parallel gate ($label) replayed the successful Clippy log:" >&2
      cat "$out" >&2
      exit 1
    }
  fi
}
parallel_case test-pr-fails 1 0
parallel_case clippy-fails 0 1

# Three consecutive review-fix commits without re-opening a closure matrix is
# the review-as-discovery-loop pattern and must fail. Neither a translated
# mirror nor an untrailered docs/impl edit releases it; the trailer plus an
# authoritative matrix change does.
tier_branch rounds-change
mkdir -p "$tier_repo/crates/thing/tests" "$tier_repo/docs/impl/ja"
printf '#[test]\nfn a() {}\n' >"$tier_repo/crates/thing/tests/rounds.rs"
printf '# mirror\n' >"$tier_repo/docs/impl/ja/00-plan.md"
git -C "$tier_repo" add crates/thing/tests/rounds.rs docs/impl/ja/00-plan.md
git -C "$tier_repo" commit -qm 'feat: add rounds owner'
for round in 1 2 3; do
  printf '#[test]\nfn r%s() {}\n' "$round" >>"$tier_repo/crates/thing/tests/rounds.rs"
  git -C "$tier_repo" commit -qam "fix: close review finding $round"
done
if tier_preflight --base main --owner-test tier -- true >/dev/null 2>&1; then
  echo "three review-fix rounds unexpectedly passed without a matrix re-open" >&2
  exit 1
fi
printf '\nmirror update\n' >>"$tier_repo/docs/impl/ja/00-plan.md"
git -C "$tier_repo" commit -qam 'fix: update the translated mirror

Closure-Matrix-Reopened: mirror only'
if tier_preflight --base main --owner-test tier -- true >/dev/null 2>&1; then
  echo "a translated mirror unexpectedly released the review-round gate" >&2
  exit 1
fi
printf '\n## reopened axis\n' >>"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" commit -qam 'fix: touch the plan without declaring the axis'
if tier_preflight --base main --owner-test tier -- true >/dev/null 2>&1; then
  echo "an untrailered docs/impl edit unexpectedly released the review-round gate" >&2
  exit 1
fi
printf '\n## second axis\n' >>"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" commit -qam 'fix: re-open the closure matrix

Closure-Matrix-Reopened: callback ABI'
tier_preflight --base main --owner-test tier -- true >/dev/null

# Fix commits are counted across the whole post-implementation range, not
# reset by an interleaved non-fix commit: fix, docs, fix, fix still trips the
# gate at 3 counted fixes.
tier_branch interleaved-rounds-change
mkdir -p "$tier_repo/crates/thing/tests"
printf '#[test]\nfn a() {}\n' >"$tier_repo/crates/thing/tests/interleaved.rs"
git -C "$tier_repo" add crates/thing/tests/interleaved.rs
git -C "$tier_repo" commit -qm 'feat: add interleaved owner'
printf '#[test]\nfn r1() {}\n' >>"$tier_repo/crates/thing/tests/interleaved.rs"
git -C "$tier_repo" commit -qam 'fix: close review finding 1'
printf '\ninterleaved note\n' >>"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" commit -qam 'docs: unrelated interleaved note'
printf '#[test]\nfn r2() {}\n' >>"$tier_repo/crates/thing/tests/interleaved.rs"
git -C "$tier_repo" commit -qam 'fix: close review finding 2'
printf '#[test]\nfn r3() {}\n' >>"$tier_repo/crates/thing/tests/interleaved.rs"
git -C "$tier_repo" commit -qam 'fix: close review finding 3'
if tier_preflight --base main --owner-test tier -- true >/dev/null 2>&1; then
  echo "3 fix commits interleaved with a docs commit unexpectedly passed" >&2
  exit 1
fi
# The release check reads the same counted set, so a trailer commit still
# releases the gate once it reopens the matrix.
printf '\n## interleaved axis\n' >>"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" commit -qam 'fix: re-open the closure matrix for the interleaved case

Closure-Matrix-Reopened: interleaved axis'
tier_preflight --base main --owner-test tier -- true >/dev/null

# The first commit in base..HEAD is always the (uncounted) implementation
# regardless of its own subject, so a branch that never has a non-fix commit
# at all cannot dodge the tripwire: 5 all-fix commits still count 4.
tier_branch all-fix-rounds-change
mkdir -p "$tier_repo/crates/thing/tests"
printf '#[test]\nfn a() {}\n' >"$tier_repo/crates/thing/tests/allfix.rs"
git -C "$tier_repo" add crates/thing/tests/allfix.rs
git -C "$tier_repo" commit -qm 'fix: add allfix owner'
for round in 1 2 3 4; do
  printf '#[test]\nfn r%s() {}\n' "$round" >>"$tier_repo/crates/thing/tests/allfix.rs"
  git -C "$tier_repo" commit -qam "fix: close review finding $round"
done
all_fix_err="$tmp_dir/all-fix-err"
all_fix_status=0
tier_preflight --base main --owner-test tier -- true >/dev/null 2>"$all_fix_err" || all_fix_status=$?
[[ "$all_fix_status" -ne 0 ]] || {
  echo "5 all-fix commits (4 counted) unexpectedly passed without a matrix re-open" >&2
  exit 1
}
grep -q '^preflight: 4 post-implementation review-fix commits' "$all_fix_err" || {
  echo "all-fix branch did not count exactly 4 post-implementation fix commits:" >&2
  cat "$all_fix_err" >&2
  exit 1
}
# The release valve scans every post-implementation commit for the trailer,
# not only fix-titled ones: a differently-titled commit still releases the
# gate once it reopens the matrix.
printf '\n## all-fix axis\n' >>"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" commit -qam 'docs(impl): re-open the closure matrix for the all-fix case

Closure-Matrix-Reopened: all-fix axis'
tier_preflight --base main --owner-test tier -- true >/dev/null

# A docs-only branch is never subject to the review-round gate, and its
# attestation survives the CI-side tier recomputation.
tier_branch docs-rounds
for round in 1 2 3; do
  printf 'line %s\n' "$round" >>"$tier_repo/docs/impl/00-plan.md"
  git -C "$tier_repo" commit -qam "fix: prose $round"
done
tier_preflight --docs-only --base main >/dev/null
(
  cd "$tier_repo"
  docs_claim_head="$(git rev-parse HEAD)"
  docs_claim_base="$(git rev-parse main)"
  real_docs_body="$tmp_dir/real-docs-body"
  {
    printf '<!-- align-preflight-version:1 -->\n'
    printf '<!-- align-preflight-head:%s -->\n' "$docs_claim_head"
    printf '<!-- align-preflight-base-ref:main -->\n'
    printf '<!-- align-preflight-base-sha:%s -->\n' "$docs_claim_base"
    printf '<!-- align-preflight-review:docs-only -->\n'
    printf '<!-- align-preflight-review-head:%s -->\n' "$docs_claim_head"
    printf '<!-- align-preflight-reviewer:docs-only -->\n'
  } >"$real_docs_body"
  "$repo_root/scripts/check-pr-preflight.sh" \
    "$docs_claim_head" main "$docs_claim_base" "$real_docs_body" >/dev/null
)

# Body-level attestation checks for the light state, including the
# recomputation that stops a library diff from claiming it.
tooling_body="$tmp_dir/tooling-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$base_ref"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:tooling -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-reviewer:tooling -->\n'
} >"$tooling_body"
sed 's/align-preflight-reviewer:tooling/align-preflight-reviewer:reviewer-1/' \
  "$tooling_body" >"$tmp_dir/bad-tooling-body"
if (cd "$attest_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$tmp_dir/bad-tooling-body" >/dev/null 2>&1)
then
  echo "tooling attestation with a named reviewer unexpectedly passed" >&2
  exit 1
fi
(
  cd "$tier_repo"
  claim_head="$(git rev-parse code-change)"
  claim_base="$(git rev-parse main)"
  claim_body="$tmp_dir/claim-body"
  {
    printf '<!-- align-preflight-version:1 -->\n'
    printf '<!-- align-preflight-head:%s -->\n' "$claim_head"
    printf '<!-- align-preflight-base-ref:main -->\n'
    printf '<!-- align-preflight-base-sha:%s -->\n' "$claim_base"
    printf '<!-- align-preflight-review:tooling -->\n'
    printf '<!-- align-preflight-review-head:%s -->\n' "$claim_head"
    printf '<!-- align-preflight-reviewer:tooling -->\n'
  } >"$claim_body"
  if "$repo_root/scripts/check-pr-preflight.sh" \
    "$claim_head" main "$claim_base" "$claim_body" >/dev/null 2>&1
  then
    echo "a library diff unexpectedly passed under a tooling attestation" >&2
    exit 1
  fi
  tooling_claim_head="$(git rev-parse tooling-change)"
  ok_body="$tmp_dir/ok-tooling-body"
  {
    printf '<!-- align-preflight-version:1 -->\n'
    printf '<!-- align-preflight-head:%s -->\n' "$tooling_claim_head"
    printf '<!-- align-preflight-base-ref:main -->\n'
    printf '<!-- align-preflight-base-sha:%s -->\n' "$claim_base"
    printf '<!-- align-preflight-review:tooling -->\n'
    printf '<!-- align-preflight-review-head:%s -->\n' "$tooling_claim_head"
    printf '<!-- align-preflight-reviewer:tooling -->\n'
  } >"$ok_body"
  "$repo_root/scripts/check-pr-preflight.sh" \
    "$tooling_claim_head" main "$claim_base" "$ok_body" >/dev/null
)

# Every base binding is the branch's merge base with the base branch, never the
# base branch tip. Both directions matter: an unrelated PR landing on main must
# leave the review log, the stamp, the PR-body marker, and the CI recomputation
# valid (the tip binding forced a rebase and a full CI rerun on every such
# merge), while a change to the branch itself must still invalidate all of them.
advance_repo="$tmp_dir/advance-repo"
mkdir -p "$advance_repo"
git -C "$advance_repo" init -q -b main
git -C "$advance_repo" config user.name workflow-test
git -C "$advance_repo" config user.email workflow-test@example.invalid
git -C "$advance_repo" config commit.gpgsign false
printf '# baseline\n' >"$advance_repo/README.md"
git -C "$advance_repo" add README.md
git -C "$advance_repo" commit -qm baseline
fork_sha="$(git -C "$advance_repo" rev-parse HEAD)"
git -C "$advance_repo" switch -qc feature
# An unknown top-level path is library tier without being a Rust build input, so
# this exercises the full review-log binding without reaching the cargo gate.
printf 'tool\n' >"$advance_repo/tool.txt"
git -C "$advance_repo" add tool.txt
git -C "$advance_repo" commit -qm 'feat: add a tool'
advance_head="$(git -C "$advance_repo" rev-parse HEAD)"
# Meanwhile another PR merges into main.
git -C "$advance_repo" switch -q main
printf 'unrelated\n' >"$advance_repo/other.txt"
git -C "$advance_repo" add other.txt
git -C "$advance_repo" commit -qm 'feat: unrelated main change'
main_tip="$(git -C "$advance_repo" rev-parse main)"
git -C "$advance_repo" update-ref refs/remotes/origin/main main
git -C "$advance_repo" switch -q feature

advance_log="$tmp_dir/advance-review.log"
(cd "$advance_repo" && "$repo_root/scripts/new-review-log.sh" --base main "$advance_log" >/dev/null)
grep -Fqx "ALIGN_REVIEW_BASE=$fork_sha" "$advance_log" || {
  echo "new-review-log.sh bound the base branch tip instead of the merge base" >&2
  exit 1
}
advance_args="$tmp_dir/advance-codex-args"
(cd "$advance_repo" && PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean \
  FAKE_CODEX_ARGS_FILE="$advance_args" ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main) >/dev/null
grep -Fq "git diff $fork_sha...$advance_head" "$advance_args" || {
  echo "review-bounded.sh bound the base branch tip instead of the merge base" >&2
  exit 1
}
advance_clean_log="$tmp_dir/advance-review-clean.log"
sed 's/^ALIGN_REVIEW_VERDICT=FINDINGS$/ALIGN_REVIEW_VERDICT=CLEAN/' "$advance_log" \
  >"$advance_clean_log"
(
  cd "$advance_repo"
  "$repo_root/scripts/pre-pr.sh" --reviewer reviewer-1 --review-log "$advance_clean_log" \
    --base main --owner-test advance -- true >/dev/null
)
advance_stamp="$advance_repo/.git/align-preflight/$advance_head"
grep -Fqx "base_sha=$fork_sha" "$advance_stamp" || {
  echo "pre-pr.sh recorded the base branch tip instead of the merge base" >&2
  exit 1
}
grep -Fqx 'kind=code' "$advance_stamp"

advance_body="$tmp_dir/advance-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$advance_head"
  printf '<!-- align-preflight-base-ref:main -->\n'
  printf '<!-- align-preflight-base-sha:%s -->\n' "$fork_sha"
  printf '<!-- align-preflight-review:clean -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$advance_head"
  printf '<!-- align-preflight-reviewer:reviewer-1 -->\n'
} >"$advance_body"
# CI hands the checker github.event.pull_request.base.sha, which is the ADVANCED
# tip; the derived base is still the merge base the body records.
(cd "$advance_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$advance_head" main "$main_tip" "$advance_body" >/dev/null)
# preflight.yml also resolves the merge base; passing it changes nothing.
(cd "$advance_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$advance_head" main "$fork_sha" "$advance_body" >/dev/null)

# The attack the derivation closes: preflight.yml is the PR's own file, so a
# one-word edit there could hand the checker the head SHA as the base. The range
# would then be empty, no library path would appear in it, and an unreviewed
# compiler diff could ride in under a tooling attestation. tool.txt is library
# tier, so this body is a lie in both directions.
forged_tooling_body="$tmp_dir/advance-forged-tooling-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$advance_head"
  printf '<!-- align-preflight-base-ref:main -->\n'
  printf '<!-- align-preflight-base-sha:%s -->\n' "$advance_head"
  printf '<!-- align-preflight-review:tooling -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$advance_head"
  printf '<!-- align-preflight-reviewer:tooling -->\n'
} >"$forged_tooling_body"
if (cd "$advance_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$advance_head" main "$advance_head" "$forged_tooling_body" >/dev/null 2>&1)
then
  echo "a forged empty-range base unexpectedly passed a library diff as tooling" >&2
  exit 1
fi
# The sharper version of the same attack, which the empty-range guard alone does
# NOT catch: pass the branch's own first commit as the base so the attested
# range is only the harmless second commit. Only deriving the base from the
# checked-out base branch rejects this.
forge_repo="$tmp_dir/forge-repo"
mkdir -p "$forge_repo/crates/thing/src"
git -C "$forge_repo" init -q -b main
git -C "$forge_repo" config user.name workflow-test
git -C "$forge_repo" config user.email workflow-test@example.invalid
git -C "$forge_repo" config commit.gpgsign false
printf '# doc\n' >"$forge_repo/note.md"
git -C "$forge_repo" add note.md
git -C "$forge_repo" commit -qm baseline
git -C "$forge_repo" update-ref refs/remotes/origin/main main
git -C "$forge_repo" switch -qc feature
printf 'fn unreviewed() {}\n' >"$forge_repo/crates/thing/src/lib.rs"
git -C "$forge_repo" add crates/thing/src/lib.rs
git -C "$forge_repo" commit -qm 'feat: unreviewed library change'
forge_library_sha="$(git -C "$forge_repo" rev-parse HEAD)"
printf '\ntweak\n' >>"$forge_repo/note.md"
git -C "$forge_repo" commit -qam 'docs: harmless tweak'
forge_head="$(git -C "$forge_repo" rev-parse HEAD)"
forged_range_body="$tmp_dir/forged-range-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$forge_head"
  printf '<!-- align-preflight-base-ref:main -->\n'
  printf '<!-- align-preflight-base-sha:%s -->\n' "$forge_library_sha"
  printf '<!-- align-preflight-review:docs-only -->\n'
  printf '<!-- align-preflight-review-head:%s -->\n' "$forge_head"
  printf '<!-- align-preflight-reviewer:docs-only -->\n'
} >"$forged_range_body"
if (cd "$forge_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$forge_head" main "$forge_library_sha" "$forged_range_body" >/dev/null 2>&1)
then
  echo "a forged truncated range hid an unreviewed library commit" >&2
  exit 1
fi

# Transition: an attestation an OLD pre-pr.sh recorded against the base branch
# tip no longer verifies once the tip has moved past the fork point. This is
# intentional — recovery is one pre-pr.sh rerun plus open-pr.sh --update — and
# it is pinned here so the behavior is not mistaken for a regression later.
legacy_tip_body="$tmp_dir/advance-legacy-tip-body"
sed "s/align-preflight-base-sha:$fork_sha/align-preflight-base-sha:$main_tip/" \
  "$advance_body" >"$legacy_tip_body"
if (cd "$advance_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$advance_head" main "$main_tip" "$legacy_tip_body" >/dev/null 2>&1)
then
  echo "a legacy tip-bound attestation unexpectedly verified" >&2
  exit 1
fi

advance_remote="$tmp_dir/advance-remote.git"
git init -q --bare "$advance_remote"
git -C "$advance_repo" remote add origin "$advance_remote"
git -C "$advance_repo" push -qu origin feature
# Local main is already ahead of the fork point here, which is exactly what used
# to abort open-pr.sh with "preflight base moved".
(
  cd "$advance_repo"
  PATH="$fake_bin:$PATH" FAKE_PR_HEAD="$advance_head" FAKE_PR_BASE="$main_tip" \
    FAKE_PR_BODY="$advance_body" "$repo_root/scripts/open-pr.sh" --update 123 >/dev/null
)
# baseRefOid can name a tip this clone has never fetched. open-pr.sh tries one
# fetch and, when that still cannot produce the object, says so instead of
# silently skipping the check.
absent_base_err="$tmp_dir/advance-absent-base-err"
(
  cd "$advance_repo"
  PATH="$fake_bin:$PATH" FAKE_PR_HEAD="$advance_head" \
    FAKE_PR_BASE="2222222222222222222222222222222222222222" \
    FAKE_PR_BODY="$advance_body" "$repo_root/scripts/open-pr.sh" --update 123 \
    >/dev/null 2>"$absent_base_err"
)
grep -q 'could not verify the PR' "$absent_base_err" || {
  echo "open-pr.sh silently skipped an unverifiable PR base tip:" >&2
  cat "$absent_base_err" >&2
  exit 1
}
# A PR opened against a different branch than the stamp's base is rejected.
wrong_base_err="$tmp_dir/advance-wrong-base-err"
wrong_base_status=0
(
  cd "$advance_repo"
  PATH="$fake_bin:$PATH" FAKE_PR_HEAD="$advance_head" FAKE_PR_BASE="$main_tip" \
    FAKE_PR_BASE_NAME=release FAKE_PR_BODY="$advance_body" \
    "$repo_root/scripts/open-pr.sh" --update 123 >/dev/null 2>"$wrong_base_err"
) || wrong_base_status=$?
[[ "$wrong_base_status" -ne 0 ]] || {
  echo "open-pr.sh accepted a PR opened against another base branch" >&2
  exit 1
}
grep -q 'does not match the preflight base' "$wrong_base_err" || {
  echo "the wrong PR base branch was rejected for the wrong reason:" >&2
  cat "$wrong_base_err" >&2
  exit 1
}

# The other direction: changing the branch moves its merge base, so everything
# bound to the old one must stop applying.
git -C "$advance_repo" merge -q --no-edit main
merged_head="$(git -C "$advance_repo" rev-parse HEAD)"
merged_err="$tmp_dir/advance-merged-err"
merged_status=0
(
  cd "$advance_repo"
  "$repo_root/scripts/pre-pr.sh" --reviewer reviewer-1 --review-log "$advance_clean_log" \
    --base main --owner-test advance -- true >/dev/null 2>"$merged_err"
) || merged_status=$?
[[ "$merged_status" -ne 0 ]] || {
  echo "a review log bound to the old merge base survived merging main in" >&2
  exit 1
}
grep -q 'review log belongs to another base' "$merged_err" || {
  echo "merging main in failed for the wrong reason:" >&2
  cat "$merged_err" >&2
  exit 1
}
merged_body="$tmp_dir/advance-merged-body"
sed "s/$advance_head/$merged_head/g" "$advance_body" >"$merged_body"
if (cd "$advance_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$merged_head" main "$main_tip" "$merged_body" >/dev/null 2>&1)
then
  echo "an attestation bound to the old merge base survived merging main in" >&2
  exit 1
fi

# open-pr.sh must still refuse a genuinely moved merge base. Attest the merged
# head, then let main absorb the branch: the merge base becomes HEAD itself.
merged_log="$tmp_dir/advance-merged-review.log"
(cd "$advance_repo" && "$repo_root/scripts/new-review-log.sh" --base main "$merged_log" >/dev/null)
grep -Fqx "ALIGN_REVIEW_BASE=$main_tip" "$merged_log" || {
  echo "the merge base did not follow the branch's merge of main" >&2
  exit 1
}
merged_clean_log="$tmp_dir/advance-merged-clean.log"
sed 's/^ALIGN_REVIEW_VERDICT=FINDINGS$/ALIGN_REVIEW_VERDICT=CLEAN/' "$merged_log" \
  >"$merged_clean_log"
(
  cd "$advance_repo"
  "$repo_root/scripts/pre-pr.sh" --reviewer reviewer-1 --review-log "$merged_clean_log" \
    --base main --owner-test advance -- true >/dev/null
)
git -C "$advance_repo" push -q origin feature
git -C "$advance_repo" switch -q main
git -C "$advance_repo" merge -q --no-edit feature
git -C "$advance_repo" switch -q feature
moved_err="$tmp_dir/advance-moved-err"
moved_status=0
(
  cd "$advance_repo"
  PATH="$fake_bin:$PATH" FAKE_PR_HEAD="$merged_head" FAKE_PR_BASE="$main_tip" \
    FAKE_PR_BODY="$merged_body" "$repo_root/scripts/open-pr.sh" --update 123 \
    >/dev/null 2>"$moved_err"
) || moved_status=$?
[[ "$moved_status" -ne 0 ]] || {
  echo "open-pr.sh accepted a stamp whose merge base had actually moved" >&2
  exit 1
}
grep -q 'merge base with main moved' "$moved_err" || {
  echo "open-pr.sh rejected the moved merge base for the wrong reason:" >&2
  cat "$moved_err" >&2
  exit 1
}
# CI reaches the same verdict from the other side: once main contains the head,
# the derived merge base IS the head and there is no range left to attest.
git -C "$advance_repo" update-ref refs/remotes/origin/main main
absorbed_err="$tmp_dir/advance-absorbed-err"
absorbed_status=0
(cd "$advance_repo" && "$repo_root/scripts/check-pr-preflight.sh" \
  "$merged_head" main "$main_tip" "$merged_body" >/dev/null 2>"$absorbed_err") ||
  absorbed_status=$?
[[ "$absorbed_status" -ne 0 ]] || {
  echo "an attestation for a head main already contains unexpectedly passed" >&2
  exit 1
}
grep -q 'already contains' "$absorbed_err" || {
  echo "an absorbed branch was rejected for the wrong reason:" >&2
  cat "$absorbed_err" >&2
  exit 1
}

# scripts/run-quiet.sh keeps routine command output out of successful logs but
# replays complete diagnostics and preserves the child status on failure.
quiet_runner="$repo_root/scripts/run-quiet.sh"
quiet_dir="$tmp_dir/quiet-runner"
mkdir -p "$quiet_dir"
printf '#!/usr/bin/env bash\nprintf "routine line 1\\nroutine line 2\\n"\n' \
  >"$quiet_dir/pass"
printf '#!/usr/bin/env bash\necho partial-stdout\necho failure-stderr >&2\nexit 7\n' \
  >"$quiet_dir/fail"
printf '#!/usr/bin/env bash\necho partial-before-signal >&2\nkill -TERM "$PPID"\n' \
  >"$quiet_dir/interrupt"
chmod +x "$quiet_dir/pass" "$quiet_dir/fail" "$quiet_dir/interrupt"

quiet_pass_out="$quiet_dir/pass-out"
"$quiet_runner" "quiet fixture" -- "$quiet_dir/pass" >"$quiet_pass_out" 2>&1
[[ "$(wc -l <"$quiet_pass_out" | tr -d '[:space:]')" -eq 1 ]] &&
  grep -Eq '^quiet fixture: ok \([0-9]+s\)$' "$quiet_pass_out" || {
  echo "the quiet wrapper did not reduce success to one summary line:" >&2
  cat "$quiet_pass_out" >&2
  exit 1
}
grep -Fq 'routine line' "$quiet_pass_out" && {
  echo "the quiet wrapper leaked successful command output:" >&2
  cat "$quiet_pass_out" >&2
  exit 1
}

quiet_verbose_out="$quiet_dir/verbose-out"
ALIGN_QUIET_VERBOSE=1 "$quiet_runner" "quiet fixture" -- "$quiet_dir/pass" \
  >"$quiet_verbose_out" 2>&1
grep -Fq 'routine line 1' "$quiet_verbose_out" || {
  echo "the quiet wrapper's verbose mode did not restore successful output:" >&2
  cat "$quiet_verbose_out" >&2
  exit 1
}

quiet_fail_out="$quiet_dir/fail-out"
quiet_fail_status=0
"$quiet_runner" "quiet failure" -- "$quiet_dir/fail" \
  >"$quiet_fail_out" 2>&1 || quiet_fail_status=$?
[[ "$quiet_fail_status" -eq 7 ]] &&
  grep -Fq 'quiet failure: FAILED (exit 7' "$quiet_fail_out" &&
  grep -Fq 'partial-stdout' "$quiet_fail_out" &&
  grep -Fq 'failure-stderr' "$quiet_fail_out" || {
  echo "the quiet wrapper did not preserve and explain a failed command:" >&2
  cat "$quiet_fail_out" >&2
  exit 1
}

quiet_expected_out="$quiet_dir/expected-out"
"$quiet_runner" --expect-failure "quiet expected failure" -- \
  "$quiet_dir/fail" >"$quiet_expected_out" 2>&1
[[ "$(wc -l <"$quiet_expected_out" | tr -d '[:space:]')" -eq 1 ]] &&
  grep -Fq 'quiet expected failure: expected failure observed (exit 7' \
    "$quiet_expected_out" &&
  ! grep -Eq 'partial-stdout|failure-stderr' "$quiet_expected_out" || {
  echo "the quiet wrapper did not summarize an expected failure:" >&2
  cat "$quiet_expected_out" >&2
  exit 1
}
quiet_expected_verbose_out="$quiet_dir/expected-verbose-out"
ALIGN_QUIET_VERBOSE=1 \
  "$quiet_runner" --expect-failure "quiet expected failure" -- \
  "$quiet_dir/fail" >"$quiet_expected_verbose_out" 2>&1
grep -Fq 'partial-stdout' "$quiet_expected_verbose_out" &&
  grep -Fq 'failure-stderr' "$quiet_expected_verbose_out" || {
  echo "verbose expected-failure mode did not restore captured output:" >&2
  cat "$quiet_expected_verbose_out" >&2
  exit 1
}

quiet_unexpected_pass_out="$quiet_dir/unexpected-pass-out"
quiet_unexpected_pass_status=0
"$quiet_runner" --expect-failure "quiet unexpected pass" -- \
  "$quiet_dir/pass" >"$quiet_unexpected_pass_out" 2>&1 ||
  quiet_unexpected_pass_status=$?
[[ "$quiet_unexpected_pass_status" -eq 1 ]] &&
  grep -Fq 'quiet unexpected pass: FAILED (expected a non-zero exit, got 0' \
    "$quiet_unexpected_pass_out" &&
  grep -Fq 'routine line 1' "$quiet_unexpected_pass_out" || {
  echo "the quiet wrapper did not explain an unexpectedly passing command:" >&2
  cat "$quiet_unexpected_pass_out" >&2
  exit 1
}

quiet_artifact="$quiet_dir/artifact"
quiet_artifact_out="$quiet_dir/artifact-out"
"$quiet_runner" --stdout "$quiet_artifact" "quiet artifact" -- \
  "$quiet_dir/pass" >"$quiet_artifact_out" 2>&1
grep -Fq 'routine line 2' "$quiet_artifact" &&
  ! grep -Fq 'routine line' "$quiet_artifact_out" || {
  echo "the quiet wrapper did not retain machine stdout silently:" >&2
  cat "$quiet_artifact_out" >&2
  exit 1
}

quiet_failed_artifact="$quiet_dir/failed-artifact"
quiet_failed_artifact_out="$quiet_dir/failed-artifact-out"
quiet_failed_artifact_status=0
"$quiet_runner" --stdout "$quiet_failed_artifact" "quiet artifact failure" -- \
  "$quiet_dir/fail" >"$quiet_failed_artifact_out" 2>&1 ||
  quiet_failed_artifact_status=$?
[[ "$quiet_failed_artifact_status" -eq 7 ]] &&
  grep -Fq 'partial-stdout' "$quiet_failed_artifact" &&
  grep -Fq 'partial-stdout' "$quiet_failed_artifact_out" &&
  grep -Fq 'failure-stderr' "$quiet_failed_artifact_out" || {
  echo "the quiet wrapper did not replay both retained failure streams:" >&2
  cat "$quiet_failed_artifact_out" >&2
  exit 1
}

quiet_interrupt_out="$quiet_dir/interrupt-out"
quiet_interrupt_status=0
"$quiet_runner" "quiet interrupt" -- "$quiet_dir/interrupt" \
  >"$quiet_interrupt_out" 2>&1 || quiet_interrupt_status=$?
[[ "$quiet_interrupt_status" -eq 143 ]] &&
  grep -Fq 'quiet interrupt: INTERRUPTED by TERM' "$quiet_interrupt_out" &&
  grep -Fq 'partial-before-signal' "$quiet_interrupt_out" || {
  echo "the quiet wrapper did not replay its log on interruption:" >&2
  cat "$quiet_interrupt_out" >&2
  exit 1
}

# scripts/run-gate-binaries.sh turns a cargo artifact stream into a set of
# concurrently executed test binaries. Exercise discovery, the fail-closed set
# check, and both failure paths against fixtures — no compilation involved, so
# these run everywhere the rest of this file does.
gate_dir="$tmp_dir/gate-runner"
mkdir -p "$gate_dir/pkg" "$gate_dir/bin"
gate_runner="$repo_root/scripts/run-gate-binaries.sh"

# A stand-in test binary, named the way cargo names one: target, then a
# disambiguating hash the runner has to strip back off.
gate_binary() {
  local path="$gate_dir/bin/$1-0123456789abcdef"
  printf '#!/usr/bin/env bash\n%s\n' "$2" >"$path"
  chmod +x "$path"
  printf '%s\n' "$path"
}
gate_pass_one="$(gate_binary pass_one 'printf "cwd=%s\n" "$PWD"; exit 0')"
gate_pass_two="$(gate_binary pass_two 'exit 0')"
gate_failing="$(gate_binary failing 'echo boom >&2; exit 3')"
# Kills the subshell that launched it, so no result is ever recorded — the one
# way to reach the runner's "killed" path deterministically. The sleep only
# runs if the kill did not land, turning a mis-fire into a fast wrong-output
# failure instead of a hang.
gate_killed="$(gate_binary suicidal 'kill -KILL "$PPID"; sleep 5')"

gate_artifact() {
  printf '{"reason":"compiler-artifact","manifest_path":"%s/pkg/Cargo.toml",' "$gate_dir"
  printf '"target":{"kind":["lib"],"name":"x","test":true},'
  printf '"profile":{"opt_level":"0","test":%s},"features":[],' "$2"
  printf '"filenames":["x"],"executable":%s,"fresh":true}\n' "$1"
}
gate_stream() {
  local out="$1"
  shift
  {
    printf '{"reason":"build-script-executed","package_id":"x",'
    printf '"linked_paths":["native=%s/bin"],"cfgs":[],"env":[]}\n' "$gate_dir"
    local executable
    for executable in "$@"; do
      gate_artifact "\"$executable\"" true
    done
    # A dependency binary (alignc, in the real gate) carries profile test:false,
    # and a plain library unit carries no executable at all. Neither may run.
    gate_artifact "\"$gate_dir/bin/not_a_test-0123456789abcdef\"" false
    gate_artifact null true
  } >"$out"
}

gate_ok_json="$tmp_dir/gate-ok.json"
gate_stream "$gate_ok_json" "$gate_pass_one" "$gate_pass_two"
gate_ok_out="$tmp_dir/gate-ok-out"
ALIGN_GATE_JOBS=2 "$gate_runner" "$gate_ok_json" pass_one pass_two \
  >"$gate_ok_out" 2>&1 || {
  echo "the gate runner failed on a clean fixture:" >&2
  cat "$gate_ok_out" >&2
  exit 1
}
grep -Fq 'gate: all 2 binaries passed' "$gate_ok_out" || {
  echo "the clean gate did not emit its aggregate success summary:" >&2
  cat "$gate_ok_out" >&2
  exit 1
}
[[ "$(wc -l <"$gate_ok_out" | tr -d '[:space:]')" -le 4 ]] || {
  echo "the clean gate emitted more than four summary lines:" >&2
  cat "$gate_ok_out" >&2
  exit 1
}
if grep -Eq '^gate: start |^--- |^cwd=' "$gate_ok_out"; then
  echo "the clean gate leaked per-binary output in routine mode:" >&2
  cat "$gate_ok_out" >&2
  exit 1
fi
if grep -Fq 'not_a_test' "$gate_ok_out"; then
  echo "the gate runner ran a non-test artifact:" >&2
  cat "$gate_ok_out" >&2
  exit 1
fi

# Verbose mode restores the detailed report used to inspect scheduling and
# working-directory parity without making it the default CI transcript.
gate_verbose_out="$tmp_dir/gate-verbose-out"
ALIGN_GATE_JOBS=2 ALIGN_TB_VERBOSE=1 \
  "$gate_runner" "$gate_ok_json" pass_one pass_two \
  >"$gate_verbose_out" 2>&1 || {
  echo "the gate runner's verbose clean fixture failed:" >&2
  cat "$gate_verbose_out" >&2
  exit 1
}
for expected_line in '--- pass_one (exit 0' '--- pass_two (exit 0' \
  'gate: start pass_one' "cwd=$gate_dir/pkg"; do
  grep -Fq -e "$expected_line" "$gate_verbose_out" || {
    echo "the verbose gate output lacks '$expected_line':" >&2
    cat "$gate_verbose_out" >&2
    exit 1
  }
done

# The declared set is fail-closed in both directions, on a multiset: a missing
# binary, an extra one, and a duplicate target name all have to fail.
gate_dup_json="$tmp_dir/gate-dup.json"
gate_stream "$gate_dup_json" "$gate_pass_one" "$gate_pass_one" "$gate_pass_two"
gate_mismatch_case() {
  local label="$1" stream="$2"
  shift 2
  local out="$tmp_dir/gate-mismatch-$label"
  local status=0
  ALIGN_GATE_JOBS=2 "$gate_runner" "$stream" "$@" >"$out" 2>&1 || status=$?
  [[ "$status" -ne 0 ]] || {
    echo "the gate runner accepted a $label binary set" >&2
    cat "$out" >&2
    exit 1
  }
  grep -Fq 'do not match the declared gate set' "$out" || {
    echo "the $label binary set failed for the wrong reason:" >&2
    cat "$out" >&2
    exit 1
  }
}
gate_mismatch_case missing "$gate_ok_json" pass_one pass_two never_compiled
gate_mismatch_case extra "$gate_ok_json" pass_one
gate_mismatch_case duplicate "$gate_dup_json" pass_one pass_two

# A failing binary fails the gate and reports its own exit code, and a child
# that dies before recording one is reported rather than counted as a pass.
gate_outcome_case() {
  local label="$1" header="$2"
  shift 2
  local stream="$tmp_dir/gate-$label.json"
  gate_stream "$stream" "$@"
  local out="$tmp_dir/gate-$label-out"
  local status=0
  local names
  names="$(printf '%s\n' "$@" | while IFS= read -r path; do
    path="$(basename "$path")"
    printf '%s ' "${path%-*}"
  done)"
  # shellcheck disable=SC2086
  ALIGN_GATE_JOBS=2 "$gate_runner" "$stream" $names >"$out" 2>&1 || status=$?
  [[ "$status" -ne 0 ]] || {
    echo "the gate runner passed despite a $label binary" >&2
    cat "$out" >&2
    exit 1
  }
  grep -Fq -e "$header" "$out" || {
    echo "the gate runner did not report the $label binary as '$header':" >&2
    cat "$out" >&2
    exit 1
  }
}
gate_outcome_case failing '--- failing (exit 3' "$gate_pass_one" "$gate_failing"
gate_outcome_case killed '--- suicidal (exit killed' "$gate_pass_one" "$gate_killed"

# The gate has no per-binary cap and must not be able to acquire one from the
# environment: its slowest binaries are single-threaded stress tests, and a
# stray exported budget would start killing exactly those. The runner pins the
# variable, so an exported value has to change nothing.
gate_slow="$(gate_binary slow 'sleep 3; exit 0')"
gate_slow_json="$tmp_dir/gate-slow.json"
gate_stream "$gate_slow_json" "$gate_slow"
gate_slow_out="$tmp_dir/gate-slow-out"
ALIGN_GATE_JOBS=2 ALIGN_TB_TIMEOUT=1 ALIGN_TB_PROGRESS_INTERVAL=1 \
  "$gate_runner" "$gate_slow_json" slow \
  >"$gate_slow_out" 2>&1 || {
  echo "an exported ALIGN_TB_TIMEOUT reached the gate and killed a slow binary:" >&2
  cat "$gate_slow_out" >&2
  exit 1
}
grep -Fq -- 'gate: all 1 binaries passed' "$gate_slow_out" || {
  echo "the gate did not aggregate the slow binary as a clean pass:" >&2
  cat "$gate_slow_out" >&2
  exit 1
}
grep -Eq '^gate: progress 0 complete, 1 running \(slow\), 1 launched$' \
  "$gate_slow_out" || {
  echo "the slow gate emitted no bounded active-binary progress record:" >&2
  cat "$gate_slow_out" >&2
  exit 1
}

# An external timeout/cancellation exits before the ordinary report. Its signal
# trap must replay partial logs before cleanup removes the temporary directory.
gate_interrupted="$(gate_binary interrupted 'echo partial-before-interrupt; sleep 30')"
gate_interrupted_json="$tmp_dir/gate-interrupted.json"
gate_stream "$gate_interrupted_json" "$gate_interrupted"
gate_interrupted_out="$tmp_dir/gate-interrupted-out"
ALIGN_GATE_JOBS=1 "$gate_runner" "$gate_interrupted_json" interrupted \
  >"$gate_interrupted_out" 2>&1 &
gate_interrupted_pid=$!
sleep 1
kill -TERM "$gate_interrupted_pid"
gate_interrupted_status=0
wait "$gate_interrupted_pid" || gate_interrupted_status=$?
[[ "$gate_interrupted_status" -eq 143 ]] &&
  grep -Fq 'gate: interrupted by TERM' "$gate_interrupted_out" &&
  grep -Fq 'partial-before-interrupt' "$gate_interrupted_out" || {
  echo "the interrupted gate did not preserve its partial binary log:" >&2
  cat "$gate_interrupted_out" >&2
  exit 1
}

# scripts/run-suite-binaries.sh is the nightly detector: the same discovery and
# concurrency engine, judged against scripts/known-failures.txt instead of a
# declared binary set. Its verdict has three directions and all three are
# exercised here against fixtures, so the nightly cannot regress into the
# always-red fail-fast state it replaced without this file failing first.
suite_dir="$tmp_dir/suite-runner"
mkdir -p "$suite_dir/pkg" "$suite_dir/bin"
suite_runner="$repo_root/scripts/run-suite-binaries.sh"

# libtest's own output shape: one line per test, then the summary line the
# runner requires as proof that the binary got to the end.
suite_binary() {
  local path="$suite_dir/bin/$1-0123456789abcdef"
  printf '#!/usr/bin/env bash\n%s\n' "$2" >"$path"
  chmod +x "$path"
  printf '%s\n' "$path"
}
suite_green="$(suite_binary suite_green '
printf "test alpha ... ok\n"
printf "\ntest result: ok. 1 passed; 0 failed; 0 ignored\n"
exit 0')"
suite_red="$(suite_binary suite_red '
printf "test alpha ... ok\n"
printf "test beta ... FAILED\n"
printf "\nfailures:\n    beta\n\n"
printf "test result: FAILED. 1 passed; 1 failed; 0 ignored\n"
exit 101')"
suite_env="$(suite_binary suite_env '
printf "test needs_network ... FAILED\n"
printf "\nfailures:\n    needs_network\n\n"
printf "test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
exit 101')"
# Reached the summary line but still exited non-zero: a harness-level failure
# that names no test must not be read as a clean binary.
suite_harness="$(suite_binary suite_harness '
printf "test result: ok. 0 passed; 0 failed; 0 ignored\n"
exit 4')"
# Never reaches the summary line, and never exits either — the hang the
# per-binary cap exists for.
suite_hang="$(suite_binary suite_hang '
printf "test alpha ... ok\n"
sleep 30')"
# Named a failure, printed the summary, and THEN died (a segfault in a
# destructor, say). Its named failure is in the manifest; the crash is not, and
# must not be absorbed by it.
suite_crash="$(suite_binary suite_crash '
printf "test beta ... FAILED\n"
printf "\nfailures:\n    beta\n\n"
printf "test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
exit 139')"

suite_artifact() {
  local executable="$1" package="${2:-pkg}" kind="${3:-test}" name="${4:-}"
  if [ -z "$name" ]; then
    name="$(basename "$executable")"
    name="${name%-*}"
  fi
  printf '{"reason":"compiler-artifact","manifest_path":"%s/%s/Cargo.toml",' "$suite_dir" "$package"
  printf '"target":{"kind":["%s"],"crate_types":["bin"],"name":"%s","test":true},' "$kind" "$name"
  printf '"profile":{"opt_level":"0","test":true},"features":[],'
  printf '"filenames":["x"],"executable":"%s","fresh":true}\n' "$executable"
}
suite_stream() {
  local out="$1" executable
  shift
  {
    printf '{"reason":"build-script-executed","package_id":"x",'
    printf '"linked_paths":["native=%s/bin"],"cfgs":[],"env":[]}\n' "$suite_dir"
    for executable in "$@"; do
      suite_artifact "$executable"
    done
  } >"$out"
}

suite_manifest() {
  local path="$tmp_dir/known-failures-$1"
  shift
  printf '%s\n' "$@" >"$path"
  printf '%s\n' "$path"
}
# The fixture manifest is written with real tabs, exactly as the shipped one is.
suite_line() {
  local target="$1"
  case "$target" in
    *::*::*) ;;
    *) target="pkg::test::$target" ;;
  esac
  printf '%s\t%s' "$target" "$2"
  [ $# -lt 3 ] || printf '\t%s' "$3"
  printf '\n'
}

suite_case() {
  local label="$1" manifest="$2" expect_status="$3"
  shift 3
  local stream="$tmp_dir/suite-$label.json"
  suite_stream "$stream" "$@"
  local out="$tmp_dir/suite-$label-out"
  local status=0
  ALIGN_GATE_JOBS=2 ALIGN_SUITE_BINARY_TIMEOUT=2 ALIGN_KNOWN_FAILURES="$manifest" \
    "$suite_runner" "$stream" >"$out" 2>&1 || status=$?
  [[ "$status" -eq "$expect_status" ]] || {
    echo "the suite runner exited $status (expected $expect_status) for $label:" >&2
    cat "$out" >&2
    exit 1
  }
  printf '%s\n' "$out"
}

# 1. The observed failures are exactly the manifest: green.
match_manifest="$(suite_manifest match "$(suite_line suite_red beta)")"
suite_match_out="$(suite_case match "$match_manifest" 0 "$suite_green" "$suite_red")"
grep -Fq 'matches' "$suite_match_out" || {
  echo "an exact manifest match was not reported as such:" >&2
  cat "$suite_match_out" >&2
  exit 1
}
# Exact matches are routine success: the manifest verdict remains visible while
# every captured per-binary line stays out of the default transcript.
[[ "$(wc -l <"$suite_match_out" | tr -d '[:space:]')" -le 4 ]] || {
  echo "an exact suite match emitted more than four summary lines:" >&2
  cat "$suite_match_out" >&2
  exit 1
}
if grep -Eq '^suite: start |^--- |^test result:' "$suite_match_out"; then
  echo "an exact suite match leaked per-binary output:" >&2
  cat "$suite_match_out" >&2
  exit 1
fi

# Verbose mode keeps known failures auditable and restores passing summaries
# when an investigation needs the old per-binary transcript.
suite_match_verbose_out="$tmp_dir/suite-match-verbose-out"
ALIGN_GATE_JOBS=2 ALIGN_TB_VERBOSE=1 ALIGN_SUITE_BINARY_TIMEOUT=2 \
  ALIGN_KNOWN_FAILURES="$match_manifest" \
  "$suite_runner" "$tmp_dir/suite-match.json" \
  >"$suite_match_verbose_out" 2>&1 || {
  echo "the verbose exact-match suite failed:" >&2
  cat "$suite_match_verbose_out" >&2
  exit 1
}
for expected_line in '--- pkg::test::suite_red (exit 101' \
  '--- pkg::test::suite_green (exit 0' 'test result: ok.'; do
  grep -Fq -e "$expected_line" "$suite_match_verbose_out" || {
    echo "the verbose suite report lacks '$expected_line':" >&2
    cat "$suite_match_verbose_out" >&2
    exit 1
  }
done

# 2. A failure the manifest does not list is red, named, and its full log is
# printed rather than summarised.
empty_manifest="$(suite_manifest empty '# nothing is expected to fail')"
suite_new_out="$(suite_case new "$empty_manifest" 1 "$suite_green" "$suite_red")"
grep -Fq 'NEW failures' "$suite_new_out" || {
  echo "an unlisted failure was not reported as new:" >&2
  cat "$suite_new_out" >&2
  exit 1
}
grep -Eq '^  pkg::test::suite_red[[:space:]]+beta$' "$suite_new_out" || {
  echo "the new failure was not named target-and-test:" >&2
  cat "$suite_new_out" >&2
  exit 1
}
grep -Fq 'test alpha ... ok' "$suite_new_out" || {
  echo "the offending binary's own output was not printed in full:" >&2
  cat "$suite_new_out" >&2
  exit 1
}

# 3. A manifest entry that now passes is red too — the ratchet direction.
stale_manifest="$(suite_manifest stale \
  "$(suite_line suite_red beta)" "$(suite_line suite_green alpha)")"
suite_fixed_out="$(suite_case fixed "$stale_manifest" 1 "$suite_green" "$suite_red")"
grep -Fq 'did NOT fail' "$suite_fixed_out" || {
  echo "a repaired test was not reported against the manifest:" >&2
  cat "$suite_fixed_out" >&2
  exit 1
}
grep -Eq '^  pkg::test::suite_green[[:space:]]+alpha \(passed\)$' "$suite_fixed_out" || {
  echo "the repaired test was not named:" >&2
  cat "$suite_fixed_out" >&2
  exit 1
}
# A manifest line naming a target that no longer exists is the same failure
# with a different explanation, never a silent pass.
gone_manifest="$(suite_manifest gone "$(suite_line suite_gone alpha)")"
suite_gone_out="$(suite_case gone "$gone_manifest" 1 "$suite_green")"
grep -Fq 'no such target in the workspace' "$suite_gone_out" || {
  echo "a manifest line for a deleted target was not reported:" >&2
  cat "$suite_gone_out" >&2
  exit 1
}

# 4. An "env" entry runs but does not decide the verdict, in either direction.
env_manifest="$(suite_manifest env \
  "$(suite_line suite_env needs_network env)" "$(suite_line suite_green alpha env)")"
suite_env_out="$(suite_case env "$env_manifest" 0 "$suite_green" "$suite_env")"
grep -Fq '0 known, 2 environment-dependent' "$suite_env_out" || {
  echo "the manifest counts were not reported as 0 known, 2 environment-dependent:" >&2
  cat "$suite_env_out" >&2
  exit 1
}
# Tolerating an outcome is not the same as tolerating a fiction: an env line
# for a target that no longer exists excuses nothing and is red, symmetrically
# with a strict line.
env_gone_manifest="$(suite_manifest env-gone "$(suite_line suite_gone needs_network env)")"
suite_env_gone_out="$(suite_case env-gone "$env_gone_manifest" 1 "$suite_green")"
grep -Eq '^  pkg::test::suite_gone[[:space:]]+needs_network \(no such target in the workspace\)$' \
  "$suite_env_gone_out" || {
  echo "an env line for a deleted target was not reported:" >&2
  cat "$suite_env_gone_out" >&2
  exit 1
}
# One key cannot be strict and tolerated at once; whichever rule won would be
# an accident of ordering, so the manifest is malformed (exit 2).
both_manifest="$(suite_manifest both \
  "$(suite_line suite_red beta)" "$(suite_line suite_red beta env)")"
suite_both_out="$(suite_case both "$both_manifest" 2 "$suite_red")"
grep -Fq 'both as a known failure and as' "$suite_both_out" || {
  echo "a key listed as both strict and env was not rejected:" >&2
  cat "$suite_both_out" >&2
  exit 1
}

# 5. A binary that exits non-zero without naming a test, and one that never
# reaches libtest's summary at all, both fail closed.
suite_harness_out="$(suite_case harness "$empty_manifest" 1 "$suite_harness")"
grep -Eq '^  pkg::test::suite_harness[[:space:]]+<binary-exit-4>$' "$suite_harness_out" || {
  echo "a harness-level non-zero exit was not reported:" >&2
  cat "$suite_harness_out" >&2
  exit 1
}
suite_hang_out="$(suite_case hang "$empty_manifest" 1 "$suite_hang")"
grep -Eq '^  pkg::test::suite_hang[[:space:]]+<binary-did-not-report>$' "$suite_hang_out" || {
  echo "a binary killed by the per-binary cap was not reported:" >&2
  cat "$suite_hang_out" >&2
  exit 1
}
grep -Fq -- '--- pkg::test::suite_hang (exit timeout' "$suite_hang_out" || {
  echo "the capped binary was not reported as a timeout:" >&2
  cat "$suite_hang_out" >&2
  exit 1
}
# A target that only produced a synthetic marker reported no trustworthy
# outcome, so its manifest entries must not be advised out of the file: the
# hung binary never got to run its known-failing test.
hang_known_manifest="$(suite_manifest hang-known "$(suite_line suite_hang alpha)")"
suite_hang_known_out="$(suite_case hang-known "$hang_known_manifest" 1 "$suite_hang")"
grep -Eq '^  pkg::test::suite_hang[[:space:]]+<binary-did-not-report>$' "$suite_hang_known_out" || {
  echo "a hung binary with a manifest entry was not reported as did-not-report:" >&2
  cat "$suite_hang_known_out" >&2
  exit 1
}
grep -Fq 'did NOT fail' "$suite_hang_known_out" && {
  echo "a hung target's manifest entry was advised out of the file:" >&2
  cat "$suite_hang_known_out" >&2
  exit 1
}
# The absorption hole: every test the crashing binary named is already in the
# manifest, so only the exit code distinguishes this run from a clean baseline.
crash_manifest="$(suite_manifest crash "$(suite_line suite_crash beta)")"
suite_crash_out="$(suite_case crash "$crash_manifest" 1 "$suite_crash")"
grep -Eq '^  pkg::test::suite_crash[[:space:]]+<binary-exit-139>$' "$suite_crash_out" || {
  echo "a crash after libtest's summary was absorbed by the known failure:" >&2
  cat "$suite_crash_out" >&2
  exit 1
}
grep -Fq 'did NOT fail' "$suite_crash_out" && {
  echo "the crashing binary's known failure was also reported as repaired:" >&2
  cat "$suite_crash_out" >&2
  exit 1
}
# Failure names come from the trailing "failures:" list, not the racy progress
# lines: stdout and stderr share one log, and a stderr write splicing into the
# middle of "test beta ... FAILED" used to lose the name and report a phantom
# <binary-exit-101> instead (observed on the first nightly). The name list is
# printed after every test finished, so this run matches its manifest line
# exactly. The detail section exercises the parser's block reset (each
# "failures:" heading discards what came before); that detail content never
# LOOKS like a name list is a property of libtest's output shape, which this
# fixture uses but does not prove.
suite_dirty="$(suite_binary suite_dirty '
printf "test alpha ... ok\ntest beta ..."
printf "stderr splice from a concurrent test\n"
printf " FAILED\n"
printf "\nfailures:\n\n---- beta stdout ----\n    indented panic detail\n\n"
printf "failures:\n    beta\n\n"
printf "test result: FAILED. 1 passed; 1 failed; 0 ignored\n"
exit 101')"
dirty_manifest="$(suite_manifest dirty "$(suite_line suite_dirty beta)")"
suite_dirty_out="$(suite_case dirty "$dirty_manifest" 0 "$suite_dirty")"
grep -Fq 'matches' "$suite_dirty_out" || {
  echo "a spliced progress line changed the verdict despite an intact failures list:" >&2
  cat "$suite_dirty_out" >&2
  exit 1
}
grep -Fq 'failure-list-unparsed' "$suite_dirty_out" && {
  echo "a failures list matching its counted total still drew the unparsed marker:" >&2
  cat "$suite_dirty_out" >&2
  exit 1
}
# A splice INTO the name list itself loses a counted name. The summary's own
# "N failed" total catches that: the collected names are reported, the deficit
# draws the synthetic <failure-list-unparsed> no manifest line can match, and
# the run is red — a mangled list can never absorb a failure. The lost name
# (beta, listed in the manifest) must NOT be advised out of the file: this
# target reported no trustworthy outcome.
suite_splice="$(suite_binary suite_splice '
printf "test alpha ... FAILED\ntest beta ... FAILED\n"
printf "\nfailures:\n    alpha\n"
printf "stderr splice inside the name list\n"
printf "    beta\n\n"
printf "test result: FAILED. 0 passed; 2 failed; 0 ignored\n"
exit 101')"
splice_manifest="$(suite_manifest splice \
  "$(suite_line suite_splice alpha)" "$(suite_line suite_splice beta)")"
suite_splice_out="$(suite_case splice "$splice_manifest" 1 "$suite_splice")"
grep -Eq '^  pkg::test::suite_splice[[:space:]]+<failure-list-unparsed>$' "$suite_splice_out" || {
  echo "a spliced failures list was not reported as unparsed:" >&2
  cat "$suite_splice_out" >&2
  exit 1
}
grep -Fq 'did NOT fail' "$suite_splice_out" && {
  echo "a name lost to the splice was advised out of the manifest:" >&2
  cat "$suite_splice_out" >&2
  exit 1
}

# 6. A malformed manifest is a configuration error (exit 2), not a verdict.
space_manifest="$(suite_manifest spaces 'suite_red beta')"
suite_case spaces "$space_manifest" 2 "$suite_red" >/dev/null
kind_manifest="$(suite_manifest kind "$(suite_line suite_red beta flaky)")"
suite_case kind "$kind_manifest" 2 "$suite_red" >/dev/null
legacy_manifest="$(suite_manifest legacy "$(printf 'suite_red\tbeta')")"
legacy_out="$(suite_case legacy "$legacy_manifest" 2 "$suite_red")"
grep -Fq 'target must be package-dir::kind::target' "$legacy_out" || {
  echo "an unqualified manifest key was rejected for the wrong reason:" >&2
  cat "$legacy_out" >&2
  exit 1
}

# 7. Two identical Cargo target identities would make a manifest line
# ambiguous, so the run refuses rather than binding the line arbitrarily.
suite_dup_out="$(suite_case duplicate "$empty_manifest" 1 "$suite_green" "$suite_green")"
grep -Fq 'share a package/kind/name identity' "$suite_dup_out" || {
  echo "duplicate Cargo target identities were not rejected:" >&2
  cat "$suite_dup_out" >&2
  exit 1
}

# Executable basenames, conversely, are not identities. Own every qualifier:
# the current workspace has a same-package `align-repl` bin and `align_repl`
# lib that Cargo emits as two `align_repl-<hash>` files; separate packages may
# reuse one integration-test target name; and one package may reuse a target
# name across kinds. All six binaries must run under distinct stable keys.
mkdir -p "$suite_dir/repl" "$suite_dir/alpha" "$suite_dir/beta" \
  "$suite_dir/kinds" "$suite_dir/bin/identity"
suite_identity_binary() {
  local directory="$1" name="$2" path
  path="$suite_dir/bin/identity/$directory/$name-0123456789abcdef"
  mkdir -p "$(dirname "$path")"
  printf '#!/usr/bin/env bash\nprintf "test result: ok. 0 passed; 0 failed; 0 ignored\\n"\n' >"$path"
  chmod +x "$path"
  printf '%s\n' "$path"
}
identity_repl_bin="$(suite_identity_binary repl-bin align_repl)"
identity_repl_lib="$(suite_identity_binary repl-lib align_repl)"
identity_alpha="$(suite_identity_binary alpha shared)"
identity_beta="$(suite_identity_binary beta shared)"
identity_kind_lib="$(suite_identity_binary kind-lib shared_kind)"
identity_kind_bin="$(suite_identity_binary kind-bin shared_kind)"
identity_stream="$tmp_dir/suite-qualified-identities.json"
{
  printf '{"reason":"build-script-executed","package_id":"x",'
  printf '"linked_paths":["native=%s/bin"],"cfgs":[],"env":[]}\n' "$suite_dir"
  suite_artifact "$identity_repl_bin" repl bin align-repl
  suite_artifact "$identity_repl_lib" repl lib align_repl
  suite_artifact "$identity_alpha" alpha test shared
  suite_artifact "$identity_beta" beta test shared
  suite_artifact "$identity_kind_lib" kinds lib shared-kind
  suite_artifact "$identity_kind_bin" kinds bin shared-kind
} >"$identity_stream"
identity_out="$tmp_dir/suite-qualified-identities-out"
ALIGN_GATE_JOBS=2 ALIGN_TB_VERBOSE=1 ALIGN_SUITE_BINARY_TIMEOUT=2 \
  ALIGN_KNOWN_FAILURES="$empty_manifest" \
  "$suite_runner" "$identity_stream" >"$identity_out" 2>&1 || {
  echo "qualified Cargo target identities still collided:" >&2
  cat "$identity_out" >&2
  exit 1
}
for identity in \
  'repl::bin::align-repl' 'repl::lib::align_repl' \
  'alpha::test::shared' 'beta::test::shared' \
  'kinds::lib::shared-kind' 'kinds::bin::shared-kind'; do
  grep -Fq -- "--- $identity (exit 0" "$identity_out" || {
    echo "the suite omitted qualified identity $identity:" >&2
    cat "$identity_out" >&2
    exit 1
  }
done

# 8. The nightly admits measured long runners first. Priority changes only
# launch order: every qualified target still runs once, and unranked targets
# retain Cargo's artifact order.
mkdir -p "$suite_dir/align_driver"
schedule_stream="$tmp_dir/suite-priority-order.json"
{
  printf '{"reason":"build-script-executed","package_id":"x",'
  printf '"linked_paths":["native=%s/bin"],"cfgs":[],"env":[]}\n' "$suite_dir"
  suite_artifact "$suite_green" align_driver test m0
  suite_artifact "$suite_green" align_driver test pkg_db_q3
  suite_artifact "$suite_green" align_driver test constants
  suite_artifact "$suite_green" align_driver test pkg_db_a1
  suite_artifact "$suite_green" align_driver test deep_type_graphs
  suite_artifact "$suite_green" align_driver test inprocess_memo
} >"$schedule_stream"
schedule_out="$tmp_dir/suite-priority-order-out"
ALIGN_GATE_JOBS=1 ALIGN_TB_VERBOSE=1 ALIGN_SUITE_BINARY_TIMEOUT=2 \
  ALIGN_KNOWN_FAILURES="$empty_manifest" \
  "$suite_runner" "$schedule_stream" >"$schedule_out" 2>&1 || {
  echo "the prioritized suite fixture failed:" >&2
  cat "$schedule_out" >&2
  exit 1
}
schedule_starts="$(sed -n 's/^suite: start //p' "$schedule_out")"
expected_schedule="$(printf '%s\n' \
  'align_driver::test::pkg_db_a1' \
  'align_driver::test::pkg_db_q3' \
  'align_driver::test::deep_type_graphs' \
  'align_driver::test::inprocess_memo' \
  'align_driver::test::m0' \
  'align_driver::test::constants')"
[[ "$schedule_starts" == "$expected_schedule" ]] || {
  echo "the suite did not admit long runners first while retaining the unranked order:" >&2
  printf 'expected:\n%s\nactual:\n%s\n' "$expected_schedule" "$schedule_starts" >&2
  exit 1
}

# 9. The self-build branch (no artifact argument) must build the workspace
# before the test-binary build — `cargo test --no-run` alone does not produce
# libalign_runtime.a, the hole that invalidated the first nightly baseline —
# and must fail closed as a configuration error (exit 2) when the build still
# yields no runtime staticlib. Exercised against a copied script tree with a
# stubbed cargo.sh, so nothing compiles here either.
suite_build_root="$tmp_dir/suite-build"
mkdir -p "$suite_build_root/scripts"
cp "$repo_root/scripts/run-suite-binaries.sh" \
  "$repo_root/scripts/test-binaries-lib.sh" \
  "$repo_root/scripts/dyld-env.sh" \
  "$repo_root/scripts/run-quiet.sh" "$suite_build_root/scripts/"
suite_build_artifacts="$tmp_dir/suite-build-artifacts.json"
suite_stream "$suite_build_artifacts" "$suite_green"
suite_build_cargo_log="$tmp_dir/suite-build-cargo-log"
# The stub serves `metadata` too: the runner asks cargo where the artifacts
# landed rather than trusting CARGO_TARGET_DIR (which would miss
# CARGO_BUILD_TARGET_DIR and any .cargo/config override), and this fixture
# deliberately exports no target-dir variable at all so the metadata answer is
# the only source.
cat >"$suite_build_root/scripts/cargo.sh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$FAKE_CARGO_LOG"
case "$1" in
  build) exit 0 ;;
  metadata) printf '{"target_directory":"%s"}\n' "$FAKE_TARGET_DIR" ;;
  test) cat "$FAKE_SUITE_ARTIFACTS" ;;
  *)
    echo "unexpected cargo.sh invocation: $*" >&2
    exit 9
    ;;
esac
STUB
chmod +x "$suite_build_root/scripts/cargo.sh"
suite_build_case() {
  local label="$1" expect_status="$2"
  local out="$tmp_dir/suite-build-$label-out"
  local status=0
  : >"$suite_build_cargo_log"
  FAKE_CARGO_LOG="$suite_build_cargo_log" \
    FAKE_SUITE_ARTIFACTS="$suite_build_artifacts" \
    FAKE_TARGET_DIR="$suite_build_root/target" \
    ALIGN_GATE_JOBS=2 ALIGN_SUITE_BINARY_TIMEOUT=2 \
    ALIGN_KNOWN_FAILURES="$empty_manifest" \
    "$suite_build_root/scripts/run-suite-binaries.sh" >"$out" 2>&1 || status=$?
  [[ "$status" -eq "$expect_status" ]] || {
    echo "the self-build branch exited $status (expected $expect_status) for $label:" >&2
    cat "$out" >&2
    exit 1
  }
  printf '%s\n' "$out"
}
# The workspace build ran but left no runtime staticlib: a configuration
# error, named, and the test-binary build is never attempted.
suite_build_missing_out="$(suite_build_case missing 2)"
grep -Fq 'did not produce' "$suite_build_missing_out" || {
  echo "a missing libalign_runtime.a was not reported:" >&2
  cat "$suite_build_missing_out" >&2
  exit 1
}
grep -q '^build --workspace --locked$' "$suite_build_cargo_log" || {
  echo "the self-build branch never ran the workspace build:" >&2
  cat "$suite_build_cargo_log" >&2
  exit 1
}
if grep -q '^test --no-run' "$suite_build_cargo_log"; then
  echo "the test-binary build ran despite the missing runtime staticlib:" >&2
  cat "$suite_build_cargo_log" >&2
  exit 1
fi
# With the staticlib in place the branch proceeds — workspace build, then the
# metadata lookup, then the test-binary build — through to the normal verdict.
mkdir -p "$suite_build_root/target/debug"
: >"$suite_build_root/target/debug/libalign_runtime.a"
suite_build_ok_out="$(suite_build_case ok 0)"
grep -Fq 'matches' "$suite_build_ok_out" || {
  echo "the self-build branch did not reach the normal verdict:" >&2
  cat "$suite_build_ok_out" >&2
  exit 1
}
[[ "$(sed -n '1p' "$suite_build_cargo_log")" == "build --workspace --locked" ]] || {
  echo "the workspace build did not run first:" >&2
  cat "$suite_build_cargo_log" >&2
  exit 1
}
case "$(sed -n '2p' "$suite_build_cargo_log")" in
  "metadata --format-version 1"*) ;;
  *)
    echo "the target-directory lookup did not follow the workspace build:" >&2
    cat "$suite_build_cargo_log" >&2
    exit 1
    ;;
esac
case "$(sed -n '3p' "$suite_build_cargo_log")" in
  "test --no-run --workspace --locked"*) ;;
  *)
    echo "the test-binary build did not follow the target-directory lookup:" >&2
    cat "$suite_build_cargo_log" >&2
    exit 1
    ;;
esac

# The shipped manifest has to satisfy the same parser, and stay free of the
# duplicates and stray whitespace that would make an entry unmatchable.
shipped_manifest="$repo_root/scripts/known-failures.txt"
[[ -f "$shipped_manifest" ]] || {
  echo "scripts/known-failures.txt is missing" >&2
  exit 1
}
awk -F'\t' '
  /^#/ || /^$/ { next }
  NF < 2 || NF > 3 { printf "%s:%d: expected 2 or 3 tab-separated fields\n", FILENAME, FNR; bad = 1; next }
  NF == 3 && $3 != "env" { printf "%s:%d: unknown third field %s\n", FILENAME, FNR, $3; bad = 1 }
  $1 ~ /^[[:space:]]|[[:space:]]$/ || $2 ~ /^[[:space:]]|[[:space:]]$/ {
    printf "%s:%d: padded field\n", FILENAME, FNR; bad = 1
  }
  { key = $1 "\t" $2; if (seen[key]++) { printf "%s:%d: duplicate entry\n", FILENAME, FNR; bad = 1 } }
  END { exit bad ? 1 : 0 }
' "$shipped_manifest" || exit 1

# The nightly job's budget is the whole point of the rewrite: 30 minutes on the
# suite job, the new runner instead of the fail-fast `cargo test --workspace`,
# and the per-binary cap documented where someone tempted to raise the job
# timeout will read it.
nightly_workflow="$repo_root/.github/workflows/nightly.yml"
if grep -Eq '^ *run: scripts/run-quiet\.sh ".*: ' \
  "$ci_workflow" "$nightly_workflow"; then
  echo "a quiet workflow command contains an unquoted YAML mapping colon" >&2
  exit 1
fi
grep -Fq 'timeout-minutes: 30' "$nightly_workflow" || {
  echo "nightly.yml no longer carries the 30-minute suite budget" >&2
  exit 1
}
grep -Fq 'scripts/run-suite-binaries.sh' "$nightly_workflow" || {
  echo "nightly.yml no longer runs the suite runner" >&2
  exit 1
}
grep -Eq '^      ALIGN_GATE_JOBS: 6$' "$nightly_workflow" || {
  echo "nightly.yml no longer pins the measured six-process suite schedule" >&2
  exit 1
}
if grep -Eq 'cargo\.sh test --workspace' "$nightly_workflow"; then
  echo "nightly.yml runs fail-fast 'cargo test --workspace' again" >&2
  exit 1
fi
grep -Fq 'ALIGN_SUITE_BINARY_TIMEOUT' "$suite_runner" || {
  echo "scripts/run-suite-binaries.sh no longer documents the per-binary cap" >&2
  exit 1
}
grep -Fq '30 minutes' "$suite_runner" || {
  echo "scripts/run-suite-binaries.sh no longer records the 30-minute rule" >&2
  exit 1
}
grep -Eq '^ALIGN_TB_TIMEOUT=0$' "$gate_runner" || {
  echo "scripts/run-gate-binaries.sh no longer pins its per-binary cap off" >&2
  exit 1
}
# A cache key fixed on Cargo.lock alone is written once and never updated, so
# the night that first wrote it — very likely a night that did not finish
# building — would freeze a partial target directory in forever. Every nightly
# save key must vary per run.
nightly_cache_keys="$(grep -cE '^ *key: cargo-nightly' "$nightly_workflow")"
nightly_run_keys="$(grep -cE '^ *key: cargo-nightly.*github\.run_id \}\}$' "$nightly_workflow")"
[[ "$nightly_cache_keys" -gt 0 && "$nightly_cache_keys" -eq "$nightly_run_keys" ]] || {
  echo "a nightly Cargo cache key is not unique per run" >&2
  echo "  key: lines: $nightly_cache_keys, ending in github.run_id: $nightly_run_keys" >&2
  exit 1
}
# ... and the prefix restore-keys are what make those per-run entries usable.
grep -Eq '^ *cargo-nightly-.*-\$\{\{ hashFiles\('"'"'Cargo.lock'"'"'\) \}\}-$' \
  "$nightly_workflow" || {
  echo "nightly.yml lost the Cargo.lock-prefixed restore key" >&2
  exit 1
}

# Release PGO profiles alignc, not the runtime archive that alignc links into
# its training outputs.
release_workflow="$repo_root/.github/workflows/release.yml"
if grep -Eq 'scripts/cargo\.sh build .* -p align_runtime -p align_driver' "$release_workflow"; then
  echo "release.yml instruments the runtime together with alignc again" >&2
  exit 1
fi
# `align_repl` depends on `align_runtime` and `align_driver` does not, so
# selecting the REPL in the profiled invocation drags the archive in through the
# dependency closure — invisible to the literal-flag check above, which is how
# this shipped once. Whenever the profiled build names the REPL, the phase must
# also restore the uninstrumented archive it stashed.
# `-e` is required: both patterns begin with `-`/`c` and the first would
# otherwise be parsed as grep options, silently disabling this check.
if grep -Fq -e '-p align_driver -p align_repl' "$release_workflow"; then
  grep -Fq -e 'cp "$RUNNER_TEMP/libalign_runtime.a" target/dist/libalign_runtime.a' "$release_workflow" || {
    echo "release.yml profiles align_repl without restoring the uninstrumented runtime archive" >&2
    exit 1
  }
fi
release_runtime_builds="$(grep -Fc 'scripts/cargo.sh build --locked --profile dist -p align_runtime' "$release_workflow")"
release_driver_builds="$(grep -Fc 'scripts/cargo.sh build --locked --profile dist -p align_driver' "$release_workflow")"
[[ "$release_runtime_builds" -eq 2 && "$release_driver_builds" -eq 2 ]] || {
  echo "release.yml no longer builds the runtime and compiler separately in both PGO phases" >&2
  exit 1
}
grep -Fq 'scripts/build-prebuilt-cache.sh' "$release_workflow" || {
  echo "release.yml no longer warms the final release compiler's adjacent cache" >&2
  exit 1
}
grep -Fq 'scripts/verify-prebuilt-cache-layout.sh package' "$release_workflow" || {
  echo "release.yml no longer verifies the staged native cache layout" >&2
  exit 1
}
grep -Fq 'ARCHIVE_MEMBERS+=(share)' "$release_workflow" || {
  echo "release.yml no longer includes the cache tree in native archives" >&2
  exit 1
}
grep -Fq 'tar -C package -czf "$ARTIFACT.tar.gz" "${ARCHIVE_MEMBERS[@]}"' \
  "$release_workflow" || {
  echo "release.yml no longer archives its exact selected artifact graph" >&2
  exit 1
}
grep -Fq "grep -q '^share/'" "$release_workflow" || {
  echo "release.yml no longer detects cache-bearing archives for Debian packaging" >&2
  exit 1
}
grep -Fq -- '-C "$ROOT/usr/lib/align" "${ARCHIVE_MEMBERS[@]}"' "$release_workflow" || {
  echo "release.yml no longer preserves the adjacent cache in Debian packages" >&2
  exit 1
}
historical_guards="$(grep -Fc 'historical tag:' "$release_workflow")"
if [[ "$historical_guards" -ne 3 ]]; then
  echo "release.yml no longer preserves historical workflow_dispatch tags" >&2
  exit 1
fi
corpus_links="$(
  find "$repo_root/apps/web/pkg" "$repo_root/apps/jwt/pkg" "$repo_root/apps/db/pkg" \
    -name '*.align' -type f -exec \
    sed -n 's/.*extern "C" link("\([^"]*\)").*/\1/p' '{}' + \
    | LC_ALL=C sort -u | paste -sd, -
)"
if [[ "$corpus_links" != "crypto,pq,sqlite3,ssl,z,zstd" ]]; then
  echo "the prebuilt-cache native-link declaration inventory changed: $corpus_links" >&2
  exit 1
fi
grep -Fq 'libpq-dev libsqlite3-dev libssl-dev zlib1g-dev libzstd-dev' "$release_workflow" || {
  echo "the release runner cannot link the complete cached pkg.db corpus" >&2
  exit 1
}
grep -Fq 'libpq-dev, libsqlite3-dev, libssl-dev, zlib1g-dev, libzstd-dev' \
  "$release_workflow" || {
  echo "the Debian compiler package cannot link the complete cached pkg.db corpus" >&2
  exit 1
}
grep -Fq 'brew install --formula --build-from-source' "$release_workflow" || {
  echo "release.yml no longer tests Homebrew's real install/cleanup path" >&2
  exit 1
}
grep -Fq 'bench/prebuilt_cache/run.sh package' "$release_workflow" || {
  echo "release.yml no longer records prebuilt-cache release evidence" >&2
  exit 1
}

formula_template="$repo_root/.github/align.rb.template"
formula_dependencies="$(
  sed -n 's/^  depends_on "\([^"]*\)"$/\1/p' "$formula_template" \
    | LC_ALL=C sort | paste -sd, -
)"
if [[ "$formula_dependencies" != "libpq,llvm@22,openssl@3,zstd" ]]; then
  echo "the Homebrew dependency mapping changed: $formula_dependencies" >&2
  exit 1
fi
grep -Fq '#{Formula["libpq"].opt_lib}:#{Formula["openssl@3"].opt_lib}:#{Formula["zstd"].opt_lib}' \
  "$formula_template" || {
  echo "the Homebrew linker search path does not cover every keg-only corpus dependency" >&2
  exit 1
}
grep -Fq 'skip_clean "libexec/alignc"' "$formula_template" || {
  echo "the Homebrew formula may rewrite the compiler after its cache is keyed" >&2
  exit 1
}
grep -Fq 'libexec.install "alignc", "align-repl", "libalign_runtime.a", "share"' \
  "$formula_template" || {
  echo "the Homebrew formula no longer installs the adjacent cache tree" >&2
  exit 1
}

cache_source="$repo_root/crates/align_driver/src/cache.rs"
cache_inventory="$repo_root/scripts/prebuilt-cache-inventory.py"
grep -Fq 'CACHE_MANIFEST_MAX_BYTES: u64 = 64 * 1024 * 1024' "$cache_source" &&
  grep -Fq 'MANIFEST_MAX_BYTES = 64 * 1024 * 1024' "$cache_inventory" || {
  echo "runtime and release inventory manifest bounds differ" >&2
  exit 1
}
grep -Fq 'CACHE_CAS_MAX_BYTES: u64 = 256 * 1024 * 1024' "$cache_source" &&
  grep -Fq 'CAS_MAX_BYTES = 256 * 1024 * 1024' "$cache_inventory" || {
  echo "runtime and release inventory CAS bounds differ" >&2
  exit 1
}
python3 -c 'import pathlib, sys; compile(pathlib.Path(sys.argv[1]).read_bytes(), sys.argv[1], "exec")' \
  "$cache_inventory"

for script in \
  bench/prebuilt_cache/run.sh \
  scripts/build-prebuilt-cache.sh \
  scripts/cargo.sh \
  scripts/check-pr-preflight.sh \
  scripts/ci-apt-llvm.sh \
  scripts/dyld-env.sh \
  scripts/new-review-log.sh \
  scripts/open-pr.sh \
  scripts/pr-tier.sh \
  scripts/pre-pr.sh \
  scripts/review-bounded.sh \
  scripts/run-gate-binaries.sh \
  scripts/run-quiet.sh \
  scripts/run-suite-binaries.sh \
  scripts/test-apt-llvm.sh \
  scripts/test-binaries-lib.sh \
  scripts/test-pr-workflow.sh \
  scripts/test-pr.sh \
  scripts/verify-prebuilt-cache-layout.sh
do
  bash -n "$repo_root/$script"
done

# scripts/ci-apt-llvm.sh gates every Linux job's toolchain and broke CI twice in
# one day; its branches are executed here, root-free and offline, so the same
# self-test step that guards the PR machinery also guards the installer.
bash "$repo_root/scripts/test-apt-llvm.sh"
