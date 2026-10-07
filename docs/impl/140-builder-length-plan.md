# Nonconsuming lengths of unfinished builders

Plan 140. The scalar-observation boundary received one fresh independent
adversarial plan review with no findings before implementation (2026-10-08).
Request 28 supplies a real need to observe accumulated progress. Add the same
length observation to both existing builder families. This is independently
useful for counters and helper admission, while the broader indexed/view-reader
request stays pending. Keep array_builder's settled no-views-before-freeze
strategy, current type/placement/borrow rules, and K1/plan61 deferrals.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Text length | `b.len() -> i64` on `builder`; exactly zero arguments, no defaults. Returns the number of initialized UTF-8 bytes already appended, including embedded NUL bytes. Counts bytes, not Unicode scalars or reserved capacity. Construction starts at zero. Every existing write kind contributes the bytes it actually emits; no new encoding/formatting rule. |
| Typed length | `b.len() -> i64` on every currently admitted `array_builder<T>` form; exactly zero arguments. Returns total initialized elements, across all region chunks, independently of reserved capacity and element stride. Applies to scalar/record, vector, mask, fixed-array and fixed-struct-array builder types, including admitted zero-width region elements. No element-domain or allocation-mode admission changes. |
| Receiver/evaluation | A stable bound local or direct named-function parameter of the exact builder family, observed once and not consumed. Both immutable and mutable bindings may be read, as may ByValue, Borrow and BorrowMut parameters when those types/modes are already admitted. Unbound temporaries and control-result receivers must be bound first, matching opaque buffer observations. No aggregate/field/capture/function-value admission is added. Validate the receiver expression first, then zero arity, then the bound place; failed new-query formation returns Error rather than a valid typed node. |
| Effects/result/lifetime | Pure, nonconsuming, read-only in-memory observation. Returns a Static Copy i64 with no dependency after the observation; it may outlive the builder. No retained view, reservation beyond the scalar operation, buffer exposure, implicit clone, allocation, growth, traversal, compaction or I/O. Multiple shared reads may overlap; no read may bypass existing exclusive-call/ownership rules. |
| State transitions | Query does not alter length, capacity, data, chunk links, allocation mode, header ownership or any payload. Push/append/write continue afterward; build/to_string still consume and transfer existing contents. Move, replacement, source nulling, Drop and shared-borrow mutation rejection retain existing behavior. Previously returned scalar lengths remain ordinary values. |
| Errors/limits | No Result or recoverable error for a valid representable length. Native conversion uses checked usize-to-i64; an unrepresentable initialized count terminates with the existing allocation-size hard error before mutation. This matters for zero-width region elements, whose count is not bounded by backing byte size. No wrapping/truncation/saturation. Constructor and growth policies are unchanged. |
| Native ABI | `align_rt_builder_len(*mut Builder) -> i64` and `align_rt_array_builder_len(*mut ArrayBuilder) -> i64`; new RuntimeKey BuilderLen/ArrayBuilderLen, both existing A29. Null returns zero. A nonnull pointer must designate a live, correctly aligned owner header with valid native representation; reads may share access and no mutation/free may overlap. Invalid/dangling pointers are unsafe-precondition violations. Read only producer-owned Builder.buf.len or ArrayBuilder.len, with checked result conversion. |
| Native effects | Both keys use conservative IndirectStorage, ArgMem Unstated, params [], escapes [0], release None, fresh false, divergence false, like BufferLen. No optimistic LLVM attribute or new effect class. Source purity and native optimizer effects remain separate. |
| Compiler/artifact owner | Source and checked-HIR independently authenticate the exact receiver family, stable local and i64 result. Explicit HIR/MIR BuilderLen and ArrayBuilderLen operations preserve the observation through all visitors, effects, hashing, printing and validation; LLVM only lowers MIR through the canonical ABI. Existing source-template generic rechecking, nominal type identity and compiler cache namespace apply. No new type, serialized interface field/tag or external file format. |
| Optimizer/ownership | Preserve existing heap/stack/arena header selection and deliberately boxed borrowed-call ABI. A length read is a barrier to builder write fusion and other movement across mutation. Include it in appropriate nonescaping-read visitors without admitting arbitrary escapes. No performance/latency claim; allocation-free read and unchanged header selection have direct invariant owners. |
| Platform/prerequisites | Existing text/typed builder and borrowed-parameter implementations on main, plans65 C and82. Linux x86_64/ARM64 and macOS support the same i64 result; whole-program and per-unit/imported/generic callers agree. No new package, ambient input, target option, persistence or CLI switch. |
| Acceptance/deferred | Query empty/reserved/partially filled/grown owners and then finish exact contents. Cover all five typed families and both allocation modes. Scalar progress observation ships as one useful capability. Indexing, byte/slice views, reset, aggregate placement, additional captures, full Request28 consumer migration and K1/plan61 are deferred. |

## Implementation closure matrix

