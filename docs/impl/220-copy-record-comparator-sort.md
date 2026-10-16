# Comparator sorting for independent Copy records

Status: implemented; owner verification and preflight review close this capability.

[Plan221](221-copy-record-sort-keys.md) subsequently admits the same independent
Copy record elements to keyed sorting. The original capability boundary below
records its own scope; the key ownership and record representation proofs remain.

LLM Request27 explicitly asks for a composite Copy key without packed scalar
encoding. Plan208 delivers scalar comparators but excludes all record elements.
Extend that same terminal to the existing independent inline Copy record class
shared by Plans182/205/207/209. A numeric span record can be compared against
captured text without storing a view in each element. All three consumer request
registers were reconciled at their unchanged current revisions; media has no
sort caller and voice's `assets.align` uses existing scalar/text sorts. Consumer
adoption, code and compiler pins remain external.

The boundary deliberately uses an existing semantic class: independent data,
not arbitrary `!is_move`. View-bearing or callable-bearing records need separate
interprocedural access authority and remain excluded. This does not resume
Plan61/K1 or define a consuming collection lifecycle. Existing scalar `str`
comparators retain their already-shipped contract. Ordinary `sort` and
`sort_by_key` retain their domains and algorithms; the new useful consumer is
one existing two-argument comparator terminal.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Signature and domain | `source.sort_by(cmp) -> array<T>`, exactly one required comparator, no defaults. Preserve the existing int/float/bool/char/str domain. Additionally T may be an already-formed ordinary record with no explicit `align(N)`, whose complete reachable graph passes `independent_copy_type`. This is the existing closed graph of numeric/bool/char/unit/RNG leaves and supported vectors/masks, records, tuples, fixed arrays and tagged/sum carriers containing only those leaves, intersected with current record-field and collection formation. Inspect all variants and zero-length element types. No new root tuple, sum, vector, mask, field type, recursive inline layout, resource, pointer, callable, borrowed view or Move-element admission. |
| Source and output representation | Existing pipeline sources and stages apply: fixed record arrays, dynamic AoS arrays, record slices and already-admitted SoA sources or mapped results. Existing source-place and stage restrictions remain. Materialize a canonical dynamic AoS `array<T>` with complete record values; the final element type, not the original source type, selects its representation. No scalar fallback or field-only reconstruction. |
| Comparator and effects | Existing named/lifted/imported callable `(T,T)->bool`, both explicit parameters by value and existing Copy captures. `true` means the left operand precedes the right. Sequential effects, once-only eager capture evaluation and both explicit parameter effect origins follow Plan208. Existing generic-body/callable and parallel-Pure restrictions remain. A capture supplies comparison behavior only and is not stored in the result. |
| Ordering and control | Preserve Plan208's exact stable insertion runs of at most 32 elements and bottom-up merges; no adaptive probes for comparators under either `ALIGN_SORT_ADAPTIVE` setting. Empty/singleton inputs call no comparator. Strict weak ordering gives stable sorted output; a terminating inconsistent comparator gives an initialized permutation. Collection completes before comparisons, with existing stage order and frequency. Hard errors/divergence retain existing behavior. |
| Ownership and lifetime | Borrow the source and copy complete independent record values into a new owned heap or explicit-arena array spine. No source consumption, record field move, hidden clone, per-element Drop, owner transfer or new permission. Preserve existing source/stage reservations and conservative source-generation loans where contents carry them. Independent record copies retain no source backing; their own array spine remains region-bound. Ordinary array move/nulling/replacement/return/Drop and arena expiry apply. Comparator-only captures stay live during the call without becoming result content. |
| Allocation and errors | Reuse result allocation and one conditional element merge-scratch allocation for N>32. Scratch is heap-owned and retired before return, including with an arena result. N<=32 has no merge scratch under either adaptive setting. Byte counts use the complete record ABI size/stride, with checked multiplication and existing empty/zero-size rules. No key column or per-field allocation. Invalid source/type/mode/effect/metadata diagnoses before LLVM; overflow/OOM is the existing materialization hard error. No new error type or native text boundary. |
| Owners and artifact identity | Sema owns one shared comparator-element predicate and exact result type. Checked HIR independently validates formation/call metadata through the same semantic class plus its existing structural validation. MIR owns typed result/scratch headers, full record loads/stores and ordinary comparator calls. LLVM remains pure existing aggregate lowering. No new HIR/MIR variant, native symbol, interface field or persisted format. Existing nominal record identities and complete structural MIR/cache graph remain authoritative; ordinary compiler identity invalidates changed artifacts. |
| Prerequisites and sources | Reuse shipped independent Copy classification, record collection/materialization, aggregate call ABI and comparator procedure. No later milestone is consumed. Update the sorting contract in `draft.md`, `docs/language-spec.md`, relevant design-notes and Settled text, Plan208 scope, Plan12, checked-HIR ledger19, core array/slice/pipeline English/ja and pipeline-guide English/ja where their scalar-only promise changes. Record status once in HANDOFF after capability closure and provider delivery in the R27 register, leaving consumer edits uncommitted. No new performance claim or benchmark gate. |

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Type formation and malformed metadata | Add a shared comparator element/result classifier using the existing independent graph plus current natural record-array formation. Source and `validate_hir` consume it; do not weaken general Copy/view or aggregate validators. Extend `sort_comparator_domain_and_metadata` with valid nested/fixed/tagged record graphs and wrong result AoS identity, missing ID, malformed/cyclic definitions, Move/view/callable/raw/resource/over-aligned negatives, including zero-length/inactive wrappers. Existing graph owner `producer_plain_copy_type_graph` remains the classifier's invariant owner. |
| Construction and source/stage product | `lower_array_collect` already materializes complete records. Reuse its canonical record-array result spelling for sort scratch; audit all sort reads, stores, insertion-held values, ping buffers and copy-back. `comparator_record_order_layout_and_replay` covers fixed/dynamic/slice/SoA, no-stage/where/map/project where already admitted, mixed-width/nested/fixed/tagged/tuple-field/RNG records, layout(C), and zero-sized records where formed. It checks every output field and length against independent expected values. |
| Stable order and inconsistent comparison | Reuse the one comparator algorithm and extend execution coverage to records at lengths 0/1/31/32/33/63/64/65/129, duplicate multi-key ordering, ascending/reverse and all-true/cyclic callbacks. Unique scalar identity fields prove exact permutation and stable ties. Existing `comparator_sort_order_and_stability` retains all scalar domains. |
| ABI and producer validation | Full typed Struct SSA operands feed ordinary named calls; never reinterpret a record as a scalar or skip fields. Extend `comparator_sort_uses_shared_storage_and_calls` with records, exact two record inputs and capture order, result/scratch AoS headers, full-stride loads/stores, no decoration or cloned owners. Add malformed typed header/load/call/allocation twins to `comparator_record_mir_storage_contract`, checked at full/per-unit/LLVM boundaries. Existing producer checks must remain authoritative rather than gaining an unchecked sort exception. |
| Move-in/out, source nulling and Drop | Only result/scratch spines are owners. `comparator_record_scratch_and_drop` checks allocation/free/live-byte parity at 0/1/32/33/65 under both adaptive settings, heap and explicit-arena result, source reuse, result move/return/replacement/discard and repeated materialization. Empty/zero-sized cases must follow existing allocation semantics: zero-byte heap requests have no physical allocation; arena requests use the existing aligned dangling pointer. Zero-sized callbacks execute on both sides of the merge threshold. No element cleanup flag is added. |
| Borrow, effects and all control paths | `comparator_record_control_lifetimes_and_effects` crosses if/match/else/?/map_err, branch/loop joins, early source exits, local comparator-only text captures, arena-result escape negatives and source reservation conflicts. Independent numeric record inputs cannot hide writable view or callable fields. Both explicit argument positions, capture effects and imported calls retain the existing parallel-Pure rejection. Reuse `sort_comparator_effects`, `comparator_capture_write_authority`, `effect_fail_closed` and `current_plan_capture_termination_preserves_only_reached_decisions` rather than duplicating those invariants. |
| Monomorphization, interface and cache | The record execution owner uses imported ordinary comparators, generic record/helper instantiations, lifted captures, whole/per-unit native execution and cold/hit frontend replay. Complete record definitions and both call operands must survive existing templates and structural artifact identities. No new wire record. |
| Sibling behavior | Focused source/HIR/MIR owners plus existing `sort_comparator`, `sort_strings`, `sort_by_key`, `sort_owned_keys`, `sort_merge` and `sort_adaptive` cover the shared sort storage helper. Source-level sibling domains remain unchanged. Select any additional lower-level owner only if an actual representation change reaches it. Workspace build, bounded gate and separate-target Clippy precede publication. |

