#!/usr/bin/env bash
# Bounded, deterministic gate for ordinary changes. Expensive regression suites
# are selected explicitly according to docs/impl/16-test-policy.md.
#
# The gate compiles every one of its test binaries in ONE cargo build graph and
# then runs those binaries CONCURRENTLY. Both halves matter, because the gate's
# cost is not spread evenly: a handful of single-threaded stress tests
# (align_codegen_llvm's malformed-embedded-id codegen guard, align_mir's deep
# type-DAG placement and fact-replay validators) own nearly all of the run time,
# so the serial sum was about twice the longest single binary.
#
# Separate `cargo test` invocations cannot overlap: cargo holds the
# target-directory lock across the test RUN as well as the build, so a second
# invocation blocks until the first has finished running its tests (measured:
# two ~60s crates launched together took 121s, each reporting "Blocking waiting
# for file lock on build directory"). Executing the compiled binaries directly
# — from the package directory, with the build scripts' link paths on the
# dynamic loader path, which is exactly what cargo does after a build — is what
# makes the overlap possible.
set -euo pipefail

scripts/cargo.sh build --workspace --locked

artifacts="$(mktemp)"
logs="$(mktemp -d)"
trap 'rm -rf "$artifacts" "$logs"' EXIT

# Keep this list explicit: a newly added workspace library must not silently
# become an every-PR gate just because it contains tests. The small
# deterministic integration targets that protect cross-crate interface
# soundness and formatter behavior are part of the gate as well; the `--test`
# names here are what scripts/pr-tier.sh pins as bounded-gate content and what
# scripts/test-pr-workflow.sh recomputes that pin from.
scripts/cargo.sh test --no-run --locked --message-format=json-render-diagnostics \
  -p align_ast \
  -p align_codegen_llvm \
  -p align_diag \
  -p align_driver \
  -p align_fmt \
  -p align_hash \
  -p align_interface \
  -p align_lexer \
  -p align_mir \
  -p align_parser \
  -p align_sema \
  -p align_span \
  --lib \
  --test effect_fail_closed \
  --test examples \
  --test m0 \
  --test summary \
  >"$artifacts"

# Cargo emits one JSON object per built unit, fresh or not. A test binary is an
# artifact whose *profile* carries "test":true — the target-level flag of the
# same name is true for an ordinary library too, and the alignc binary the
# driver tests depend on is an executable with "test":false — and whose
# `executable` is not null. `manifest_path` gives the package directory, the
# working directory cargo runs a test binary in.
binaries="$(
  sed -n 's/.*"manifest_path":"\([^"]*\)".*"profile":{[^}]*"test":true}.*"executable":"\([^"]*\)".*/\1 \2/p' \
    "$artifacts"
)"
[ -n "$binaries" ] || {
  echo "the gate build produced no test binaries (cargo artifact format changed?)" >&2
  exit 1
}

# Reproduce cargo's runtime dynamic-library search path: every build script's
# `linked_paths` (this is how a dynamically linked libLLVM is found) plus the
# two profile output directories. Replayed for fresh units too, so it does not
# depend on anything actually recompiling.
deps_dir="$(dirname "$(printf '%s\n' "$binaries" | sed -n '1s/.* //p')")"
dylib_path="$(
  {
    sed -n 's/.*"linked_paths":\[\([^]]*\)\].*/\1/p' "$artifacts" |
      tr ',' '\n' |
      sed -n 's/^"\([a-z-]*=\)\{0,1\}\(\/[^"]*\)"$/\2/p'
    printf '%s\n' "$deps_dir" "$(dirname "$deps_dir")"
  } | awk '!seen[$0]++' | tr '\n' ':' | sed 's/:$//'
)"
if [ "$(uname -s)" = Darwin ]; then
  export DYLD_FALLBACK_LIBRARY_PATH="$dylib_path:${DYLD_FALLBACK_LIBRARY_PATH:-$HOME/lib:/usr/local/lib:/usr/lib}"
else
  export LD_LIBRARY_PATH="$dylib_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# One process per binary, capped so the gate does not oversubscribe a small CI
# runner. The heavy tests are single-threaded, so the cap only bounds the tail
# of short binaries.
gate_jobs="${ALIGN_GATE_JOBS:-$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)}"
case "$gate_jobs" in
  '' | *[!0-9]*) gate_jobs=4 ;;
esac
[ "$gate_jobs" -ge 1 ] || gate_jobs=1

while read -r manifest executable; do
  [ -n "$executable" ] || continue
  # Strip cargo's disambiguating -<hash> suffix so the log name is stable across
  # builds, which keeps the aggregated output below in a deterministic
  # (glob-sorted) order. Two packages may still name a target the same thing;
  # claim the slot here so a collision cannot silently drop a binary's result.
  name="$(basename "$executable")"
  name="${name%-*}"
  slot="$name"
  suffix=2
  while [ -e "$logs/$slot.log" ]; do
    slot="$name-$suffix"
    suffix=$((suffix + 1))
  done
  : >"$logs/$slot.log"
  while [ "$(jobs -pr | wc -l | tr -d '[:space:]')" -ge "$gate_jobs" ]; do
    sleep 0.2
  done
  (
    cd "$(dirname "$manifest")"
    binary_status=0
    started="$(date +%s)"
    "$executable" >"$logs/$slot.log" 2>&1 || binary_status=$?
    printf '%s %s\n' "$binary_status" "$(($(date +%s) - started))" >"$logs/$slot.status"
  ) &
done <<<"$binaries"
wait

status=0
for log in "$logs"/*.log; do
  name="$(basename "$log" .log)"
  record="$(cat "$logs/$name.status" 2>/dev/null || true)"
  binary_status="${record%% *}"
  elapsed="${record#* }"
  [ -n "$binary_status" ] || { binary_status="killed"; elapsed="?"; }
  printf -- '--- %s (exit %s, %ss) ---\n' "$name" "$binary_status" "$elapsed"
  cat "$log"
  [ "$binary_status" = 0 ] || status=1
done
exit "$status"
