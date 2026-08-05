#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
exec "$repo_root/scripts/cargo.sh" run \
  --quiet \
  --release \
  --manifest-path "$repo_root/bench/library_boundary/Cargo.toml" \
  -- "$@"
