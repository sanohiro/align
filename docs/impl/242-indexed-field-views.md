# Retained indexed array-field views

## Evidence and boundary

AM-R16 requests `inputs[i].shape[..end]`; R137 requests non-consuming access
through indexed records containing owned ID arrays. Plans196/205 already ship
length and independent element reads. Plan232 ships fixed-field Copy snapshots.
Those observations remain independent values. This capability forms an explicit
read-only slice into existing selected field storage, with no field snapshot.

The three consumer registers were reconciled on 2026-10-25. Media's
`gpu.iteration` uses fixed shape/stride descriptors; its original indexed-field
packet retains the exact range witness. LLM's `native_cuda_model_smoke` borrows
complete Case records for the failure owners. Voice's `assets` copies a fixed
shape field into a named local before slicing it for upload. These are working
consumer representations, without a measured copy cost. No consumer adoption,
code change or performance claim belongs to this provider capability.

Plan240 supplies ordinary-call read-only authority and publication validation.
The chosen public spelling is the existing explicit range expression. Direct
owning-field values, new implicit coercions, direct pipeline receivers without a
range, exclusive indexed owner access and view-bearing/Move element formation
remain separate. A formed slice uses all existing admitted slice readers,
pipelines, carriers and captures. Ordinary by-value readers can consume the
range directly. An explicit shared call keeps its existing stable-argument
grammar: bind the slice before passing it as `borrow slice<T>`; this capability
does not admit an unbound range as a new borrowed place. This is one useful stable
consumer of the exact indexed-place producer, not a dormant producer chain.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Source and result | `rows[index].path.field[start..end] -> slice<T>`, with the existing optional start/end syntax (`0` and selected field length). The nonempty ordinary record-field path ends in an existing fixed scalar/record array or ordinary dynamic scalar/AoS-record array. The complete T graph must satisfy the unchanged independent_copy_type classifier; view, pointer, callable, task, native/heap owner, malformed/cyclic/missing definitions fail, including zero-length cases. No collection/type-formation or function-value signature rule is widened; current function values cannot return slices. Admitted indirect scalar readers and captured views retain their existing rules. |
| Parent | Same stable local, shared/exclusive parameter, ordinary record-field and admitted borrowed-match parent grammar as Plan196. Parent type is a fixed AoS record array, dynamic AoS record array or record slice. Runtime and constant i64 indices qualify. Temporary/call-result and nested-index outer parents retain their existing refusals; bind an existing supported parent first. Existing whole fixed AoS record-array call producer refusals (including Copy records) and Move construction restrictions are not widened. Fixed-parent view formation and checked borrowed-parameter bodies qualify independently; native helper owners use supported dynamic/slice parents. This is an existing implementation limit, not a new language restriction. |
| Evaluation | Resolve the exact physical field and stable parent. Evaluate the parent and outer index once in written order; reserve its live generation before the index. Perform signed outer bounds before observing selected storage or evaluating range bounds. Form the full selected field view without copying elements, then evaluate start and end once in order and apply existing signed range bounds. A terminating operand performs no later action. Even an empty selected field requires successful outer bounds. An outer failure suppresses both range operands. |
| Lifetime | The parent retains ownership and sole recursive Drop. The slice depends on the parent backing and the selected field storage; ordinary field-path/index generation selection must preserve both dependencies where required. An owning parent move, destruction, or replacement that retires backing invalidates old views; copying a Copy parent does not retire it. Replacing a fixed inline Copy array preserves its storage address, so existing views observe the new elements, exactly as ordinary inline slices do. Exclusive access conservatively invalidates when existing effect/provenance rules cannot prove backing preserved; owning field retirement cannot be hidden by a still-live parent header. Scope/arena exit cannot publish a usable view. Same-parent mutations that cannot be proved disjoint remain conservative. Proven independent owners and disjoint ordinary fields retain the existing Plan240 precision; do not flatten every view to all reachable owners. |
| Authority | Always read-only, including a mutable/owned parent. Local alias, subrange, ordinary/helper/generic/imported return, admitted mutable-output retention, carrier, capture and view projection cannot acquire write authority. Assignment, mutable borrowing and safe native byte writers reject through this view. An explicitly owned copy and a completed independent element read retain their ordinary independence and writable owned storage. Ordinary returned views follow Plan240. |
| Mutation timing | Parent lifetime and selected storage must remain valid through both range operands and the final view action. A bound that moves/replaces/releases or overlaps exclusive access to the selected backing rejects; independent mutation passes. Validated text derived from a byte view retains the ordinary byte-generation invalidation rule. This capability adds no mutation, source nulling or cleanup action. |
| Errors | Existing source index/range types, stable-parent/field-formation checks and hard signed bounds failures apply. Field/receiver typing precedes range-bound typing. Native order is outer index, outer bounds, start, end, range bounds, final subview. No Result/error variant/default/ambient option is added. |
| Ownership, allocation and effects | Pure except for written receiver/index/bound effects. No allocation, owning clone, array-element copy, transfer, release or native call. A slice header is the existing Copy `{ptr,len}` descriptor into the selected backing. A zero-length indexed fixed field still produces its selected field address and keeps the parent lifetime, even though no element can be read. It is not the standalone canonical empty slice; its outer bound still executes. Dynamic empty fields retain their existing physical header. No runtime or throughput promise needs a benchmark. |
| Identity and other boundaries | No syntax, public type/signature, interface field or runtime ABI/export change. Source/compiler identity invalidates artifacts and cached lowered bodies. Existing generic substitution checks the complete field graph. No wire format, text/native input, reflection, process-global state or ambient configuration changes. Prerequisites are Plans196/205/232/240 and the existing slice range operation. |
| Normative sources | Update the indexed-field paragraphs in draft.md, docs/language-spec.md, docs/design-notes.md, the corresponding Settled entry, the ArrayToSlice/SliceRange/ElemField HIR records, Plan73/196/205/232 disposition and core-design/array-slice-pipeline EN/ja. Record the capability once in HANDOFF at completion and write provider receipts to affected request registers after merge. |

