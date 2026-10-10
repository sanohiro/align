# Fixed arrays of ordinary owned dynamic arrays

Status: implementation for align-llm Request143; evidence baseline main `f5443b35`.

## Capability and classification

The consumer keeps four separately named `array<i64>` admission fixtures because
`[empty.build(), [-1].to_array(), [vocab].to_array(), oversized.build()]` rejects
its element type. The exact diagnostic in `reject_fixed_array_element` names a
missing per-element Move/Drop path. Plan08 already derives aggregate ownership
from its contents; plans163/164 close that path for records and Strings.
PR739's rejection of leaking non-record Move elements is implementation evidence,
not permission to silently reopen every collection restriction.

The proposed Category A boundary is ordinary dynamic-array headers in inline
fixed storage, with the existing shared indexed-call model supplying a useful
consumer. Independent design review must verify this classification against
plans23/28/44/73 and the Settled entries before implementation. In particular,
plan28's fixed-element exclusion and plan44's nested-owning-element exclusion
must be distinguished from deliberate restrictions on mutable element references,
by-value Move indexing, or new recursively nested dynamic-array representations.
If the evidence does not establish that distinction, defer the disputed surface.

This is one capability: formation without transfer, Drop and stable inspection
would expose an unusable or unsafe producer. The implementation may exceed 1,000
handwritten lines because one ownership boundary crosses sema, checked HIR, MIR,
LLVM, interface replay and native lifecycle owners. One complete boundary avoids
duplicating its source-authority, layout and partial-cleanup proof. Specialized
dynamic vector/mask/fixed/slice arrays, owning element extraction and exclusive
indexed access are separate failure domains and remain excluded.

