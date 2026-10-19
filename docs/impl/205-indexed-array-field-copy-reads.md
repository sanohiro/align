# Independent Copy reads through indexed owning array fields

LLM Request137's scalar length observation is shipped under Plan196. Its next
ordinary read, `rows[row].ids[index]`, still rejects at `06a4141f` because the
immediate array field is Move. The complete-row shared helper remains a working
consumer workaround. This capability reads an independent Copy element without
publishing the selected owning array as a value or a borrowed source-level view.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Expression and result | Existing `rows[row].path.values[index] -> T`. The field is an existing ordinary `array<scalar>` or AoS `array<Struct>` and T belongs to Plan182's closed independent-inline-Copy graph: numeric/bool/char/unit/RNG leaves, valid numeric vectors/masks and ordinary records, tuples, fixed arrays and tagged/sum carriers containing only those leaves. Only source-admitted collection elements qualify; this adds no type-formation rule. Every reachable payload and even zero-length element participates. No borrowed view, raw pointer, callable, task, native/heap owner, missing id, malformed shape or recursive inline cycle qualifies. |
| Parent and indices | The same stable local/borrowed/nested-record-field/borrowed-match parent grammar as Plan196: fixed AoS records, dynamic AoS records or record slices; nonempty ordinary field path. Both indices may be constant or runtime i64 expressions under existing index typing. Temporary and nested-index outer parents retain their existing refusal. |
| Ownership | The complete parent retains the selected array and sole Drop. Read T by value, with no heap allocation, source transfer/nulling/replacement or new cleanup. The completed T has no dependency on the parent and may survive its replacement, move or Drop. No newly readable array header may escape into an alias, store, argument, return, slice, pipeline, mutable place or capture. Existing explicit shared calls retain their own independent grammar. |
| Evaluation and errors | Reserve the complete parent before evaluating the outer index. Evaluate the outer receiver/index once, then its signed bounds; only a valid outer index can observe the selected array header. Evaluate the inner index once under the same reservation, then its signed bounds and element load. Parent move/Drop/replacement/exclusive access during either index rejects; unrelated writes remain valid. An outer bounds failure precedes the inner expression. A terminating outer index performs no outer read; a terminating inner index performs no inner bounds or element read. Preserve the existing hard bounds diagnostics and eager error order. |
| Compiler proof | Sema grants permission only to the immediate indexed array-field receiver of Index. Checked HIR reconstructs the exact stable parent/path/physical array/independent result and resets permission at every nested child. MIR reuses `SliceLen` and `SliceIndex` over the existing exact `BorrowedElementPlace`, with both signed bounds and source-preservation windows. No owner or source-level slice SSA is manufactured. Producer/codegen validation authenticates the selected header, physical element/result, matching inner length/index and both guard intervals, including unused results. Ordinary descriptor consumers remain forbidden. LLVM loads the exact header pointer and by-value physical element only after these proofs. |
| Shared type authority | Extract Plan182's existing iterative independent-Copy classifier to align_sema and reuse it from the MIR producer and the new source/HIR/MIR admission. Preserve its complete type graph, numeric/vector validity, cycle rejection and completed-node reuse. A general `!Move` or `!may_borrow` shortcut is not the independent-data contract. |
| Identity and scope | No new syntax, type, IR variant, interface field/version, persisted format, runtime symbol/ABI, native input, global state or ambient configuration. The compiler/source identity invalidates caches. No benchmark or performance/resource promise. Plan61/K1, general indexed Move-field views, writes and owner extraction remain deferred. |

## Implementation closure matrix

