#!/usr/bin/env bash
# Bounded, deterministic gate for ordinary changes. Expensive regression suites
# are selected explicitly according to docs/impl/16-test-policy.md.
set -euo pipefail

cargo build --workspace --locked
cargo test --workspace --lib --exclude align_runtime --locked
cargo test -p align_driver --test m0 --locked
