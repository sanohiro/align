# Fallible raw allocation and non-owning carriers

## Evidence and boundary

Media AM-R15's current `gpu.internal.create` and `gpu.internal.storage`
allocate private metadata with checked C malloc/free and explicit rollback.
Their requested `Result<raw, Error>` is refused, as are raw Option, tuple,
sum payload and ordinary record-field twins. A C-record wrapper already works.
All three consumer registers were reconciled. This is current evidence for
LLM R34's raw carrier request; it does not revive retired ggml adoption.
Voice has no additional allocator request. Consumer adoption remains external.

This is an implementation capability under the existing Copy/Static raw,
explicit unsafe and single Result error models, not a locked-language reopening.
Plan190's product/carrier composition is the precedent: admit the complete
transport family, preserving independent collection/box/FFI restrictions.
K1 is not a prerequisite. AM-R07's externally owned writable views still need
their separate authority, initialization, lifetime and I/O contract.

## Public-contract ledger

| Surface | Exact promise, owner and acceptance |
| --- | --- |
| API | `raw.try_alloc(size: i64) -> Result<raw, Error>`, exactly one argument, no defaults, in `unsafe`. Check the operand once in source order with exact i64 context; an already-invalid operand remains invalid. No integer-width conversion, ambient policy, retry or new error type. Sema and independent checked HIR own signature/unsafe/effect replay; `fallible_raw` checks literal, typed, effectful, diverging and invalid arguments in both frontends. |
| Admission and result | Negative or target-unrepresentable size gives `Error.Invalid` before allocation. Zero gives `Ok(raw.null())` without allocation. Positive size through `isize::MAX` attempts one existing malloc-family allocation; refusal gives `Error.Code(12)` (ENOMEM); success gives one non-null pointer to `size` uninitialized bytes. No host exhaustion test, residency/overcommit guarantee or implicit initialization. `align_runtime::fallible_raw` injects refusal at the actual allocator and checks no-attempt invalid/zero paths, retry and explicit free. |
| Alignment and lifetime | Same malloc-family alignment as `raw.alloc`, at least eight bytes on the supported 64-bit targets; no over-aligned promise or alignment parameter. The caller owns all unsafe effective-type, initialization, bounds, lifetime and exactly-once release obligations. `raw.free` is the matching allocation family. A positive successful pointer may instead transfer to an existing declared resource with its matching free hook. Zero is null and cannot be transferred to non-null resource construction. |
| Carrier formation | `raw` in Option, both Result positions, concrete/generic user-sum payloads, existing flat tuple elements, and direct or recursively tagged ordinary record fields. Existing other-element grammar, inline-cycle checks and nominal identities remain. Ordinary record composition may use already-admitted record containers; merely adding Scalar::Raw does not admit naked raw array/slice/box/vec/mask/builder/task-result/JSON/global or foreign carrier signatures. Sema and checked HIR enforce the same contextual placement; positive/negative twins own this boundary. |
| Ownership and operations | Raw is Copy and Static and owns no pointee. Carrier copying, replacement, extraction, return and Drop never free, copy or null a pointee and infer no lifetime/non-null/no-alias/read/write authority. Mixed existing Move/view members keep their existing cleanup and provenance. Pointer transport adds no allocation and requires no additional unsafe operation; raw creation, allocation, dereference, free and foreign calls keep their existing unsafe/effect rules. No comparison, numeric conversion or implicit clone is added. |
| Control and function values | Existing if/match/else/?/map_err, branch/loop joins, early exits, eager divergence and direct/indirect calls preserve their control and cleanup rules. Raw error discard never frees the pointee; it remains the programmer's explicit duty. Exact native counters own partial resource construction, failure rollback, retry, active/inactive variants and manual final free. Shared raw transport does not authorize safe pointee access. |
| Representation | Scalar::Raw reversibly maps to existing Ty::Raw and uses its pointer size/alignment and LLVM ptr. Existing tags, payload layout and inactive-payload rules remain. New HIR RawTryAlloc carries one size expression and exact Result<Raw,builtin Error>; one semantic MIR Result operation owns the outcome. Native out-slot/status details stay private LLVM lowering. No new owning type or cleanup flag. |
| Private runtime ABI | `i32 align_rt_raw_try_alloc(i64 size, ptr out)`, one aligned writable pointer cell at non-null out. Null out returns status 2 without allocation; otherwise clear out before size validation. Status 0 means the documented zero/success result, 2 means Invalid, 17 means Code(12); failure leaves out null. No other status is produced; LLVM rejects an impossible status through the existing invalid-runtime-result failure path. IndirectStorage return, conservative Unstated argument-memory effects; no noalias/readnone claim on the status result. Registry, runtime exports/signature audit and generated ABI goldens own this exact row. |
| Persisted identity | Canonical MIR type-graph v3 adds scalar tag 63:u8 with no trailing field, mapping to Raw; existing Ty::Raw tag 21 is unchanged. All other tags/widths/orders remain. The existing header is version:u8, definition-count:u32 little-endian, then ordered definitions and finally the root; Option<Raw> is `[3,0,0,0,0,4,63]`. Independent semantic-to-byte and byte-to-semantic goldens own this vector and a nonempty-definition `Option<(raw,bool)>` graph, nested carriers, changed leaf identity, truncation/trailing and unknown tag rejection. Existing structural interface Raw and generic source reconstruction require no format bump; compiler identity invalidates artifacts. Cold/hit/change/revert imports qualify both frontends. |
| Prerequisites and source agreement | Uses shipped raw operations, resource hooks, tuple/tagged carriers and existing fallible allocator family. No new milestone or performance claim. This plan owns the extension to Plan137's ordinary raw-field restriction. Update draft, language-spec, design-notes, Settled, Plan17 formation/codec, Plan19 HIR, Plan20 runtime ABI, core option-result EN/ja and unsafe/FFI guide EN/ja. Record capability status once in HANDOFF and Plan230; provider answers stay separate from adoption. |

