# Indexed writes to owned dynamic-array fields (AM-R01, R147, R46)

## Evidence and capability boundary

The unchanged media `dynamic-field.align` witness still rejects on Plan193's
compiler: indexed writes through fields admit only fixed arrays. Media's
`safetensors.Archive.ids` builds a separate handle array and replaces the whole
field; LLM's `native_cuda_load_admission_smoke::corrupt` constructs each malformed
staged map separately. Voice's tokenizer fills a local slots array before
constructing its Lookup. All three registers and those uses were inspected.
R46's builder-value provenance was separately delivered by Plan136.

This is a valid composition of existing owned storage and element assignment.
Use the existing indexed-store element class, including borrowed text, rather
than adding a numerical special case. R132/R142's partial exclusive calls,
R137's indexed shared projection and AM-R07's external writable views remain
separate. No parked K1 or aggregate-provenance implementation is selected.

The requests' blanket overlapping-view wording needs qualification: existing
sequential local views can observe in-place array element updates. Shared
parameters cannot be written, and conflicting shared/exclusive call arguments
reject. Preserve that distinction; replacement or Drop still invalidates views.

## Public-contract ledger

| Surface | Exact rule and owner |
| --- | --- |
| Syntax and types | Existing `root.field[ index ] = value`, including a finite ordinary nested record field path. The selected collection is `array<T>` with T in the existing `indexed_element_store_ok` class: integer, float, bool, char or borrowed `str`. Existing fixed scalar-array fields use the same selected-backing lifetime rule. No new syntax, nominal type, public function, argument/default or overload. Source checking and both checked-HIR statement validators own formation. |
| Authority | The root is a mutable owning local/ByValue parameter or an exclusive whole-record `borrow mut`/`out` parameter under existing mode admission. The selected dynamic leaf is an owned allocation, never a slice field. Its complete typed owned producer and live root/path supply authority; absence of a read-only marker never proves an arbitrary view writable. Shared parameters, read-only borrowed match projections, immutable/moved roots and expired selected backing reject. No partial Move-field call is admitted. |
| Evaluation and errors | Evaluate index then RHS exactly once, complete both, load the then-current selected header, check `0 <= index < len`, then write. A completed root/field replacement can establish a fresh destination. A terminating operand performs no later destination read, bounds check or write. Bounds failure retains the existing hard-error behavior. Static invalid shapes and malformed HIR reject before emission, without panic. |
| Value/ownership | Store one Copy element. The record, siblings, array header, length and allocation remain owned in their existing places. A stored str retains its input owner/arena dependency and never clones bytes. No allocation, Drop, source nulling or new runtime entry occurs at the write. Array construction, replacement, extraction, return and recursive cleanup retain their existing rules. |
| Destination lifetime | Check a stored view against the selected backing generation's storage lifetime, independently of its previous contents and sibling fields. At a control join use the longest-lived possible destination. Individual storage follows its installed release place; arena storage follows its allocation arena. Unknown or incompatible selected storage cannot authorize a retained view. A caller-backed view store requires an existing mutable-retention output for that root (Out/BorrowMut), preserving the input lifetime at callers. |
| Contents and aliases | Update only the selected collection generation's contents. Only an in-range literal index on a syntactically fixed collection selects one inline slot for strong replacement. Dynamic/runtime indices and slice/range aliases conservatively join possible element contents; a fixed backing descriptor does not supply a view offset. Already completed scalar reads retain their old value dependency; existing header aliases observe later stores. Source replacement/Drop invalidates a retained view through every alias, including imported calls and loop joins. Sibling storage cannot acquire the selected field's content edge. Sequential local element observations remain permitted; concurrent task conflicts and explicit call no-alias obligations remain enforced. |
| Effects | Existing mutable-access, retention and purity inference applies to the whole borrowed record parameter and selected backing. Private owned stores keep existing purity; an explicit exclusive parameter keeps its established effect. New input dependencies must reach callers through existing summaries or their existing conservative fallback. No new interprocedural view writability promise follows. |
| Exclusions | Slice/SoA fields, dynamic record-element replacement, owning String or aggregate element overwrite, SIMD/mask/function elements, indexed Move extraction and partial exclusive field borrowing retain their existing admission. This capability changes scalar dynamic-array fields, not collection formation or every indexable carrier. |
| Representation and artifacts | Existing HIR AssignIndex path, MIR Field/SliceLen/SlicePtr/PtrStore and bounds CFG suffice. No new IR variant, native ABI, persistent format or public cache option. Existing complete type graph and compiler identity own artifacts. Existing mutable summaries must round-trip unchanged through interface19; body replay recomputes the new admission and lifetime checks. This does not close the existing native exclusive-call refusal for borrowed-content collections described below. |
| Prerequisites and sources | Plan52 requires proved authority for a new write path; the owned array leaf and existing whole-record exclusive mode satisfy that boundary without granting view authority. Plan171 supplies post-eager liveness; Plan19 supplies exact projected storage generations. Update draft.md, language-spec.md, design-notes.md, the Settled implementation account, Plan19 and Plan171's historical deferral. No library API or translated library surface changes. |