K1/plan61 and the parked general aggregate-provenance experiment remain deferred.
Existing producer-certification refusals, including fallible aggregate exports,
remain fail-closed. No general provenance expansion is part of this work.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Element domain | Admit `[A; N]` when A is an existing ordinary `array<P>` representable by `Scalar::DynArray(PrimScalar)`, or an existing AoS `array<Record>` representable by `Scalar::DynStructArray`. P is an existing valid PrimScalar: integer, float, bool, char, str or string. The existing element/record formation and restricted-resource rules still apply recursively, including the non-struct slice-bearing element exclusion and natural alignment for an inner dynamic record array. No new Scalar or Ty variant is introduced. Unit/function/sum elements without that exact ordinary header representation, specialized arrays, nested fixed arrays, opaque response arrays, buffers/handles and recursively nested dynamic arrays remain outside this extension. |
| Type, length and layout | Existing `[T; N]` syntax and u32 length identity apply at every annotation, field, parameter, result and generic substitution site. N headers occupy consecutive inline slots, each using the existing target dynamic-array header ABI (pointer and i64 length); there is no outer allocation. N=0 retains the exact element type and Move authority. Record and wrapper layout uses existing inline stride/alignment rules. |
| Construction | Check cardinality before surplus elements, then evaluate admitted elements left to right once. Fresh values, named owners, calls and existing value-producing controls use one existing transfer path. Every initialized payload has exactly one current cleanup authority: a deferred direct source or an already-transferred synthetic owner. A later terminating element releases all reached owners exactly once, without publishing the incomplete outer owner or nulling an untransferred source. Repeated overlapping consuming sources reject across constructors and calls. Empty literals retain the existing exact expected-element contexts, including an expected slice. |
| Ownership and allocation mode | The outer array is Move for every N. Whole moves transfer each contained header and null the complete source; recursive Drop releases each live inner owner in ascending outer index order using its existing shallow/deep Drop contract. Completed empty arrays contribute true to parent cleanup while releasing nothing. All directly owned leaves obey plan08's uniform free-standing/arena mode; mixed modes reject. Arena allocations remain arena-owned, and by-value calls/returns keep existing free-standing restrictions. |
| Shared element call | `inspect(places[i])`, with `fn inspect(borrow value: A)`, may inspect an admitted Move element of an ordinary fixed array, ordinary dynamic array or admitted slice through an existing shared-borrow parameter. The selected element must satisfy the unchanged shared-payload grammar. Fixed String and Move-record siblings use the same structural place rule. A stable local, parameter, borrowed binding or recursive record-field base is required. Both constant and runtime i64 indices evaluate once; temporary/nested-index bases reject. Copy index arguments retain their existing behavior. No reference value, owner, cleanup bit, allocation or transfer is created. |
| Reservation and bounds | Reserve the complete source place, transitive payload/region roots and any view-header identity before index evaluation. After index fallthrough, perform the existing signed bounds check at the indexed argument position. Keep the reservation through every later eager argument and the call. Form an element pointer only at the call action after all arguments fall through. A terminating index forms no guard, later argument or call; a terminating later argument forms no element pointer or call. Mutation, replacement, move, Drop and exclusive access that may overlap reject; unrelated mutation remains valid. |
| Views and ordinary reads | Existing range/coercion rules may produce a read-only `slice<A>` over the admitted inline headers. The view owns neither outer nor inner storage. Its `.len()` and shared indexed calls are available. Ordinary by-value indexing of A remains rejected, including generic `slice<T> -> T` instantiated at a Move A; no implicit conversion to `slice<slice<P>>` occurs. Existing String indexing still returns str, and record field reads keep their existing domains. |
| Lifetime and retention | Views returned from an element helper retain the fixed inline backing, selected inner release obligations and all contained input/arena roots. Direct/imported/generic calls and mutable destination retention use existing summaries conservatively. Moving or replacing the containing owner invalidates prior views; preserving an inline backing address is not proof that its old heap payload remains live. Returning a view of a local owner or escaping arena/input storage rejects. Borrowed sum payloads do not acquire a new fixed-array payload grammar through this change. |
| Replacement and element moves | Whole mutable local/field/enclosing-record replacement stages the RHS first, retires transferred sources, drops the old destination and publishes once. RHS failure preserves the old owner's cleanup; self-replacement preserves one owner. Moving a named dynamic array into a fixed element and moving the complete fixed owner are supported. Extracting or replacing an indexed element of this new header family, extracting a nested Move field, and element borrow mut remain rejected; existing fixed-record element stores are unchanged. This plan does not claim Request143's possible element-extraction interpretation is complete. |
| Other consumers | Preserve existing Move-element exclusions for materializers, to_array/chunks/shuffle/sample/map_into, by-value callback/pipeline inputs or outputs, owning captures and tasks. Fixed-owner and derived-slice twins must agree. An explicit helper may inspect the shared inner array using its existing APIs; no hidden clone or allocation is added. |
| Effects and errors | Outer construction, transport, views and Drop add no allocation and no new recoverable error or ambient input. Inner constructors retain their existing effects, allocation and error contracts. Existing hard bounds errors and allocation failure semantics remain. Type/cardinality errors precede lowering; malformed checked artifacts reject before pointer formation, loads or cleanup. |
| Owners and prerequisites | Sema owns formation, recursive Drop/Move, modes and lifetime/reservation validation. Checked HIR recomputes the same element/place equations. MIR owns staging, cleanup/nulling and the inert reservation/guard descriptor. LLVM only lowers certified layout and addresses. Runtime allocation/free ABIs are unchanged. Prerequisites are the shipped plans08/28/44/73/163/164; no later milestone is consumed. |
| Artifact/cache identity | Existing fixed-array type tag, nested named array spelling, nominal record graph and u32 length encode the type. Existing complete reachable definitions, concrete generic bodies, body identity and compiler identity govern replay. There is no new serialization record/tag, native signature, cache input, runtime inspection field or global native state. |
| Resource acceptance | Counted native execution must observe positive inner allocations, exact frees and zero live bytes after function cleanup, with shallow and deep inner owners. IR inspection establishes no outer array materializer/allocator. No throughput or stack-size promise is made; no performance benchmark is required. External consumer adoption and its named integration owner remain consumer-owned. |
| Source agreement | Update draft.md, docs/language-spec.md, docs/design-notes.md, the fixed-array and shared-index Settled entries, plans23/28/44/73 and the current HIR ledger rows; update core-design/array-slice-pipeline.md and its Japanese mirror. Review plan17 and existing guides for directly contradicted current promises. Record completion once in HANDOFF.md. Do not rewrite historical exclusions as historical admissions. |

There is no new text/wire boundary: existing inner strings/views preserve their
length-delimited UTF-8 and embedded-NUL semantics. No optional CLI state,
discriminator/detail product, canonical byte format or native global-state
overlap rule is added.

## Implementation closure matrix

