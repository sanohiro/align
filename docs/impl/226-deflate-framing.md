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

## Caller-selected output limits (LLM R156)

R156's PNG consumer knows its scanline extent before inflating IDAT. Its current
post-decode size check cannot prevent a larger output allocation. Media's
`video_png` has the same requirement behind its native `uncompress2` binding;
voice has no compression consumer. This extension changes only the existing
decoder parameter contract; the earlier no-configurable-limit boundary above
describes the original framing delivery.

### Public contract ledger

| Surface | Exact record |
| --- | --- |
| Signatures | Each of `compress.gzip_decompress`, `compress.zstd_decompress`, `compress.zlib_decompress`, `compress.deflate_raw_decompress` takes `(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>`. The second positional argument is optional; no general default-parameter language feature is introduced. Existing one-argument calls normalize to the same two-operand HIR. |
| Domain and validation | Source arity is one or two; data is the existing call-borrowed byte view, followed by an exact i64 limit. Evaluate explicit operands once in source order, keeping input live through limit evaluation. A terminating operand performs no native action. After operand evaluation, a limit outside `0..=1073741824` aborts before input inspection or native initialization, even for malformed/empty compressed input. No ambient configuration. |
| Output and errors | The aggregate output across the existing accepted members/frames is at most max_bytes, inclusive. Excess output is Error.Invalid. Zero accepts complete streams with no output (and zstd skippable-only input), but not empty encoded input. Exactly-full output still validates all trailing metadata/empty frames and checksums. Existing format/completion and native-error precedence remain unchanged. |
| Ownership/allocation | Input is arbitrary binary bytes, including NUL, borrowed only for the call. Success returns one independent Move buffer; error publishes null and releases native state and staged output. Existing buffer transfer/replacement/Drop remains unchanged. Every output allocation request and native payload-buffer span is clamped to max_bytes; output grows on demand, starting at min(64 KiB, max_bytes). The separate one-byte overflow probe stays on the stack, including at zero limit. Allocator rounding, transient realloc old/new overlap, engine workspace, buffer header and process RSS are outside this payload-storage bound. No new clone or allocation category. |
| Owners and ABI | Sema inserts/checks max_bytes; all HIR walkers and replay/monomorphization visit it. MIR requires it before the call; LLVM purely lowers it. All four existing native symbols become `i32(ptr data, i64 len, i64 max_bytes, ptr out)`. Registry, emitted declarations and Rust exports change atomically; no parallel legacy ABI. |
| Artifact identity | Existing compiler build identity invalidates frontend and object caches; generic source replay preserves the argument. No serialized interface type or public type layout changes, so no interface format bump. Whole-program, per-unit, imported generic and cold/hit execution must agree. |
| Documentation | This ledger, draft, language-spec, design-notes, Settled, English/Japanese compress design, HIR-validation and native ABI ledgers agree. Existing Plan124 framing semantics continue; the caller limit specializes its aggregate cap. Request receipts distinguish delivery from adoption. |
| Prerequisite and evidence | Existing complete-input engines (Plan124/226) already implement parameterized inclusive caps. No future milestone is consumed. Runtime allocation observation is a correctness/resource owner, not a speed claim; no timing benchmark is required. |

### Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation, malformed input | Extend `Decompress` with required i64 `max_bytes`; source arity/type/domain owners in driver `compression_framing`; extend `deflate_codec_hir_signature_matrix` and `deflate_codec_mir_signature_matrix` to all four codecs and malformed limit children/types. Existing result/out/data controls remain. |
| Every eager/control path | Sweep `Decompress` alongside two-child `Compress`: borrow reservations, move/escape/effect walks, finalization, depth traversal, replay clone, MIR required-child lowering, MIR operand inventory. `compression_framing::limits_*` covers side-effect order, input invalidation, nested helper/closure captures, `if`, `match`, `else`, `?`, `map_err`, loop/replacement and limit early exit. Existing Result-buffer owners retain move-in/out, source nulling and Drop proof. |
| Native bounds/completion | `decompression_limit_tests` parameterizes all four exported decoders over zero, exact, one-below and spare limits, empty/nonempty streams, malformed/truncated/trailing input, and gzip/zstd aggregate frames. Existing `decompression_frames_tests` and `deflate_framing_tests` retain independent wire vectors. |
| Allocation and atomic failure | Observe actual output growth requests/capacities during public native calls with test-local instrumentation, including a compressed large-output witness and zero/tiny/growth-boundary limits. Assert no requested output capacity exceeds limit, no partial handle escapes, and successful handles free normally. Existing native stream cleanup is reused on every return. |
| Return, imports and caches | `compression_framing::limits_*` whole/per-unit and cold/hit imported generic helpers exercise owned output after input expiry, replacement, propagation and both result arms. Default and explicit 1 GiB calls are equivalent on bounded fixtures. |
| ABI and publication | Exact runtime registry/type/declaration-golden owners cover all four changed symbols; existing capabilities still require libz/libzstd. Native limit-abort subprocess cases cover -1 and 1073741825 for each codec, including malformed input. |

One family-wide capability is the smallest useful boundary: exposing only one
decoder or a dormant native parameter would duplicate identical operand,
allocation and ABI proofs. If the complete sweep and parameterized owners exceed
1,000 handwritten lines, retaining this boundary lowers integration risk by
keeping all four ABI changes and the common HIR/MIR operand contract atomic.
Before implementation, author-check the ledger against existing cap/probe,
required-child and cache contracts, then obtain one independent strategy review.

The author strategy pass and independent inspection found no P1/P2 issue. The
span wording now distinguishes payload-buffer spans from the one-byte stack
probe, including the zero-limit case.

The author matrix-to-diff pass binds the new operands to the shared two-child
walks, required-child MIR lowering, strict HIR/MIR signature checks, A07 native
shape and updated pointer escape ordinal 3. Source owners are
`limits_whole_per_unit_and_cached_lifecycle`,
`limits_source_domain_and_input_liveness`, and
`limits_abort_on_invalid_domain_before_decoding` in `compression_framing`.
Native owners are `caller_limits_bound_actual_output_growth_and_publication`
and `caller_limit_covers_all_members_and_metadata_tails`; existing frame and
framing owners retain independent malformed-wire and cleanup coverage.
The exact ABI owners named above also compare the changed exports to emitted
LLVM declarations. No type formation, ownership or interface-format cell is
silently widened by the additional scalar operand.
