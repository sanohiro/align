# Decimal parsing after validated zero prefixes

Long decimal inputs currently apply checked multiply/add or subtract to every
leading zero. For inputs longer than the existing 18-digit fast-path bound,
admit the initial ASCII zero run without arithmetic, then select the bounded
or checked scanner by the remaining significant length. Keep the existing
optional-sign and nonempty admission first. An entirely zero input is valid
and leaves the already initialized scalar output at zero.

The public contract remains [Plan83](83-checked-text-integer-plan.md): complete
ASCII decimal, inclusive i64 range, canonical Invalid/zero output, allocation
freedom and no retained input. This changes neither the short-input grammar nor
native preflight, output publication, effects, ABI, ownership or compiler code.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Prefix and arithmetic proof | Only the byte `0` may be discarded, after the optional sign and original nonempty guard. A zero prefix cannot change magnitude, overflow or sign. Empty remaining input means a previously nonempty zero sequence, not an empty/sign-only input. At most 18 remaining digits use the existing bounded arithmetic; longer significant strings retain checked signed accumulation and asymmetric endpoints. |
| Complete validation | Every skipped byte is proven ASCII zero; every remaining byte still enters the existing digit check. No whitespace, NUL, high byte or second sign may be skipped. Reuse the existing parameterized independent Rust oracle: every byte replacement at every position across widths 1..22 and both signs, plus 19/20/64/4096 leading zeros before zero, ordinary values, both range edges, overflow and malformed suffixes. This owner already distinguishes incorrect trimming or threshold selection; no redundant fixture family is needed. |
| Native/storage/source | Retain raw extent/output tests and the four complete driver `str_parse_i64` owners for imports/generics, whole/per-unit, control paths, input ownership and Pure parallel use. Extend the existing allocation-count inputs with valid and invalid long zero prefixes. Slice operations remain safe, stack-only and non-retaining; no new dependency or mutable global state. |
| Measurement | Reuse the unchanged 95-golden `bench/str_parse_i64` C fixture and existing bounded supervisor. Compare actual non-test release producers in two complete seven-group ABBA passes, seven samples per process, no concurrent builds/tests and no discarded samples. Require at least 10% improvement in the leading-zero group and no group regression exceeding both 5% and 0.5 ns. Preserve short positive/negative/explicit-plus controls, endpoints/overflow and short/long malformed controls, all status/output/work checks and producer identities. No portable/application claim or timing CI gate. |

This author matrix precedes implementation. It reuses the existing checked
scanner and safe native strategy. One fresh independent preflight review covers
the entire change; no separate cross-cutting strategy review or public grammar
revision is selected. The expected handwritten diff is below 1,000 lines.

## Author matrix-to-diff pass

`align_rt_str_parse_i64` retains the original native and sign/nonempty checks.
Only an original digit slice longer than 18 enters `position(byte != '0')`;
the returned index, or the slice length when all bytes are zero, is in bounds.
Every discarded byte is an admitted zero and changes neither value nor range.
An empty remainder enters the existing bounded path with accumulator zero;
an originally empty or sign-only input never reaches it. No secondary sign
admission is performed. Every nonzero remainder byte still undergoes the
original ASCII digit check, and the original checked negative accumulation
retains i64::MIN without negating its magnitude.

The unchanged native grammar/extent and `str_parse_i64_short_boundary_matches_decimal_oracle`
owners already cover each selected cell, including long zero prefixes and
malformed bytes at every position. The allocation owner gains fixed valid and
invalid prefix inputs; no new fixture or execution harness is needed. Plan83's
scanner row now records this implementation while its public contract remains
unchanged. The four source owners retain whole/per-unit/generic/control/purity
and ownership coverage.

The [complete measurement](../../bench/str_parse_i64/README.md) meets the
predeclared bounds: leading-zero and long-invalid groups improve 66.9%/42.8%,
and the largest control cost is the short-invalid group's +0.292 ns (6.9%).
All 392 samples are retained, with independent golden status/value checks and
source/archive/executable identities. No uniform or portable speedup is claimed.
