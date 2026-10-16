# Exact RFC1123 name dispatch

RFC1123 parsing currently scans the formatter's weekday and month tables for
each three-byte token. Replace those two linear searches with exact byte-pattern
dispatch. Keep the displayed English spellings, numeric ordinals and all other
timestamp parsing behavior. The public contract remains
[`std-design/time.md`](std-design/time.md); no grammar, API, ABI, ownership,
allocation, error, compiler or cache-format change is selected.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Closed name language | Private weekday/month decoders match exactly the existing three-byte English names and return their existing ordinals. A parameterized native owner compares each name and every single-byte mutation against the independent formatter tables, and rejects shortened/extended tokens. Case, ASCII and weekday/date agreement remain exact. |
| Parser and native admission | Change only name recognition in the RFC1123 branch. Existing `time_formats::tests` owns Gregorian validity, widths, punctuation, complete consumption, representability, malformed native metadata, overlap and canonical zero on failure. No lifetime, allocation or cleanup path changes. |
| Source composition | Existing driver `time_formats` owns all five names, exact values/errors, imports/generic replay, whole/per-unit compilation, control paths and purity. No semantic-analysis, interface or lowering change. |
| Measurement | Reuse the unchanged 147-record `bench/time_parse/main.c` fixture and its exact status/value, call-count and checksum checks. Compare actual non-test release producers in balanced per-cell ABBA order, twice, over all five formats and four input groups. Keep all samples, source/archive/executable identities and observed compilation. Run without concurrent builds/tests. Adopt only a useful measured gain with no material ordinary-case regression; report costs and do not infer Linux or application throughput. |
| Probe ownership | Use the existing native supervisor's bounded build/link/probe phases, signal handling and process-group retirement for qualification and paired runs. Retain producer refresh before linking against a shared target. No new process cleanup strategy. |

This matrix precedes implementation. The change is private token recognition
inside the already reviewed timestamp contract, below 1,000 handwritten lines.
One fresh independent preflight review checks the complete implementation and
measurement evidence. The formatter tables and the C fixture remain unchanged.

The native owners pass all ten active time tests, including the new closed-name
comparison and the independent 213,504-day calendar/rendering owners. All five
source whole/per-unit owners pass. Two complete Apple M1 comparisons reduce
all four RFC1123 group medians by 13.7–63.3%; the largest other-format cost is
+0.433 ns (3.0%) for RFC3339 January, reproduced in both passes. This bounded
control cost is accepted alongside the RFC1123 gain. The unchanged C fixture
checks all 147 fixed status/value goldens. The [measurement record](../../bench/time_parse/README.md#rfc1123-name-dispatch-2026-10-17)
retains all samples, producer identities and limits; no universal or portable
throughput improvement is promised.
