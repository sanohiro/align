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

good_sha="0123456789abcdef0123456789abcdef01234567"
base_sha="1111111111111111111111111111111111111111"
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

"$repo_root/scripts/check-pr-preflight.sh" "$good_sha" "$base_ref" "$base_sha" "$good_body"
if "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$bad_docs_body" >/dev/null 2>&1
then
  echo "docs-only attestation with a reviewer unexpectedly passed" >&2
  exit 1
fi
if "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$stale_body" >/dev/null 2>&1
then
  echo "stale preflight unexpectedly passed" >&2
  exit 1
fi

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
  printf '  pr:view:headRefOid,baseRefName,baseRefOid) printf "%%s\\tmain\\t%%s\\n" "$FAKE_PR_HEAD" "$FAKE_PR_BASE" ;;\n'
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

fake_args="$tmp_dir/codex-args"
review_base_sha="$(git rev-parse 'main^{commit}')"
review_head_sha="$(git rev-parse HEAD)"
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean FAKE_CODEX_ARGS_FILE="$fake_args" \
  ALIGN_REVIEW_STALL_SECONDS=5 ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null
grep -Fq "git diff $review_base_sha...$review_head_sha" "$fake_args" || {
  echo "review prompt did not bind the requested base" >&2
  exit 1
}
set +e
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=findings ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null
findings_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=stall ALIGN_REVIEW_STALL_SECONDS=1 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
stall_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=progress ALIGN_REVIEW_STALL_SECONDS=2 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
progress_status=$?
max_started=$SECONDS
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=progress ALIGN_REVIEW_STALL_SECONDS=60 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=30 ALIGN_REVIEW_MAX_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
max_status=$?
max_elapsed=$((SECONDS - max_started))
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=trailing ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
trailing_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=duplicate ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
duplicate_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
native_clean_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-findings ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
native_findings_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-readonly ALIGN_REVIEW_STALL_SECONDS=5 \
  ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
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
[[ $max_elapsed -lt 10 ]] || {
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
for stub in lint-ratchet.sh test-pr.sh cargo.sh; do
  printf '#!/usr/bin/env bash\nexit 0\n' >"$tier_repo/scripts/$stub"
  chmod +x "$tier_repo/scripts/$stub"
done
git -C "$tier_repo" init -q -b main
git -C "$tier_repo" config user.name workflow-test
git -C "$tier_repo" config user.email workflow-test@example.invalid
git -C "$tier_repo" config commit.gpgsign false
printf 'fn main() {}\n' >"$tier_repo/crates/thing/src/lib.rs"
printf '#[test]\nfn baseline() {}\n' >"$tier_repo/crates/thing/tests/baseline_owner.rs"
printf '# plan\n' >"$tier_repo/docs/impl/00-plan.md"
git -C "$tier_repo" add .
git -C "$tier_repo" commit -qm baseline

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
if "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$tmp_dir/bad-tooling-body" >/dev/null 2>&1
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

for script in \
  scripts/cargo.sh \
  scripts/check-pr-preflight.sh \
  scripts/open-pr.sh \
  scripts/pr-tier.sh \
  scripts/pre-pr.sh \
  scripts/review-bounded.sh \
  scripts/test-pr-workflow.sh
do
  bash -n "$repo_root/$script"
done
