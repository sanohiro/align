#!/usr/bin/env bash
# Native CPU/profile, entry ABI and byte-execution owners shared by local and CI runs.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/cargo.sh build --workspace --locked
scripts/cargo.sh test --locked -p align_codegen_llvm --lib return_transport::tests
scripts/cargo.sh test --locked -p align_driver --bin alignc \
  test_limit_tests::cpu_selection_rejects_missing_values_before_flag_stripping -- --exact
scripts/cargo.sh test --locked -p align_driver \
  --test build_target --test build_profiles --test target_cpu_isa \
  --test function_thin_lto --test emit_llvm_stage --test explain_opt \
  --test main_abi --test rt_lto --test runway_a2_binary_codec
