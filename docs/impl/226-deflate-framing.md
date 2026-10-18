# Complete DEFLATE framing in std.compress

Provider capability for align-llm R150, also replacing the native
`uncompress2`/uLong-width workaround in align-media video_png. PNG parsing,
filters, color policy and consumer adoption remain application work. Voice
has no matching new compression request. The three registers were inspected.

## Public and native contract

| Surface | Exact record |
| --- | --- |
| Functions | `compress.zlib_compress(data: bytes, level: i64) -> Result<buffer, Error>`, `compress.zlib_decompress(data: bytes) -> Result<buffer, Error>`, `compress.deflate_raw_compress(data: bytes, level: i64) -> Result<buffer, Error>`, `compress.deflate_raw_decompress(data: bytes) -> Result<buffer, Error>`. Require existing `import std.compress`; no defaults, aliases or new types. |
| Engine and framing | Reuse libz with windowBits 15 for RFC1950 zlib and -15 for RFC1951 raw DEFLATE. Compression emits one complete stream, including for empty input. Zlib checksum and preset-dictionary validation belong to libz; dictionaries are unsupported. Raw DEFLATE has no checksum or format-identifying magic, so validity is its grammar and complete consumption, not an additional format detector. Existing gzip uses 31 and retains concatenated members. Zstd is unchanged. |
| Levels and order | Compression accepts i64 0..=9, including 0; any other level terminates with the existing family programmer-error policy before native stream initialization. Input and level expressions evaluate in source order. Source import/arity/bytes/exact-i64 validation follows the existing gzip/zstd dispatch. No hidden level clamp. |
| Decompression completion | Require exactly one complete selected-format stream and consume all input bytes. Empty encoded input, truncation, invalid compressed data, a zlib checksum mismatch or dictionary requirement, a second stream (even empty), and trailing bytes return Error.Invalid. Padding bits belonging to the final raw DEFLATE byte are not trailing bytes. No partial prefix is returned. Independent cross-format fixtures must reject under each wrong decoder; do not promise magical discrimination of bytes that themselves satisfy raw DEFLATE. |
| Output and cap | Borrow input only for the call. Return one independent owned buffer after complete success. Decompressed payload has the existing inclusive 1 GiB cap; cap+1 rejects Invalid, including at native flush. Bound returned length, not engine workspace or total RSS. At cap use the existing one-byte stack probe, without publishing/growing it. Compression keeps its existing input-dependent output growth, without an artificial decompression cap. |
| Errors and cleanup | Preserve existing libz error mapping: invalid data/truncation/dictionary -> Invalid; native initialization/allocation/internal failures -> Error.Code. Fallible output growth -> Error.Code; final buffer-header allocation retains terminal OOM. Native error takes precedence over cap-probe overflow in the same step. Every initialized stream ends once on all returns; partial output drops. Invalid level precedes engine work; a rejected source call produces no runtime operation. |
| Effects and ABI | Impure, existing BufferStorage transfer/Drop and Result error model. Four new runtime keys and symbols: align_rt_compress_zlib_compress / align_rt_compress_deflate_raw_compress have A07 `(ptr, i64 len, i64 level, ptr out) -> i32 status`; align_rt_compress_zlib_decompress / align_rt_compress_deflate_raw_decompress have A08 `(ptr, i64 len, ptr out) -> i32 status`. `out` is a writable Buffer-handle slot, null on ordinary failure; source ptr/len must meet the existing unsafe native byte-view contract. Native effect rows match gzip Foreign/Unstated/no released source and its conservative input/output escape indices. No new library or discovery setting; both use existing -lz. |
| Artifact and cache | Extend CompressKind with Zlib and DeflateRaw and exhaustively select native keys. Existing byte/result/HIR nodes and source AST format stay. Audit interface effect/capability attribution and native dependency mapping. Compiler identity and runtime digest invalidate generated artifacts; matching compiler/runtime is required. No new portable serialized field, persisted native resource, ambient mode or user cache option. |
| Mirrors | This plan; std-design/compress.md and ja/compress.md; draft.md, language-spec.md, design-notes.md, Settled compress decision; runtime ABI inventory, native-library boundary ledger (including its exact compression key inventory), and checked-HIR ledger where exact key/domain records change. Plan124 remains the concatenated gzip/zstd contract; add a narrow link to the framing family. |
| Performance | No throughput, RSS or latency promise. No benchmark gate. This borrows the existing tuned engine and shares its memory wrapper. |

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Kind formation/validation | Add exact method dispatch and two CompressKind cases. Sweep checker, checked HIR, MIR producer, key selection, runtime dependency/capability attribution and ABI/effects tables. Existing byte-view and Result<buffer,Error> grammar remains authoritative; wrong argument types/arity/imports and malformed record/result/level types reject. |
| Shared libz lifecycle | Factor format and single-versus-concatenated completion into the existing deflate/inflate engine loops. Gzip wrappers retain their exact behavior. Initialization failure owns no stream; successful init has exactly one End even after invalid suffix, cap overflow or output allocation failure. Reuse existing native guard/owner patterns, no copied full codec loop or native-width assumption. |
| Input/output boundaries | Parameterized native owner crosses both new formats with levels0..9, empty/small/binary and multichunk data. Use small production-loop input windows to split headers/blocks/checksums and small caps for cap-1/cap/cap+1 and zero-cap empty output. Retain no-progress rejection and checked output bounds when Vec overallocates. |
| Whole-input and error precedence | Every truncation of independent fixtures, altered zlib Adler32, dictionary zlib fixture, garbage suffixes and complete second streams reject with no published buffer. Existing gzip empty/member concatenation and zstd frame owners remain positive controls. Native errors retain precedence over same-step overflow. |
| Independent bytes | Commit Python-zlib-produced zlib/raw vectors with independently recorded decoded bytes, including one small valid PNG whose concatenated multiple IDAT chunks yield the recorded scanlines. Reference production is offline fixture generation, not a runtime Python dependency. Cross the four decoder families on fixed valid reference vectors. |
| Ownership/control | Native public-entry owner checks null output on error and independent output after input expiry, using existing buffer release. Driver whole/per-unit imported helper uses success/error, match, ?, map_err, repeated replacement and post-arena output. No new move/null/Drop policy; existing Result-buffer owners close those unchanged cells. |
| Publication identity | Persistent cold/hit imported codec source owner; emitted native symbols/ABI shape and matching runtime execute on Linux/macOS. Existing capability attribution/ABI inventory owners cover exact entries and -lz linkage. |
| Local verification | Build workspace before driver tests; native compression/decompression owners, driver m11_compress plus focused framing owner, relevant HIR/MIR/ABI owners; then one full-diff review and final preflight. No service job unless actual committed scope classifier requires it. |

