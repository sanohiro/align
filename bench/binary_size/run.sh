#!/usr/bin/env bash
# M13 Slice 2 — capability-based linking + link hygiene: binary-size & dynamic-dependency benchmark.
#
#   bench/binary_size/run.sh
#
# For each program under progs/ it links TWO ways against the same object + release runtime:
#   BEFORE — the pre-Slice-2 driver: every gated library linked unconditionally, no --gc-sections.
#   AFTER  — the current driver (`alignc build`): only the used capabilities' libraries + link hygiene
#            (--gc-sections / --as-needed).
# and reports the file size and the gated dynamic-dependency set (DT_NEEDED on ELF, LC_LOAD_DYLIB on
# Mach-O; z / zstd / crypto / ssl) for each.
#
# The size win is dominated by --gc-sections (dead-code removal); the dependency-hygiene win (fewer
# gated DT_NEEDED) is dominated by capability collection. `https` is built, not run (no network).
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=lib.sh
. ./lib.sh

# Release for realistic codegen + runtime size.
( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="$(cd ../.. && pwd)/target/release/alignc"
RT="$(cd ../.. && pwd)/target/release/libalign_runtime.a"
[ -f "$RT" ] || { echo "error: $RT missing — run cargo build --release first" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Legacy (pre-Slice-2) flag set the old driver used.
BEFORE_LIBS=(-lpthread -ldl -lm -lz -lzstd -lcrypto -lssl)

# llvm-readobj for the `gated` DT_NEEDED/LC_LOAD_DYLIB inspection (lib.sh) — degrades to "?" if
# absent rather than aborting; the size/before-after comparison is still meaningful without it.
READOBJ="$(llvm_tool llvm-readobj || true)"

printf '%-9s | %-8s %-6s %-28s | %-8s %-6s %-28s\n' program before size deps after size deps
printf -- '----------+-------------------------------------------+------------------------------------------\n'
progdir="$(pwd)/progs"
for f in progs/*.align; do
  name="$(basename "${f%.align}")"
  "$ALIGNC" emit-obj "$f" "$tmp/$name.o" >/dev/null || { echo "error: emit-obj failed for $f" >&2; exit 1; }
  # BEFORE: legacy unconditional libs, no link hygiene.
  cc "$tmp/$name.o" "$RT" -o "$tmp/${name}_before" "${BEFORE_LIBS[@]}" 2>/dev/null
  # AFTER: the real driver (`alignc build` writes the exe named <stem> into the cwd).
  ( cd "$tmp" && "$ALIGNC" build "$progdir/$name.align" >/dev/null ) \
    || { echo "error: alignc build failed for $f" >&2; exit 1; }
  mv "$tmp/$name" "$tmp/${name}_after" 2>/dev/null || true
  bsz=$(filesize "$tmp/${name}_before" 2>/dev/null || echo 0)
  asz=$(filesize "$tmp/${name}_after"  2>/dev/null || echo 0)
  printf '%-9s | %8d %-6s %-28s | %8d %-6s %-28s\n' \
    "$name" "$bsz" "" "$(gated "$READOBJ" "$tmp/${name}_before")" "$asz" "" "$(gated "$READOBJ" "$tmp/${name}_after")"
done