## Implementation closure matrix

| Axis | Implementation boundary and discriminating owner |
| --- | --- |
| Source formation | Extend immediate indexed-field permission only for the explicit range receiver. Normalize that receiver to `ArrayToSlice(ElemField)` inside ordinary SliceRange. Use one shared exact fixed/dynamic field and independent-element classifier. Checked HIR independently rederives this exact immediate relationship, stable parent, field path/type and complete graph; permission is reset at every child boundary. `indexed_field_views::formation_and_values` covers primitive and nested independent records, fixed/dynamic/slice parents, local/field/borrowed/match roots, runtime indices, N=0/nonzero and unsupported neighbors. |
| Retained provenance | Derive a read-only root view from the selected storage, retaining parent generation and selected field backing even when the fixed field has no header. Reuse existing typed generation projections for owning fields and Plan240 call/publication policy. Audit both MoveCheck and independent escape/body-fact replay, return summaries, mutable outputs and region/arena checks. `indexed_field_views::lifetime_and_authority` crosses direct/copied/subrange/carrier/capture/imported results with Drop/move/whole replacement/selected-field retirement, byte writes and readonly publication; independent-owner/disjoint-field and owned-copy controls must pass. |
| Eager order and joins | Source checking and independently replayed checked-HIR body facts preserve the parent and selected backing across outer index, full view formation, both bounds and final view. The MIR borrowed-element reservation authenticates creation of the full field view before range-bound evaluation; it does not extend by itself to the later SubSlice. `indexed_field_views::evaluation_and_control` covers once-only effects, signed outer/range failures, empty parent/field, invalidating bounds, unrelated writes, if/match/else/?/map_err, branch/loop joins and early return/break/divergence. Read-after-source-retirement rejects; independent scalar copies survive. |
| MIR producer | Add one dedicated `IndexedFieldView(Operand::BorrowedElementPlace)` rvalue returning the exact slice type. Reuse the existing reservation, physical field/path and signed outer guard producer, extending readable selected-field proof to fixed and dynamic independent arrays. Do not admit descriptors through ordinary Use, Store, Return, Drop or arbitrary native operands. Exhaustively sweep every MIR rvalue analysis (operands, effects, alias/lifetime/region, optimization, serialization/hash/dump, frontend partition/cache and LLVM). The representation carries no separate authority bit or duplicate lifetime model. |
| LLVM and malformed IR | Authenticate exact base layout, field path/type, slice element/result, index/length, unique reservation, dominating signed outer guard and unchanged parent interval before forming a pointer. Inline fields expose their existing address and constant length; dynamic fields load the existing header, never a new owner. Ordinary range lowering emits the existing signed range guard before SubSlice; SubSlice itself does not authenticate that guard. Source + independently replayed checked HIR own the valid backing interval during bound evaluation and final subview formation. Actual MIR independently authenticates the new full-field producer up to its action, and the ordinary SubSlice type/readable-source relations, not arbitrary post-producer lifetime or range-guard dominance. `indexed_field_view_hir_matrix` mutates immediate context, receiver/path/types/permissions and nested-child leakage; `indexed_field_view_mir_matrix` mutates producer/guard/layout/result/reservation and unused-result twins. All four checked-HIR lowering entrypoints and actual LLVM rejection boundaries remain fail-closed. The existing arbitrary-MIR post-producer lifetime/range-guard proof and byte-writer authority are separate boundaries; this capability must not claim them. Wrong/missing outer guards and pre-producer retirement are new-producer negatives; invalidating range operands and range errors are source/checked-HIR/native-lowering owners, not claimed actual-MIR rejection cases. |
| Lifecycle and resource ownership | Existing parent constructors, heap/arena provenance, move-in/out, source nulling, replacement and recursive Drop are unchanged. `indexed_field_views::nested_parents_and_cleanup` observes positive parent allocation then zero view allocation/free deltas, correct values, completed independent copies after parent retirement and exactly-once final owner cleanup. No standalone slice cleanup exists. |
| Imports and artifacts | `indexed_field_views::imports_and_cache` covers concrete/generic borrowed helpers returning views and reader functions through whole/per-unit native builds, cold/hit/field-definition edit/exact-restore caches. Sema, checked-HIR replay, MIR serialization and producer evidence must agree. No consumer pin or build is modified. |
| Neighbor coverage | Reuse indexed_array_field_length, indexed_array_field_copy_reads, indexed_fixed_field_values, fixed_array_fields and call_boundary_views for existing scalar/snapshot and ordinary-call rules; add only relevant owners that fail for this change. The unchanged independent graph owner closes invalid/nested/zero-element classification. |