One capability closes both framing modes and reuses one native memory/cleanup
wrapper. Splitting the two modes would duplicate the framing/completion proof.
The runtime/registry/compiler family plus native/source/ABI owners and required
mirrors may exceed 1,000 handwritten lines. One complete framing boundary avoids
duplicating stream-lifetime, whole-input, cap/probe and atomic-publication proof
for two modes of the same engine. Separate mode or producer-only PRs would leave
a dormant surface and repeat the same native safety verification. No configurable limit, streaming API, PNG codec,
dictionary API, new ownership model or consumer-code migration is included.
Complete author ledger extraction and fresh independent strategy review before
changing FFI/ABI/cleanup; one final independent full-diff review after owners.

## Exact planned owner mapping

- `align_runtime::deflate_framing_tests::framing_round_trip_sizes_levels_and_windows`
  owns levels, payload classes and small native input windows.
- `deflate_framing_tests::single_stream_completion_and_reference_vectors` owns
  independent wire fixtures, every truncation, checksum/dictionary errors,
  format cross-fixtures and second-stream/trailing-byte rejection.
- `deflate_framing_tests::inclusive_cap_and_atomic_publication` owns cap edges,
  native public-entry null-on-failure, source independence and buffer cleanup;
  existing `decompression_frames_tests` remains the gzip/zstd regression owner.
- Driver target `compression_framing` owns
  `framing_source_domain_and_effects`, `framing_whole_per_unit_and_cached_lifecycle`
  and `framing_levels_abort_before_native_work`; existing `m11_compress` and
  `compression_frames` retain old-family behavior.
- Existing checked-HIR `validate_hir_tests`, MIR producer and LLVM runtime ABI
  inventory/capability owner targets receive parameterized new-kind/key rows.
  Exact existing function names are bound during author extraction before code.

Author extraction anchors: the exact native signature/registry obligations map
to `runtime_abi_registry_is_complete_and_unique`,
`runtime_abi_extern_type_matrix_is_exact_for_every_row_and_ordinal`, and
`runtime_abi_registry_matches_checked_in_declaration_golden`. The exact native
link closure maps to `capability_linking::gated_feature_argv_is_exactly_its_capability_closure`
and interface `summary::capabilities_captured_per_unit`. Extend the existing
parameterized checker's native-operation domain owner with both new kinds and
`deflate_codec_hir_signature_matrix` and
`deflate_codec_mir_signature_matrix` express the new discriminator coverage.

New native wrappers preserve the existing unsafe byte-range precondition; they
do not establish validity of arbitrary foreign pointers. A null out slot returns
Invalid through the existing publication helper and publishes nothing; do not
claim native work has not occurred for that unsafe null-output call. Invalid
source arguments fail before a runtime call; invalid numeric compression levels
abort before engine initialization. These are separate validation boundaries.

Strategy review found no P1/P2 strategy issue. Its P3 documentation finding is
closed by including Plan17 section2.2 runtime-key inventory in the mirror/closure
row. The native, source, HIR/MIR signature, capability and ABI owners pass locally.

The author implementation pass binds MIR signature validation to
`native_owner_mir_contract`: every codec has exact i32 status, Buffer output,
byte-view input and (compress only) i64 level. The existing generic native
out-slot fallback did not certify these operand relations. One family-wide
contract row closes this for gzip/zstd as well as the new framings, without a
new IR shape or ABI. `deflate_codec_hir_signature_matrix` and
`deflate_codec_mir_signature_matrix` own malformed data/level/result/output
relations alongside the successful source/capability owners.
