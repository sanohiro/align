#!/usr/bin/env bash
# Decide whether a committed diff needs the provisioned PostgreSQL integration job.
#
# The caller supplies exact base and head commits. This script prints GitHub
# Actions output assignments, but they are also convenient to assert locally:
#
#   required=true|false
#   reason=<single-line explanation>
#
# Classification fails closed. An unreadable range, a deletion, dependency or
# DB-gate machinery, a pkg.db surface/owner, or compiler source that names the
# database boundary all require the provisioned job.
set -euo pipefail

cd "$(dirname "$0")/.."

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

if ! git diff --quiet --no-renames --diff-filter=D "$base_sha..$head_sha" --; then
  emit true deletion
  exit 0
fi

source_names_database_boundary() {
  local revision="$1" path="$2"
  git show "$revision:$path" 2>/dev/null |
    grep -Ei 'pkg[._]db|postgres|libpq|sqlite|align_pkg_db' >/dev/null
}

while IFS= read -r -d '' path; do
  [ -n "$path" ] || continue
  case "$path" in
    Cargo.toml | Cargo.lock | rust-toolchain.toml | crates/*/Cargo.toml | \
    .github/workflows/ci.yml | \
    scripts/db-ci-scope.sh | scripts/test-db-ci-scope.sh | \
    scripts/db-verify-local.sh | scripts/check-libpq-version.sh | \
    scripts/ci-pgdg.sh | scripts/ci-apt-llvm.sh | \
    apps/db/* | \
    crates/align_driver/tests/pkg_db_*.rs | \
    crates/align_driver/tests/db_harness/* | \
    crates/align_driver/tests/fixtures/pkg_db_* | \
    crates/align_driver/tests/golden/*postgres* | \
    crates/align_driver/tests/golden/*sqlite* | \
    crates/align_driver/tests/golden/migration_catalog_*)
      emit true database-boundary
      exit 0
      ;;
    crates/*/src/*)
      if source_names_database_boundary "$base_sha" "$path" ||
         source_names_database_boundary "$head_sha" "$path"; then
        emit true database-source
        exit 0
      fi
      ;;
  esac
done < <(git diff --no-renames --name-only -z "$base_sha..$head_sha")

emit false no-database-boundary
