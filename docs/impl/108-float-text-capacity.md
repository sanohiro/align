# Complete fixed-storage float text

The package-internal `F32TextLen`/`F64TextLen` and `F32TextWrite`/`F64TextWrite`
helpers promise the canonical `push_float` rendering without heap allocation.
Plan A1 in `pkg-design/db.md` uses those lengths during PostgreSQL Text parameter
measurement and the same bytes during encoding. `20-runtime-abi-ledger.md` owns
their existing signatures and effects.

The previous 128-byte scratch cannot hold ordinary finite `f64` decimal text.
At the baseline, the exact bits of `1e-200` report 66 bytes instead of 202;
`f64::MAX` reports 83 instead of 311. The formatter exhausts scratch, and its
ignored write error leaves a prefix that both helpers mistake for complete text.
The existing native owner covered zero, infinity and NaN, but no finite extremes.

## Implementation boundary and bound

Use one private 384-byte capacity for all three fixed-scratch construction sites.
Rust's shortest `f64` rendering needs at most 17 significant decimal digits.
Its first nonzero decimal digit has an exponent between -324 and 308. Expanding
the small end takes at most one sign, `0.`, 323 leading fractional zeros and 17
significant digits: 343 bytes. Expanding the large end takes at most one sign,
309 integer digits and the canonical `.0`: 312 bytes. Special values are shorter;
`f32` is contained by these bounds. 384 is a conservative fixed capacity, not a
claim that some value has a 384-byte canonical representation.

This restores the existing format rather than selecting scientific notation or
allocating a fallback. It changes no public contract, pointer admission, FFI
ownership, native signature, effect, IR, interface or cache schema. The buffer is
call-local initialized byte storage; no pointer to it escapes. The existing
caller-capacity check still precedes the sole output copy. Print and builder
storage are independent of the fixed helper and need no change.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Value formation and rendering | One shared fixed capacity covers f32/f64, both signs, zero, normal/subnormal finite values, infinity and NaN. `package_float_helpers_preserve_bits_and_measure_canonical_text` sweeps every exponent with representative mantissas and compares actual length/write helpers to independent Rust Display plus the canonical `.0` rule. |
| Return and output admission | The same native owner verifies exact capacity, complete bytes, guard bytes, and negative/zero/one-short/null destination refusal without writes. No caller storage is accessed until capacity and pointer admission succeed. |
| Allocation and lifetime | The existing thread-local Rust allocation counter, with an allocating positive witness, surrounds complete length/write calls. Expected text and destination setup are outside the interval. Fixed scratch has no heap owner, move, replacement, source nulling or Drop obligation. |
| Actual package consumer | A real PostgreSQL Text parameter owner sends long finite f64 values through generated direct and prepared binders and checks exact server-side bit identity. Existing package owners cover f32/nullable/binary, mode/error/cleanup and whole/per-unit admission. |
| Source-to-native and unchanged control paths | Existing package compilation reaches the same registered extern ABI and generated Measure/Encode calls. No language construction, generic, branch, loop, early-exit, malformed-HIR or ownership analysis changes; their cumulative owners remain applicable. |
| Mutation and resource evidence | Restoring the 128-byte capacity must fail both the native canonical-text owner and the actual package consumer. Zero observed Rust allocations closes the existing helper promise; no latency, throughput or RSS improvement is claimed. |

The author-side boundary pass selects one runtime correctness capability and its
direct package regression. No new public or safety strategy requires a separate
design review; the matrix and complete diff receive the one independent preflight
review. Required local PostgreSQL verification precedes push.
