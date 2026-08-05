#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
if [[ "${1:-}" == "move-return" || "${1:-}" == "shared-borrow" || "${1:-}" == "exclusive-borrow" ]]; then
  "$repo_root/scripts/cargo.sh" build \
    --quiet \
    --release \
    --manifest-path "$repo_root/Cargo.toml" \
    -p align_runtime
fi
exec "$repo_root/scripts/cargo.sh" run \
  --quiet \
  --release \
  --manifest-path "$repo_root/bench/library_boundary/Cargo.toml" \
  -- "$@"