| Axis | Implementation and exact owners |
| --- | --- |
| Source/type formation | All five array-builder variants dispatch via array_builder_element/is_array_builder, plus text Builder. Zero arity and stable receiver independently checked. `builder_length::formation_and_ownership` owns positive shared/exclusive/ByValue reads and wrong arity/type/temporary/moved cases. Generic element formation retains original heap/RegionPlain gates. |
| Construction and native observation | `builder_lengths_observe_initialized_prefix` queries heap/stack text headers, empty/reserved state, mixed UTF-8/NUL/scalar writes, and heap/region typed headers across capacity and chunk growth. Observe exact total count and unchanged header/payload bytes/pointers/capacity/chunk identity; use immediate RAII owners. Native null and i64 representability boundaries use isolated bounded hard-error probes for overflow. |
| Payload and sibling type closure | `builder_length::all_element_families_and_allocation_modes` covers Copy scalar/record, heap owned string/Move record, region views/records, vec/mask/fixed/fixed-record, and zero-width region record containing a fixed-zero field. Query never inspects or copies element payloads. Existing builder allocation/recursive Drop owners close payload cleanup. |
| Move-in/out, nulling, replacement, Drop, return | `builder_length::formation_and_ownership` and existing text_builder_params, m12_array_builder, array_builder_transfer owners retain helper borrowing, owned transfer, replacement/finish and exact cleanup. A returned scalar survives owner Drop. Borrowed owners still cannot finish/escape; no field/capture admission. |
| Control/ordered evaluation | `builder_length::observations_cross_control_and_units` covers if/match/else/?/map_err, branch/loop joins, early exits and imported helper calls. Query before/after mutation and replacement must observe exact counts. Ordinary same-call alias restrictions remain. |
| Optimizer barrier/header ABI | An exact MIR owner observes write(str)/len/write_int/len/write(str), plus adjacent-fusion control; runtime output must retain both intermediate lengths. Existing stack-header and borrowed-builder boxed-call owners gain length observations. No new header escape merely because a query is present. |
| Malformed checked HIR/MIR | `builder_length_hir_rejects_forged_receiver_place_and_result` independently forges every family/result/place mismatch through all checked lowering entrypoints. `builder_length_mir_requires_exact_owner_and_i64` rejects wrong operands/results before LLVM. Exhaustive new-variant tripwires and explicit sibling inventory close effects, provenance, replay clone, borrow/move/escape, generics, hashing and traversal. |
| Runtime ABI | Typed native signatures plus key/effect/declaration/export inventories and A29 golden owner cover both keys. No new layout; conversions and null handling must agree with the ledger. |
| Generic/interface/cache | Existing source-template and nominal identity; imported generic and whole/per-unit execution through builder_length owner plus existing interface suite. Cache-reuse execution must keep exact outputs. No schema bump unless implementation reveals a real serialized shape change; revise the ledger first in that case. |
| Resource policy | Getter itself performs no allocation/copy/mutation. Use current native allocator-observation machinery only with its existing isolation discipline; avoid a process-global probe in concurrently running tests. Direct header/pointer invariants and existing allocation parity cover both construction representations. No timing benchmark required. |

## Author consistency / capability boundary

Two scalar queries add no borrowed storage or new ownership strategy. The exact
receiver, count units, error precedence, signed-size conversion, null ABI,
allocation and source/native effect rules above cover every new argument/result.
No text or wire input crosses a new boundary. No canonical persisted format,
process-global state, configuration input or structural fingerprint is added.
The whole useful capability includes both sibling queries, all IR/ABI producers
and their consumers. Mechanical exhaustive sweeps may exceed 1,000 handwritten
lines; splitting dormant IR/native pieces would duplicate validation and leave
no useful consumer, so retain one end-to-end capability if that occurs.

The independent plan/boundary review is complete. Implementation must satisfy
the matrix-to-diff pass, one fresh committed-candidate review and ordinary gates.
Required normative agreement: draft.md, docs/language-spec.md, docs/design-notes.md,
Settled open-questions, relevant roadmap capability status, core-design/string.md
and array-slice-pipeline.md plus matching ja mirrors, HIR validation ledger19 and
runtime ABI ledger20. Update HANDOFF once at completion. The external request
register gets only Align's partial shipped answer, uncommitted; no consumer code,
branches, tests, pins or adoption work.

## Implementation qualification

The matrix maps to explicit HIR visitors in align_sema, checked-HIR validation,
MIR lowering/producer contracts and printing, canonical runtime keys/effects,
LLVM lowering/header selection, and the two runtime count getters. The source
owner is `crates/align_driver/tests/builder_length.rs`; the native owner is
`crates/align_runtime/src/builder_length_tests.rs`. The malformed-HIR, MIR,
write-fusion and header owners named above live beside their implementation.

Five new driver owners and 95 existing text/typed/region/transfer tests pass
(one isolated transfer helper remains ignored). Source programs cover ordinary
positional calls, imported RegionPlain generics, whole/per-unit execution and
cache hit/edit/restore. Pure effects and a dependency-free result are asserted;
scalar observations used as arguments to writes/pushes retain source ordering.
Native owners pass with thread-local allocation observation and an active
allocation witness, including empty/reserved/grown headers, mixed UTF-8/NUL
writes, multi-chunk counts, zero stride, signed-count limits and bounded
kill/reap of an intentionally stalled overflow child. Substituting reserved
capacity for initialized text length fails the native observation owner.

The admitted zero-width source owner is a region record containing `[i64; 0]`.
Standalone fixed-zero-array builder canonicalization is a preexisting separate
rejection, and unit/empty-record literal forms are not admitted; no type-domain
repair is part of this capability. ABI export/signature checks cover exactly
480 base, 487 alloc-count, 484 par-map-probe and 491 maximum symbols.

No new executable documentation examples or interface fields are introduced.
The English and Japanese library contracts, specifications, HIR/native ledgers
and Settled record agree. Assessment, author obligation extraction, mutation
control and independent review evidence are retained under
`.git/builder-length-assessment/`.
