# Hexadecimal group decoding

Replace the scalar per-nibble range dispatch in hex_decode with one private
portable lookup decoder. Consume complete eight-symbol groups into four bytes
and handle the remaining complete pairs. Keep safe Vec writes and the existing
Buffer publication path. Qualify actual release-runtime latency before accepting
this internal implementation change.

## Unchanged contract and closure

`encoding.hex_decode(s: str) -> Result<buffer, Error>` remains Pure, borrows its
input only for the call, and returns independently owned arbitrary bytes. Accept
ASCII digits and either case of A through F, reject odd length or any other byte
with Error.Invalid, and preserve empty success. Native A08 is unchanged: status 0
and a Buffer handle on success; Invalid publishes a null handle. Existing unsafe
pointer/readable-range preconditions and bytes_view normalization do not change.
No options, type/IR/effect/interface/cache-key/dependency change, new architecture
path, or new allocation/cleanup strategy. K1 remains deferred.

| Axis | Implementation and owner |
| --- | --- |
| Alphabet/admission | One immutable 256-entry nibble table has exactly 22 accepted byte spellings. Exhaust all 65,536 two-byte products against an independent range decoder, including NUL and non-ASCII. Odd length is refused before payload reserve, as today. |
| Complete groups/tail | chunks_exact(8) bounds all symbol accesses; exact even input leaves 0/2/4/6 symbols. Validate each group's values before extending four fully initialized bytes. Pair tail uses identical table admission; no invalid tail or partial output can publish. Cross every symbol position, group boundary and tail with invalid bytes and independent whole-output goldens. |
| Allocation/ownership | Retain Vec::with_capacity(input.len()/2), safe extend/push, unchanged decode_into and Buffer Drop. Empty private payload allocates nothing; nonempty valid output reserves once without growth. Invalid private Vec drops before error publication. Native owner overwrites input after success, then inspects and frees the independent Buffer. |
| ABI/control/siblings | Native entry still uses bytes_view and decode_into; no raw write, new user-view validation, Result join/return cleanup, monomorphization or per-unit contract. Leave hex_val unchanged for percent/form decoding. Existing m10_encoding and relevant crypto consumers cover the existing public path; no new driver fixture helper. |
| Resource proof | Use existing actual thread-local Rust allocation counters for the private decoder, separately from C live instrumentation. Assert exact successful payload allocation behavior, not global allocator RSS or a new rejection allocation guarantee. |
| Performance | One identical C independent encoder/oracle drives actual baseline/candidate non-test release runtimes. Check full bytes before timing; verify status, Buffer count, observed calls/output bytes and free every timed result. Cover empty/short/large lower/upper/mixed case, odd input, and early/late invalid symbols. Actual producer rebuild logs/distinct executable hashes prevent shared Cargo archive confusion. Per-case ABBA, adequate warmup/repeats, one guest vCPU for Linux. No concurrent local builds. Reject material latency regression; no application throughput/SIMD/RSS claim. |

This follows the existing checked-Vec and atomic Buffer-publication strategy.
The author matrix-to-diff pass and one fresh full-diff review own the boundary;
no separate strategy review is required. One capability contains loop/table/tail
and the shared parity owners. Expected diff stays below 1,000 handwritten lines.
Normative encoding specifications remain unchanged.

## Implementation closure

`HEX_DECODE_TABLE` and `hex_decode_impl` own table/group/tail processing in
`align_runtime`; the exported `align_rt_hex_decode` delegates through unchanged
`bytes_view` and `decode_into`. `hex_val` remains the percent/form helper.

The five `hex_group_tests` owners close the matrix:

- `hex_group_every_byte_pair_matches_independent_oracle`: complete byte product
  and accepted-alphabet cardinality.
- `hex_group_every_symbol_position_and_tail_matches_oracle`: every byte at every
  position of lengths 0 through 23, spanning two groups and every pair tail.
- `hex_group_binary_lengths_and_cases_preserve_all_bytes`: independent lower,
  upper and mixed-case goldens, short lengths, 1023/1024/1025 and 65536 bytes.
- `hex_group_native_publication_is_atomic_and_independent`: success including
  empty, source overwrite, early/middle/late invalid group/tail and odd input;
  an immediately armed owner frees every successful Buffer.
- `hex_group_allocates_exact_nonempty_payload_once_and_rejects_odd_before_reserve`:
  actual thread-local Rust allocation calls and requested bytes, with an active
  counter witness and no process-global C instrumentation.

macOS ARM64 and Linux ARM64 pass these five native owners with `alloc-count`,
a workspace build, 18 `m10_encoding` owners and 38 `m11_crypto` owners. Linux
uses OpenSSL 3.5.7 for the existing crypto provider tests. The author extraction
pass closes every applicable obligation against these implementations/owners.
The actual runtime timing fixture and accepted measurements live in
`bench/hex_decode/README.md`; timing remains separate from correctness gates.
