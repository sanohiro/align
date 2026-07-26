#!/usr/bin/env bash
# Focused tests for the PR workflow guards. No compiler build or test corpus.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

good_sha="0123456789abcdef0123456789abcdef01234567"
base_sha="1111111111111111111111111111111111111111"
base_ref="main"
good_body="$tmp_dir/good-body"
stale_body="$tmp_dir/stale-body"
{
  printf '<!-- align-preflight-version:1 -->\n'
  printf '<!-- align-preflight-head:%s -->\n' "$good_sha"
  printf '<!-- align-preflight-base-ref:%s -->\n' "$base_ref"
  printf '<!-- align-preflight-base-sha:%s -->\n' "$base_sha"
  printf '<!-- align-preflight-review:clean -->\n'
  printf '<!-- align-preflight-reviewer:reviewer-1 -->\n'
  printf '<!-- align-post-review-head:%s -->\n' "$good_sha"
  printf '<!-- align-post-review-base-sha:%s -->\n' "$base_sha"
} >"$good_body"
sed "s/$good_sha/ffffffffffffffffffffffffffffffffffffffff/" "$good_body" >"$stale_body"

"$repo_root/scripts/check-pr-preflight.sh" "$good_sha" "$base_ref" "$base_sha" "$good_body"
if "$repo_root/scripts/check-pr-preflight.sh" \
  "$good_sha" "$base_ref" "$base_sha" "$stale_body" >/dev/null 2>&1
then
  echo "stale preflight unexpectedly passed" >&2
  exit 1
fi

fake_bin="$tmp_dir/bin"
mkdir -p "$fake_bin"
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
  printf '  timeout) sleep 30 ;;\n'
  printf 'esac\n'
} >"$fake_codex"
chmod +x "$fake_codex"

fake_args="$tmp_dir/codex-args"
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=clean FAKE_CODEX_ARGS_FILE="$fake_args" \
  ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null
grep -Fq 'git diff main...HEAD' "$fake_args" || {
  echo "review prompt did not bind the requested base" >&2
  exit 1
}
set +e
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=findings ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null
findings_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=timeout ALIGN_REVIEW_TIMEOUT_SECONDS=1 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
timeout_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=trailing ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
trailing_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=duplicate ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
duplicate_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
native_clean_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-findings ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
native_findings_status=$?
PATH="$fake_bin:$PATH" FAKE_CODEX_MODE=native-clean-readonly ALIGN_REVIEW_TIMEOUT_SECONDS=5 \
  "$repo_root/scripts/review-bounded.sh" --base main >/dev/null 2>&1
native_readonly_status=$?
set -e
[[ $findings_status -eq 2 ]] || {
  echo "findings review returned $findings_status, expected 2" >&2
  exit 1
}
[[ $timeout_status -eq 124 ]] || {
  echo "timed review returned $timeout_status, expected 124" >&2
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

for script in \
  scripts/check-pr-preflight.sh \
  scripts/open-pr.sh \
  scripts/pre-pr.sh \
  scripts/record-post-review.sh \
  scripts/review-bounded.sh \
  scripts/test-pr-workflow.sh \
  scripts/update-pr-preflight.sh
do
  bash -n "$repo_root/$script"
done
