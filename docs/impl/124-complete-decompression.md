# Complete gzip and zstd decompression

The current decoders return at the first native member/frame completion, even
when input remains. A production-runtime probe on main returns four bytes for
compressed `left` followed by compressed `right` (nine expected), and succeeds
on compressed `left` followed by garbage. Both gzip and zstd share the defect.

[RFC 1952 §2.2](https://www.rfc-editor.org/rfc/rfc1952.html#section-2.2) defines
concatenated gzip members; [RFC 8878 §3.1](https://www.rfc-editor.org/rfc/rfc8878.html#section-3.1)
defines concatenated ordinary/skippable zstd frames. The
[zlib manual](https://zlib.net/manual.html) requires reset and continued inflate
after member end when input remains. The installed libzstd header and upstream
[streaming example](https://github.com/facebook/zstd/blob/dev/examples/streaming_decompression.c)
permit continuing the same DStream across frames. Borrow both engines; do not
implement their wire parsers in Align or Rust.

## Exact public contract

| Surface | Contract |
| --- | --- |
| `compress.gzip_decompress(data: bytes) -> Result<buffer, Error>` | Borrow binary input for the call. Require one or more complete gzip members and consume every byte. Concatenate member outputs in order, including empty members. Reject empty input, partial/corrupt members (including later headers/trailers/checksums), any other format, and trailing non-member bytes including zero padding with `Error.Invalid`. |
| `compress.zstd_decompress(data: bytes) -> Result<buffer, Error>` | Borrow binary input for the call. Require one or more complete current ordinary or skippable zstd frames and consume every byte. At each frame boundary admit only little-endian magic `0xFD2FB528` or `0x184D2A50..=0x184D2A5F`; reject pre-1.0 legacy formats independently of native build flags. Concatenate ordinary outputs in order; skippable payloads contribute zero bytes. Skippable-only input succeeds with an empty buffer. Reject empty input, partial/corrupt frames (including later headers/checksums and truncated skippable payloads), other formats and trailing non-frame bytes with `Error.Invalid`. |
| Output and limit, both decoders | One independent owned Move buffer is published only after complete success. Aggregate decoded length is at most 1 GiB, inclusive, across all members/frames; exceeding it is `Error.Invalid`. At the exact limit, further valid empty/skippable frames remain admissible. No partial output escapes on any failure. This limits returned payload, not native engine workspace or total RSS. |
| Allocation and errors | Reuse one native stream and one growing output Vec per call. Native stream initialization/reset/allocation failures preserve existing `Error.Code` mapping; invalid native data remains `Error.Invalid`. A call that both reports an engine error and produces a probe byte uses the engine error mapping first; otherwise any probe byte means cap overflow. Output allocation failure preserves `Error.Code`; final buffer-header allocation retains existing terminal OOM policy. No new fallback, UTF-8 conversion, I/O or ambient configuration. |
| Ownership, effects and identity | Input is never retained or consumed. Success transfers the Vec through existing BufferStorage; source/return/replacement/Drop behavior stays unchanged. Failure drops partial storage and frees native state. Existing Impure classification and A08 ABI remain. No type, interface/cache identity, artifact format, option/default or prerequisite milestone changes. |

Compression functions still produce one member/frame with existing levels and
behavior. No streaming source API, partial decode API, dictionary support or
configurable public limit is added. K1 remains deferred.

## Implementation closure

| Obligation | Implementation and owner |
| --- | --- |
| Member/frame completion versus whole-input completion | gzip returns only at `Z_STREAM_END` with no buffered or unsubmitted input; otherwise reset the existing stream and retain the unread input window. zstd admits the exact current/skippable magic before the first frame and each following frame, returns only at `ret == 0` and input.pos == input.size, and otherwise continues the same stream. A shared native owner covers one/two/many, leading/interleaved/trailing empty members, skippable zstd frames, and binary concatenated output. |
| Later invalid input cannot publish a valid prefix | All errors return through existing gzip_inflate/zstd_decompress_impl cleanup, then publish_buffer leaves a null result. The native owner appends wrong-format, garbage and each truncation of a second member/frame; public-entry and compiled source owners assert that later failures publish no partial output. Corrupt later bodies/checksums fail. A valid independently decoded legacy zstd fixture is rejected alone and after a current frame; all sixteen current skippable magics remain positive controls. |
| Inclusive aggregate cap and initialization | Below cap, existing checked Vec growth and native reported initialized length remain. At cap, point the engine at one initialized stack byte without growing or changing Vec length. Zero-byte progress may finish metadata/empty/skippable frames; any produced byte rejects, never publishes or appends the probe. Shared tiny-cap owners cover cap-1/cap/cap+1, split aggregate totals, zero cap/empty output, exact-cap suffixes and overallocated backing. |
| Progress and input windows | A nonterminal step must consume input or produce output; otherwise fail Invalid instead of spinning. Each completed member/frame must have consumed compressed input since its preceding boundary; its final step may only flush previously buffered output and need not consume new input. Native gzip owner uses the production loop with bounded small input windows to cross header/body/trailer/member boundaries; production window remains c_uint::MAX. No zero output window reaches either engine. |
| Reset and cleanup | Add only libz's stable inflateReset binding; retain unread next_in/avail_in explicitly around reset. The initialized stack probe stays live through all step calls and is never retained by a published object. Existing wrappers run inflateEnd/freeDStream once after their inner Result, including a later-member error/reset failure. Native stream guards in owner fixtures free initialized state on test failure. |
| Public/compiler consumers | No IR/type/ownership changes. Existing m11_compress owns imports, effects, levels, source Result handling and returned-buffer Drop. Add one bounded actual-source consumer for concatenated output and late-invalid propagation. Source check/build owners cover whole/per-unit calls through the unchanged ABI. |
| Contract consistency | Update std-design/compress.md and its Japanese mirror, draft.md, language-spec.md, design-notes.md and the Settled decision. Keep this ledger authoritative; no per-PR status journal. |

Owner mapping (all native owners are in
`crates/align_runtime/src/decompression_frames_tests.rs`):

| Closure row | Exact owner |
| --- | --- |
| Completion and empty/binary ordering | `decompress_concatenation_preserves_every_payload_and_empty_member` |
| Later malformed input and checksums | `decompress_rejects_every_invalid_suffix_and_late_checksum` |
| zstd admission and skippable truncation | `zstd_skippable_frames_and_legacy_rejection_are_independent_of_native_flags` |
| Inclusive cap, progress and gzip input windows | `decompress_aggregate_cap_is_inclusive_across_members_and_input_windows`; existing `grow_output_cap_enforced_on_len_despite_overallocation` |
| Atomic publication and borrowed input independence | `decompress_publishes_independent_complete_output_or_null` |
| Imported generic return, replacement, `?`, `map_err`, `match` and Drop | `compression_frames_whole_and_per_unit_publish_only_complete_results` in driver `compression_frames`; existing `m11_compress` owns import/effect/level admission |

Type formation, source nulling, control-flow joins, interface serialization and
allocation provenance are unchanged: all successful results use the existing
A08 buffer path. No IR, generic representation or ownership carrier is added.
Native test guards and the bounded child owner retain cleanup through assertion
failure; existing outer production wrappers own stream teardown after all inner
results. Error-code mapping helpers remain unchanged.

One capability fixes the two sibling whole-input completion defects and their
shared aggregate-cap invariant. Splitting format handling from cap completion
would leave exact-cap empty suffixes broken; splitting gzip from zstd duplicates
the same proof and permits the same silent truncation to survive. Expected
hand-written diff is below 1,000 lines. No speed/resource reduction is claimed,
so no benchmark is required. Native and compiled owners run on macOS/Linux,
followed by the bounded gate and Clippy.

Before implementation: complete the author contract/matrix pass and one fresh
independent adversarial strategy review of the whole-input/cap/cleanup boundary.
After implementation: one fresh full-diff review and normal preflight/PR flow.
