# Borrowed slices of existing Move elements

Status: implementation candidate under the reviewed V1/R67 contract selected in plan 40. This closes an
existing array/slice implementation gap without widening owning collection types.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Formation | Existing `slice<T>` annotation/argument/field coercion and `values[start..end]` can borrow an already-admitted contiguous AoS collection whose element is an ordinary record, including a Move record, or owned string. Fixed source arrays keep their existing literal-or-named-local restriction; dynamic arrays keep existing receiver rules. Re-slicing the resulting slice is admitted. Bounds default to zero/length. No new collection owner or element formation is implied. |
| Record reads | `view[index].field` keeps ordinary ElemField semantics: Copy leaves stay Copy, owned string leaves project to str, other Move leaves cannot be loaded as values. Nested paths follow existing record projection rules. `row := view[index]`, whole-record match/return/store/capture/by-value argument, mutable element borrow and writes through the slice remain rejected for Move elements. |
| Shared call place | `inspect(view[index])` can select an explicitly shared `borrow` parameter when the element satisfies the existing plan-28 BorrowedDynamicPayload classifier. The slice receiver must be a stable named local, borrowed projection, or struct-field path, as existing indexed borrowed arguments require. No temporary slice call-place admission or broader opaque/resource payload grammar is added. Ordinary field reads do not acquire whole-record shared-call authority. |
| String reads | `view[index]` for slice<string> returns str using the same physical String/logical Str rule as dynamic array<string>. This does not return string or widen by-value generic `slice<T> -> T` at T=string. Fixed string collections, if admitted by existing formation, can produce this same view; no newly owning fixed-string family is introduced. |
| Lifetime and ownership | Slice is the existing Copy pointer/length view. It retains the complete source storage generation and transitive input/arena/owned-field roots. It creates no allocation, clone, element load/transfer, nulling, Drop or cleanup bit. Source destruction, move, replacement, overlapping exclusive borrow or element replacement invalidates live derived views under existing conservative rules. Generic/imported return and mutable-retention summaries preserve every source root. |
| Control/error order | Statically check receiver then start/end or index in source order. Evaluate each once. Existing signed bounds hard error occurs before later indexed-call arguments. A terminating receiver/bound/index/later argument prevents all subsequent bounds/pointer/call/result actions. Indexed shared-call pointers form only after the existing eager-operand invalidation checks. Ordinary slice formation itself is Pure; effects of receiver/bounds remain visible and inferred. |

An existing Copy-element view remains unchanged. No slices of new owning arrays,
SoA, specialized response arrays, nested owning array elements, sums, resources,
boxes, builders, buffers or opaque handle elements are admitted by this change.
A record's admissible field graph remains the existing owning-record collection
formation rule; projection and shared-call consumer classifiers are independently
unchanged except for accepting the new slice base. No hidden owner is fabricated
for a borrowed element. Temporary owning sources keep existing synthetic-owner
and frame/arena escape rules; fixed-array literal backing storage must remain
alive for its view and cannot escape its owner scope.

## Compiler and identity

Keep Index, ArrayToSlice, SliceRange, ElemField and BorrowedIndex; no new HIR/MIR
variant, public syntax, native ABI, runtime helper or canonical scalar tag.
Introduce one source/HIR shared classifier for viewable elements, separate from
ordinary value readability. Do not weaken collection_element_read_ok: exact Move
whole-element loads stay closed. Exclude non-AoS record receivers before applying
the element classifier.

Extend BorrowedElementBase/borrowed dynamic place certification to Slice only
for the existing shared-call payload domain. The descriptor reserves the exact slice header place/generation as well as the
complete transitive backing-storage roots from index evaluation through later
arguments and call action. Rebinding a local or struct-field header, even to a
shorter range of the same backing source, invalidates this pending indexed call.
Bounds and deferred pointer construction must use the same reserved header. It conveys read/shared
access only. HIR recomputes source type/layout, result, stable base, concrete
payload, target mode, child termination and eager snapshots. MIR/LLVM recover the
physical source element from the authenticated collection, guard bounds and
validate every source/result/access relation before pointer construction.
For Slice(String), and only that added mismatch, physical String may load as
logical Str. Slot and projected owner provenance must never be inferred solely
from an asserted element type or forged path descriptor.

No persisted encoding changes. Existing complete canonical type graph and body
fingerprints identify the new allowed programs; changed bodies invalidate caches.
Imported declarations reconstruct ordinary slice/record graphs and return/retention
summaries. There is no text/wire/native boundary, global native state, inspection
table or optional field product; those ledger dimensions are N/A.

## Implementation closure matrix

