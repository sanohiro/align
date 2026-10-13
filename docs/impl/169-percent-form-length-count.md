# Bounded narrow counts for percent and form output

Plan156 counts escaped bytes before checked output sizing. Its percent/component,
percent/path and form counters still accumulate one machine-width value per
input byte. Evaluate bounded u8 reductions over 128-byte chunks, then combine
their counts and retain the existing checked multiply/add for the final size.
Each chunk contributes at most 128; the combined escape count cannot exceed
the source slice length. Express counting's unreserved predicate as independent
ASCII lane comparisons so the reductions can vectorize; retain the writers'
existing scalar classifier. The independent all-byte oracle owns their agreement.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Exact size and bytes | Component escapes every non-unreserved byte; path additionally preserves slash; form maps space to one plus byte. All other bytes, including NUL and UTF-8 octets, retain their existing treatment. The independent encoding_writes_tests oracle covers all byte values, the three selectors, dense/unreserved/space/slash cases and reduction boundaries. Existing mismatch/sentinel owners retain exact initialized output. |
| Arithmetic | A chunk has at most 128 one-bit contributions, so u8 cannot overflow. Disjoint chunks contribute at most data.len() overall; final expansion multiplication and total addition remain checked before allocation. Empty input remains zero. Boundary and repeated-byte owners distinguish the count from the unchanged writer. |
| Allocation and publication | No new allocation, unsafe code, copy, input retention or alias rule. Existing exact owned-string allocation, failure policy and publication remain unchanged. Inspect the unchanged owned_str_exact call path; the exact counter/writer owner and m10_encoding source owner retain output extent and owned-string cleanup. |
| Compiler and control | No API, type, IR, ABI, generic, interface, cache or ownership change. Existing source consumers retain ordinary Result/control/Drop behavior. K1 and aggregate-provenance work stay deferred. |
| Measurement | Reuse encoding_writes' identical C source, independent oracle and observed call/output extents. Compare rebuilt ordinary release archives in ABBA order on macOS ARM64 with no overlapping build/test work. Retain every observation, short/dense/control costs and producer identities. Adopt only with useful gains; no portable speed or application-throughput promise and no timing correctness gate. |
| Lifecycle | Reuse native_probe.run_phase for bounded build/link/probe process groups. Preserve compact results and remove temporary archives/executables after the final consumer. |

This representation-preserving refinement needs the author matrix-to-diff pass
and one fresh independent implementation review, with no separate design review.

The first experiment changed only the counter width and left short-circuit
classification intact. Its 7,200 macOS observations show no consistent benefit:
long component/form medians across cases increase about 2.3%. Reject it and
retain the measurements. Two further experiments changed the shared scalar
predicate as well; form improved but ordinary percent/path output regressed.
Keep the scalar writer unchanged and specialize only the count's lane predicates
for component, path and form. All 256 repeated bytes match the independent
output oracle across chunk boundaries.

## Qualification

The three native counter/writer owners and eighteen m10_encoding source owners
pass on macOS ARM64. All extracted exact-byte, pre-allocation-size and initialized
publication obligations map to the unchanged writers/owned_str_exact call path
and the independent all-byte/count/mismatch tests. There is no new public or
unsafe boundary. The final scalar writers and their predicate are unchanged.

The [measurement record](../../bench/encoding_writes/README.md) retains all
28,800 observations from the three rejected experiments and final comparison.
Every final 4/64-KiB component/path/form case improves, by 26.1–45.1%; the
largest short increase is 1.9 ns. HTML control variation and host limitations
remain explicit. This qualifies the local refinement without a portable speed,
allocation-count, application-throughput or RSS promise.
