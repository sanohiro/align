#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $# -eq 0 ]] || { echo "usage: bench/rand_sample/run.sh" >&2; exit 2; }
repo_root="$PWD"
target_dir="${CARGO_TARGET_DIR:-target}"
case "$target_dir" in
  /*) ;;
  *) target_dir="$repo_root/$target_dir" ;;
esac
# Cargo's top-level static archive can belong to another worktree sharing this
# target, even when this worktree's fingerprint says Fresh. Force its producer
# to run before linking; only the source mtime changes, not its contents.
touch crates/align_runtime/src/lib.rs
bash scripts/cargo.sh build -p align_runtime --release >&2
scratch="$(mktemp -d "${TMPDIR:-/tmp}/align-sample-bench.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
runtime="$target_dir/release/libalign_runtime.a"
case "$(uname -s)" in
  Darwin)
    "${CC:-cc}" -O2 -Wall -Wextra -Werror -Wl,-dead_strip bench/rand_sample/main.c "$runtime" -o "$scratch/probe"
    ;;
  Linux)
    "${CC:-cc}" -O2 -Wall -Wextra -Werror -Wl,--gc-sections bench/rand_sample/main.c "$runtime" -lpthread -ldl -lm -o "$scratch/probe"
    ;;
  *) echo "sample benchmark supports macOS and Linux" >&2; exit 2 ;;
esac
"$scratch/probe"
