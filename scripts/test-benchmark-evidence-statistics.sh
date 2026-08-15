#!/usr/bin/env bash
# Deterministic owner for the protected JSON benchmark's exact inner statistic.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-benchmark-statistics.XXXXXX")"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT HUP INT TERM

rustc --edition=2021 --test \
  "$REPO_ROOT/bench/json_escape/evidence/statistics.rs" \
  -o "$TEST_ROOT/statistics-test"
"$TEST_ROOT/statistics-test"
