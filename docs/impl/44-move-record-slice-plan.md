# Borrowed slices of existing Move elements

Implemented extension: [plan 54 §6](54-r69-r76-prerequisite-batch-plan.md#6-shared-projection-contract-r69-and-r71)
admits shared indexed calls for existing records owning directory/cursor handles,
including slice receivers. It preserves separate header/backing-generation
reservation and excludes whole Move reads or mutable indexed borrowing.

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

No public persisted encoding changes. The reopened projection-path axis below changes an existing MIR record shape. Existing complete canonical type graph and body
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

## Reopened closure axis: complete nested projection paths

The candidate's independent review found one P2: ElemField admitted nested Move
records but lowering materialized the first intermediate record as an owning
SSA value. The existing IndexFieldPtr validator correctly rejected that Move
result. Reopening the path axis replaces that intermediate load rather than
relaxing ownership certification.

Exact MIR correction: `IndexFieldPtr { base: Operand, index: Operand,
path: Vec<u32>, struct_id: u32 }` replaces its single `field: u32`. Path is
nonempty, in declaration-field order; each nonfinal selection must be a declared
struct and every index must exist. The producer-owned root must independently be
the exact AoS dynamic record array or Slice(Struct(struct_id)). Only the complete
leaf may be loaded: Copy as itself or String as Str. No owning Move leaf or
intermediate record value, temporary owner slot, cleanup, clone or source null
is generated. A fixed array uses the existing full-path IndexField operation.
SoA keeps its single scalar-column path; it gains no nested collection shape.

LLVM resolves every logical field through that record's physical permutation,
then constructs one element-plus-full-field-path GEP and one leaf load. Producer
provenance prepends Element plus the entire declared path before the existing
selected-result path. Empty, out-of-range, non-struct intermediate, wrong root
nominal/layout, and owning-leaf mutations reject before LLVM pointer construction.
Pipeline first-level field selectors keep their existing domain and produce a
one-field vector. The text printer records the complete path. Canonical type and
interface formats are unchanged; MIR body/cache identity must include every path
ordinal in order (the existing codegen hash consumes the MIR text representation).
No runtime ABI changes or application persisted format follow.

| Reopened cell | Owner |
| --- | --- |
| Dynamic/fixed/slice root × direct/nested Copy/String leaf | move_record_slices::field_and_shared_calls and formation_and_type_domain; whole/per-unit runtime and repeated original-source use. |
| No intermediate owner and complete physical path | owned_source_cleanup with nested owning row; MIR shape assertion ensures leaf-only IndexField/IndexFieldPtr results; LLVM nested-layout output oracle. |
| All pointer-path invalidity and owning results | move_slice_mir_gate complete-path mutation sweep; move_slice_records_reject_forged_shapes HIR paths; all emission/lowering entrypoints. |
| Pipeline siblings and cache | existing struct_index and m5 owner cases plus interfaces_and_cache edit/revert; printer path-order assertion and codegen hash inequality for distinct nested paths. |

Author plan pass: every existing IndexFieldPtr construction/match is enumerated
by repository search. The change has one producer shape, one text representation,
one provenance path and one physical GEP path; it never represents a borrowed
Move intermediate as an ordinary owned value. Request a fresh independent
strategy review before implementation, then a fresh full-diff review because
this correction changes an IR shape.

The fresh independent strategy review accepted the complete-path correction and
identified the fixed IndexField String-to-Str sibling as part of the same owner.
Both dynamic and fixed paths now certify the final leaf without an intermediate
Move load. The existing fixed-array template-resource method place remains in
its existing domain; slice/dynamic resource-field value reads still reject.
The pkg_template fixed-resource borrow/finish/Drop owner pins that unchanged path.
Old struct_index and HIR view-domain expectations now distinguish admitted views
from the unchanged forbidden fixed-array parameter/type forms.

The fixed-resource sibling owner reproduced a pre-existing per-unit rejection on
clean main as well as this candidate: xml_borrowed_access required the dynamic
fixed-element-array variant for a fixed-slot BorrowedFixedElementPlace. It now
requires exactly StructArray, matching physical place validation; no dynamic
alternative is admitted. Existing index/path/type/callee/cleanup validation and
root authority are preserved. The original owner closes this correction.

The revised full-diff review found one P2 in malformed MIR: generic provenance
field traversal admitted a SoA intermediate although an inline GEP requires an
ordinary Struct. A shared strict inline-path classifier now certifies both
IndexField and IndexFieldPtr. The unconditional indexed-load preflight applies
the path and leaf checks to scalar results as well as ownership-bearing results;
scalar leaves cannot rely solely on the ownership graph. Slice, dynamic and fixed
root mutation twins reject the SoA substitution across all emission entrypoints.
This closes the original path invariant without changing the public contract or
IR strategy.

Unconditional scalar validation also exposed an existing pipeline producer gap:
a second field Project, or a field predicate after Project, read the original
source row rather than the current Copy record. All five pipeline lowering
consumers now project from the current value after the first indexed read.
This does not admit Move pipeline elements or add an intermediate Move owner.
struct_index::nested_pipeline_fields_use_the_current_record exercises reduction,
collection, map_into, partition, parallel reduction and JSON scanning with distinct
outer padding/inner values and an inner bool predicate. The existing whole/per-unit
plan-decision owner closes imported producer certification for this same shape.
