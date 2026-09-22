#!/usr/bin/env bash
# Native CPU/profile, entry ABI and byte-execution owners shared by local and CI runs.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=full
if [[ "${1:-}" == "--ci-core" ]]; then
  mode=ci-core
  shift
fi
if [[ "$#" -ne 0 ]]; then
  echo "usage: scripts/test-codegen-performance.sh [--ci-core]" >&2
  exit 2
fi

# Cargo builds the exact selected targets and their Rust dependencies. The `size --thin-lto`
# owner also links a program, so it needs the runtime's `staticlib`: building `align_driver` as a
# test dependency produces only the runtime `rlib`. Build that one link input explicitly. This is
# a no-op on non-lint CI legs whose preceding workspace build already produced it, and closes the
# lint leg's cold-cache path without restoring a duplicate workspace build.
scripts/cargo.sh build --locked -p align_runtime --lib
scripts/cargo.sh test --locked -p align_codegen_llvm --lib return_transport::tests
scripts/cargo.sh test --locked -p align_driver --bin alignc \
  test_limit_tests::cpu_selection_rejects_missing_values_before_flag_stripping -- --exact
core_targets=(
  --test build_target --test target_cpu_isa --test function_thin_lto
  --test main_abi --test rt_lto
)
if [[ "$mode" == full ]]; then
  # These platform-independent or x86-only owners remain available for a changed codegen boundary;
  # the nightly full-suite is their recurring out-of-gate detector.
  core_targets+=(
    --test build_profiles --test emit_llvm_stage --test explain_opt
    --test runway_a2_binary_codec
  )
fi
scripts/cargo.sh test --locked -p align_driver "${core_targets[@]}"