| Axis | Exact implementation and owner |
| --- | --- |
| Closed element graph | Canonical align_sema classifier with the existing `align_mir::producer::copy_tests::producer_plain_copy_type_graph` graph/invalid-leaf/cycle owner. `indexed_array_field_copy_reads::values_and_storage` exercises source-admitted numeric/bool/char and Copy-record elements with nested inline/tagged fields, all three parent classes, nested paths and stable borrowed/local/field/match roots. View/Move/pointer/callable controls cannot gain read permission. |
| Evaluation, control and reservation | Sema/MoveCheck reserve the parent through the complete read. `indexed_array_field_copy_reads::evaluation_and_control` crosses both indices' effects, negative/high bounds, empty outer versus selected array, unrelated writes and overlapping replacement/move/exclusive calls; if/match/else/?/map_err, loop joins and early return/break/exit retain written-order behavior. Outer failure must suppress inner effects; inner divergence must suppress the element read. |
| Independent result and cleanup | `indexed_array_field_copy_reads::allocation_and_drop` reads primitive and nested Copy data, then replaces/moves/drops the parent before using the result. Existing heap/arena/JSON constructors remain prerequisites. Positive requested-live instrumentation and allocation/free deltas prove the read allocates/frees nothing and leaves sole parent cleanup. Reuse admitted fixture construction rather than widening builders. |
| Checked HIR | `indexed_array_field_copy_reads_hir_matrix` in validate_hir_tests mutates selected type/result/path/index/parent and immediate consumer; rejects view/owner results and nested direct/indirect call permission leaks. Existing `indexed_array_field_length_permission_stops_at_calls` remains. Whole/partition lowering replay the same exact boundary, including discarded expressions. |
| MIR/LLVM | `indexed_array_field_copy_reads::producer_and_bounds` mutates exact field/element/parent/index/length/result, outer/inner guards, source-preservation interval and forbidden descriptor store/return/Drop consumers. Native bounds/value owners and existing `indexed_array_field_length_mir_matrix` retain adjacent length/shared-call behavior. Copy data becomes independent only after the ordinary readable producer proof, never as an unfounded seed. |
| Imports and cache | `indexed_array_field_copy_reads::imports_and_cache` uses imported concrete/generic helpers in whole/per-unit execution and cold/hit/type-edit/restore caches. No retained view or mutable-output summary is introduced. Existing `copy_record_producer`, `borrowed_params`, `owned_structs_arrays` and `fixed_array_fields` targets retain neighboring transfer and projection behavior. |
| Documentation and consumer boundary | Update the existing indexed-field paragraphs in draft.md, docs/language-spec.md, docs/design-notes.md and the corresponding Settled entry in docs/open-questions.md, plus Plan196's forward disposition and the exact Index/ElemField records in docs/impl/19-hir-validation-ledger.md (immediate permission, independent result and both guard intervals). The English memory guide has no conflicting indexed-field exclusion. Record partial provider progress in R137 after merge; consumer code/pins and adoption remain external. Media/voice use the same array/record ownership model and receive no consumer changes. |

This is one useful read capability, with a separate failure domain from retained
view transport. Its source permission, two bounds, producer proof and final load
must ship together. If the complete closure exceeds 1,000 handwritten lines,
keep that coherent consumer boundary: splitting it would duplicate the same
reservation/bounds proof or leave a dormant permission/producer chain.

The author ledger pass precedes one fresh independent boundary review of this
new read context. Resolve findings before implementation. Finish one matrix-to-
diff pass, the named owners, one fresh preflight code review, bounded gate and
Clippy before publication. Source qualification never executes a rejected case.

The independent boundary review accepted the Copy-only ownership strategy and
identified the checked-HIR ledger update as a missing authoritative record. The
documentation row above includes that obligation before implementation.

The existing whole-fixed-Move-record-array call formation remains outside this
read boundary: even Plan196's `fixed(rows, i) -> rows[i].ids.len()` control fails
its caller's owned-leaf producer proof. Local fixed parents and a checked fixed
borrowed-parameter body retain this read rule; this capability does not widen
that separately unshipped whole-place call authority. Replacing a complete fixed
Move-record array after a read likewise fails the pre-existing Plan196 length
producer control; lifecycle owners use admitted dynamic parents for replacement
and move/Drop, and fixed arena parents for scope retirement.

## Implementation closure

`independent_copy_type` in align_sema is the unchanged Plan182 graph authority;
the producer delegates to it. `check_index` grants CopyRead only to the immediate
field receiver. MoveCheck excludes this action from its eager worklist and keeps
one complete-parent reservation around both indices. EscapeCheck gives only the
exact independent result a Static region, including inline records returned
across arena retirement; view-bearing peers retain their existing lifetime.

