#!/usr/bin/env bash
# Plan 58's portable compiler and native regular-file admission owners.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/cargo.sh build --workspace --locked
admission_status=0
scripts/cargo.sh test -p align_runtime --lib --locked regular_reader || admission_status=1
scripts/cargo.sh test -p align_driver --locked --test client_admission_composition || admission_status=1
exit "$admission_status"
