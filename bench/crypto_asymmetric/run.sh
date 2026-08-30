#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --release