The allocation alignment follows the [POSIX malloc contract](https://pubs.opengroup.org/onlinepubs/9799919799/functions/malloc.html).
The eight-byte supported-target bound is also checked by the native owners,
including sizes smaller than eight; no stronger alignment is inferred.

## Implementation closure matrix and owner checklist

| Axis | Implementation and exact discriminating owner |
| --- | --- |
| Type formation and validation | Scalar/Ty conversions, concrete/generic enum payloads and tuple/field admission, constructor replay and complete type graph. `align_driver --test fallible_raw` covers every carrier family and its nearest forbidden collection/FFI/JSON placements, including generic instantiation and scan/to_array/literal/box/builder producers. `align_mir --lib fallible_raw` mutates one checked-HIR property at a time and checks RawLoad cannot use the new Scalar to bypass RawPointerLoad or C-only struct storage; body-only array/slice/builder/box and raw literal mutations also reject without relying on a record-field placement gate. Existing c_layout_raw_fields positives remain; its ordinary-field negative becomes a positive in the new owner. |
| Copy, move-in/out, nulling, Drop, replacement and return | Explicit Raw leaf in ownership/region/drop/layout classifiers; existing composite algorithms traverse other elements unchanged. Driver `fallible_raw` returns static native pointers through every carrier, copies and replaces them without automatic free; mixed resource/view controls retain exactly-once cleanup and escape rejection. Reuse tuples, tuple_carriers, owned_tagged_payloads, else_result and unsafe_raw cumulative owners. |
| Control and inference | Source owner crosses if/match/else/?/map_err, branch and loop joins, both active tags, early exits and partial eager construction. Include expected type present/absent, immediate/diverging operand completion, source-order effects and clean/already-invalid arguments. Existing joins are reused, not redesigned. Borrowed local/arena views inside mixed carriers retain their rejection owners. |
| Generic, interface and cache | Imported public fields/sums and generic raw transport; direct/indirect callbacks and map_err. Driver cold/hit/leaf-edit/revert executions prove actual values; the same owner round-trips each actual structural interface, including Raw under tagged/product positions. `align_mir --lib fallible_raw` owns exact scalar-63 canonical bytes and malformed unknown tags. |
| Atomic MIR and native boundary | HIR variant inventory/visitor sweep includes every operand, effect, type and provenance pass. MIR validation authenticates size/result/error identity and runtime dependencies before lowering. `align_codegen_llvm --lib fallible_raw` checks exact call ABI, output initialization, status conversion and malformed operation refusal, alongside existing runtime registry/effect owners. No source-visible temporary pointer slot or conditional initialization proof is introduced. |
| Refusal, rollback and allocation provenance | Runtime `fallible_raw` uses a scoped thread-local failpoint at the existing allocator attempt; restore its prior state on Drop and exclude nested scopes. Size admission, null out, dirty out, zero, first refusal, later retry and alignment have deterministic owners. Driver C observation may intercept the ABI to force source failure mapping, but that is distinct from the runtime owner proving real allocator refusal. Source whole/per-unit resource acquisition counts close partial rollback, retry and explicit/manual/free-hook release; no allocation is hidden by a carrier. |
| Platforms and test ownership | Reuse the bounded owned_fixture harness and exclusive artifact directories. Run actual generated pointer-carrier/allocator objects on macOS ARM64, Linux ARM64 and Linux x86_64; the Linux C probe wraps malloc and the macOS runtime owner uses the scoped allocator failpoint; no GPU/model/consumer adoption required. Shared ABI changes get matching local verification before push, followed by one independent code review and final-SHA preflight. |

This capability may exceed 1,000 hand-written lines because one discriminator
must cross formation, validation, layout, codecs and native Result construction.
The allocator and its usable carrier form a strict producer/consumer chain;
splitting them would duplicate safety proof and leave no requested fallible raw
API. One complete capability has lower integration risk. No benchmark is required:
allocation parity is correctness evidence, not a performance improvement claim.

Before implementation, the author checks every row against existing source and
all three requests, then obtains one fresh independent adversarial plan review.
After implementation every applicable cell must point to production and its
owner, or an explicit boundary above. No broad review is used to discover the
implementation. Consumer code, pins, branches and verification remain untouched.

## Preimplementation review resolution

The independent review found one P2: the initial codec prose reversed the
existing definition/root order. The ledger now fixes header, definitions, root
and requires a nonempty-definition golden. `Option<(raw,bool)>` has exact bytes
`[3,1,0,0,0,2,2,0,0,0,63,2,4,62,0,0,0,0]`: one tuple definition,
two scalar members Raw/Bool, then Option of tuple node zero. This agrees with
the existing independent Plan190 tuple golden and codec order. No production
edit preceded resolution; no other plan finding was reported.

## Author closure and qualification

The author matrix-to-diff pass closes the complete raw transport family through
existing structural ownership and provenance algorithms. Raw remains a Copy,
Static, non-owning leaf; the collection classifiers explicitly reject it rather
than letting the expanded carrier classifier widen collection storage. HIR and
MIR authenticate the allocator's exact i64 and builtin Error types independently;
malformed operations fail before LLVM functions are published.

`fallible_raw_values_and_transport`, `fallible_raw_rejects_invalid_domains`,
`fallible_raw_generic_cache` and `fallible_raw_partial_cleanup_and_retry` close
the source matrix in whole/per-unit modes. The last owner observes eight successful
allocations, eight matching frees and two refused allocations, including resource
pair construction, eager failure, mixed raw/resource fields and tuple extraction.
The canonical and checked-HIR `fallible_raw` owners close formation and exact
bytes; `fallible_raw_native_result_and_malformed_mir` closes native Result layout
and malformed MIR. Existing unsafe_raw, c_layout_raw_fields, tuples,
tuple_carriers, owned_tagged_payloads and else_result owners retain their controls.

Actual emitted allocator/carrier programs passed on macOS ARM64, Linux ARM64
and Linux x86_64. The checked-in `raw_allocator_probe.c` additionally wrapped
malloc on both Linux targets to force actual refusal, then retry and release
seven sizes with the eight-byte alignment check. macOS's runtime owner covers
that actual allocator refusal through a scoped thread-local failpoint, including
another thread's independence and restoration. No probe exhausts host memory.
Native artifact hashes and exact executions are retained in the capability
receipt; consumer integration remains outside this provider qualification.

## Code-review closure

The independent code review identified the new leaf's unintended admission by
`scan` and body-only collection validation, two stale ExprKind inventory counts,
and stale Raw entries in Plan17's exact type-placement table. The correction
closes the collection producer and body/type/literal validators together, updates
both complete variant inventories to 360 after the visitor sweep, and reconciles
Plan17's Raw producer sets and field/enum/tuple rows. A parameterized source owner
covers collection-producing contexts in both frontends; malformed body-only HIR
and accepted raw carrier twins distinguish placement from transport. The audit
of every `ty_to_scalar` use retains independent builder, const, task, sort, JSON
and FFI domain gates. No public surface, IR shape or ownership strategy changes.