The checked-HIR context resets CopyRead beside the length/shared-call permissions
at every child, including direct/indirect calls. `lower_borrowed_element_field`
retains the outer reservation and signed guard; `lower_index` observes the
selected length before the inner expression and emits its signed guard before
SliceIndex. Both reads carry the same exact descriptor. Producer certification
requires the exact readable parent/field and the closed physical result even
when unused. LLVM reconstructs both canonical guards, compares descriptor
identity including its reservation, and preserves the root from reservation
through the load. It loads by-value data only; no field-header SSA is published.

The author matrix-to-diff pass binds every ledger row to the named graph, HIR,
source/native and MIR mutation owners. The MIR matrix crosses all mutations with
used/discarded results and includes equal-type different-field length evidence,
noalias misuse, reversed/bypassed guards and parent retirement before either
observation. Native lifecycle owners positively witness requested-live counters,
allocation/free parity during the read, independent primitive/record use after
admitted parent replacement/move/Drop, and arena retirement. Imported generic
helpers return both scalar and Copy-record data through whole/per-unit and
cold/hit/type-edit/restore builds. The shared fixture harness is used by the
length and Copy-read targets; its change also requires the existing local
PostgreSQL service gate under the repository's fail-closed shared-helper rule.

The independent code review found a missing control-flow obligation shared by
both guards: a success node may have a bypass predecessor. The shared guard
validator now requires the canonical guard block itself to dominate the action,
in addition to its success node. Retained-Branch bypass twins for both levels,
with used and discarded results, are accepted by the reviewed pre-fix compiler
and rejected after this correction. The full matrix has 27 mutations in both
result-use states; no representation or public boundary changes for the fix.

## Chained Copy-record field projection (AM-R21)

At `dde9d0de`, `poses.frames[i].points[j]` copies an independent record, but
appending `.x` incorrectly asks for the owning `points` header as a value. The
working explicit local binding establishes the complete existing operation.
Normalize this source spelling to an ordinary block containing one synthetic
Let of the existing checked Index and a Field projection from that local.
Nested ordinary record fields use one exact field path. The complete selected
element must still satisfy the same independent-Copy classifier; selecting a
scalar field does not admit a view-bearing or owning element.

| Axis | Implementation and owner |
| --- | --- |
| Formation and diagnostics | `check_index_field` checks an indexed field receiver once using the existing Copy-read context. Only an owning array with a complete independent record normalizes to Index/Field; existing borrowed slice-field projections retain direct ElemField semantics. Resolve both indices and the ordinary field path once. `indexed_array_field_copy_reads::chained_record_fields` covers direct/nested fields, real media record shapes, unknown fields, invalid indices and excluded view/Move elements. |
| Evaluation and lifetime | The canonical Index retains both signed bounds and the complete parent reservation. One synthetic Let snapshots its independent result before Field, without a heap allocation, retained view, source transfer or new Drop. The owner crosses runtime indices, effect order, outer bounds before inner effects, inner failure, early exit and parent invalidation; existing lifecycle/bounds/malformed owners remain unchanged. |
| Imports and publication | The owner uses imported generic helpers, whole/per-unit native execution and cold/hit/body-edit caches. Ordinary Let/Index/Field validation, monomorphization, serialization and MIR/LLVM lowering remain the sole implementation; no new HIR/MIR shape, permission or summary is introduced. |
| Consistency | Clarify the composed spelling in the existing spec/digest/Settled paragraphs and design rationale, and record its canonical source normalization in the HIR ledger. Consumer code/pins and guide-vector acceptance remain external. |

This follows the already-reviewed Plan205 proof through existing canonical
nodes; it changes no ownership or validation strategy. The author matrix pass
and ordinary independent preflight review cover the source normalization.

Author closure: the new imported/generic owner rejects on the pre-fix compiler
at the owning array field and now executes in whole/per-unit/cache modes. Its
borrowed slice-field control preserves the existing view-bearing record path.
All seven existing/new target tests pass, including the extended allocation/free
and post-parent-retirement owner. The malformed HIR/MIR guards remain unchanged;
the generated nodes pass their ordinary publication boundaries. The three
consumer registers and relevant pose, trace and tensor source were inspected;
no consumer implementation or pin is changed by this capability.