One capability includes admission, replay, typed buffers/calls and executable
consumers. Splitting a dormant admission or buffer change would duplicate proof
and leave an unusable or unauthenticated path. The expected handwritten diff is
below 1,000 lines; if complete owner coverage exceeds it, retain this single
failure domain and explain the measured scope before expanding implementation.

## Author ledger-to-boundary pass

Every public promise is recorded above: exact T and callback types, no option
state/default, effect and comparison order, array ownership/arena lifetime,
allocation/error behavior and existing artifact identity. There is no new wire
format, native text encoding, global state, reflection, or ambient configuration;
the existing adaptive diagnostic switch keeps its comparator semantics.

The source sweep found the concrete seams before implementation: source admission
and checked-HIR currently require primitive/str scalars; collect already creates
`DynStructArray(Aos)` but sort scratch creates only `DynArray`; all element reads
are ordinary `SliceIndex` and stores `PtrStore`, whose exact physical record
contracts already exist. Comparator borrow/region arms distinguish str from
independent data, so the selected record graph adds no view provenance or hidden
callee writes. The new source and malformed owners must verify this boundary,
including formation restrictions; `!is_move` alone is explicitly insufficient.

The ordinary call ABI, scalar-only neighboring terminals and all captured-view
restrictions remain unchanged. Request27's remaining Move-string lifecycle and
view-bearing aggregate authority stay separate. The independent adversarial strategy review below closed this ledger and matrix
before production edits.

