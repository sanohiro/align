# Bounded short decimal parsing

`str.parse_i64()` currently checks signed overflow at every decimal digit.
After the existing optional sign and nonempty admission, at most 18 digits
have magnitude at most 999999999999999999, below `i64::MAX`. Use that bound
to scan short inputs with ordinary i64 arithmetic, validating every digit,
then apply the sign. Longer inputs retain the checked scanner, including
arbitrarily many leading zeros and the asymmetric i64 endpoints.

The public contract is unchanged from [Plan83](83-checked-text-integer-plan.md):
whole-input ASCII decimal, one Invalid error, canonical zero on failure,
no allocation, and no retained input. Native pointer/extent validation,
publication, ownership, ABI, compiler lowering and cache formats stay intact.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Arithmetic bound | The fast path is selected by digit count after removing the optional sign, only for 1 through 18 bytes. Each prefix remains nonnegative and below 10^18. All bytes must be ASCII digits before publication; sign application cannot overflow. |
| Boundary and malformed input | Existing raw-extent owner plus a parameterized oracle owner exercise lengths around 18/19, both signs, every byte replacement at every position, i64 endpoints/overflow and long zero prefixes. Compare exact status and canonical output with independent Rust decimal conversion. |
| Allocation and source composition | Existing allocation-count owner and complete driver `str_parse_i64` target retain no-allocation, input ownership, scalar return, Result control flow, generic/imported whole/per-unit and Pure parallel behavior. No new cleanup or alias proof. |
| Measurement | New direct native fixture fixes short positive, negative and explicit-plus inputs, endpoint controls, long-zero controls and short/long malformed inputs. Check exact status/value in every call and exact call/invalid/checksum totals. Two complete per-group ABBA passes, seven samples per process, no discarded samples, non-test release producers and no concurrent builds/tests. Require at least 10% improvement for a short-valid group, and no group regression above both 5% and 0.5 ns. Record all producer identities and samples; no application or portable throughput claim. |
| Probe ownership | Reuse `bench/native_probe.py` for bounded build/link/run phases and signal/process-group cleanup. Register the new wrapper in its existing parameterized lifecycle owner. No new process ownership strategy. |

This matrix precedes implementation. The boundary is one private arithmetic
fast path and its evidence, below 1,000 handwritten lines. One fresh independent
preflight review checks the complete diff; no public/safety strategy changes.

The native grammar/extent and parameterized decimal oracle owners pass, as
does the allocation-count owner. The [local measurement](../../bench/str_parse_i64/README.md)
retains 392 samples from two complete comparisons. Short valid groups improve
30.6–32.8%; the endpoint/overflow control costs +0.349 ns (1.4%). Every group
meets the predeclared adoption bounds. The long checked path and native
publication/validation order are unchanged.
All four source driver owners pass after a workspace build. The shared runner's
five owners pass, including 210 actual phase/signal lifecycle cells across its
14 registered probes and the new wrapper's strict compiler flags and selector.
