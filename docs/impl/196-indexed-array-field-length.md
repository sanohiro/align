# Indexed owned-array field length

LLM R137's real Case.ids use needs `rows[index].ids.len()`. The reduced source
rejects at 9227651f while a complete-Case shared helper works. This is a valid
scalar observation of existing storage, not an owning element extraction.
Media's Archive arrays and voice's tokenizer arrays retain the same owner model.
Consumer sources and pins remain external.

The initial broader view proposal failed its strategy review: existing lifetime
summaries deliberately erase ReadOnly and cannot transport a newly restricted
view through returns or mutable-destination retention. Plan61 owns that unshipped
access problem. This capability admits the useful scalar observation, without
publishing a source-level view or changing those summaries. General direct
indexed Move-field views, indexing and pipelines remain pending; this delivery
must not claim their authority or lifetime proof.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Receiver and result | Existing `rows[index].path.values.len() -> i64`, with zero arguments. The final field is an already-admitted ordinary dynamic scalar array or AoS record array. The nonempty struct-field path may be nested. The outer collection is an ordinary fixed AoS record array, dynamic AoS record array or record slice, rooted in a stable local, shared/exclusive parameter, checked borrowed match binding or ordinary record-field path. Constant/runtime indices are admitted for every outer class. No temporary or nested-index outer base, specialized new field grammar or new syntax. |
| Ownership and lifetime | The containing collection remains the sole owner of the selected array. The operation reads its initialized length without allocation, element copies, transfer, nulling, replacement, independent cleanup or Drop. Source storage and contained roots stay live through the read. The i64 result retains no borrow and may outlive the collection; subsequent ordinary moves/replacement remain valid. Existing owner transfers retain their established generation rules. |
| Evaluation and errors | Resolve the exact stable base, field path and array type; arity remains zero and index type is i64. Evaluate base and index once in written order. Reserve the complete parent generation before evaluating the index and preserve it until the header observation. Overlapping move/Drop/replacement/exclusive access during index evaluation rejects; unrelated mutation is permitted. After index fallthrough, check the outer signed bounds before loading the selected length, even when the selected field is empty. A terminating index performs no bounds/header/read. Existing hard bounds failure and diagnostics retain their ordering. |
| Excluded uses | An ordinary alias, owning return/store/capture, indexed Move extraction, replacement, exclusive field/element borrow, range or slice conversion from this Move field remains subject to its existing rejection. Existing explicit shared-call arguments keep their prior grammar, including the fixed Move-field literal-index gate. A successful length read does not grant a new view or write permission. |
| Compiler proof | Sema admits the immediate selected Move array only under Len. Checked HIR independently verifies that exact context, stable parent, path, physical leaf and index; permission cannot leak into nested children. MIR reuses the exact bounds-guarded BorrowedElementPlace as the operand of SliceLen; no owner or view header SSA is formed. Only this ordinary array-field scalar read and existing shared calls accept that descriptor. Producer validation requires readable exact selected storage; all ordinary value/store/return/Drop contexts retain their descriptor refusal. Existing exact path/type/index/construction/bounds validation remains. LLVM only lowers the authenticated read. |
| Identity and other boundaries | No HIR/MIR variant, serialized/interface field, runtime ABI, public parameter/return signature, persisted format, native text boundary, global state, ambient configuration or runtime helper changes. Existing compiler/source identity invalidates caches. All allocation and cleanup remain with existing constructors and owners. No performance promise or benchmark. Plans28/44/56/165 and the existing length operation are prerequisites. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and selected use | `indexed_array_field_length::values_and_storage` crosses scalar/str/string/record array fields, direct/nested fields, local/borrowed/field/slice/borrowed-match bases, fixed/dynamic outer arrays and constant/runtime indices. Non-array Move fields, temporary/nested-index bases and ordinary by-value/slice/mutable uses remain negative controls. Type constructors retain their existing grammar. |
| Evaluation and control | `indexed_array_field_length::evaluation_and_control` covers once-only effects, negative/high indices, empty parent versus empty selected array, same-root invalidating indices and unrelated-root positives, if/match/else/?/map_err, branch/loop joins and early return/break/exit. A terminating index cannot produce a bounds action; a completed scalar survives subsequent parent retirement. |
| Construction and lifecycle | `indexed_array_field_length::allocation_and_drop` uses existing heap/arena/JSON construction, complete owner moves/returns, replacement and ordinary/error exits. Native allocation/free/live-byte counters prove that observation neither allocates nor releases the field. Source nulling and recursive Drop remain on existing paths. |
| Checked HIR | `indexed_array_field_length_hir_matrix` mutates the immediate consumer, physical leaf/result, source root/path/type, index and nested-child permission; ordinary Move reads and forged mutation still reject, including unused peers. Full and partition lowering replay the same proof. |
| MIR and LLVM | `indexed_array_field_length_mir_matrix` authenticates the exact array-field descriptor at SliceLen. Wrong element/layout/path/index/producer/guard, owner-typed loads and forged return/store/Drop consumers reject before emission. Existing shared-call descriptor admission remains unchanged. Existing Copy/String field reads remain controls. |
| Imports and replay | `indexed_array_field_length::imports_and_cache` covers imported and concrete-generic scalar helpers in whole/per-unit execution and cold/hit/type-edit/restore caches. No view result or mutable retention is added. Existing borrowed_params, owned_structs_arrays and fixed_array_fields owners cover adjacent source and lowering rules. |