This crosses three compiler layers and may exceed 1,000 handwritten changed
lines because a new internal MIR producer must participate in every exhaustive
analysis plus malformed-IR and native lifetime owners. One capability PR has
lower integration risk than splitting the strict permission/provenance/producer/
consumer chain: every part is necessary for an immediately usable retained view.

## Author ledger pass

The public operation publishes only a slice descriptor of existing independent
data. The exact parent and selected backing survive from index evaluation to
all permitted retained uses. All read-only call forms use Plan240; no partial
return-only permission or broad arbitrary-MIR writer proof is introduced.
The matrix explicitly includes fixed inline fields, which have no selected
owning header to forward, and dynamic fields whose owner can retire separately.
No existing indexed Copy read becomes a view or changes snapshot timing.

Before implementation, independently review this ledger and the proposed
source/HIR/MIR boundary. Resolve strategy findings first. At completion, perform
one matrix-to-diff pass and one fresh full-diff review, run the focused owner and
final-SHA preflight, then publish and merge. No new performance claim is made.

## Independent boundary review corrections

The inspection identified an ambiguity between full-field producer validation
and the later range operation. The matrix now assigns the post-producer
lifetime interval to source checking and independent checked-HIR replay, and
range bounds to ordinary lowering's explicit guard. The new actual-MIR proof
ends at full-field view formation; SubSlice does not independently certify
arbitrary forged range guards or post-producer liveness. This preserves the
existing slice validation strategy and makes its limit explicit, rather than
claiming that the new borrowed-element reservation covers a later action.
Explicit shared calls also retain their existing binding requirement.

### Inline storage identity

The initial blanket replacement wording was stronger than the existing storage
model. A fixed Copy parent retains its inline backing when assigned a new value;
a previously formed view sees those new contents. Copying that parent likewise
is not an owning move. Owning backing retirement and selected owned-field
retirement still end the view. Source selection remains reserved during the
index and bounds; byte-validated observations retain ordinary write invalidation.
The zero-length indexed field is a real selected address and retains the parent
lifetime, unlike an existing standalone canonical empty slice.

### Inline byte validation and preserved precision limits

All inline installation paths share one write boundary: whole/local/field
replacement and loop re-entry invalidate byte validation of the previous physical
destination generations. They preserve ordinary inline views. Rebinding a stored
slice descriptor or copying data into a fresh inline destination does not write
that old backing. Selected inline-field observations include their physical-parent
fallback dependency beside any constructor-content header identity.
`inline_validation_writes` closes ordinary local/field and transparent-block paths,
disjoint fields, exact self-assignment, descriptor rebinding and independent copies;
`validation_and_disjoint_fields` closes direct indexed views and native writers.

An arbitrary helper whose input and result element graphs differ can still have
unknown returned byte backing under the existing summary contract. Lifetime
fallbacks alone do not prove disjointness. Such validated text conservatively
expires on a possible write, including a newly recognized inline replacement;
the direct indexed view's authenticated parent remains precise. This capability
does not add a relational byte-return summary or claim that precision repair.

### Fixed Copy publication closure required by retained views

