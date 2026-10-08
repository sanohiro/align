# Bulk discovery of CSV closing quotes

Status: **IMPLEMENTED 2026-10-08 — native/source owners and local ARM64 measurements qualified.**

`apps/csv/main.align` supplies a bounded typed CSV consumer with quoted multiline
and unselected text columns. The existing parser examines every quoted byte
separately in both validation and fill passes. Use the existing `memchr` dependency
to find quotes in long runs, retaining a scalar prefix for short fields.

## Scope and closure

| Boundary | Invariant and owner |
| --- | --- |
| Discovery | Search only for the next ASCII double quote inside an already-open quoted field. Preserve exact start/end spans, doubled-quote handling, escaped flag, record separators, callback order and parser position on error. Unquoted discovery remains unchanged. A bounded byte product compares search offsets against the scalar oracle; separate boundary-position cases pin complete quoted spans and error outcomes. |
| Public decode | Existing UTF-8 and descriptor admission, header projection, scalar conversion, row/layout bounds, error precedence, validation-before-allocation and arena-backed outputs remain unchanged. Existing `align_runtime::csv::tests` owns grammar, private ABI, output bytes/views and allocation/conversion counts. Existing `pkg_csv` owns canonical generic/source, whole/per-unit, lifetime and actual bounded application behavior. |
| Memory and ABI | The search borrows a valid suffix for the call and allocates nothing. No new unsafe code, buffer, retained view, ownership, public operation, IR shape or ABI attribute. A123 stays `nounwind` with no other curated attribute; the existing library search handles supported CPU paths. |
| Measurement | `bench/csv_quoted_scan` calls the ordinary release CSV ABI, including arena creation, decode and teardown. Quoted/escaped text at short and long sizes, dense quotes, unquoted and malformed controls retain producer-returned status/row counts and independent output checks. Compare baseline and candidate in paired ABBA processes on macOS/Linux ARM64, retaining all samples and control regressions. No timing threshold, x86, RSS or application-I/O speed promise. |
| Probe lifecycle | The Python runner bounds build/link/probe phases and owns each process group through interruption, timeout, leader exit and reaping before scratch cleanup. `bench/test_native_probe.py` supplies the same stalled-child, exited-leader descendant and HUP/INT/TERM matrix for all three phases. |
| Documentation | Record implementation and measured limits here, the benchmark README and one HANDOFF capability entry. Package design and language mirrors retain their existing contract. |

This is one parser refinement following the existing CSV contract. Perform the
author-side invariant-to-diff pass, existing focused owners on macOS/Linux, one
fresh independent preflight review and the final SHA-bound gate. Do not change
normalization or unquoted parsing to compensate for an unqualified measurement.

## Qualification

The parser keeps its original quote-at-cursor transition; only nonquote runs use
`next_quote`, with a 16-byte scalar prefix and the existing `memchr` fallback.
An absent quote advances to the original EOF position before Invalid. The new
search owner checks a bounded byte product against scalar positions and long
boundary offsets; the record owner pins exact spans, escaped flags, physical
callback order and malformed suffix positions under both line-ending modes.
The eleven native CSV owners and eleven active `pkg_csv` owners pass on macOS
and Linux ARM64, including actual whole/per-unit summary execution and the
existing native descriptor/conversion/arena-allocation counts.

The 31-case benchmark retains all 1736 final observations. For a 65536-byte
ordinary quoted body, complete-call medians change from 44.954 to 5.358 us on
macOS and 52.464 to 23.291 us on Linux ARM64. Dense doubled-quote and unquoted
large controls stay close to baseline. Small one-row measurements vary even for
byte-identical corpus entries; they do not establish a short-input effect.
The 128-row short case increases 2.9% on macOS and decreases 2.1% on Linux.
`bench/csv_quoted_scan/README.md` records the complete method, selected controls
and limits. No broader speed guarantee follows.
