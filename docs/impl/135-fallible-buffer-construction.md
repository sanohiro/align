# Fallible owned buffer construction

Status: implemented capability, independent of deferred K1 and plan 61.

Request 35's capacity query and requests 33/127's alignment are shipped. Bounded
read windows and initialized resident-image buffers still need a constructor
that reports allocator refusal before publishing an owner. This capability
adds explicit fallible construction through the existing Result error model;
it does not catch fatal errors or introduce unwinding.

## Public contract ledger

Declarations (optional final arguments are builtin arities, not general defaults):

```text
buffer.try_new(capacity: i64, alignment: i64 = 1) -> Result<buffer, Error>
buffer.try_filled(length: i64, value: u8, alignment: i64 = 1) -> Result<buffer, Error>
```

Positional calls:

```align
fn read_window(size: i64) -> Result<buffer, Error> = buffer.try_new(size, 64)
fn initialized(size: i64) -> Result<buffer, Error> = buffer.try_filled(size, 0)
```

| Surface | Exact contract |
| --- | --- |
| `buffer.try_new(capacity: i64, alignment: i64 = 1) -> Result<buffer, Error>` | One or two positional arguments; capacity is required. Pure. Success has initialized length 0 and usable read-window capacity exactly capacity. Zero requests no payload but still acquires one owned handle. No source byte copy. |
| `buffer.try_filled(length: i64, value: u8, alignment: i64 = 1) -> Result<buffer, Error>` | Two or three positional arguments. Pure. Success has length and usable capacity exactly length, with every byte initialized to value. Zero acquires no payload. Nonzero initialization is O(length), with one payload allocation; no timing/throughput/RSS promise. |
| Admission/evaluation | All supplied arguments evaluate once left-to-right before native admission. Existing exact i64/u8 source typing and ordinary inference apply. Alignment is a power of two 1..536870912; validate it first, even for zero/negative/oversized length. Then validate nonnegative count and target Layout representability, including alignment padding/limits. Invalid alignment or size returns Error.Invalid before allocation. |
| Allocation/errors | After admission, acquire the aligned payload (unless count 0), initialize it only for filled, then acquire the header using the same Rust global allocation family and Layout as canonical Box-based Drop. Either allocator refusal returns Error.Code(ENOMEM), currently 12 on supported Linux/macOS. A header refusal drops the acquired payload with its exact original layout before returning. No successful degraded window, partial owner, hidden retry or fallback alignment. |
| Ownership/lifetime | Ok owns one independent Move buffer, including inside an arena. Err owns no handle/payload. Ordinary construction, move-in/out, source nulling, return, replacement and canonical recursive Drop apply; no view is returned by a constructor. The alignment guarantee and existing byte-view lifetime/write-authority rules follow the buffer owner. |
| Subsequent operations | Existing bounded read/pread_into reuse admitted capacity and preserve its payload address. Ordinary append/put/read_line growth retains its existing terminal allocation-failure policy. Constructor success does not promise page residency, physical-memory admission, future growth, or recovery from OS process termination/overcommit failure during byte initialization. |
| Existing surface | `buffer(...)`, `buffer.filled(...)`, capacity(), their defaults and best-effort/terminal failure semantics are unchanged. The try constructors are explicit fallible operations, not deprecated aliases, implicit retries or a second optional/error model. No fallible growth/reset/truncate operation is added. |
| Native ABI | `i32 align_rt_buffer_try_new(i64 capacity, i64 alignment, ptr out)` and `i32 align_rt_buffer_try_filled(i64 length, i8 value, i64 alignment, ptr out)`. Codegen zeroes the Buffer output slot; native entry clears a nonnull output before admission, and publishes exactly once on success. Null out returns AL_INVALID without allocation. Otherwise statuses are 0 success, AL_INVALID invalid admission, AL_CODE+ENOMEM allocator refusal. Value follows the existing unsigned-byte C ABI rule: declaration/call zeroext on x86_64 and Apple ARM64, plain i8 on Linux AArch64. Pointer and integer widths follow the runtime ledger. |
| Compiler ownership | Sema owns arity/type checking and Pure effects; checked HIR authenticates the exact Result<Buffer,Error> result and every operand. MIR owns source order, normal status-to-Result construction and cleanup. LLVM only lowers authenticated MIR/native ABI. Native implementation shares one private allocation/cleanup path for both entrypoints. |
| Artifact identity | No new public type, error variant, layout or nominal identity. Generic interfaces persist source templates, not HIR/MIR bodies: replay retains both operation names and all arguments, with no interface format/version change. Existing source, compiler-binary and runtime content fingerprints separate old/new artifacts. No new type fingerprint, ambient configuration or persistent/exchanged format. |
| Prerequisites/scope | Existing Buffer Result payload/control/drop and plan 131 aligned storage. K1/plan 61, external consumer code/adoption, fallible growth and general allocator configuration remain outside scope. This is a complete usable constructor capability. |

