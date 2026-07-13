#!/usr/bin/env bash
# M13 Slice 4 — build profiles: per-profile binary-size table.
#
#   bench/binary_size/profiles.sh [prog ...]
#
# For each program under progs/ (or the ones named on the command line) it builds the executable at
# every `--profile` (dev/release/fast/small/tiny) with `alignc build` and reports the file size,
# the stripped state (does the image keep a symbol table?), and the gated dynamic-dependency set.
#
# The pipeline is the STOCK `default<O0|O2|O3|Os|Oz>` set (M13 Slice 4). Note: LLVM does NOT guarantee
# `Oz <= Os <= O2` byte-for-byte on a given program, so this table reports the REAL numbers rather
# than asserting an ordering. The `small`/`tiny` rows are additionally stripped (`-Wl,--strip-all`).
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=lib.sh
. ./lib.sh

( cd ../.. && cargo build -q --release --bin alignc )
ALIGNC="$(cd ../.. && pwd)/target/release/alignc"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

PROFILES=(dev release fast small tiny)

# llvm-nm / llvm-readobj for the `stripped` / `gated` inspection (lib.sh) — each degrades to "?" if
# its tool is absent rather than aborting.
NM="$(llvm_tool llvm-nm || true)"
READOBJ="$(llvm_tool llvm-readobj || true)"

# Portable equivalent of `mapfile -t progs < <(...)` — `mapfile`/`readarray` need bash >= 4 (absent
# on e.g. macOS's system bash 3.2), so build the array with a plain loop instead.
if [ "$#" -gt 0 ]; then
  progs=("$@")
else
  progs=()
  for f in progs/*.align; do
    progs+=("$(basename "${f%.align}")")
  done
fi

printf '%-8s | %-8s | %10s | %-8s | %s\n' program profile size symbols "gated deps"
printf -- '---------+----------+------------+----------+-----------\n'
for name in "${progs[@]}"; do
  src="$(pwd)/progs/$name.align"
  [ -f "$src" ] || { echo "error: no such prog: $name" >&2; exit 1; }
  for p in "${PROFILES[@]}"; do
    ( cd "$tmp" && "$ALIGNC" build "$src" --profile "$p" >/dev/null ) \
      || { echo "error: build failed: $name --profile $p" >&2; exit 1; }
    bin="$tmp/$name"
    sz=$(filesize "$bin")
    printf '%-8s | %-8s | %10d | %-8s | %s\n' "$name" "$p" "$sz" "$(stripped "$NM" "$bin")" "$(gated "$READOBJ" "$bin")"
    rm -f "$bin"
  done
done
