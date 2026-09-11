#!/usr/bin/env bash
# Plan 54 native filesystem/identity qualification; shared by local and CI runs.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/cargo.sh build --workspace --locked
observation_status=0
scripts/cargo.sh test -p align_runtime --lib --locked fs_retained_tree || observation_status=1
scripts/cargo.sh test -p align_runtime --lib --locked os_host || observation_status=1
scripts/cargo.sh test -p align_driver --locked --no-fail-fast \
  --test fs_observation_extensions --test fs_retained_tree --test m11_os_identity || observation_status=1
exit "$observation_status"
