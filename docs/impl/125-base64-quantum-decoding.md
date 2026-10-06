# Base64 quantum decoding

The scalar decoder processes one symbol at a time, carrying an accumulator and
bit count through each byte. Decode complete four-symbol quanta into three
bytes, with the final two/three-symbol tail admitted into fixed scratch before output allocation. Keep the
existing compile-time alphabet tables and safe Vec writes. Qualify the change
against the actual release runtime on macOS and Linux before accepting it.

## Unchanged contract and implementation closure

`encoding.base64_decode(s: str)` and `encoding.base64url_decode(s: str)` retain
`Result<buffer, Error>`, call-borrowed input, independently owned binary output,
Pure effects, A08 publication, terminal allocation policy and ordinary buffer
Drop. There is no IR, source/type admission, interface, artifact/cache identity,
public option or native ABI change. No architecture-specific implementation or
new dependency is introduced. K1 remains deferred.

| Obligation | Implementation and discriminating owner |
| --- | --- |
| Alphabet separation | Select the existing standard or URL table once. Reject a nonempty input whose first symbol is invalid before reserving output. Test every byte value at every symbol position in a complete quantum, including the other alphabet and `=`. |
| Optional but exact padding | Preserve existing trailing-pad count, impossible remainder and complete padded-group checks. Test padded/unpadded tails and misplaced/excess padding. |
| Canonical tail bits | A two-symbol tail requires low four bits of the second value zero; a three-symbol tail requires low two bits of the third zero. Exhaust every tail value for both alphabets, with/without padding, against an independent bit-accumulator oracle. |
| Complete output or Invalid | Reject any invalid quantum/tail before publication; partial private Vec drops through the unchanged decode_into path. Native entries assert Invalid/null after a valid prefix and verify binary output independently. |
| Safe sizing and initialized storage | Checked decoded-length arithmetic sizes the one Vec; safe extend/push writes remain. No unchecked indexing outside chunks_exact/remainder bounds and no raw set_len. Empty input needs no payload allocation; every nonempty success needs one Vec allocation, without growth. Feature-gated actual thread-local allocation owner observes the private decoder separately from the unchanged public buffer shell. |
| Length/offset/caller ownership | Full-quantum, remainder and padded/unpadded sizes agree with independent output across short lengths and larger binary payloads. Input is not retained; native owner overwrites it after decode. Existing m10_encoding covers source use, Result and Drop; unchanged ABI does not require new driver machinery. |
| Consumer/sibling sweep | Both base64 variants use the same decoder. Existing pkg.auth JWT and password-record callers retain their own validation layers; WebSocket admission has its own decoder and is unchanged. Run the existing narrow owners that call these paths. |
| Performance | Compare static binaries linked to baseline/candidate non-test release runtime with identical compiler flags and corpus. Confirm complete bytes before timing and producer status, length, call/byte counts during timing. Balance AB/BA order; cover empty/short, 1 KiB/64 KiB, both alphabets and early/late invalid data. Accept only a repeatable improvement without a material short/error regression on either host. No application throughput, SIMD, RSS or CI timing claim. |

This follows the existing checked-Vec/atomic-buffer allocation strategy; the
author matrix pass and one fresh full-diff preflight review own the boundary.
No separate public design review is required. One capability keeps the quantum
loop, exact length, tail checks and shared parity owners together. Expected diff
is below 1,000 hand-written lines. Normative encoding specifications are unchanged.

## Qualification

`base64_quantum_every_symbol_position_and_tail_matches_scalar_oracle` exhausts
symbol bytes in full quanta and two/three-symbol tails with/without padding.
`base64_quantum_binary_lengths_and_padding_preserve_all_bytes` covers all short
lengths and large binary data. `base64_quantum_native_publication_is_atomic_and_independent`
owns late-error/null publication and input independence. The actual thread-local
allocator owner `base64_quantum_allocates_only_nonempty_output_once` closes both
exact output allocation and pre-allocation first-symbol rejection.

The final actual-runtime AB/BA comparison uses the same independently checked
C harness on both arms: fourteen samples per arm/case, no concurrent local
builds. Normal 1 KiB/64 KiB inputs improve 2.92–3.07× on macOS ARM64 and
2.06–2.22× on Linux ARM64. One-byte output on Linux is 0.55 ns/call slower;
other short cases are effectively unchanged or improve. Empty input improves
1.61–1.66×. Invalid-prefix and malformed-tail admission improve. The full
corpus, producer/byte-count checks, clocks, toolchain and reproduction are in
[`bench/base64_decode`](../../bench/base64_decode/README.md). These are local
measurements, not public throughput guarantees. A discarded early sample set
had linked the baseline archive twice through a shared-target Cargo Fresh path;
qualified runs force actual producer rebuilds and verify distinct executable
hashes. Another discarded set had insufficient repeats for constant-time tail
rejection; final invalid-case sample counts exceed timer resolution.
