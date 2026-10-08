# Shared hex lookup for percent and form decoding

`pkg.web` returns borrowed raw query values and documents explicit
`encoding.form_decode` for callers that need decoded bytes; its existing
`apps_web_query` owner exercises that composition. Percent and form decoding
currently classify each escape digit through branch-based character ranges.
Reuse the existing case-insensitive hex decode table through one private
byte-pair helper. Keep both existing scans, plus handling and allocation strategy.

This is a private, safe-Rust refinement of `draft.md`'s `std.encoding`
contract. Percent decoding preserves `+`; form decoding changes it to space.
Both accept upper/lower hex, preserve arbitrary other bytes, and reject
incomplete or non-hex escapes. There is no public API, ABI, compiler, ownership,
allocation strategy or cache-format change. K1/plan61 stay deferred.

| Closure obligation | Implementation and owner |
| --- | --- |
| Complete escape admission | Both callers prove two following bytes exist before calling `percent_escape_byte`. Every table entry is a valid nibble or 0xff; either invalid entry rejects the pair. `escaped_decode_tests` compares an independent scalar oracle across all 65,536 suffix pairs, mixed/plain/binary inputs and malformed positions. |
| Allocation and publication | Reserve the original input length once, as before. Every append emits no more bytes than it consumes. Empty output and failure retain the existing `decode_into` and `buffer_from_vec` behavior; `.capacity()` still reports the decoded length. Native owners check independent output, failure slots and exact allocation requests in an isolated child. |
| Ownership and control flow | Existing vector RAII drops partial output on error; success transfers through the unchanged buffer wrapper and Drop. No source retention, new unsafe code or native side effects. Driver `m10_encoding` and `apps_web_query` cover ordinary Result handling, decode/as_str and actual query consumers. No generic, interface, IR or compilation-mode changes. |
| Performance acceptance | Compare the actual exported entries plus buffer view/free in ordinary release builds. Plain, sparse, dense, plus-heavy and short controls use independent expected bytes and observed completed calls/output lengths. Retain all paired samples on macOS ARM64 and Linux ARM64; require an escape-heavy improvement without a material plain-input regression before adoption. No timing correctness gate or network/application speed claim. |

The author matrix pass and one fresh independent implementation review close
this representation-preserving change; no new public design review is needed.

Focused acceptance uses `align_runtime --lib --features alloc-count
escaped_decode_tests`, and the driver targets `m10_encoding` and
`apps_web_query`, on macOS and Linux ARM64. The native matrix includes the
process-isolated allocation owner; the existing driver tests retain their
source/control/consumer coverage without new compiler fixtures.

The [release measurement](../../bench/escaped_decode/README.md) retains every
sample and completed-work count. Dense-escape medians improve 1.70–1.92× on
macOS and 1.70–1.90× on Linux; mixed-input medians improve 1.33–1.38× and
1.31–1.37×. Large plain/sparse/plus controls stay within about 2%. This qualifies
the native decoder path only, with no network, application or RSS promise.