| Cell | Planned implementation and discriminating owner |
| --- | --- |
| Formation and substitution | `fixed_array_type`, `reject_fixed_array_element`, `collection_element_view_ok` and checked-HIR array/type equations share the exact admitted domain. Extend source/HIR tables over primitive/view/String/AoS headers, generic substitution, N=0/1/2, cardinality and excluded specialized/restricted families. No rejected scalar becomes admitted only through annotation, a generic or a slice. |
| Recursive ownership/layout | Generalize scalar `DropPlan::FixedArray` over the admitted header leaves; reuse `store_array_elems`, complete-source nulling, `SlotZeroShape::Array` and iterative `drop_ty_at`. Extend the existing canonical Drop and semantic/LLVM parity matrices over both header variants, empty/adjacent/wrapped arrays and deep inner Drop. |
| Construction/control | `fixed_dynamic_arrays::fixed_dynamic_arrays_lifecycle_and_replay` covers fresh, direct-bound, wrapped-bound and branch-selected elements followed by ?, else, map_err, if/match joins, loop reads, partial-construction break and early return. The conservative loop export/replacement refusals below remain explicit. Failure stays local with a scalar result where the existing producer boundary requires it. Duplicate source-place negatives include local/record/tuple sources and eager later release; independent/copy-observation controls pass. |
| Shared place domain | Introduce one ordinary collection element classifier consumed by sema, HIR, MIR and emission certification, covering fixed scalar/record, dynamic scalar/AoS and slice bases without widening the payload grammar. Extend BorrowedIndex's existing pointer-free descriptor rather than manufacturing an owner. Parameterized source owners cover local/field/parameter, constant/runtime index, direct/imported/generic/function-value target and fixed String/record controls. |
| Fixed bounds/addressing | Preserve the existing unique `SliceLen(BorrowedPlace)` proof, extending its exact fixed-base meaning to the constant type length. LLVM must certify the base before returning that length; fixed data addresses derive from inline storage rather than loading a header pointer. The existing guard proves exact base/index/length, success-edge dominance, root preservation and action ordering for fixed and dynamic siblings. No pointer survives a later operand. |
| Lifetime and invalidation | A fixed literal creates its outer and nested inline generations; descendant owned/view headers forward their existing producer identity. Contained owned header storage is transported with the outer value, while its actual borrowed contents, ended/unknown evidence and arena allocation mode still constrain escape. Audit collection-generation discovery, `has_inline_owned_storage`, eager value snapshots and view fallback roots for inline inner headers. Preserve inner release authority separately from fixed backing generation. Source owner crosses stale view after whole replacement/move, returned local view, retained borrow, source-index mutation, later-argument invalidation, same-source shorter view rebind and independent siblings. No K1 precision or general producer changes. |
| Replacement/cleanup | Whole replacement/self-replacement, containing-record and generic/imported transport execute with exact native counters. Failed replacement, nested wrappers and both empty-array/sibling orders preserve one cleanup authority. Indexed Move stores/extraction and exclusive element calls have source/HIR negative controls. |
| Views/consumers | Range, default bounds and annotation/call/field coercion use existing stable backing. Fixed and derived-slice twins cover shared inspection, all excluded materializers/callbacks/captures and generic owned reads. Any previously unsupported producer path remains an explicit refusal rather than a weakened certificate. |
| Allocation provenance | Isolated counted runtime child witnesses positive shallow/deep inner allocations before exact frees and zero-live assertions at function exits. Cover free-standing, uniform arena, mixed-mode rejection and escaping region-bearing str/record contents. Header transport/view formation has no outer allocator call. Reuse existing inner allocation owners; do not invent a new runtime allocation mechanism. |
| HIR trust | Extend `hir_body_validator_storage_vector_array` and indexed-borrow mutation owners over fixed bases, wrong element/length/root/path/index/owner facts, unsupported families, ordinary owned Index and mutable authority. Exercise all checked lowering entrypoints, including unreachable malformed nodes. |
| MIR trust | Extend the existing exact fixed-array construction proof to admitted owned headers: each unique constant-index initialization must dominate publication, including across inner materializer loops. Preserve exact cardinality, type, duplicate/late/non-dominating store and SSA rejection; owning results never receive Copy-only authority. Parameterize the existing borrowed-element guard owner over fixed/dynamic bases: wrong or missing length/base/index, missing/late reservation, non-dominating guard, mutated/replaced root, bad field path and duplicate SSA must reject before emission. Existing owner certificates remain mandatory for whole/partition output; empty-array cycles and forged stores retain negative owners. |
| Interface/cache | Imported helper fixture transports fixed owners and shared returned views. Whole/per-unit cold/hit/body-change runs preserve behavior, exact ownership and cache invalidation. Existing fixed-array interface codec plus nested array semantic round trip owns unchanged format identity. |
| Test lifecycle | `helpers/owned_fixture.rs` owns an exclusive parent stage and bounded compile/link/native process group. Runtime allocation counters run only in isolated children; positive witnesses precede zero checks. Retain compact evidence and remove obsolete scratch after merge. |

