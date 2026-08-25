#!/usr/bin/env bash
# Focused owner tests for scripts/db-ci-scope.sh. No compiler build or service.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

git -C "$fixture" init -q
git -C "$fixture" config user.name db-scope-test
git -C "$fixture" config user.email db-scope-test@example.invalid
mkdir -p "$fixture/scripts" "$fixture/crates/demo/src" \
  "$fixture/crates/demo/tests/common" "$fixture/apps/web"
cp "$repo_root/scripts/db-ci-scope.sh" "$fixture/scripts/db-ci-scope.sh"
chmod +x "$fixture/scripts/db-ci-scope.sh"
printf 'baseline\n' > "$fixture/apps/web/main.align"
printf 'pub fn ordinary() {}\n' > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm baseline
base="$(git -C "$fixture" rev-parse HEAD)"

assert_scope() {
  expected="$1"
  shift
  output="$(cd "$fixture" && scripts/db-ci-scope.sh "$@")"
  printf '%s\n' "$output" | grep -Fxq "required=$expected" || {
    echo "expected required=$expected, got:" >&2
    printf '%s\n' "$output" >&2
    exit 1
  }
}

# An unrelated application edit does not provision PostgreSQL.
printf 'changed\n' > "$fixture/apps/web/main.align"
git -C "$fixture" add .
git -C "$fixture" commit -qm unrelated
unrelated="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$base" "$unrelated"

# Path records are NUL-delimited, so an unrelated filename cannot inject a
# forged GitHub output assignment.
weird_path=$'apps/web/unrelated\nrequired=true'
printf 'still unrelated\n' > "$fixture/$weird_path"
git -C "$fixture" add .
git -C "$fixture" commit -qm unusual-path
unusual="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$unrelated" "$unusual"

# A source file that newly names the DB boundary is included without maintaining
# a parallel filename list.
printf 'pub fn postgres_owner() {}\n' > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-source
db_source="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$unusual" "$db_source"

# Removing the last visible DB marker still qualifies because the base content
# participates in classification.
printf 'pub fn ordinary_again() {}\n' > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm remove-db-source
db_source_removed="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_source" "$db_source_removed"

# Shared owner infrastructure reaches every pkg.db suite even when its own text
# does not name a database.
printf 'pub fn shared_fixture() {}\n' > "$fixture/crates/demo/tests/common/mod.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm shared-test-harness
shared_harness="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_source_removed" "$shared_harness"

# A non-pkg_db owner test that directly names the boundary is also classified
# by content.
printf 'fn postgres_regression() {}\n' > "$fixture/crates/demo/tests/direct.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm direct-db-owner
direct_owner="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$shared_harness" "$direct_owner"

# Adding and deleting an unrelated leaf owner must not provision PostgreSQL.
printf 'fn identity_owner() {}\n' > "$fixture/crates/demo/tests/json_identity.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm unrelated-leaf-owner
leaf_owner="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$direct_owner" "$leaf_owner"

rm "$fixture/crates/demo/tests/json_identity.rs"
git -C "$fixture" add -u
git -C "$fixture" commit -qm delete-unrelated-leaf-owner
leaf_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$leaf_owner" "$leaf_deleted"

# Direct package and workflow machinery changes are always included.
mkdir -p "$fixture/apps/db" "$fixture/.github/workflows"
printf 'db\n' > "$fixture/apps/db/main.align"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-package
db_package="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$leaf_deleted" "$db_package"

printf 'name: CI\n' > "$fixture/.github/workflows/ci.yml"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-workflow
workflow="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_package" "$workflow"

printf '#!/usr/bin/env bash\n' > "$fixture/scripts/run-db-suites.sh"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-suite-runner
suite_runner="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$workflow" "$suite_runner"

# An unrelated deletion stays out of the service job.
rm "$fixture/apps/web/main.align"
git -C "$fixture" add -u
git -C "$fixture" commit -qm unrelated-deletion
unrelated_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$suite_runner" "$unrelated_deleted"

# Deleted DB-naming source is classified from its base content, and a deleted
# direct DB path is classified by path. Unreadable ranges still fail closed.
rm "$fixture/crates/demo/tests/direct.rs"
git -C "$fixture" add -u
git -C "$fixture" commit -qm delete-db-source
db_source_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$unrelated_deleted" "$db_source_deleted"

rm "$fixture/apps/db/main.align"
git -C "$fixture" add -u
git -C "$fixture" commit -qm delete-db-path
db_path_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_source_deleted" "$db_path_deleted"
assert_scope true not-a-commit "$db_path_deleted"

# CI extracts the trusted classifier outside the checkout. Its explicit root
# binding must still classify the repository rather than the temporary file's
# parent directory.
trusted_copy="$fixture/trusted-db-ci-scope.sh"
cp "$repo_root/scripts/db-ci-scope.sh" "$trusted_copy"
chmod +x "$trusted_copy"
output="$(DB_CI_REPO_ROOT="$fixture" "$trusted_copy" "$base" "$unrelated")"
printf '%s\n' "$output" | grep -Fxq 'required=false'

ci_workflow="$repo_root/.github/workflows/ci.yml"
test "$(grep -Fc 'name: PostgreSQL integration (required)' "$ci_workflow")" -eq 1
grep -Fq "if: needs.db-scope.outputs.required == 'true'" "$ci_workflow"
grep -Fq 'if: ${{ always() }}' "$ci_workflow"
grep -Fq 'reason=protected-pr-merge' "$ci_workflow"
grep -Fq 'commits/$HEAD_SHA/pulls' "$ci_workflow"
grep -Fq 'git show "$TRUSTED_TIP:scripts/db-ci-scope.sh"' "$ci_workflow"
grep -Fq 'reason=classifier-bootstrap' "$ci_workflow"
grep -Fq 'test "$SCOPE_RESULT" = success' "$ci_workflow"
grep -Fq 'true) test "$DB_RESULT" = success' "$ci_workflow"
grep -Fq 'false) test "$DB_RESULT" = skipped' "$ci_workflow"
grep -Fq 'timeout-minutes: 30' "$ci_workflow"
test "$(grep -Fc 'run: scripts/run-db-suites.sh' "$ci_workflow")" -eq 1
test "$(grep -Fc 'scripts/run-db-suites.sh' "$repo_root/scripts/db-verify-local.sh")" -eq 1

for owner in \
  pkg_db_q1 pkg_db_q2 pkg_db_q3 pkg_db_q4a pkg_db_q4b pkg_db_q5a \
  pkg_db_q5b1 pkg_db_q5b2 pkg_db_q6 pkg_db_a1 pkg_db_pool pkg_db_a2 \
  pkg_db_callbacks pkg_db_vc1; do
  test "$(grep -Ec "^[[:space:]]*--test $owner( |$)" "$repo_root/scripts/run-db-suites.sh")" -eq 1
done

echo "database CI scope tests passed"