| Axis | Exact owner |
| --- | --- |
| Formation and carrier domain | align_driver `move_record_slices::formation_and_type_domain`: dynamic/fixed existing record arrays, annotation/call/field coercion, explicit bounds/re-slicing, String sibling, Copy controls; reject specialized/opaque/sum/nested owning element views and Move whole-value reads. No new owner construction, replacement or Drop behavior outside the view. |
| Borrowed places and consumers | `move_record_slices::field_and_shared_calls`: direct/nested Copy and string fields, direct/imported/function-value shared helpers, helper returned view, generic concrete and symbolic callers, forbidden move/mutable borrow; preserve the existing payload classifier. |
| Other slice consumers | Existing `align_sema::tests::move_copy_positions_are_rejected` plus `move_record_slices::formation_and_type_domain` pin unchanged domains for to_array/chunks/shuffle/sample/map_into and callback/pipeline consumers, using Move-record/String twins. No whole Move-slice clone, element materialization or callback ownership is admitted. |
| Roots and invalidation | `move_record_slices::source_generation_and_retention`: direct/field/projected/slice-parameter sources, return and mutable retention; source move, replacement, element mutation and exclusive borrowing; input/frame/arena escape; unrelated-root positives. Exact direct and imported/indirect fallback summaries. |
| Control and eager order | `move_record_slices::control_and_eager_operands`: if/match/else/?/map_err/loop joins; receiver/start/end/index/later argument evaluation once; terminating return/break/?/abort/exit paths; negative/end bounds isolated subprocesses; later source invalidation rejection before action; local and struct-field slice-header replacement, including same-source shorter slices, must reject. Existing owned_temporaries scope owner covers block/unsafe/arena/named-arena/task-group receiver frames, extended if needed. |
| Cleanup and no copy | `move_record_slices::owned_source_cleanup`: whole/per-unit allocation/free deltas around projection calls, original source reusable, projected str explicitly cloned into an owned string; root and element Drop witnessed exactly once, omitted-source-Drop negative control. MIR/LLVM inspect no clone, source null, element owner or cleanup generated by the view operation itself. |
| HIR/MIR trust | `align_mir::validate_hir_tests::move_slice_records_reject_forged_shapes`; LLVM `move_slice_mir_gate`: Copy and Move record twins, String/Str physical/logical twins, forged SoA and unsupported elements, owned result, mutable authority, base/root/path/index metadata, missing header-place reservation, and stale/missing source facts reject across every lowering/emission entrypoint. |
| Interface/cache | `move_record_slices::interfaces_and_cache`: whole/per-unit output, borrowed return/retention from imported and generic code; cold/hit, body edit and revert, no consumer-source modifications. |

One end-to-end capability closes admission, consumers, provenance and validation.
A dormant view producer without safe consumers is not a merge boundary. Expected
handwritten changes may exceed 1,000 lines because the shared-place and temporary
source proof spans all compiler stages; keeping one boundary avoids duplicate
ownership and producer certification. No performance/resource benchmark is added:
no-allocation/no-copy is a correctness invariant witnessed by IR and allocation
counters, not a throughput/RSS promise.

Propagate normative contract to draft.md, language-spec, design-notes, Settled,
plan28/30's current boundary references, HIR/MIR guides, array guide EN/JA, and
plan40. Record capability status once. Consumer adoption remains external.

Author pass: viewability and value readability are separate; AoS source layout
is checked independently; only Slice(String)->Str changes physical/logical
indexing; indexed shared calls retain stable-base and payload restrictions.
Transitive source roots, early termination, and cleanup are named per owner.
No later runtime milestone or language decision is consumed.

Independent review (2026-09-10): two P2 findings fixed before implementation. Header-place/generation reservation is separate from backing-root lifetime and spans every later eager operand; the matrix includes same-source shorter-header rebinding. Non-indexed consumer domains remain closed under their existing owner, and explicit cloning refers only to the projected str. No broader materialization or callback domain is admitted.

## Implementation closure

The producer and HIR validator share collection_element_view_ok; ordinary value
read and materialization predicates remain unchanged. MIR reuses SliceRange,
SliceIndex, IndexFieldPtr and BorrowedElementPlace, with no new variant or ABI.
Slice element return facts use backing headers; separate pre-index and completed
argument reservations include the header slot until the shared call completes.
Receiver-first ElemField lowering and non-fallthrough index/range typing close
existing eager-order gaps for every sibling receiver shape.

Driver owners cover formation, direct/imported/indirect calls, String twins,
returned and mutably retained views, source/header invalidation, control joins,
whole/per-unit execution and cache edit/revert. The allocation witness checks two
explicit string allocations, three frees including realloc-origin builder
storage, and zero requested live bytes; removing source Drop makes it fail.
View formation and projection leave allocation counters unchanged. HIR/MIR
mutation owners reject malformed shapes, owning reads and missing reservations.

Author matrix-to-diff pass: new readable receiver shapes use the existing closed
value/payload classifiers and exact physical types. Header reservation and
backing retention are separate in MoveCheck; EscapeCheck does not cap a borrowed
slice element at its Copy header frame. Shared-path metadata still undergoes
independent root/path/owner checks. Existing borrowed_params and owned_temporaries
owners remain green; new receiver/type cases and fail-closed IR mutations close
this extension. No ABI/tag/canonical format changes or runtime dependencies occur.