One parameterized owner can close several matrix cells. Before requesting code
review, every applicable row must name its final implementation and exact owner,
or an explicit supported-boundary refusal. Extract every normative must/exact/
every/before/reject/required promise and reconcile it with that matrix once.

## Author-side design pass

The ledger fixes the existing type representation, uniform ownership modes,
source-order partial construction, stable shared place and bounds timing, payload
release roots, cleanup/replacement ordering, exclusions and resource witnesses.
No new serialization/native contract needs a byte vector or ABI thunk. The
principal review questions are Category A evidence for the shared fixed place,
closure of contained dynamic allocation provenance, and whether the proposed
ordinary-array domain is independently useful without specialized representations
or indexed ownership extraction. Resolve these before implementation.

## Independent design review

The inspection-only review found no must-fix design issue and accepted Category A
for this exact ordinary-header lifecycle and stable shared-place boundary. The
existing literal-only fixed indexed Move-field path remains unchanged; admitting
a whole shared fixed element does not silently widen field projection. Indexed
extraction, exclusive access, specialized dynamic representations and general
producer expansion remain excluded. The original review is retained locally.

## Supported boundary and completed owners

The provider remains conservative for an unbound owned fixed-array call result
used as a loop's break value, and for some loop-carried replacements of an inline
owner whose original dynamic headers came from named sources. These shapes reject
before MIR rather than losing lifetime evidence. The same frame-local escape
refusal is observable on the baseline's already-admitted fixed Move-record
wrapper. General loop/aggregate provenance is deferred; this capability does not
reopen K1 or the parked experiment. The negative source owner pins both refusals.
Loop reads, scalar break/return during partial construction, ordinary whole
replacement, self-replacement and if/match owner joins are covered positively.
Fallible aggregate exports and indexed ownership extraction remain separately
excluded. Request143 therefore receives the ordinary-header lifecycle and shared
inspection surface, not a claim that every possible element-transfer interpretation
or conservative control limitation is resolved.

- `fixed_dynamic_arrays::fixed_dynamic_arrays_lifecycle_and_replay` executes shallow
  and deep owners, tuple/generic/imported transport, field/self replacement, control
  termination, empty siblings, shared String/record siblings, once-only runtime
  indices, primitive/view elements and uniform arena allocation. Whole/per-unit,
  cache-hit and changed-provider-body results agree. Before the separate arena
  domain probe, 66 `align_rt_alloc` calls and 70 `align_rt_free` calls are exact:
  four deep builders acquire their buffers via `realloc(null)` instead of alloc.
  Positive allocation witnesses precede the zero-live assertions at function boundaries.
- `fixed_dynamic_arrays_reject_invalid_transfers_and_views` covers source and
  derived-slice exclusions, duplicate/source-after-transfer moves, borrowed owner
  movement, stale/escaping/retained views, arena/mixed allocation, eager argument
  invalidation and unsupported control shapes in whole and per-unit checking.
- `fixed_dynamic_arrays_reject_forged_construction` checks malformed HIR across all
  four lowering entrypoints, complete/dominating initialization and empty-owner
  cycles/type/SSA mutations in whole and partition producer validation.
- `empty_fixed_borrowed_elements_retain_bounds` covers zero-length String,
  record and dynamic-header bases through local construction, record fields and
  borrowed parameters. Whole/per-unit native execution preserves a runtime
  length guard and stops an actual out-of-bounds access before later arguments.
  Empty construction still needs its producer witness; missing witnesses and
  the existing empty-owner cycle/type/store mutations fail closed. Matching
  forged base/length descriptors cannot retype inline storage as a slice header.
- `move_slice_records_reject_forged_shapes` and
  `borrowed_element_guard_fails_closed_before_pointer_codegen` now parameterize
  the existing malformed place/reservation/bounds/root proofs over dynamic,
  fixed record/String/header and header-slice bases.
- `drop_plan_fixed_array_composes_through_every_wrapper`,
  `sema_and_codegen_struct_layout_agree` and
  `storage_generation_expr_variant_sweep` extend the canonical ownership, target
  layout and outer-producer/contained-header-forwarder matrices. The native
  escaping `array<str>` and stale outer/inner-view negatives protect the matching
  escape and release obligations.

No serialization or runtime ABI changes require a new byte format or thunk.
Existing fixed-array interface replay is exercised by the imported native fixture.
