#!/usr/bin/env bash
# Focused owner tests for scripts/db-ci-scope.sh. No compiler build or service.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

git -C "$fixture" init -q
git -C "$fixture" config user.name db-scope-test
git -C "$fixture" config user.email db-scope-test@example.invalid
mkdir -p "$fixture/scripts" "$fixture/crates/demo/src" "$fixture/apps/web"
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

# Direct package and workflow machinery changes are always included.
mkdir -p "$fixture/apps/db" "$fixture/.github/workflows"
printf 'db\n' > "$fixture/apps/db/main.align"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-package
db_package="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_source_removed" "$db_package"

printf 'name: CI\n' > "$fixture/.github/workflows/ci.yml"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-workflow
workflow="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_package" "$workflow"

# Deletions and unreadable ranges fail closed.
rm "$fixture/apps/web/main.align"
git -C "$fixture" add -u
git -C "$fixture" commit -qm deletion
deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$workflow" "$deleted"
assert_scope true not-a-commit "$deleted"

ci_workflow="$repo_root/.github/workflows/ci.yml"
test "$(grep -Fc 'name: PostgreSQL integration (required)' "$ci_workflow")" -eq 1
grep -Fq "if: needs.db-scope.outputs.required == 'true'" "$ci_workflow"
grep -Fq 'if: ${{ always() }}' "$ci_workflow"
grep -Fq 'reason=protected-pr-merge' "$ci_workflow"
grep -Fq 'test "$SCOPE_RESULT" = success' "$ci_workflow"
grep -Fq 'true) test "$DB_RESULT" = success' "$ci_workflow"
grep -Fq 'false) test "$DB_RESULT" = skipped' "$ci_workflow"

echo "database CI scope tests passed"
