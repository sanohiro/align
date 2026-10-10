# Bulk ordinary runs after percent/form escapes

Plan155 copies only an initial ordinary prefix. A leading escape, or any later
escape, leaves the remaining ordinary bytes on the scalar push path. Count
ordinary bytes already copied by the scalar loop. After sixteen consecutive
ordinary bytes, when at least 64 input bytes remain, search and copy the rest
of that run in bulk. Delimiters reset the count. Preserve the same safe
initialized-output and allocation strategy. Inputs below 80 bytes select a
const-specialized copy of the same decoder without the later-run counter;
their initial-prefix optimization remains unchanged.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Admission and bytes | Percent preserves plus; form maps plus to space. Every percent consumes exactly two case-insensitive hex digits or rejects the complete input. Ordinary binary bytes, including NUL and non-UTF-8, remain unchanged. The independent escaped_decode_tests oracle covers every suffix pair and repeated runs at short/search boundaries, including malformed tails. |
| Allocation and cleanup | The sole input-sized Vec reservation remains exact; each append emits no more than the consumed extent. Safe push/extend initialize all published bytes; failure drops partial output before the unchanged Invalid/null publication. The isolated allocation owner covers repeated long runs and malformed late escapes as well as existing empty/plain/dense cases. No new unsafe code, ownership or ABI. |
| Consumers | Existing native publication/capacity/independence owners, m10_encoding and apps_web_query cover both exported decoders and the raw-query consumer. Source, IR, generic interfaces, caches and all ownership/control-flow representations remain unchanged. |
| Measurement | Extend the existing escaped_decode probe with a leading escape followed by a long literal tail and repeated long literal runs. Keep the independent oracle, status/extent/capacity checks and exact observed work counts. Use identical C source/flags and ordinary release archives in ABBA order, without overlapping builds/tests, on Linux x86_64. Retain all observations and short/dense/plus/control costs. Adopt only with useful later-run gains and acceptable control costs; no portable throughput promise or timing gate. |
| Lifecycle | Reuse native_probe.run_phase deadlines and process-group cleanup for builds, links and execution. Remove the owned temporary archives/executables after retaining compact measurements and artifact identity. |

The author matrix-to-diff pass and one fresh independent implementation review
close this representation-preserving refinement. No new public contract or
safety strategy requires a separate design review. K1, general aggregate
provenance and exclusive sum-payload borrowing remain deferred.

The first experiment retried prefix discovery after every decoded delimiter.
It improved later long runs but regressed dense percent/form input by 69.1% /
159.6% and plus-only form input by 223.7% at 64 KiB. Reject it. The revised
experiment enters bulk search only after observing a literal run in the existing
scalar loop.
Counting on every input length still added up to 13.9 ns on a short case, so
the final candidate compiles out that unnecessary work below 80 bytes. Preserve
both superseded experiments with the final comparison.

## Qualification

All five native owners, including isolated allocation accounting, and all 21
encoding/raw-query source owners pass on Linux x86_64. The same independent
oracle covers both const specializations and percent/form delimiter rules.

The [measurement record](../../bench/escaped_decode/README.md) retains all
7,392 observations across the two rejected experiments and two final ABBA
blocks. At 4/64 KiB, early-escape tails improve 97.2–97.8%, repeated long runs
88.6–89.5% and sparse runs 33.6–52.0%. The largest short increase is 3.05 ns.
Dense percent decoding costs up to 11.5% (186 ns at 4 KiB); this explicit
tradeoff is accepted for the later-run gains. No architecture-independent or
application throughput claim follows.
