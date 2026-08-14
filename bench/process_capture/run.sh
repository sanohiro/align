#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"

"$repo_root/scripts/cargo.sh" run \
  --manifest-path "$repo_root/bench/process_capture/Cargo.toml" \
  --release
