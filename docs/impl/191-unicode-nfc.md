# Explicit Unicode NFC normalization (voice V004)

## Evidence and capability boundary

Voice's pinned Qwen2 tokenizer normalizes to NFC. Five retained decomposed-text
cases otherwise produce different token IDs; its current application owns
Unicode 16.0.0 tables, an 8192-byte cap and a buffer-plus-clone implementation.
All three consumer registers were reconciled. LLM's Unicode trim requirement is
a view-provenance request, not normalization; media has no NFC request. Consumer
adoption and original tokenizer parity remain external.

The settled library direction places explicit normalization in `pkg.unicode`,
outside core/std byte semantics. This capability supplies one complete NFC
operation. It changes no equality, ordering, implicit conversion, syntax, locale
or error model. Other normalization forms and grapheme/case/locale operations
remain unselected. The algorithm follows Unicode 16 UAX15 revision 56.

## Public-contract ledger

| Surface | Exact contract | Owner / acceptance |
| --- | --- | --- |
| `pkg.unicode.version() -> str` | No arguments/defaults/errors/allocation. Returns static UTF-8 `16.0.0`, the normalization data version, independent of OS, locale and process configuration. | Canonical source wrapper; source export/signature/value owner. |
| `pkg.unicode.nfc(input: str) -> Result<string, Error>` | Pure explicit NFC transform. Input is a borrowed valid UTF-8 view, including embedded NUL, noncharacters and unassigned scalar values; no replacement decoding. No default, arbitrary input cap or stream-safe insertion. Uses canonical decomposition, canonical ordering, canonical composition with full composition exclusions, and algorithmic Hangul. | Native conformance and source values/transport owners. |
| Output / ownership | Success owns an independent UTF-8 string, even for already-normalized nonempty input; input/arena storage is never retained. Empty success is canonical `{null,0}`. Ordinary Move, replacement, active Result cleanup and string Drop apply. No implicit source copy is introduced. | Counted native lifecycle and source input-escape/replacement/control owners. |
| Allocation / errors | Scratch and final string allocation are fallible. Every count, multiplication, layout and output byte extent must fit target `usize`, `isize::MAX` and source i64 as applicable. Unrepresentable extent returns existing `Error.Invalid`; allocator refusal returns existing `Error.Code(ENOMEM)` (host native ENOMEM). No hard-OOM allocation, heap growth, hidden final clone or locale setup occurs inside the transform. Empty input allocates nothing. | Checked extent helper and injected allocation-failure owner at both phases, zero outstanding storage. |
| Data identity | Unicode 16.0.0 is compiled into the runtime with source SHA256 and Unicode license. Updates are explicit source changes and may change version/result together. Normal builds/tests use no network, OS Unicode library or ambient Python Unicode database. | Deterministic offline generator, independent official conformance fixture and data identity owner. |
| Package / compiler | One vendorable `apps/unicode/pkg` subtree: public `pkg.unicode` has exactly the two functions above and imports empty `pkg.unicode.internal.descriptor`. The canonical `nfc` body delegates there. Only this canonical wrapper can invoke the private compiler bridge. No public native helper or application-declared `align_rt_*` entry. | Canonical AST admission, internal import/extern rejection; direct, imported, function-value and cached whole/per-unit owners. |
| Artifact identity | Ordinary source/interface function signatures and compiler/runtime artifact identity include the changed implementation. Unicode version and table bytes are runtime source inputs. No new persisted type or wire format is added. Internal HIR/MIR gain one dedicated operation; compiler identity invalidates private artifacts. | Existing source/interface/cache closure plus warm/cold source owner, runtime-LTO executable. |
| Prerequisite / sources | Existing Move string, Result, sealed package bridge and runtime ABI registry. No later milestone or external native dependency. This ledger owns implementation; pkg-design/unicode.md and ja mirror own package usage. draft.md, language-spec.md, design-notes.md, open-questions.md, Plan19 and Plan20 must reflect the exact new surface and private operation. | Author extraction pass and one independent strategy review before implementation. |

## Native ABI and total validation order

