# Fixed scratch for scalar float printing

The existing `print` contract in `draft.md` and `docs/language-spec.md` requires
one canonical primitive value followed by a newline. The two scalar float
exports currently render into a new `Vec` before taking the stdout lock.
The native baseline counts 1--2 Rust allocations per f32 call and 1--5 per f64
call across the selected value matrix on both Linux ARM64 and macOS ARM64.
The fixed-scratch candidate counts zero for all 22 warmed print calls on both
hosts. Actual compiled Align output for that matrix matches the independent
decimal oracle, and the existing generated print/template owner passes.
Plan 108 proves that 384 bytes cover canonical f32/f64 text: the conservative
343-byte bound also leaves space for the newline.

Replace only that temporary render storage with the existing initialized fixed
scratch. Keep formatting before stdout locking, one complete-line `write_all`,
the chained flush and existing ignored-I/O-error policy. No exported signature,
effect, language rule, caller-owned storage or builder path changes. This is
private call-local storage; no pointer escapes and no new resource owner exists.

| Closure axis | Implementation and acceptance |
| --- | --- |
| Both scalar widths and all value classes | The shared rendering path accepts only the existing f32/f64 exports. The plan 108 parameterized canonical-text owner covers every exponent, both signs and representative mantissas; its bound plus one newline fits fixed scratch. |
| Actual printing and allocation | `float_print_uses_fixed_rendering_scratch` measures complete warmed native print calls with the test-only thread-local Rust allocator and an allocating positive witness. Signed zero, ordinary values, finite extrema, subnormals, infinities and NaNs run through both exports. Restoring the Vec path must fail. Stdout first-use allocation is outside the interval. |
| Source output and unchanged I/O behavior | Existing `m5::print_and_template_float` owns generated print/template bytes and newlines. A local exact-bit source probe checks long finite output against an independent decimal oracle. The stdout lock, one write and flush/error chain stay unchanged. |
| Ownership, control and compiler pipeline | Initialized scratch is local to one call. There is no move, replacement, Drop, branch/loop join, native admission, IR, interface or cache change. Existing source and package owners remain applicable. |
| Resource claim | Compare actual native Rust allocation counts on Linux and macOS before/after. This claims removal of temporary rendering allocations only; it makes no throughput, latency, RSS or first-use stdout guarantee. |

One independent full-diff inspection closes the private boundary with the
existing self-review gates. No public contract or safety-strategy change needs
a separate design review.