Independent adversarial strategy review: CLEAN before production edits. It
confirmed the existing formation/graph intersection, complete AoS storage,
ordinary aggregate ABI and both-argument effect authority. Zero-size callback
execution and physical-allocation accounting remain explicit owner cells.

Implementation qualification found a missing unconditional allocation equation:
transient scratch can avoid returned-owner graph validation. The existing physical
collection validator now authenticates every HeapAllocBuf/ArenaAlloc element
pointer, signed count and arena-handle type before full/partition/LLVM use. Every materialized array header also requires its
canonical physical element and signed length, including locally dropped scratch.
The malformed storage owner mutates both result and scratch producers. This
closes the matrix's existing exact-buffer contract without a new representation
or ownership strategy. Independent record source replacement is an existing
collector behavior, covered by a positive owner; comparator capture mutation of
the reserved source remains a negative.

## Author matrix-to-diff closure

`comparator_sort_result` and checked-HIR formation share the closed graph; the
HIR domain owner and existing graph sweep cover malformed/hidden authority.
`collected_array_type` unifies result and scratch spelling; ordinary typed
loads/stores/calls remain unchanged. The physical collection validator closes
locally dropped allocation/header equations, with scalar and record, heap and
arena, whole and per-unit malformed twins. `sort_records` supplies the complete
layout/order/lifecycle/control and cold/hit consumers named in the matrix; the
existing scalar sort/effect and capture-termination owners close unchanged axes.
No public example introduces syntax; the numeric span/captured-text example is
executed by the record owner. The synchronized contract prose changes only the
comparator element domain and complete-record representation. No benchmark is
required because this capability makes no new performance/resource promise.
