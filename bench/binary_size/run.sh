#!/usr/bin/env bash
# M13 Slice 2 — capability-based linking + link hygiene: binary-size & dynamic-dependency benchmark.
#
#   bench/binary_size/run.sh
#
# For each program under progs/ it links TWO ways against the same object + release runtime:
#   BEFORE — the pre-Slice-2 driver: every gated library linked unconditionally, no --gc-sections.
#   AFTER  — the current driver (`alignc build`): only the used capabilities' libraries + link hygiene
#            (--gc-sections / --as-needed).
# and reports the file size and the gated DT_NEEDED set (z / zstd / crypto / ssl) for each.
#
# The size win is dominated by --gc-sections (dead-code removal); the dependency-hygiene win (fewer
# gated DT_NEEDED) is dominated by capability collection. `https` is built, not run (no network).
set -euo pipefail
cd "$(dirname "$0")"

# Release for realistic codegen + runtime size.
( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="$(cd ../.. && pwd)/target/release/alignc"
RT="$(cd ../.. && pwd)/target/release/libalign_runtime.a"
[ -f "$RT" ] || { echo "error: $RT missing — run cargo build --release first" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Legacy (pre-Slice-2) flag set the old driver used.
BEFORE_LIBS=(-lpthread -ldl -lm -lz -lzstd -lcrypto -lssl)

gated() { # binary -> the gated libs in its DT_NEEDED, space-separated ("-" if none)
  local g
  g="$(readelf -d "$1" 2>/dev/null | grep -oE 'lib(z|zstd|crypto|ssl)[^]]*' | sort -u | tr '\n' ' ')"
  [ -n "$g" ] && echo "$g" || echo "-"
}

printf '%-9s | %-8s %-6s %-28s | %-8s %-6s %-28s\n' program before size deps after size deps
printf -- '----------+-------------------------------------------+------------------------------------------\n'
progdir="$(pwd)/progs"
for f in progs/*.align; do
  name="$(basename "${f%.align}")"
  "$ALIGNC" emit-obj "$f" "$tmp/$name.o" >/dev/null 2>&1
  # BEFORE: legacy unconditional libs, no link hygiene.
  cc "$tmp/$name.o" "$RT" -o "$tmp/${name}_before" "${BEFORE_LIBS[@]}" 2>/dev/null
  # AFTER: the real driver (`alignc build` writes the exe named <stem> into the cwd).
  ( cd "$tmp" && "$ALIGNC" build "$progdir/$name.align" >/dev/null 2>&1 )
  mv "$tmp/$name" "$tmp/${name}_after" 2>/dev/null || true
  bsz=$(stat -c%s "$tmp/${name}_before" 2>/dev/null || echo 0)
  asz=$(stat -c%s "$tmp/${name}_after"  2>/dev/null || echo 0)
  printf '%-9s | %8d %-6s %-28s | %8d %-6s %-28s\n' \
    "$name" "$bsz" "" "$(gated "$tmp/${name}_before")" "$asz" "" "$(gated "$tmp/${name}_after")"
done
