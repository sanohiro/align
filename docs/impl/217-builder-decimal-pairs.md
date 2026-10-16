# Paired decimal emission for integer builders

The existing signed builder writer preserves a dedicated -999..999 path and
otherwise emits one decimal digit per division. The unsigned writer delegates
values through i64::MAX to that path and duplicates the scalar loop above it.
Use one bounded stack helper to emit two decimal digits per division for the
remaining magnitudes. Preserve both small-value dispatches, exact text, the
single final buffer append and every existing allocation/limit decision.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Exact integer representation | Shared magnitude emission uses a fixed 200-byte table for 00..99, division/remainder by 100, and one final one/two-digit prefix. u64 requires at most 20 stack bytes; a signed magnitude is at most 2^63 and requires at most 19, leaving the existing sign byte. Keep unsigned_abs for i64::MIN. Native comparison against Rust formatting covers all small values, decimal-width boundaries, signed extrema, u64::MAX and deterministic full-width samples. |
| Unified consumers | Signed, unsigned and batched str/int/str entrypoints keep the same helper paths used by JSON integer fields. Existing native writer/batched owners and source builder/JSON owners retain exact text, complete unsigned range and ordinary whole/per-unit lowering. No new entrypoint, IR variant or type admission. |
| Storage and failure | Safe initialized stack storage and bounded safe table slices only; no pointer arithmetic, new allocation or transitive dependency. Preserve the one extend_from_slice call, failed-limit behavior, initialized output, existing heap/arena builders, finish ownership and Drop. Existing bounded scalar-writer, exact-capacity, stack-header and finish owners close these cells. Runtime effects and ABI stay unchanged. |
| Measurement | A direct C consumer checks fixed independent decimal strings, complete batched output bytes, lengths and observed call/byte counts. Use actual release baseline/candidate producers and two full ABBA passes, seven samples per process, no concurrent builds/tests and no discarded samples. Cover small/medium/large signed values, small/wide unsigned values and small/large batched writes. Require at least 10% improvement in a wide-value group and no group regression exceeding both 5% and 0.5 ns. Retain all identities and samples; no portable/application promise or timing CI gate. |
| Probe lifecycle | Register the new C fixture and exact group selector in the existing native supervisor. Reuse its source-refresh, bounded build/link/probe, signal and descendant-retirement strategy, plus the parameterized actual-process owners. No new lifecycle machinery. |

This matrix precedes implementation. It follows the established fixed-stack
integer writer and buffer publication strategy. One fresh preflight review
covers the complete capability. The handwritten diff is expected below 1,000
lines. No public contract or language mirror changes are needed.

## Author matrix-to-diff pass

The shared `decimal_magnitude` removes both long-value loops. Each repeated
remainder is 0..99 before conversion to u8 and then lossless usize; after the
loop the remaining prefix is below 100. The table contains exactly 100 pairs.
At most nine loop iterations consume 18 bytes and a final two-digit prefix
fills the 20-byte u64 bound. Signed magnitudes use at most 19 bytes, so their
existing sign write cannot underflow. Every read and write uses safe slices.
The helper introduces no transitive library call, allocation or mutable global.

The existing signed/unsigned format owners now cover 308,365 values in total:
exhaustive small ranges, every decimal-width transition, both signed endpoints,
u64::MAX and deterministic full-width samples. The existing parameterized
bounded-writer owner gains wide signed, unsigned and batched cases at exact,
one-byte-short and one-byte-spare capacities. Existing stack/finish, batched,
source builder and JSON unsigned-width owners remain the consumer proof.

The [measurement record](../../bench/builder_decimal/README.md) retains the
complete final release comparison. Wide signed/unsigned writes improve by
20.1%/12.9%; the recurring small signed cost is 0.4595 ns (5.5%). All seven
groups meet the predeclared admission; no uniform speedup is claimed. The
shared runner's parameterized lifecycle owner includes the new wrapper, its
required selector, exact flags and all timeout/signal/leader-exit paths.
