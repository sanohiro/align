#!/usr/bin/env bash
# Build one required pkg.db owner shard in one Cargo graph, then run its
# binaries concurrently through the same fail-closed runner as the bounded
# gate. No shard means the complete local CI-parity set.
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  echo "usage: scripts/run-db-suites.sh [--list] [all|catalog-stream|delivery-callbacks|vector-static|portable-pool]" >&2
  exit 2
}

list_only=false
if [ "${1:-}" = --list ]; then
  list_only=true
  shift
fi
[ $# -le 1 ] || usage
db_shard="${1:-all}"

db_catalog_stream="pkg_db_q3 pkg_db_q4b pkg_db_q5b1"
db_delivery_callbacks="pkg_db_a1 pkg_db_callbacks pkg_db_q6 pkg_db_q5a"
db_vector_static="pkg_db_vc1 pkg_db_q1 pkg_db_q5b2"
db_portable_pool="pkg_db_q2 pkg_db_q4a pkg_db_pool pkg_db_a2"

case "$db_shard" in
  all)
    db_binaries="$db_catalog_stream $db_delivery_callbacks $db_vector_static $db_portable_pool"
    ;;
  catalog-stream) db_binaries="$db_catalog_stream" ;;
  delivery-callbacks) db_binaries="$db_delivery_callbacks" ;;
  vector-static) db_binaries="$db_vector_static" ;;
  portable-pool) db_binaries="$db_portable_pool" ;;
  *) usage ;;
esac

if [ "$list_only" = true ]; then
  # Deliberately unquoted: names are fixed [a-z0-9_] identifiers.
  printf '%s\n' $db_binaries
  exit 0
fi

artifacts="$(mktemp)"
trap 'rm -f "$artifacts"' EXIT

# Build argv without eval so fixed target names cannot become shell syntax.
set -- -p align_driver
for db_binary in $db_binaries; do
  set -- "$@" --test "$db_binary"
done
scripts/run-quiet.sh --stdout "$artifacts" "database gate: test binaries" -- \
  scripts/cargo.sh test --no-run --locked --message-format=json-render-diagnostics \
  "$@"

# shellcheck disable=SC2086
scripts/run-gate-binaries.sh "$artifacts" $db_binaries