`i32 align_rt_unicode_nfc(AlignStr *out, const u8 *input, i64 input_len)`.
`AlignStr` uses the existing native `{pointer, i64 length}` layout and string
allocator/Drop family. Return 0 on success, AL_INVALID on invalid metadata,
UTF-8 or representability, AL_CODE + ENOMEM on allocation refusal. The native
key is private; LLVM declares `(ptr, ptr, i64) -> i32` and MIR owns status-to-Result
construction. The native operation reads input, writes out and acquires fresh
storage without retaining input or touching process-global state.

The unsafe caller supplies writable output storage for every nonnull aligned
non-wrapping output span and, after metadata admission, initialized readable
input bytes. Output contains no live owner. No Rust reference may forbid writes
to output even on an overlap rejection. Source calls always allocate a separate
output slot. These pointer-liveness obligations cannot be inferred from addresses.

Ordered phases (a later failure cannot supersede an earlier one):

1. Reject null, misaligned or wrapping output metadata without dereference.
2. Write canonical zero output before semantic admission; it remains zero on
   every later error. Check input nonnegative/target-representable length,
   nonnull when nonempty, non-wrapping address span and disjointness from output.
   Null/zero is valid empty. Zero-length input needs no readable allocation.
3. Validate complete UTF-8 before allocating or constructing scalar references.
   Embedded NUL is ordinary text. An overlapping input is not read; output zeroing
   may overwrite it under the explicit unsafe precondition above.
4. Count exact fully decomposed scalars with checked arithmetic and admit the
   scratch layout. Empty succeeds immediately. Allocate scratch fallibly and
   arm its unique cleanup owner before any further fallible step.
5. Fill exact scratch, canonically order each nonstarter run using allocation-free
   sorting by `(combining_class, original_ordinal)`, and compose in place. Original
   ordinals keep equal classes stable; composition obeys starter/blocking rules.
6. Count exact final UTF-8 bytes with checked arithmetic. Allocate final storage
   fallibly and arm cleanup. Encode every byte before publishing `{ptr,len}`.
   Retire scratch on success or failure; transfer final storage exactly once.

Generated canonical mappings are fully recursively expanded (maximum four
scalars in this pinned data); Hangul is algorithmic. Sorting uses the standard
allocation-free unstable slice sort with the ordinal tie-breaker, not an
insertion-sort unbounded run. Scratch and output use the existing malloc-compatible owned allocator family. A
private fallible helper centralizes successful allocation instrumentation; the
existing `align_rt_alloc` wrapper keeps its unchanged hard-OOM behavior. Both
new owners free through the existing counted `align_rt_free`. Generated tables
are Rust source, so the existing runtime source identity includes their bytes.
Official UCD inputs, conformance fixture, source hashes and license are committed;
`python3 scripts/generate-unicode.py --check` independently reproduces the tables
and authenticates input hashes without network access.

No throughput/complexity improvement or allocation
count is a public performance promise; no benchmark is required.

## Implementation closure matrix and exact owners

