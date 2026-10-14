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
| Compiler proof | Sema admits the immediate selected Move array only under Len. Checked HIR independently verifies that exact context, stable parent, path, physical leaf and index; permission cannot leak into nested children. MIR may load only the representation-compatible Slice-typed header and immediately observe SliceLen; no owner-typed indexed SSA is admitted. Producer validation restricts this new header producer to length observations, never storage, return, capture, argument or mutation. Existing exact path/type/index/construction/bounds validation remains. LLVM only lowers the authenticated read. |
| Identity and other boundaries | No HIR/MIR variant, serialized/interface field, runtime ABI, public parameter/return signature, persisted format, native text boundary, global state, ambient configuration or runtime helper changes. Existing compiler/source identity invalidates caches. All allocation and cleanup remain with existing constructors and owners. No performance promise or benchmark. Plans28/44/56/165 and the existing length operation are prerequisites. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and selected use | `indexed_array_field_length::values_and_storage` crosses scalar/str/string/record array fields, direct/nested fields, local/borrowed/field/slice/borrowed-match bases, fixed/dynamic outer arrays and constant/runtime indices. Non-array Move fields, temporary/nested-index bases and ordinary by-value/slice/mutable uses remain negative controls. Type constructors retain their existing grammar. |
| Evaluation and control | `indexed_array_field_length::evaluation_and_control` covers once-only effects, negative/high indices, empty parent versus empty selected array, same-root invalidating indices and unrelated-root positives, if/match/else/?/map_err, branch/loop joins and early return/break/exit. A terminating index cannot produce a bounds action; a completed scalar survives subsequent parent retirement. |
| Construction and lifecycle | `indexed_array_field_length::allocation_and_drop` uses existing heap/arena/JSON construction, complete owner moves/returns, replacement and ordinary/error exits. Native allocation/free/live-byte counters prove that observation neither allocates nor releases the field. Source nulling and recursive Drop remain on existing paths. |
| Checked HIR | `indexed_array_field_length_hir_matrix` mutates the immediate consumer, physical leaf/result, source root/path/type, index and nested-child permission; ordinary Move reads and forged mutation still reject, including unused peers. Full and partition lowering replay the same proof. |
| MIR and LLVM | `indexed_array_field_length_mir_matrix` authenticates array-to-Slice physical identity and limits the header SSA to SliceLen. Wrong element/layout/path/index/producer, owner-typed loads and forged return/store/call/Drop consumers reject before emission. Existing Copy/String field reads remain controls. |
| Imports and replay | `indexed_array_field_length::imports_and_cache` covers imported and concrete-generic scalar helpers in whole/per-unit execution and cold/hit/type-edit/restore caches. No view result or mutable retention is added. Existing borrowed_params, owned_structs_arrays and fixed_array_fields owners cover adjacent source and lowering rules. |

This is one independently useful scalar consumer matching the real request.
Splitting its source/producer/validation chain would leave a dormant or unsafe
intermediate. If complete proof exceeds roughly 1,000 handwritten lines, keep the
coherent boundary rather than duplicating its integration obligations.

Before implementation, independently review this revised boundary against the
initial authority finding and complete the author obligation pass. Before code
review, bind every applicable row to implementation and a passing regression
owner. Normative prose changes together in draft.md, language-spec.md,
design-notes.md, open-questions.md, Plan28 and the Len/ElemField checked-HIR rows.
Record the delivered scalar scope separately from R137's pending wider views.
