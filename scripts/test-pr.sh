#!/usr/bin/env bash
# Bounded, deterministic gate for ordinary changes. Expensive regression suites
# are selected explicitly according to docs/impl/16-test-policy.md.
set -euo pipefail

cargo build --workspace --locked

# Keep this list explicit: a newly added workspace library must not silently
# become an every-PR gate just because it contains tests. Multiple `-p` flags
# keep the list bounded without paying Cargo startup overhead once per crate.
cargo test --lib --locked \
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
  -p align_span

# Keep the small deterministic integration targets that protect cross-crate
# interface soundness and formatter behavior in the ordinary gate as well.
cargo test -p align_interface --test effect_fail_closed --test summary --locked
cargo test -p align_fmt --test examples --locked

cargo test -p align_driver --test m0 --locked
