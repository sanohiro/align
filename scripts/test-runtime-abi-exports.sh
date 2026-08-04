#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
golden="$repo_root/crates/align_codegen_llvm/tests/golden/runtime_abi_declarations.txt"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
archive="$target_dir/debug/libalign_runtime.a"
cd "$repo_root"

if [[ -n "${LLVM_CONFIG:-}" && -x "${LLVM_CONFIG}" ]]; then
  llvm_nm="$(dirname "$LLVM_CONFIG")/llvm-nm"
elif command -v llvm-nm-22 >/dev/null 2>&1; then
  llvm_nm="$(command -v llvm-nm-22)"
elif command -v llvm-nm >/dev/null 2>&1; then
  llvm_nm="$(command -v llvm-nm)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-nm ]]; then
  llvm_nm=/opt/homebrew/opt/llvm/bin/llvm-nm
else
  echo "test-runtime-abi-exports: llvm-nm not found" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT
sed -nE 's/.*@(align_rt_[A-Za-z0-9_]+)\(.*/\1/p' "$golden" | sort -u > "$work_dir/base"
if [[ "$(wc -l < "$work_dir/base" | tr -d ' ')" != 286 ]]; then
  echo "test-runtime-abi-exports: declaration golden does not contain 286 base symbols" >&2
  exit 1
fi

audit_case() {
  local label=$1
  local features=$2
  local expected_count=$3
  local expected="$work_dir/expected-$label"
  local actual="$work_dir/actual-$label"

  cp "$work_dir/base" "$expected.raw"
  case "$label" in
    alloc)
      printf '%s\n' \
        align_rt_alloc_count \
        align_rt_free_count \
        align_rt_str_finder_new_count \
        align_rt_str_finder_free_count >> "$expected.raw"
      ;;
    par)
      printf '%s\n' \
        align_rt_test_par_map_force_caller \
        align_rt_test_par_map_min_chunk \
        align_rt_test_par_map_min_chunk_for \
        align_rt_test_par_map_workers >> "$expected.raw"
      ;;
    all)
      printf '%s\n' \
        align_rt_alloc_count \
        align_rt_free_count \
        align_rt_str_finder_new_count \
        align_rt_str_finder_free_count \
        align_rt_test_par_map_force_caller \
        align_rt_test_par_map_min_chunk \
        align_rt_test_par_map_min_chunk_for \
        align_rt_test_par_map_workers >> "$expected.raw"
      ;;
  esac
  sort -u "$expected.raw" > "$expected"

  if [[ -n "$features" ]]; then
    cargo build --quiet --locked --manifest-path "$repo_root/Cargo.toml" \
      -p align_runtime --features "$features"
  else
    cargo build --quiet --locked --manifest-path "$repo_root/Cargo.toml" -p align_runtime
  fi
  "$llvm_nm" --defined-only --extern-only "$archive" 2>/dev/null \
    | awk '{print $NF}' \
    | sed 's/^_//' \
    | grep '^align_rt_' \
    | sort -u > "$actual"
  diff -u "$expected" "$actual"
  if [[ "$(wc -l < "$actual" | tr -d ' ')" != "$expected_count" ]]; then
    echo "test-runtime-abi-exports: $label count differs from $expected_count" >&2
    exit 1
  fi
  printf 'runtime-abi-exports %s %s\n' "$label" "$expected_count"
}

audit_case base "" 286
audit_case alloc alloc-count 290
audit_case par par-map-probe 290
audit_case task task-group-probe 286
audit_case all alloc-count,par-map-probe,task-group-probe 294
