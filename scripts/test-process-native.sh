#!/usr/bin/env bash
# Native process and signal semantics, identical on Linux and macOS.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/cargo.sh build --workspace --locked
process_status=0
scripts/cargo.sh test -p align_runtime --lib --locked process_ || process_status=1
scripts/cargo.sh test -p align_driver --locked --no-fail-fast --test m11_process --test m11_process_command --test m11_process_live --test m11_process_signals || process_status=1
exit "$process_status"