This is one independently useful scalar consumer matching the real request.
Splitting its source/producer/validation chain would leave a dormant or unsafe
intermediate. If complete proof exceeds roughly 1,000 handwritten lines, keep the
coherent boundary rather than duplicating its integration obligations.

The revised strategy was independently reviewed after the initial authority
finding. The final representation reuses `SliceLen(BorrowedElementPlace)` rather
than introducing even an internal slice-valued header. `checked_borrowed_element_place_ty`
returns the outer physical element type for pointer stride; the scalar consumer
separately requires the exact selected `element_ty` to be an ordinary array.

## Implementation closure

The source/native owners live in `crates/align_driver/tests/indexed_array_field_length.rs`.
The MIR unit owner `indexed_array_field_length_permission_stops_at_calls` directly
checks the body validator: neither direct nor function-value calls may pass the
length-only permission to their argument children. Its fixed runtime-index twins
fail on the uncorrected implementation, independently of later MIR rejection.
Sema's `check_index_field` admits only the immediate Len receiver. MoveCheck
reserves the source before the index; both its eager worklist and transparent
spine preserve that action boundary. Existing escape analysis walks the same
children and gives the scalar result no borrowed payload. `evaluation_and_control`
rejects move/replacement/exclusive access and slice rebinding during the index,
while scalar survival after parent retirement and unrelated mutation pass.

Checked HIR resets the length permission at every child boundary. MIR's
`lower_borrowed_element_field` shares the existing reservation and signed bounds
construction with shared calls. Producer validation replays exact base/path/type
and readable storage, including unused scalar results; LLVM's existing guarded
pointer validator authenticates the bounds and source-preservation interval.
The HIR/MIR mutation matrices cover malformed context, selected type, path, index,
slot, reservation, bounds, and ordinary value/return/store/free consumers.
Existing shared-call and fixed-field owners retain their adjacent refusals.

Native owners cover fixed/dynamic/slice bases, nested scalar/text/string/record
array fields, borrowed match and exclusive parameters, empty selected arrays and
empty outer fixed/dynamic arrays, termination, heap/arena/JSON construction,
complete moves/replacement and cleanup. Requested-live instrumentation is enabled
and positively witnessed; allocation/free counters remain unchanged across the
repeated observations. Builder buffers use a separate allocator, so global
alloc-call/free-call equality is not claimed. Imported generic helpers execute
whole/per-unit and through cold/repeated/type-edit/restored cache builds.

Existing constructors remain prerequisites: a heap record builder cannot contain
`array<str>`, and a region record builder cannot acquire independently owned
array fields. The owners use admitted fixed storage for arena/text-array cases.
The existing whole-fixed-Move-record call producer restriction is separate from
checking the admitted borrowed function body; this capability does not widen
whole-place calls. No consumer code or compiler pin changes are required here.
