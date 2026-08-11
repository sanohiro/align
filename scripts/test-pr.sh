#!/usr/bin/env bash
# Bounded, deterministic gate for ordinary changes. Expensive regression suites
# are selected explicitly according to docs/impl/16-test-policy.md.
#
# The gate compiles every one of its test binaries in ONE cargo build graph —
# one dependency tail instead of four — and then hands them to
# scripts/run-gate-binaries.sh, which runs them concurrently. That matters
# because the gate's cost is not spread evenly: a few single-threaded stress
# tests (align_codegen_llvm's malformed-embedded-id codegen guard, align_mir's
# deep type-DAG placement and fact-replay validators) own nearly all of the run
# time, so the serial sum was about twice the longest single binary.
set -euo pipefail

scripts/cargo.sh build --workspace --locked

artifacts="$(mktemp)"
trap 'rm -f "$artifacts"' EXIT

# Keep this selection explicit: a newly added workspace library must not
# silently become an every-PR gate just because it contains tests. The small
# deterministic integration targets that protect cross-crate interface
# soundness and formatter behavior are part of the gate as well; the `--test`
# names here are what scripts/pr-tier.sh pins as bounded-gate content.
scripts/cargo.sh test --no-run --locked --message-format=json-render-diagnostics \
  -p align_ast \
  -p align_codegen_llvm \
  -p align_diag \
  -p align_driver \
  -p align_fmt \
  -p align_hash \
  -p align_interface \
  -p align_lexer \
  -p align_mir \
  -p align_parser \
  -p align_sema \
  -p align_span \
  --lib \
  --test effect_fail_closed \
  --test examples \
  --test m0 \
  --test summary \
  >"$artifacts"

# Every binary that selection must produce: one per selected library plus each
# named integration target. The runner fails when the compiled set differs in
# either direction, and scripts/test-pr-workflow.sh checks this list against
# the selection above, so neither a new `-p` nor a second package's
# same-named `--test` target can quietly enter or leave the gate.
gate_binaries="
align_ast align_codegen_llvm align_diag align_driver align_fmt align_hash
align_interface align_lexer align_mir align_parser align_sema align_span
effect_fail_closed examples m0 summary
"

# shellcheck disable=SC2086
scripts/run-gate-binaries.sh "$artifacts" $gate_binaries