## Representation and allocation authority

Use a distinct HIR `BufferTryNew { capacity, fill: Option<Expr>, alignment }`
returning exact `Result<Buffer,builtin Error>`. A MIR `BufferTryNew { capacity,
fill: Option<Operand>, alignment, out: Slot }` returns i32 status and writes
one Buffer slot. The lowering evaluates operands with termination guards,
allocates the output slot, then reuses `emit_status_buffer_result` for
normal Result construction. LLVM zeroes the output before the native call and lowers that operation, selecting
`BufferTryNew`/`BufferTryFilled` RuntimeKeys. Both keys are `IndirectStorage`
(no noalias return claim on their scalar status), sorted with all existing keys.
The native registry grows by two: 460 keyed, 478 base, 485 alloc-count,
482 par-map-probe and 489 maximum exports; unsigned-byte rows grow to seven.

A shared native helper validates alignment via checked conversion, power of
two and upper bound, then count via checked conversion and
`Layout::from_size_align`. It reserves exact BufferStorage payload, initializes
filled bytes without another reserve, and acquires the header with
`std::alloc::alloc(Layout::new::<Buffer>())`. Null means ENOMEM, with stack-owned
BufferStorage dropping the payload. On success `ptr.write(Buffer { ... })`
transfers both owners; canonical `Box::from_raw` Drop uses the identical layout
and global allocator. Out is cleared before validation and published only after
header initialization. There is no raw pointer escape or public allocator hook.

The existing thread-local BufferStorage allocation probe deterministically
refuses payload acquisition. A scoped cfg(test) thread-local header-refusal hook
and acquisition/free observer cover header failure and exact layout retirement;
they reset on unwind and add no production feature or exported ABI. Owners which
use existing process-global allocation counters run in bounded isolated children.
Do not infer recoverability from speculative huge allocations or process signals.

The new HIR variant must explicitly join clone/depth/finalization/substitution,
region, effects, move and borrow traversals. The variant-sweep tripwire remains
exhaustive. Buffer results are independently owned; argument-side effects and
terminations are still visited. MIR print/operands/byte-storage/loop-facts,
producer/checked-HIR validation and runtime capability analysis include the new
operation. Producer validation authenticates exact operand types, defined SSA,
Buffer out-slot storage and i32 status before LLVM. Fallible construction is
ineligible for current stack promotion, whose admission and OOM behavior differs.

## Implementation closure matrix

The named owner matrix closes the implementation. One parameterized owner may
close several cells.

| Axis | Implementation and exact acceptance owner |
| --- | --- |
| Formation/admission | `align_driver --test fallible_buffer`, `fallible_buffer_source_contract`: both arities/defaults, exact i64/u8, required size, expression receivers rejected, sibling constructor semantics. `align_mir::fallible_buffer_hir_contract` and `align_codegen_llvm::fallible_buffer_mir_contract` mutate result/size/fill/alignment, missing SSA and output-slot metadata; reject before LLVM. |
| Native validation precedence | `align_runtime::buffer_constructor_probe::fallible_buffer_admission_and_failures`: new/filled × invalid alignment × negative/oversized/zero/positive count × armed payload/header probe; invalid input consumes neither probe nor allocation. Null out returns Invalid without allocation. Positive controls consume the probes and return exactly AL_CODE+ENOMEM with null out. |
| Acquisition/publication/Drop | `fallible_buffer_admission_and_failures` plus `fallible_buffer_layout_and_window`: zero/nonzero, alignment 1/2/64/4096 and every supported alignment as an empty sentinel; payload/header refusal, retained null out and exact pointer/size/alignment releases. Nonzero success has one payload and one header, zero only header. Header refusal frees its one acquired payload. |
| Control transfers | Driver `fallible_buffer_whole_unit_control_and_cleanup`: direct return, if/match/else/?/map_err, loop/branch joins, early exits, binding, owner move/replacement; argument effects once in order and early exit from each operand skips construction. Counted acquisitions and canonical frees balance across Ok/Err. |
| Lifetime/authority | `fallible_buffer_source_contract`: views of admitted buffers remain valid within owner scope; source move, expired/stale views and readonly mutation still reject. Runtime `fallible_buffer_layout_and_window` covers growth and bounded read address/bytes. Reuse `buffer_pread_into` and existing native capacity owners for surrounding read semantics. |
| Existing constructors/growth | Reuse `aligned_buffer` driver and `aligned_buffer_invalid_alignment_precedes_every_allocation`, `aligned_storage_growth_and_adoption_match_every_allocation_layout`, `buffer_huge_capacity_degrades_to_empty_window_not_abort`, and buffer capacity native owners. New helper cannot change old best-effort or terminal behavior. |
| Generic/interface/full/per-unit/cache | Driver `fallible_buffer_whole_unit_control_and_cleanup` imports generic wrappers and a permitted Pure lifted call, roundtrips interface templates and runs both compilation paths. `fallible_buffer_cache_identity` checks cold/hit/changed alignment and operation-name edit/restore. No serialized HIR format is introduced. |
| ABI/optimization | Existing `runtime_abi_registry_is_complete_and_unique`, `runtime_effects_registry_is_total_and_structurally_valid`, `runtime_abi_extern_type_matrix_is_exact_for_every_row_and_ordinal`, declaration/effect goldens, `native_unsigned_byte_abi_matches_declarations_calls_and_cache_policy` and runtime export inventory own exact new rows. `fallible_buffer_mir_contract` pins no stack promotion and native status/out formation. |
| Resource/platform | Driver fixtures use exclusive ArtifactStage and immediately guarded bounded children. Native failpoint observers are thread-local/scoped; process-global count owners are isolated. Focused native and driver owners run locally on macOS and Linux before push; CI is the final guard. No benchmark: no throughput/RSS improvement is claimed. |

