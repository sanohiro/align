#!/usr/bin/env bash
# Build and cache the alignc binary for a baseline commit (default: origin/main),
# so "is this failure my regression or pre-existing?" is answered by running the
# cached binary instead of stashing work and rebuilding the current tree twice.
#
#   scripts/baseline-alignc.sh            # ensure cache for origin/main, print path
#   scripts/baseline-alignc.sh <ref>      # cache another baseline ref
#
# The build runs in a detached git worktree with its own target directory, so the
# working tree, its build cache, and any uncommitted work are never touched.
set -euo pipefail

ref="${1:-origin/main}"
root="$(git rev-parse --show-toplevel)"
sha="$(git -C "$root" rev-parse --verify "${ref}^{commit}")"
cache_dir="$root/target/baseline/$sha"
binary="$cache_dir/alignc"

if [[ -x "$binary" ]]; then
  echo "$binary"
  exit 0
fi

worktree="$cache_dir/src"
mkdir -p "$cache_dir"
trap 'git -C "$root" worktree remove --force "$worktree" >/dev/null 2>&1 || true' EXIT
git -C "$root" worktree add --detach "$worktree" "$sha" >/dev/null

(cd "$worktree" && CARGO_TARGET_DIR="$cache_dir/target" scripts/cargo.sh build -p align_driver >&2)
cp "$cache_dir/target/debug/alignc" "$binary"
rm -rf "$cache_dir/target"

# Keep only the two newest baselines to bound disk use.
ls -1t "$root/target/baseline" 2>/dev/null | tail -n +3 | while IFS= read -r stale; do
  rm -rf "$root/target/baseline/$stale"
done

echo "$binary"
