#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
golden="$repo_root/crates/align_codegen_llvm/tests/golden/runtime_abi_declarations.txt"
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
audit_target="$work_dir/audit-target"
archive="$audit_target/debug/libalign_runtime.a"
sed -nE 's/.*@(align_rt_[A-Za-z0-9_]+)\(.*/\1/p' "$golden" | sort -u > "$work_dir/base"
if [[ "$(wc -l < "$work_dir/base" | tr -d ' ')" != 337 ]]; then
  echo "test-runtime-abi-exports: declaration golden does not contain 337 base symbols" >&2
  exit 1
fi

# Independently compile the Rust runtime surface to LLVM IR and compare every base function type
# with codegen's declaration golden. Rust represents both two-word C aggregates as `[2 x i64]` on
# this 64-bit target; codegen distinguishes `{ i64, i64 }` and `{ ptr, i64 }` internally, but both
# cross the native ABI as the same ordered pair of 64-bit machine words. Parameter types are all
# scalar/pointer primitives, so the normalized record remains exact by return and every ordinal.
ir_target="$work_dir/ir-target"
cargo rustc --quiet --locked --manifest-path "$repo_root/Cargo.toml" \
  -p align_runtime --lib --crate-type=lib --target-dir "$ir_target" -- \
  --emit=llvm-ir -Ccodegen-units=1
runtime_ir="$(find "$ir_target/debug/deps" -maxdepth 1 -name 'align_runtime-*.ll' -print -quit)"
if [[ -z "$runtime_ir" ]]; then
  echo "test-runtime-abi-exports: rustc did not emit align_runtime LLVM IR" >&2
  exit 1
fi

perl -ne '
  sub native_type {
    my ($text) = @_;
    return "pair64" if $text =~ /\[2 x i64\]|\{\s*(?:ptr|i64)\s*,\s*i64\s*\}/;
    my @types = ($text =~ /\b(void|ptr|i64|i32|i8|float|double)\b/g);
    die "no native type in golden: $text\n" unless @types;
    return $types[-1];
  }
  chomp;
  next unless /\bdeclare\s+(.+?)\s+\@(align_rt_[A-Za-z0-9_]+)\((.*)\)(?:\s+#\d+)?$/;
  my ($ret, $symbol, $params) = ($1, $2, $3);
  my @params = length($params) ? map { native_type($_) } split /,/, $params : ();
  print join("|", $symbol, native_type($ret), @params), "\n";
' "$golden" | sort > "$work_dir/golden-abi"

perl -ne '
  sub native_type {
    my ($text) = @_;
    return "pair64" if $text =~ /\[2 x i64\]|\{\s*(?:ptr|i64)\s*,\s*i64\s*\}/;
    my @types = ($text =~ /\b(void|ptr|i64|i32|i8|float|double)\b/g);
    die "no native type in runtime IR: $text\n" unless @types;
    return $types[-1];
  }
  chomp;
  next unless /^define\s+(.+?)\s+\@(align_rt_[A-Za-z0-9_]+)\((.*?)\)\s/;
  my ($ret, $symbol, $params) = ($1, $2, $3);
  my @params = length($params) ? map { native_type($_) } split /,/, $params : ();
  print join("|", $symbol, native_type($ret), @params), "\n";
' "$runtime_ir" | sort > "$work_dir/runtime-abi"

if [[ "$(wc -l < "$work_dir/golden-abi" | tr -d ' ')" != 337 ]] \
  || [[ "$(wc -l < "$work_dir/runtime-abi" | tr -d ' ')" != 337 ]]; then
  echo "test-runtime-abi-exports: normalized base ABI does not contain 337 rows" >&2
  exit 1
fi
diff -u "$work_dir/golden-abi" "$work_dir/runtime-abi"
printf 'runtime-abi-signatures base 337\n'

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
      -p align_runtime --target-dir "$audit_target" --features "$features"
  else
    cargo build --quiet --locked --manifest-path "$repo_root/Cargo.toml" \
      -p align_runtime --target-dir "$audit_target"
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

audit_case base "" 337
audit_case alloc alloc-count 341
audit_case par par-map-probe 341
audit_case task task-group-probe 337
audit_case all alloc-count,par-map-probe,task-group-probe 345
