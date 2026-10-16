# Four-digit accumulation within the bounded decimal parser

The existing parser validates each digit and uses bounded nonnegative i64
arithmetic for at most 18 significant bytes. For lengths 8..18, evaluate
four-byte groups as independent decimal coefficients, then append each group
with one multiply by 10,000. Retain the existing scalar loop for shorter inputs
and the final 0..3 bytes. Sign admission, validated zero prefixes and the longer
checked signed path remain unchanged.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Digit and arithmetic proof | A four-byte safe slice group has four initialized positions. Wrapping subtraction of ASCII zero yields 0..9 exactly for ASCII digits; reject any component above 9 before arithmetic. The group value is at most 9999. Accumulating groups followed by scalar remaining bytes gives the same base-10 value, and every partial prefix remains below 10^18. No sign is applied until successful complete bounded parsing. |
| Threshold and malformed coverage | Reuse the existing independent decimal oracle's every-byte substitution across lengths 1..22, both signs, zero/nine extremes and long-prefix suffixes. It spans 7/8, every four-byte boundary and 18/19, and pins malformed bytes in every group/remainder position. The raw-span owner retains native admission and canonical failure output. |
| Storage, effects and consumers | Safe byte slices and scalar stack values only; no unchecked loads, new allocation/dependency/global state or ABI change. Add 18-digit positive/negative inputs to the existing allocation-count owner. Reuse all four source parser owners, including whole/per-unit/generic/control/ownership/Pure parallel behavior. No public grammar or mirror changes. |
| Measurement | Extend the existing native C corpus with 7/8-digit positive, negative and explicit-plus controls and malformed 7/8-byte controls before building either producer. Use the identical resulting 111 fixed goldens for actual release baseline/candidate builds. Run two complete seven-group ABBA passes with seven samples per process, no concurrent builds/tests, no discarded samples and complete status/output/work checks. Require at least 10% improvement in one short-valid group and no group regression exceeding both 5% and 0.5 ns. Retain all identities and samples; no portable/application claim or timing CI gate. |

This matrix precedes implementation. The operation remains within Plan83's
existing bounded scalar arithmetic and safe native boundary. One fresh
independent preflight review covers the retained implementation and evidence.
The unchanged seven-group supervisor protocol needs no new lifecycle path.
The expected handwritten diff is below 1,000 lines.

## Author matrix-to-diff pass

`align_rt_str_parse_i64` uses `chunks_exact(4)` only for 8..18 remaining bytes.
Each indexed byte is in a complete four-byte group. Each wrapping difference
must be at most 9 before conversion to i64; the resulting coefficient is at
most 9999. `groups.remainder()` feeds the original scalar loop. Both accumulation
forms preserve the same prefix bound, and only a complete parse publishes the
signed value. Native admission, long checked arithmetic, errors, ownership and
ABI metadata are unchanged.

The existing `str_parse_i64_short_boundary_matches_decimal_oracle` covers every
byte and position across 1..22 digits and all signs, including each grouped and
remainder boundary. `str_parse_i64_grammar_range_and_raw_extents` retains native
preflight and canonical error output. `str_parse_i64_allocates_nothing` adds both
18-digit signs; the four `align_driver::str_parse_i64` owners retain source,
control, imported generic, whole/per-unit and Pure parallel behavior.

The 111-golden release comparison retains all 392 samples and producer identities
under `bench/str_parse_i64/digit-blocks-macos.*`. Short-valid groups improve
14.0–14.6%; the only group regression is endpoints at +0.063 ns (+0.24%). The
predeclared gate passes. This is a local primitive observation, not a portable
or application throughput promise.