The public contract changes two coupled constructors. The expected implementation
crosses more than three compiler layers and may exceed 1,000 handwritten lines.
One capability keeps admission, native publication, Result cleanup, ABI and
consumer validation together; dormant producer/consumer splits duplicate the
allocation proof and temporarily expose no useful stable consumer.

## Source agreement and author consistency pass

Agreeing sources are draft.md, docs/language-spec.md, design-notes.md, Settled
open-questions.md, roadmap, core string English/ja, plan 19 checked-HIR records
and plan 20 runtime ABI inventory. Plan 131 remains the alignment capability
record and points here for recoverable construction. Existing construction/growth
policies remain separately explicit. Request 35's status is updated only after
merge; external adoption remains consumer-owned and the register edit uncommitted.

Every public field, type, error, evaluation/admission phase, ownership transfer,
allocation and cache identity has a row above. There is no text/native encoding,
connection/process-global state, runtime-inspection table, canonical wire format,
or new nominal/structural type fingerprint. Representation and failure owners
cover every constructor/fill/zero/alignment/refusal axis. No later milestone is
consumed. Normative positional examples pass the existing formatter/parser;
new API type checking belongs to the named implementation owners. The author
extracts exact normative obligations and checks all agreeing prose before
requesting the fresh independent adversarial strategy review. That review must
finish before implementation; the current user instruction already authorizes
this capability workflow.

## Independent strategy review

The fresh adversarial inspection against main 803c2843 was CLEAN. It checked
source agreement, exact allocation/Drop layout, status-to-Result ABI, analysis
sweeps, no stack promotion, cache/interface identity and the named owner matrix.
No production implementation preceded this review.

## Author implementation closure

The distinct HIR variant participates explicitly in replay, depth, effect,
escape/region, move/storage-content and finalization traversals. Both native
HIR admission inventories authenticate its Result equation. The storage variant
tripwire includes its fresh-owned policy. MIR guards each operand before status
formation and uses the existing Result cleanup tail; exact native-owner contracts
validate status, every input and Buffer out slot. LLVM zeroes the out slot and
applies the target unsigned-byte attributes. Local-use MIR owners retain the
fallible call where ordinary buffer stack promotion would otherwise apply.

The native helper validates before either allocation, acquires payload then
header, and immediately guards the initialized header with canonical Box
ownership before publication. Thread-local refusal/observation scopes own both
allocation sites and exact layout retirement, including zero-only handle failure
and payload cleanup after header refusal. Native bounded-read/growth tests observe
address, capacity, bytes and allocation events. The two public entrypoints share
this one helper; ordinary constructors are untouched.

The generated whole/per-unit owner counts exactly 44 successful handles and
exactly 44 matching frees across imports, lifted/generic calls, arenas, control,
replacement and errors. Its explicit status-17 ABI fixture checks Code(12)
conversion separately from native allocator-site failure injection. Native
allocator refusal is not inferred from that fixture. Invalid arity/type/view/move
programs reject; a function-return view owns the expired-owner negative (ordinary
block-valued binding lifetime extension remains valid). Interface/cache owners
cover cold/hit/alignment-change/operation-change/restore. HIR/MIR mutation owners
refuse every altered operand/status/output equation before LLVM. ABI declarations,
effects, target attributes and native export inventory close both new symbols.
There is no performance benchmark because no performance improvement is claimed.
