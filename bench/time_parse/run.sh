#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $# -eq 2 ]] || { echo "usage: bench/time_parse/run.sh kind[0..4] group[0..3]" >&2; exit 2; }
case "$1:$2" in
  [0-4]:[0-3]) ;;
  *) echo "kind must be 0..4 and group must be 0..3" >&2; exit 2 ;;
esac
repo_root="$PWD"
target_dir="${CARGO_TARGET_DIR:-target}"
case "$target_dir" in
  /*) ;;
  *) target_dir="$repo_root/$target_dir" ;;
esac
# Rebuild the producer: a shared target's top-level archive can belong to another checkout.
touch crates/align_runtime/src/lib.rs
bash scripts/cargo.sh build -p align_runtime --release >&2
scratch="$(mktemp -d "${TMPDIR:-/tmp}/align-time-parse.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
runtime="$target_dir/release/libalign_runtime.a"
case "$(uname -s)" in
  Darwin)
    "${CC:-cc}" -O2 -Wall -Wextra -Werror -Wl,-dead_strip bench/time_parse/main.c "$runtime" -o "$scratch/probe"
    ;;
  Linux)
    "${CC:-cc}" -O2 -Wall -Wextra -Werror -Wl,--gc-sections bench/time_parse/main.c "$runtime" -lpthread -ldl -lm -o "$scratch/probe"
    ;;
  *) echo "time parser benchmark supports macOS and Linux" >&2; exit 2 ;;
esac
"$scratch/probe" "$@"