## Implementation strategy

Admit DynArray alongside fixed Array in the source field-path gate and both
checked-HIR gates, retaining the shared Copy-element classifier and exact width,
index, RHS and mutability validation. MIR reads a nonempty selected path with
its existing typed Field operation after both eager children. It then uses the
same bounds CFG and pointer store as a root dynamic array. Do not load or reserve
a stale header before the operands.

EscapeCheck already records exact field headers and a generation directory with
separate strict `storage_region`, shortest source lifetime, contents, allocation
mode and release identities. Consume that selected outer-header evidence for
field destinations rather than the legacy whole-local backing map. Require exact
compatible identity and a live directory entry; use caller roots/modes for the
existing retention-visibility check. Preserve the root-only fallback behavior
for existing view paths; this extension creates no fallback writable view.

Both `apply_escape_index_store` and `update_generation_collection_contents`
currently enumerate all headers and prepend the syntactic collection path to
each generation's content. That is not the meaning of a field's independent
buffer. Select the exact collection header first, then append only the element
selector/field suffix to its generation content selector. This shared axis must
cover existing root/fixed-field/SoA stores as well as the new dynamic field.
Strong replacement also requires syntactic fixed-collection coordinates. A slice/range has no tracked offset, even when its generation descriptor is fixed: join all compatible backing slots instead of clearing the relative-index slot. Keep the ordinary finite generation domain and existing worklists; no recursive
summary expansion, new lattice or iteration limit is introduced.

Audit both MoveCheck walkers, selected lifetime/read-only checks, mutation roots,
byte-observation invalidation, mutable retention and fallback observer updates.
Generation contents are the authority for selected aliases; legacy conservative
facts must not erase incoming roots or turn a sibling into selected storage. The
syntactic dynamic collection's fallback retains its record prefix. An observer with
a complete typed header set receives legacy dependencies only at headers sharing a
selected generation and overlapping content path; unknown/missing/incompatible metadata keeps the whole-local
fallback. `expired_contents_preserve_independent_siblings` pairs expired selected
contents with readable field/nested/fixed/root siblings and aggregate observers;
`dynamic_array_field_observer_fallback` pins complete versus malformed metadata.
Known fixed backing viewed through a compatible slice collapses its internal slot
coordinates within that view header before aggregate normalization. Every lifetime,
ended, read-only and validation root remains; the generation content table still
supplies indexed selection; `dynamic_array_field_slice_content_scope` pins every
retained root and the unchanged content table. Generic malformed-path fallback stays conservative;
`projected_return_provenance_fails_closed` and the readonly/validated observation
matrices retain that boundary.
Existing conservative whole-parameter summaries may over-retain input roots;
this capability does not promise new cross-call field independence.

## Implementation closure matrix and owners

All new driver owners live in `dynamic_array_fields` under the existing bounded
`owned_fixture` supervisor, with exclusive scratch ownership and direct compiler
APIs. Reuse existing owner targets where they discriminate the invariant.

