#!/usr/bin/env bash
# Decide whether a committed diff needs the provisioned PostgreSQL integration job.
#
# The caller supplies exact base and head commits. This script prints GitHub
# Actions output assignments, but they are also convenient to assert locally:
#
#   required=true|false
#   reason=<single-line explanation>
#
# Classification fails closed for an unreadable range. Dependency or DB-gate
# machinery and pkg.db surfaces/owners require the provisioned job by path.
# Other compiler sources qualify only when a changed zero-context hunk (including
# its function header) names the database boundary. That keeps an unrelated edit
# in a monolithic source file from inheriting an unchanged DB marker elsewhere in
# the file. Deletions use the same diff, so removed markers remain visible.
set -euo pipefail

cd "${DB_CI_REPO_ROOT:-$(dirname "$0")/..}"

base_sha="${1:-}"
head_sha="${2:-}"

emit() {
  printf 'required=%s\nreason=%s\n' "$1" "$2"
}

if [ -z "$base_sha" ] || [ -z "$head_sha" ] ||
   ! git cat-file -e "$base_sha^{commit}" 2>/dev/null ||
   ! git cat-file -e "$head_sha^{commit}" 2>/dev/null ||
   ! git diff --no-renames --name-only "$base_sha..$head_sha" >/dev/null 2>&1; then
  emit true uncomputable-diff
  exit 0
fi

source_change_names_database_boundary() {
  local path="$1" changed
  if ! changed="$(git diff --no-ext-diff --no-textconv --no-renames --unified=0 \
    "$base_sha..$head_sha" -- "$path" 2>/dev/null)"; then
    return 0
  fi
  printf '%s\n' "$changed" |
    grep -Ei 'pkg[._]db|postgres|libpq|sqlite|align_pkg_db' >/dev/null
}

while IFS= read -r -d '' path; do
  [ -n "$path" ] || continue
  case "$path" in
    Cargo.toml | Cargo.lock | build.rs | rust-toolchain | rust-toolchain.toml | \
    .cargo/config | .cargo/config.toml | crates/*/Cargo.toml | \
    .github/workflows/ci.yml | \
    scripts/db-ci-scope.sh | scripts/test-db-ci-scope.sh | \
    scripts/db-verify-local.sh | scripts/run-db-suites.sh | \
    scripts/run-gate-binaries.sh | scripts/test-binaries-lib.sh | \
    scripts/dyld-env.sh | scripts/run-quiet.sh | \
    scripts/check-libpq-version.sh | \
    scripts/ci-pgdg.sh | scripts/ci-apt-llvm.sh | scripts/cargo.sh | \
    apps/db/* | \
    crates/*/src/db_*.rs | \
    crates/align_driver/src/query_meta_codegen.rs | \
    crates/align_driver/src/static_artifacts.rs | \
    crates/align_driver/src/static_inputs.rs | \
    crates/align_driver/src/static_runtime.rs | \
    crates/align_interface/src/static_artifact.rs | \
    crates/align_driver/tests/pkg_db_*.rs | \
    crates/*/tests/common* | crates/*/tests/helpers/* | \
    crates/align_driver/tests/db_harness/* | \
    crates/align_driver/tests/fixtures/pkg_db_* | \
    crates/align_driver/tests/golden/*postgres* | \
    crates/align_driver/tests/golden/*sqlite* | \
    crates/align_driver/tests/golden/migration_catalog_*)
      emit true database-boundary
      exit 0
      ;;
    crates/*/src/* | crates/*/build.rs | crates/*/tests/*.rs | crates/*/tests/*/*.rs)
      if source_change_names_database_boundary "$path"; then
        emit true database-source
        exit 0
      fi
      ;;
  esac
done < <(git diff --no-renames --name-only -z "$base_sha..$head_sha")

emit false no-database-boundary