| Axis | Implementation authority | Exact discriminating owner |
| --- | --- | --- |
| Type formation / malformed source | Canonical package AST and private descriptor checker; `str` input, builtin `Result<string, Error>` output. No new type variant. | `align_driver --test pkg_unicode`: `unicode_package_admission` parameterizes bad arity/type, forged package/helper/import/extern and altered signature/body. |
| HIR formation / all pass consumers | Dedicated `UnicodeNfc { input }`, exact Str child and Result type; borrowed child visit; fresh owned result, no retained input; Pure with explicit fresh heap ownership; ordinary heap allocation remains allowed in Pure closures. Existing new-variant tripwires require complete pass coverage. | `align_mir --lib unicode_nfc`: `unicode_nfc_checked_hir` mutates child/result/checked facts; `unicode_values_and_transport` Pure heap-transform closure positive and `unicode_package_admission` non-Send region-capability negative, plus existing variant-sweep owners. |
| MIR / ABI / malformed private records | Dedicated status-producing `UnicodeNfc { input, out }`; exact Str operand, String out slot and i32 result even when unused or in partition peer; call only after input completion. Typed runtime key, effects and private symbol registry. | `align_codegen_llvm --lib unicode_nfc`: `unicode_nfc_mir_validation` mutates input/out/result/slot and unused/peer placement. `align_codegen_llvm --lib runtime_abi` declaration/type/effect owners and `bash scripts/test-runtime-abi-exports.sh` native export/signature audit. |
| Construction / moves / Drop / replacement | Existing Result/string transfer and source nulling, unique scratch/output RAII, no input mutation for admitted disjoint spans. Ordinary allocator provenance and arena independence. | `align_runtime --lib unicode_nfc` with `alloc-count`: `unicode_nfc_allocation_lifecycle` exact acquisition/free/failpoint counts and source input sentinels; `pkg_unicode::unicode_values_and_transport` and `unicode_lifecycle` source witnesses. |
| Native metadata / errors / bounds | Ordered phases above, no forged references before admission; checked scalar/layout/byte counts. | `unicode_nfc_native_admission` parameterizes null, negative, overflow, alignment, overlap, malformed UTF-8 and multi-invalid/failpoint precedence. Direct pure extent-helper limits test representability without huge allocation. |
| Unicode semantics / data | Offline generator from official UnicodeData and full exclusions in DerivedNormalizationProps; flattened canonical decomposition, CCC and composition pair tables. | `unicode_nfc_conformance`: all 19,965 official NormalizationTest rows, all five NFC columns (99,825 comparisons); all Unicode scalars absent from Part1 normalize identically, satisfying header clause 2. Expected data comes from the independent official NormalizationTest, not implementation tables. `unicode_nfc_data_identity` pins table structure; `scripts/generate-unicode.py --check` pins source hashes and exact generation. |
| Control / authority | if/match/else/?/map_err, loop joins, early/implicit return, eager child divergence use existing Result cleanup; string views retain their actual output owner. | `pkg_unicode::unicode_values_and_transport` and `unicode_lifecycle` cover success/error cleanup, result replacement, bound/temporary/arena source and returned independent string; `unicode_package_admission` rejects escaped result view and consumed owner reuse. The existing driver `else_result` target is reused for unchanged Result control. |
| Generics / interfaces / calls / caches | Canonical ordinary wrappers retain standard signatures through direct/imported/generic/indirect calls, whole-program/per-unit and warm/cold caches. No raw table id persists. | `unicode_values_and_transport` runs both modes with imported generic/helper and function values, and cold/hit builds. One runtime-LTO executable uses the same native symbol/table ownership. |
| Resource bounds / concurrency | No retained/global native state or locale; per-call scratch/output owners. Conformance can invoke internal helper without leaking processglobal failpoints; allocation probes use existing isolated child harness. | Native owner input >8192 and long nonstarter/equal-CCC/Hangul/NUL/unassigned cases; isolated allocation owner and bounded driver harness. |

The complete package-to-native path may exceed 1,000 hand-written lines because
one operation needs independently checked compiler passes, native safety,
conformance and ownership. Splitting this strict producer/consumer chain would
leave a dormant primitive or an unusable wrapper and duplicate review/gates.
Generated Unicode tables and official conformance data are separate mechanical
inputs. Deliver one useful capability PR; consumer adoption remains pending.

Independent preimplementation strategy review completed with no findings. The
author corrected the parallel owner to preserve ordinary Pure heap allocation
before review completion. No production implementation preceded this review.

## Implementation closure

The native owner passes every official NFC column and the full scalar complement,
including long nonstarter runs, canonical blocking, exclusions, Hangul and NUL.
Both allocation phases use the shared fallible owned allocator, exact acquired/free
counts and zero requested live bytes. Source owners cover direct/indirect/generic,
whole/per-unit, cold/hit and runtime-LTO execution, arena-independent output,
replacement, early error/return, Pure heap closures and retained region rejection.
The documentation example owner checks both language mirrors.

Malformed HIR authenticates canonical origin, signature, exact parameter body,
result and cleanup facts. MIR authenticates borrowed Str input, owned String output
and i32 status even when the result is unused or no function is selected for a
partition. The runtime-key extern matcher rejects even the exact source prototype
at native ABI admission; source parsing/type checking alone is not that gate.
The registry retains its existing curated read-parameter policy: only input gains
readonly, both pointers are noncapturing and overall memory effects stay unstated.
No new mutable-parameter attribute rule or native effect class is introduced.
