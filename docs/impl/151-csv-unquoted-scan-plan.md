# Bulk discovery of unquoted CSV field boundaries

Status: **IMPLEMENTED 2026-10-08 — native/source owners and local ARM64 measurements qualified.**

The existing bounded CSV summary accepts long unselected text fields. Plan150's
local probe records roughly 147–151 us per complete decode for a 65536-byte
unquoted field; quoted discovery is already faster. Assess the unquoted byte
walk without changing CSV grammar, conversion, allocation or ownership.

## Scope and closure

| Boundary | Invariant and owner |
| --- | --- |
| Discovery | Find the first comma, CR, LF or double quote. Keep a short scalar prefix; use the existing portable byte searches in bounded chunks for longer runs. Search chunks in order and return the earliest special byte, including when a quote precedes a separator. Scalar-oracle products and prefix/chunk/EOF boundary offsets own the search result. |
| Parser | Retain the original delimiter/EOL/invalid-quote decisions at the returned cursor. Preserve exact spans, physical callback order, cursor on refusal, field-limit precedence, trailing empty fields and unterminated final records. Quoted parsing remains unchanged. Exact record and refusal owners cross both line endings, long fields and malformed suffixes. |
| Public decode and memory | Existing UTF-8/descriptor admission, conversion and row/layout bounds, complete validation before allocation, zero-copy text and arena output remain unchanged. Borrow only valid source suffixes; allocate and retain nothing. No unsafe, public API, IR or ABI change. Existing native CSV allocation/output owners and whole/per-unit `pkg_csv` application owners qualify this boundary. |
| Measurement | Reuse the unchanged 31-case `bench/csv_quoted_scan/main.c` against baseline and candidate ordinary release archives. Retain producer-returned statuses/counts, all paired ABBA observations and short/quoted/malformed controls on macOS/Linux ARM64. Adopt only with a demonstrated long-unquoted benefit; no timing threshold, x86, RSS or application-I/O claim. |
| Documentation | Qualify the result here, in the existing probe README and one HANDOFF capability entry. Preserve the existing public design and language mirrors. |

The bounded search chunks avoid scanning an entire remaining field merely to
discover that an earlier invalid quote or line break already ends it. One
chunk is searched at most twice; no copied buffer or new parser state is added.
The original scalar control decisions still own malformed line-ending tags.

This follows the existing CSV contract and search dependency. Perform one
author-side invariant-to-diff pass, focused native/source owners on macOS/Linux,
one fresh independent preflight review and the final SHA-bound gate.

## Qualification

`next_unquoted_boundary` inlines the 16-byte scalar prefix;
`unquoted_boundary_tail` keeps the 256-byte search loop shared. The record loop
retains its original scalar decisions at the discovered byte. An exhaustive
short byte product and ordered pairs across prefix/chunk/EOF offsets agree with
the independent scalar oracle. Record owners pin exact callbacks, spans, EOF,
malformed suffix positions, field-limit precedence and malformed mode refusal.
Thirteen native CSV and eleven active `pkg_csv` owners pass on macOS/Linux ARM64,
including allocation/conversion counts and whole/per-unit application execution.

The unchanged 31-case probe retains every final paired ABBA observation. For a
65536-byte unquoted body, complete-call medians change from 147.490 to 10.928 us
on macOS and 148.288 to 55.159 us on Linux ARM64. Long quoted and malformed
controls remain close. Short one-row values vary even for byte-identical inputs;
the 128-row short control increases 1.0% on macOS and 0.4% on Linux.
`bench/csv_quoted_scan/README.md` records all selected controls, regressions and
measurement limits. No broader speed guarantee follows.
