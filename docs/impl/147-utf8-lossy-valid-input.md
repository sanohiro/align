# Reuse valid-input admission in UTF-8 replacement decoding

The existing diagnostic decoder in the external align-llm client calls
`encoding.utf8_decode_lossy` for captured bytes and bounded text. Plan54 owns
its shipped maximal-subpart replacement contract. This capability changes only
the runtime's private repeated work; consumer code and adoption remain external.

The initial validation already determines whether every input byte is valid UTF-8.
Retain that result for counting and copy an unchanged input directly into the
exact owned output instead of validating it a second time. Inputs requiring
replacement reuse the initial error location in both passes and keep the
existing partition and second emitting pass. No new allocation,
unsafe operation, native ABI, input admission or public contract is proposed.

## Implementation closure matrix

| Boundary | Implementation and acceptance owner |
| --- | --- |
| Valid text and exact initialization | `align_rt_utf8_decode_lossy` validates the complete input once. Success fixes the output length to input length; failure passes its exact initial error to `utf8_lossy_after_error` for checked output counting. Both retain the representability check. Only a successful initial validation selects direct copy. New native owners cover empty, ASCII/NUL, multibyte and literal U+FFFD inputs around scan/copy boundaries; every returned byte must equal input. |
| Replacement partition and mixed input | Existing `batch_byte_transform_tests::replacement_vectors_and_every_scalar_split` pins independent CPython vectors. New native owners put malformed and truncated sequences after long valid prefixes and around boundaries, checking exact literal output. |
| Ownership, allocation, cleanup | Both paths retain `owned_str_exact`, one exact payload allocation for nonempty output, no allocation for empty output, and ordinary `align_rt_free`. An isolated actual-allocation owner checks counts/requested bytes and output survival after source mutation. Existing `m10_encoding::lossy_utf8_imported_owned_and_pure` owns source/imported whole/per-unit moves, return and purity. No IR, generic, interface/cache or resource-provenance change. |
| Performance and controls | Paired ordinary-release native calls include final allocation/copy/free and observed call/output-byte counts. Compare valid ASCII/multibyte text, empty/short controls, and malformed early/late/dense inputs on macOS and Linux. Accept only a useful valid-input improvement without material replacement-path regression; retain every sample and report scope. No consumer/network throughput or RSS promise. |

This follows plan54's existing safety strategy. The author-side matrix pass and
one preflight adversarial review close the private implementation change; no
separate public-contract design review or normative mirror update is needed.

## Qualification

The native boundary/allocation owners and the existing independent replacement
vector and `m10_encoding` owners pass on macOS and Linux ARM64. The final helper
preserves the original malformed-suffix loop; only the first validated prefix
is reused across counting and writing.

The ordinary-release corpus in [`bench/utf8_lossy`](../../bench/utf8_lossy/README.md)
qualifies the valid-input benefit. Approximately 64 KiB ASCII/NUL input improves
1.74x on macOS and 1.14x on Linux; multibyte text improves 2.00x and 1.75x.
Malformed controls mostly remain close or improve, with the recorded macOS
4 KiB variation and 1.25/1.57 ns empty-call costs explicitly retained. No
universal latency or consumer-throughput claim follows.