| Axis | Exact implementation / acceptance owner |
| --- | --- |
| Formation and siblings | `dynamic_array_fields::values_and_types`: every admitted primitive width/family, str, direct/nested/generic fields, zero/one/multiple lengths, unchanged scalar/owned/readonly-view siblings; unchanged media witness. `retention_and_aliases`: immutable/shared/borrowed-match/moved roots and forbidden slice/String/record fields. `align_mir::delegated_ownership_and_shape_gates_agree_with_sema` retains the complete unsupported scalar-class and malformed-width sweep, including vector/mask/function rejection. |
| Selected storage and retention | `dynamic_array_fields::retention_and_aliases`: fixed/dynamic field twins; different sibling buffers; local/parameter/arena source versus longer destination, whole/field replacement and source expiry. Existing `return_provenance::{indexed_str_store_mixed_region_and_heap_join_keeps_the_longer_backing,storage_generation_move_replacement_escape_matrix}` closes heap/arena joins and transferred ownership; the exact selected-storage unit also joins Caller with Frame/inner-Arena destinations. Preexisting slice aliases observe a store; expired incoming text rejects via root, selected field, scalar read and slice. Exact selected-header unit owners in `align_sema --lib dynamic_array_field` verify unaffected sibling generation content and fail-closed unknown/missing/mismatched metadata. `dynamic_array_field_offset_alias_content` pairs fixed-root literal strong replacement with full/offset/nested range aliases using literal/runtime indices; both analysis contents must retain every possibly affected old/source dependency for offset-free views. |
| Evaluation/control | `dynamic_array_fields::evaluation_and_control`: index then RHS once, replacement during either operand selects fresh header, consumption rejects after either child, if/match/else/?/map_err/loop joins, return/break/error before final bounds/write. Both transparent-spine child-size choices are covered. Existing `assignment_target_liveness` remains in the owner bundle. |
| Calls and effects | `dynamic_array_fields::imports_and_cache`: whole-record exclusive direct/imported calls and concrete generic record helpers, checked-HIR caller retention after stored str and caller input replacement/Drop rejection; native scalar helpers execute, while borrowed-content native calls preserve the separately recorded existing producer refusal, shared/exclusive overlap rejection and valid sequential local observations. Parameterized task-capture conflict and parallel-purity negatives; existing mutable-retention/interface owners cover summary transport. |
| Construction / move / null / Drop / return | `dynamic_array_fields::allocation_lifecycle`: native counted owners construct, mutate repeatedly, move/extract/replace and drop records/arrays through success, early error and arena paths; no allocation at mutation, no ownership transfer or double cleanup. Existing `owned_field_extraction`, `owned_field_replacement` and storage-generation owners retain genuine Move behavior. |
| Bounds | `evaluation_and_control`: negative/len/empty runtime bounds fail with the normal diagnostic; eager side effects precede the trap, and a terminating child never traps. Successful edge indices update only the selected element. |
| Checked HIR/MIR | `align_mir --lib dynamic_array_field`: malformed root/path/width/index/RHS/mutability and forged slice/Move element admission reject; semantic replay rejects eager root consumption and retained-view escape. Typed Field must select the right header; malformed root/path/header-result witnesses reject through full validation and the scope-independent preflight shared by partition emission; `malformed_header_precedes_codegen` rejects an unused peer before partition formation and full object publication. Native bounds owners exercise the unchanged checked pointer-store sequence. |
| Generic / interface / cache | `imports_and_cache`: imported ordinary/concrete-generic records and helpers execute whole/per-unit, cold/hit and same-layout field-type/source edit/restore. Existing interface summary codec owner runs with the selected summaries, no format change. |
| Resource/termination | Existing finite projected paths and CFG joins; counted lifecycle proves no mutation allocation. No throughput/resource improvement promise or benchmark gate. Owner binaries remain below the repository's bounded deadlines. |

One capability may exceed 1,000 hand-written lines because the field admission,
selected destination lifetime, alias-content publication and malformed-record
proof must land together. Splitting admission from retention would publish an
unsafe or dormant chain and duplicate proof. Independent partial-exclusive or
view-authority extensions remain deferred. Before coding, finish the author
ledger-to-source pass and obtain one fresh independent adversarial strategy
review. Before publication, perform the matrix-to-diff pass, the compiler
self-review, one full-diff independent review, then final-SHA preflight.

Independent strategy review found one P2 coordinate-system omission. The ledger, shared-updater strategy and `dynamic_array_field_offset_alias_content` owner above now restrict strong replacement to syntactic fixed collections and require conservative alias joins. Other authority, retention and post-eager boundaries were accepted; no ownership strategy or new summary was added.

Author baseline qualification: function-value exclusive calls already conservatively invalidate subsequent use of an owned record on merged main, for both whole-array-field and String-field replacement. Indirect calls carry no precise mutable-retention summary and retain all compatible argument roots. Plan194 preserves and tests that conservative refusal; precise indirect mutable outputs remain a separate existing boundary. Native call positives use direct/imported helpers. Generic record instances are concrete; unconstrained T indexed assignment retains the existing root-array restriction. This does not widen generic element bounds or claim new call-summary precision. The focused independent boundary inspection confirmed the same pre-existing cause.

## Existing borrowed-content native-call boundary

Merged main rejects native emission for ordinary `borrow mut array<str>` indexed
stores, fixed str-array field replacement, and owned str-array field replacement.
The source checker and checked HIR accept their valid lifetimes. Canonical borrowed
call validation currently reuses an aggregate-wide payload state for every selected
leaf and requires exclusive payload provenance for a Move parameter, conflating
read-only text contents with exclusive owner/buffer authority. Plan194 leaves this
fail-closed producer boundary intact. It delivers the full local element class,
including str, and native direct/imported numeric helpers; borrowed-str helper
retention is certified through checked HIR/interface transport only, not claimed as
a completed native call surface.

`imports_and_cache` pairs the new field store and existing whole-field replacement:
both valid text helpers pass whole-program semantic replay, publish the callee's
retention summary through its interface, and retain the existing producer refusal
at whole-program emission and per-unit certification, while expired caller text fails before MIR. Local str writes,
including preexisting aliases, execute natively. The immediately following separate
capability must distinguish selected payload readability from complete owner/buffer
authority, with malformed-MIR, mixed-owner, root/field and shared-input controls;
simply weakening BorrowMut to READ is forbidden. The focused independent boundary
review accepted this useful provider boundary and required the explicit qualification.
