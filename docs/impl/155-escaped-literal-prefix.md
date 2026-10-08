# Bulk ordinary prefixes in percent/form decoding

Status: **IMPLEMENTED 2026-10-08 — native/source owners and local ARM64 measurements qualified.**

Percent and form decoding currently append every ordinary input byte separately.
Reuse the existing CPU-portable byte search and safe slice append for the
initial ordinary prefix. Inputs below 64 bytes or with an active delimiter in
the first 16 bytes keep the original byte loop. Otherwise search the remaining
prefix in bulk and resume that loop at its first escape/plus transition. Both decoders share private
prefix discovery, specialized for their established plus rule. This follows
plan146's allocation and publication strategy.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Exact admission and bytes | Percent preserves `+`; form maps it to space. Both decode exactly two hex digits after `%`, preserve every other byte including NUL/non-UTF-8, and reject incomplete/invalid escapes. Existing independent scalar oracle covers every suffix byte pair and malformed positions; extend ordinary-run boundary coverage with escapes and plus before/at/after the short prefix and bulk-search lengths. |
| Initialized storage and allocation | One input-sized Vec reservation, with safe push/extend only. Every append emits no more bytes than it consumes, so no growth; partial output drops on failure. Existing isolated actual-allocation owner covers empty/plain/dense/invalid inputs. No new unsafe code or ownership strategy. |
| Native publication and consumers | Existing Invalid/null, independent owned bytes, decoded read capacity and buffer Drop remain. Native publication owner plus m10_encoding and apps_web_query retain Result/as_str and real raw-query consumer coverage. No source/IR/interface/ABI/cache changes; unrelated type/control axes are unchanged. |
| Performance | Reuse the exported-entry probe, independent byte oracle and observed per-call status/length/work counts. Compare ordinary release archives in ABBA order on macOS/Linux ARM64 without overlapping builds/tests. Retain every sample for plain/sparse/dense/plus/mixed/late-escape, empty/short/long controls; adopt only with useful ordinary-prefix improvement and no material dense/short regression. No timing correctness gate or network/application/RSS claim. |

The author matrix-to-diff pass and one fresh inspection-only full-diff review
close this internal refinement. Public encoding specifications remain unchanged.


## Qualification

All five native owners, including exact allocation requests and the complete
65,536-pair escape oracle after a long prefix, pass on macOS/Linux ARM64.
The 18 encoding source tests and three actual web-query consumers pass on both.
The ordinary-prefix matrix crosses short/search boundaries, binary bytes,
active delimiters and malformed tails with the independent scalar oracle.

The [retained comparison](../../bench/escaped_decode/README.md) contains every
final ABBA observation. Long plain/late-escape inputs improve on both hosts;
short-case increases remain at most 0.55 ns on macOS and 2.20 ns on Linux.
Linux sparse controls rise up to 9.8%; this measured tradeoff is explicit.
Post-delimiter decoding still uses the existing loop, so no general decoding,
network/application or hardware-portable speedup is promised.