The full-diff author pass exposed a missing existing producer edge: a later whole
Copy fixed-record-array replacement copies an anonymous array built with
StoreElemField, but the all-time selected-source graph asks its Load for a complete
root producer. Inline element-field stores do not supply that root. Retained views
must remain valid after that replacement, so remove neither the unconditional
IndexedFieldView READ query nor its readable-content obligation.

Extend the existing strict anonymous fixed-array Load publication proof to fixed
record arrays whose complete graph is independent Copy. Before a root Load, every
record-field leaf of every element must have one exact typed initialization that
dominates publication. Authenticate constant i64 indices, complete record paths,
all input values, cardinality and zero-size cases; reject missing, duplicate,
mis-typed, out-of-range, late, conditional or mixed incompatible writes. Whole
stores retain their existing dominance/publication and recursively checked source
obligations; wrapper/Use/Select paths cannot erase the root Load proof. No mutable
or Move authority is inferred. After proof, the Copy Load derives its readable
contents from the element path, just as scalar fixed-array Loads do. This does not
change whole-fixed-array borrowed-call admission.

The indexed fixed-field view authenticates selected element contents when
nonempty, like MakeFieldSlice; empty fields have no readable element requirement.
Dynamic selected fields always authenticate the actual owning header read.
The unconditional new-producer check applies even when the view result is unused.
Extend the malformed-MIR matrix with anonymous Copy parent replacement, missing/
duplicate/wrong/late field initialization and used/discarded view twins; native
inline replacement and independent scalar reads remain positive controls. This
closes the necessary Copy publication edge in the same producer failure domain,
without broadening the selected view's element graph or call grammar.

### Returned-view candidate closure

A returned indexed child exposed an incomplete compatible-header union: an Outer
with `rows: array<Row>` and `other: array<i64>` could return `rows[0].data[..]`,
then be incorrectly assigned only `other`'s compatible header. Replacing rows
would leave the returned view live. A type-compatible sibling is not proof that
all possible backing candidates were represented. Type-changing byte views are
the same failure class.

MoveCheck ordinary-call, map_err, mutable-output rebind and call-return carrier
completion, and EscapeCheck's shared unknown-return leaf keep original lifetime
fallbacks whenever any selected source header is missing, unknown, or incompatible with the returned header. Only a
complete compatible known candidate set may erase those fallbacks. An unmatched
aggregate/collection header can hide nested storage; it cannot be ignored because
a sibling matches. This deliberately retains conservative expiry for unresolved
returned children, including disjoint sibling replacement that the ordinal-only
summary cannot prove. Direct selected views and closed same-type return candidates
retain their precision. No field-relational return summary or writable return is
introduced. Owners cross direct/imported/generic calls, Result/map_err, same-type
and different-type siblings, backing replacement and byte reinterpretation, with
known closed-candidate controls and independent checked-HIR replay.

Candidate leaves' own parent, observation and other fallback dependencies remain
mandatory even when the broad source-selection roots can be removed. Missing
source information is retained before candidate concatenation. Escape fallback
resolution includes unmatched sources conservatively instead of reapplying a
partial type filter. Content-write destination collection remains a conservative
accumulation; only dependency publication uses the complete-set rule.
`retained_header_candidate_completeness_matrix` parameterizes missing, unknown,
incompatible and absent-descriptor candidates plus mandatory parent dependencies.
`returned_candidate_closure` covers imported/generic returns, output rebinding,
record outputs, Result/map_err, returned chunks and type-changing byte views.
`indexed_view_retention_replays_field_retirement` mutates a checked disjoint write
into selected-parent retirement and requires independent replay plus every public
lowering entrypoint to reject it.

## Author matrix-to-diff pass

The extracted ledger obligations map to the source/HIR formation owner, the
retained-provenance/control/native owners, the actual-MIR malformed matrix and
strict Copy-publication owner above. The new rvalue is explicit in operand/effect
inventory, mutable operand remapping, direct-slot inventory, readable producer,
LLVM view-fact eligibility and printing. Specialized loop rematerialization,
byte-storage/byte-range optimization and stack-builder whitelists retain their
fail-closed refusals. Existing descriptor operands carry the same exact physical
types; no new tagged-type identity, runtime export or cleanup operation is added.
Imported generic native/cache owners cover the ordinary partition artifact path.

The implementation and owners exceed 1,000 handwritten lines because retained
views require both the new selected-place consumer and closure of existing Copy
publication/returned-dependency defects that it exposes. These are necessary
proof edges of this one capability; separating an enabled source spelling from
its lifetime and producer checks would leave an unsafe intermediate consumer.
The arbitrary-MIR post-producer lifetime/range-guard and byte-writer proofs and
whole fixed-array borrowed-call expansion remain explicitly deferred above.
