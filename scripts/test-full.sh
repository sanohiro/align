#!/usr/bin/env bash
# Explicit full regression run. This is intentionally not an ordinary PR gate.
set -euo pipefail

cargo build --workspace --locked
cargo test --workspace --locked
