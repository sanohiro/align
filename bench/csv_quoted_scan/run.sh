#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $# -eq 0 ]] || { echo "usage: bench/csv_quoted_scan/run.sh" >&2; exit 2; }
repo_root="$PWD"
target_dir="${CARGO_TARGET_DIR:-target}"
case "$target_dir" in /*) ;; *) target_dir="$repo_root/$target_dir" ;; esac
bash scripts/cargo.sh build -p align_runtime --release >&2
scratch="$(mktemp -d "${TMPDIR:-/tmp}/align-csv-quoted.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
runtime="$target_dir/release/libalign_runtime.a"
case "$(uname -s)" in
  Darwin) "${CC:-cc}" -O3 -Wl,-dead_strip bench/csv_quoted_scan/main.c "$runtime" -o "$scratch/probe" ;;
  Linux) "${CC:-cc}" -O3 -Wl,--gc-sections bench/csv_quoted_scan/main.c "$runtime" -lpthread -ldl -lm -o "$scratch/probe" ;;
  *) echo "CSV benchmark supports macOS and Linux" >&2; exit 2 ;;
esac
"$scratch/probe"
