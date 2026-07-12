#!/usr/bin/env bash
# M13 Slice V (c) — the Clang-IR comparison harness.
#
#   bench/clang_ir_compare/run.sh
#
# For each kernel pair under kernels/ (kNN_name.align + kNN_name.c — semantically equal), this
# compiles BOTH through the SAME LLVM 22: Align via `alignc emit-llvm --stage optimized`, C via
# `clang-22 -O2`, both pinned to the same `--target-cpu` / `-march`. It then diffs the load-bearing
# optimized-IR SHAPE (not bytes): did the pipeline vectorize, at what width, with which horizontal
# reduction intrinsic, and did a runtime overlap guard (`vector.memcheck`) appear.
#
# This is a HARNESS + a recorded baseline, not a pass/fail gate — divergences are FINDINGS (see
# README.md "Recorded baseline"), i.e. future optimization leads, not failures. It exits 0 whether
# shapes match or diverge; it exits 0 (skips) when clang-22 is absent.
#
# Env knobs: ALIGNC (path to a prebuilt alignc; else `cargo build`), CLANG (default clang-22),
# CPU (default x86-64-v3 — an AVX2 tier that gives stable 256-bit widths).
set -euo pipefail
cd "$(dirname "$0")"

CPU="${CPU:-x86-64-v3}"
CLANG="${CLANG:-clang-22}"

if ! command -v "$CLANG" >/dev/null 2>&1; then
  echo "clang-IR compare: '$CLANG' not found — skipping (this harness needs the same-version clang)."
  exit 0
fi

if [ -z "${ALIGNC:-}" ]; then
  ( cd ../.. && cargo build -q --release --bin alignc )
  ALIGNC="$(cd ../.. && pwd)/target/release/alignc"
fi
if [ ! -x "$ALIGNC" ]; then
  echo "clang-IR compare: alignc not found at '$ALIGNC' — run \`cargo build --release\` first." >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Extract the load-bearing IR shape facts from an optimized-IR file, echoed as `VEC|WIDTH|REDUCE|MEM`:
#   VEC    = did the pipeline LOOP vectorize? = a horizontal reduce intrinsic OR a vectorized store
#            whose operand is an SSA value (`store <N x ..> %..`). The SSA test excludes the constant
#            literal-array init that Align's `main` vectorizes (`store <N x ..> <i64 ...>`), which is
#            not a loop fact — so a scalar kernel reads `no` despite that init.
#   WIDTH  = widest integer vector `<N x i{32,64}>` (only meaningful when VEC=yes).
#   REDUCE = the set of `llvm.vector.reduce.*` intrinsics (toolchain-neutral spelling).
#   MEM    = count of the `vector.memcheck` string. CAVEAT: LLVM prints this NAMED block only in
#            named-block IR; clang -O2 emits NUMBERED blocks, so a clang runtime guard can be present
#            without the string. Trust MEM for Align; read it as a lower bound for clang.
facts() {
  local f="$1" reduce width loopstore memcheck vec
  # `|| true` on every grep: a scalar kernel legitimately matches nothing, and while the
  # $(facts ...) command-substitution context happens to suppress errexit today, the function
  # must stay safe under ANY invocation shape.
  reduce=$(grep -oE 'llvm\.vector\.reduce\.[a-z]+' "$f" | sed 's/llvm.vector.reduce.//' | sort -u | paste -sd, - || true)
  loopstore=$(grep -cE 'store <[0-9]+ x i(32|64)> %' "$f" || true)
  memcheck=$(grep -c 'vector.memcheck' "$f" || true)
  width=$(grep -oE '<[0-9]+ x i(32|64)>' "$f" | sed -E 's/<([0-9]+) x.*/\1/' | sort -rn | head -1 || true)
  if [ -n "$reduce" ] || [ "${loopstore:-0}" -gt 0 ]; then vec=yes; else vec=no; width="-"; fi
  echo "${vec}|${width:--}|${reduce:--}|${memcheck:-0}"
}

printf 'kernel               | side  | vec | width | reduce            | memcheck | shape\n'
printf -- '---------------------+-------+-----+-------+-------------------+----------+-------\n'

status=0
for a in kernels/*.align; do
  base="$(basename "${a%.align}")"
  c="kernels/$base.c"
  [ -f "$c" ] || { echo "warning: no C twin for $base" >&2; continue; }

  "$ALIGNC" emit-llvm "$a" --stage optimized --target-cpu "$CPU" > "$tmp/$base.align.ll" 2>"$tmp/$base.align.err" \
    || { echo "error: alignc failed on $a:"; cat "$tmp/$base.align.err"; exit 1; }
  "$CLANG" -O2 -march="$CPU" -emit-llvm -S "$c" -o "$tmp/$base.c.ll" 2>/dev/null \
    || { echo "error: $CLANG failed on $c" >&2; exit 1; }

  IFS='|' read -r av aw ar am <<<"$(facts "$tmp/$base.align.ll")"
  IFS='|' read -r cv cw cr cm <<<"$(facts "$tmp/$base.c.ll")"

  # Load-bearing verdict: vectorization decision, width, and reduction intrinsic must agree. (memcheck
  # is reported but excluded from the verdict — see the clang numbered-block caveat above.)
  if [ "$av" = "$cv" ] && [ "$aw" = "$cw" ] && [ "$ar" = "$cr" ]; then shape=MATCH; else shape=DIVERGE; status=1; fi

  printf '%-20s | %-5s | %-3s | %-5s | %-17s | %-8s | %s\n' "$base" align "$av" "$aw" "$ar" "$am" "$shape"
  printf '%-20s | %-5s | %-3s | %-5s | %-17s | %-8s |\n'    ""     clang "$cv" "$cw" "$cr" "$cm"
done

echo
if [ "$status" -eq 0 ]; then
  echo "All kernels: Align's optimized vector shape MATCHES clang's on the load-bearing facts."
else
  echo "Some kernels DIVERGE on a load-bearing fact — a finding, not a failure (see README.md)."
fi
# Always succeed: this is a findings harness, not a CI gate.
exit 0
