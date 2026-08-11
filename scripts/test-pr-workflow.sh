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
  $native_findings_status -eq 2 ]] || {
  echo "native review output was classified incorrectly" >&2
  exit 1
}

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
printf '#!/usr/bin/env bash\nexit "${STUB_TEST_PR_EXIT:-0}"\n' >"$tier_repo/scripts/test-pr.sh"
chmod +x "$tier_repo/scripts/test-pr.sh"
{
  printf '#!/usr/bin/env bash\n'
  printf 'case "$1" in\n'
  printf '  clippy) exit "${STUB_CLIPPY_EXIT:-0}" ;;\n'
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
tier_preflight --reviewer reviewer-1 --review-log "$code_log" --base main \
  --owner-test tier -- true >/dev/null
code_head="$(git -C "$tier_repo" rev-parse HEAD)"
grep -Fqx 'kind=code' "$tier_repo/.git/align-preflight/$code_head"

# The parallel scripts/test-pr.sh/Clippy block in the code tier must fail —
# and show BOTH outputs — no matter which side actually fails.
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
  grep -q 'scripts/test-pr.sh output' "$out" || {
    echo "parallel gate ($label) did not show the scripts/test-pr.sh output:" >&2
    cat "$out" >&2
    exit 1
  }
  grep -q 'Clippy output' "$out" || {
    echo "parallel gate ($label) did not show the Clippy output:" >&2
    cat "$out" >&2
    exit 1
  }
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

for script in \
  scripts/cargo.sh \
  scripts/check-pr-preflight.sh \
  scripts/new-review-log.sh \
  scripts/open-pr.sh \
  scripts/pr-tier.sh \
  scripts/pre-pr.sh \
  scripts/review-bounded.sh \
  scripts/test-pr-workflow.sh
do
  bash -n "$repo_root/$script"
done
