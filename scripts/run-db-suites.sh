#!/usr/bin/env bash
# Build the required pkg.db owner binaries in one Cargo graph, then run them
# concurrently through the same fail-closed binary runner as the bounded gate.
set -euo pipefail

cd "$(dirname "$0")/.."

artifacts="$(mktemp)"
trap 'rm -f "$artifacts"' EXIT

scripts/cargo.sh test --no-run --locked --message-format=json-render-diagnostics \
  -p align_driver \
  --test pkg_db_q1 \
  --test pkg_db_q2 \
  --test pkg_db_q3 \
  --test pkg_db_q4a \
  --test pkg_db_q4b \
  --test pkg_db_q5a \
  --test pkg_db_q5b1 \
  --test pkg_db_q5b2 \
  --test pkg_db_q6 \
  --test pkg_db_a1 \
  --test pkg_db_pool \
  --test pkg_db_a2 \
  --test pkg_db_callbacks \
  --test pkg_db_vc1 \
  >"$artifacts"

db_binaries="
pkg_db_q1 pkg_db_q2 pkg_db_q3 pkg_db_q4a pkg_db_q4b pkg_db_q5a
pkg_db_q5b1 pkg_db_q5b2 pkg_db_q6 pkg_db_a1 pkg_db_pool pkg_db_a2
pkg_db_callbacks pkg_db_vc1
"

# shellcheck disable=SC2086
scripts/run-gate-binaries.sh "$artifacts" $db_binaries
