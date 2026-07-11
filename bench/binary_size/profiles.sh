#!/usr/bin/env bash
# M13 Slice 4 — build profiles: per-profile binary-size table.
#
#   bench/binary_size/profiles.sh [prog ...]
#
# For each program under progs/ (or the ones named on the command line) it builds the executable at
# every `--profile` (dev/release/fast/small/tiny) with `alignc build` and reports the file size, the
# stripped state (does the image keep a .symtab?), and the gated DT_NEEDED set.
#
# The pipeline is the STOCK `default<O0|O2|O3|Os|Oz>` set (M13 Slice 4). Note: LLVM does NOT guarantee
# `Oz <= Os <= O2` byte-for-byte on a given program, so this table reports the REAL numbers rather
# than asserting an ordering. The `small`/`tiny` rows are additionally stripped (`-Wl,--strip-all`).
set -euo pipefail
cd "$(dirname "$0")"

( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="$(cd ../.. && pwd)/target/release/alignc"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

PROFILES=(dev release fast small tiny)

stripped() { # binary -> "stripped" if no .symtab, else "symbols"
  if readelf -SW "$1" 2>/dev/null | grep -q '\.symtab'; then echo symbols; else echo stripped; fi
}
gated() { # binary -> the gated libs in its DT_NEEDED ("-" if none)
  local g
  g="$(readelf -d "$1" 2>/dev/null | grep -oE 'lib(z|zstd|crypto|ssl)[^]]*' | sort -u | tr '\n' ' ')"
  [ -n "$g" ] && echo "$g" || echo "-"
}

if [ "$#" -gt 0 ]; then progs=("$@"); else mapfile -t progs < <(for f in progs/*.align; do basename "${f%.align}"; done); fi

printf '%-8s | %-8s | %10s | %-8s | %s\n' program profile size symbols "gated deps"
printf -- '---------+----------+------------+----------+-----------\n'
for name in "${progs[@]}"; do
  src="$(pwd)/progs/$name.align"
  [ -f "$src" ] || { echo "error: no such prog: $name" >&2; exit 1; }
  for p in "${PROFILES[@]}"; do
    ( cd "$tmp" && "$ALIGNC" build "$src" --profile "$p" >/dev/null ) \
      || { echo "error: build failed: $name --profile $p" >&2; exit 1; }
    bin="$tmp/$name"
    sz=$(stat -c%s "$bin" 2>/dev/null || echo 0)
    printf '%-8s | %-8s | %10d | %-8s | %s\n' "$name" "$p" "$sz" "$(stripped "$bin")" "$(gated "$bin")"
    rm -f "$bin"
  done
done
