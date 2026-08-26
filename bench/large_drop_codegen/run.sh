#!/usr/bin/env bash
# Local Request-19 compile-cost comparison. This is a measurement, not a CI gate.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
baseline="${BASELINE_ALIGNC:-}"
candidate="${CANDIDATE_ALIGNC:-}"
request_source="${REQUEST19_SOURCE:-$here/../../../align-llm/src/prompt_verifier_smoke.align}"

if [ -z "$baseline" ] || [ -z "$candidate" ]; then
  echo "set BASELINE_ALIGNC and CANDIDATE_ALIGNC to optimized compiler binaries" >&2
  exit 2
fi
for compiler in "$baseline" "$candidate"; do
  if [ ! -x "$compiler" ]; then
    echo "compiler is not executable: $compiler" >&2
    exit 2
  fi
  if [ ! -f "$(dirname "$compiler")/libalign_runtime.a" ]; then
    echo "instrumented libalign_runtime.a is missing beside $compiler" >&2
    exit 2
  fi
done
if [ ! -f "$request_source" ]; then
  echo "Request 19 source is missing; set REQUEST19_SOURCE explicitly" >&2
  exit 2
fi

work="$(mktemp -d)"
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT

measure_case() {
  revision="$1"
  compiler="$2"
  case_name="$3"
  source="$4"
  dir="$work/$revision-$case_name"
  mkdir -p "$dir"
  # Preserve sibling modules imported by the entry fixture. The Request 19 owner imports
  # prompt_artifacts.align and prompt_score.align from its source directory.
  cp "$(dirname "$source")"/*.align "$dir/"
  cp "$source" "$dir/main.align"

  (
    cd "$dir"
    ALIGNC_CACHE="$dir/cache" "$compiler" build main.align --profile release --no-rt-lto \
      --cache-stats >build.stdout 2>build.stderr
  )
  frontend_misses="$(awk '/^alignc: cache: .* frontend miss \(/ { count++ } END { print count + 0 }' "$dir/build.stderr")"
  frontend_hits="$(awk '/^alignc: cache: .* frontend hit$/ { count++ } END { print count + 0 }' "$dir/build.stderr")"
  codegen_misses="$(awk '/^alignc: cache: .* miss \(/ && $0 !~ / frontend miss \(/ { count++ } END { print count + 0 }' "$dir/build.stderr")"
  codegen_hits="$(awk '/^alignc: cache: .* hit$/ && $0 !~ / frontend hit$/ { count++ } END { print count + 0 }' "$dir/build.stderr")"
  if [ "$frontend_hits" -ne 0 ] || [ "$codegen_hits" -ne 0 ]; then
    echo "$revision/$case_name unexpectedly hit a fresh cache" >&2
    sed -n '/^alignc: cache:/p' "$dir/build.stderr" >&2
    exit 1
  fi

  (
    cd "$dir"
    "$compiler" emit-llvm main.align --stage raw --no-rt-lto >raw.ll
    "$compiler" emit-obj main.align main.o --profile release --no-rt-lto
  )
  timing="$(ALIGNC_CACHE=off python3 "$here/measure.py" "$dir" -- \
    "$compiler" build main.align --profile release --no-rt-lto)"
  "$dir/main" >"$dir/program.stdout"

  raw_lines="$(wc -l <"$dir/raw.ll" | tr -d ' ')"
  object_bytes="$(wc -c <"$dir/main.o" | tr -d ' ')"
  echo "$revision/$case_name: frontend_misses=$frontend_misses codegen_misses=$codegen_misses raw_ir_lines=$raw_lines object_bytes=$object_bytes timing=$timing"
}

measure_case baseline "$baseline" control "$here/control.align"
measure_case candidate "$candidate" control "$here/control.align"
cmp "$work/baseline-control/program.stdout" "$work/candidate-control/program.stdout"
if [ "$(tr '\n' ' ' <"$work/candidate-control/program.stdout")" != "7 1 1 " ]; then
  echo "small control must report value 7, one allocation, and one destructor free" >&2
  exit 1
fi

measure_case baseline "$baseline" request19 "$request_source"
measure_case candidate "$candidate" request19 "$request_source"
cmp "$work/baseline-request19/program.stdout" "$work/candidate-request19/program.stdout"
grep -Fx "prompt verifier smoke: complete, incomplete, compact, and tamper cases PASS" \
  "$work/candidate-request19/program.stdout" >/dev/null
echo "program output: identical; control destructor count: 1/1; Request 19: PASS"
