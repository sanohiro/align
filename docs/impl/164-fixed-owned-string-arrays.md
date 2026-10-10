# Fixed arrays of owned strings

Request129 asks for a fixed set of prepared metadata keys without a dynamic
array allocation. Its existing builder workaround remains valid and nonblocking.
This capability completes construction and cleanup for `[string; N]` using the
existing inline array, owned string, and borrowed string-collection models.
The evidence baseline is main `093d05c4` after plan163.

## Classification and boundary

Independent design review classified the missing element lifecycle as Category A
under plan23 before implementation. The baseline plan73 and Settled entry explicitly
excluded independently owned scalar elements; that historical text must not be
silently reinterpreted as an implemented promise. The reason
for the restriction is documented by PR739: the earlier string admission leaked
because fixed scalar arrays had no element Drop/source-nulling path. Both
The baseline `reject_fixed_array_element` diagnostic called that path unsupported
yet. Plan08 already defines uniform ownership and recursive aggregate cleanup;
plan44 defines owned-string collection views and explicitly anticipates fixed
string collections if formation admits them. Plan163 supplies the canonical
fixed-array DropPlan edge, but deliberately does not lift scalar formation.
This is completion of that deferred ownership capability, not a new allocation,
ownership, optional, or error model. The reviewer must check this classification
against the locked-decision protocol as well as the safety strategy.

Only the independently owned text element gains the complete lifecycle here.
Nested fixed-array elements, dynamic-array elements, arbitrary Move scalars,
handles, sums and builders remain outside fixed scalar formation. Their distinct
read/projection contracts are not prerequisites of a usable fixed key list.
K1 and the parked general aggregate-provenance experiment remain deferred.
Existing producer-certification refusals described in plan163 stay fail-closed;
this capability must not weaken them or promise general fallible aggregate
return support. A failure during local array construction is in scope. A generic-returned String slice captured
by a closure also retains the existing producer refusal: the dynamic-array baseline rejects the
same shape. Direct slice capture and direct generic-returned view observation remain covered;
this capability does not expand interprocedural closure producer certification.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Type and identity | `[string; N]` uses existing fixed-array syntax, unsuffixed decimal u32 length and structural identity. It is Move even at N=0. Its checked representation is `Ty::Array(Scalar::String, N)`, with the ordinary `{ptr,len}` String element layout. No new type, scalar or expression variant. Existing type annotation, nominal/generic record field, parameter and return positions apply; native extern/raw aggregate exclusions remain. |
| Construction | A literal contains exactly N String expressions, checked and evaluated once in source order. Existing cardinality/type diagnostic precedence is unchanged. `[]` requires an exact expected String element type, supplied by `[string; 0]` or an existing `slice<string>` coercion context. Each completed owned element has exactly one cleanup authority until the entire construction succeeds. A direct bound local remains its authority until deferred transfer completes. A wrapped bound source may instead transfer immediately into a registered synthetic owner; its original slot is then nulled before later elements run. Fresh temporaries are registered immediately. Successful publication retires all staging authorities exactly once. A later return, break or `?` cleans the completed prefix exactly once. No action is appended after a terminating child. Bare literal tail/return/break restrictions remain: bind the array where existing fixed-array syntax requires it. |
| Ownership and mode | The array is one inline owner with one runtime cleanup bit. Every directly owned payload has the same free-standing/arena mode under plan08; mixed modes reject before lowering. Free-standing array Drop releases initialized String payloads in ascending index order; uniform arena payloads are bulk-freed by their arena, never individually freed. A zero-length owner has no payload cleanup. Whole-array and enclosing-record moves null the complete source representation and retire its cleanup bit. A by-value call requires free-standing ownership, including through generic/imported signatures. |
| Reads and views | `place.len() -> i64` is N with existing receiver evaluation. `place[i] -> str` borrows the selected String; it does not copy or move an owned value. Signed bounds use the existing hard error. Existing range/array-to-slice rules produce a read-only `slice<string>` whose index also yields `str`; no implicit `slice<str>` conversion. The source is an existing fixed-array literal or stable local/parameter/recursive record-field place. Arbitrary temporary receivers retain the bind-first diagnostic. A literal-backed view uses the existing frame-bounded synthetic owner. |
| Other consumers and captures | Borrowed Str indexing does not turn a String collection into a collection of Copy Str values. Existing Move-element exclusions remain for `to_array`, `chunks`, `shuffle`, `sample`, `map_into`, materializing/by-value pipeline arguments and results, generic `slice<T> -> T` at T=string, and whole-owner closure/task capture. Explicitly clone a projected Str when an independent String is required. Existing borrow-only slice capture retains its lifetime roots. |
| Lifetime | An indexed `str` and a `slice<string>` retain the complete source storage and contained payload/arena roots. Return/retention/import/generic summaries preserve them. Source move, destruction, whole replacement and overlapping exclusive access invalidate live views. Returning a local-owned view or escaping arena data rejects. No projection creates a new owner, clone or cleanup bit. Conservative root retention is allowed under the existing analysis; this capability adds no interprocedural view precision. |
| Replacement | Whole mutable local/field or enclosing-record replacement uses existing RHS-first semantics: stage and own the complete replacement, retire a consumed source, drop the old live destination, then publish. A failing RHS leaves the old owner for ordinary cleanup. Allocation mode must agree. Self-replacement preserves exactly one owner. Nested Move-array field extraction remains rejected; move the complete containing record. Direct indexed String replacement and writes through `slice<string>` remain rejected by `indexed_element_store_ok`, exactly as for dynamic String arrays. |
| Allocation and effects | Inline array construction, transport, indexing, slicing and Drop allocate no array payload, descriptor buffer or builder. Each explicit String-producing expression retains its individual allocation and effect contract. No implicit clone, outer heap allocation or hidden runtime allocation call. Bounds errors and process aborts retain existing semantics; there is no new recoverable error, ambient option or native global state. |
| Owners and prerequisites | Sema owns formation, recursive Drop/Move, mode, views and escape validation. Checked HIR recomputes the same exact physical/logical index and formation equations. MIR owns staging, deferred transfer, partial cleanup, flags and nulling. LLVM lowers the existing inline layout and element cleanup. Runtime String free ABI is unchanged. Prerequisites are shipped plans08/44/73/163; no later milestone or package feature is consumed. |
| Artifacts and cache | Existing fixed-array interface tag, String type encoding, canonical type graph, nominal identities and u32 length are unchanged. Public definitions, concrete generic bodies and body identity still enter existing fingerprints. Compiler identity invalidates old checked artifacts. No new persisted field, scalar tag, wire format, native signature or inspection table is introduced. |
| Acceptance and measurement | Parameterized compiler owners and an isolated native lifecycle owner below establish the contract in whole-program/per-unit/cache modes. Allocation deltas must have positive String witnesses, exact payload allocations/frees and zero live bytes after function cleanup. IR must contain no outer array builder/allocation. These are correctness/resource-invariant checks, not a throughput or stack-size promise; no speed benchmark is required. Consumer adoption stays external. |
| Source agreement | Update draft.md, docs/language-spec.md, docs/design-notes.md, the fixed-array Settled entry, plan73's current element domain, plan23's Category A status, the ArrayLit/Index/current-boundary rows of plan19, and core-design/array-slice-pipeline.md with its Japanese mirror. Review plan17's matching boundary statements and the array guides for any directly contradicted promise; update only those. Record completed capability once in HANDOFF.md. Historical refusals remain historical evidence. |

There is no new native text boundary: strings retain their existing valid UTF-8,
length-delimited, embedded-NUL-preserving representation. Indexing a collection
selects a whole string, not a text byte. No defaults/discriminator products,
runtime inspection data, native process-global state or serialization record
are added. Existing element allocation failure remains terminal.

## Implementation closure matrix

Every implementation row must name its final sites and exact owner before code
review. Existing parameterized owners may close unchanged sibling rules.

| Cell | Implementation sites and acceptance owner |
| --- | --- |
| Formation and generic substitution | `fixed_array_type`, `reject_fixed_array_element`, checked-HIR `array_literal_element_ok` and type validation admit exactly String plus the existing domain. Sema and HIR tables cover inferred/annotated/field/generic arrays, N=0/1/2, exact cardinality, and excluded dynamic array/tagged-Move/handle/slice/nested elements. Invalid substitutions and retained dead malformed nodes reject before MIR. |
| Canonical ownership and layout | Extend the existing `DropPlan::FixedArray` to scalar String arrays and index its element plan in codegen. Parameterize the plan163 wrapper/deep/validity owners over scalar and record arrays. Semantic/LLVM layout parity covers String arrays at 0/1/2/37, adjacent fields and tagged wrappers; no header default or zero-length Copy shortcut. |
| Construction and deferred transfer | Generalize `store_array_elems`' guarded Move-element path to the admitted String leaf through the existing consumed-argument owner machinery. Audit direct let, field initialization, synthetic materialization, call argument, whole replacement and literal-backed view paths. New driver `fixed_owned_strings` observes fresh/direct-bound/wrapped-bound/call/branch String elements, repeated-source rejection, left-to-right side effects and interrupted construction. A later terminating element must clean both deferred direct locals and immediately transferred synthetic owners without duplication. |
| Duplicate deferred sources | Before a successful constructor/call commits its deferred transfers, distinct consuming operands must not select overlapping Move places. Compare source-place sets across operands, unioning alternatives within one if/match result without treating mutually exclusive arms as simultaneous transfers. Transparent scopes preserve sources; fresh calls/results have their own authority. Parameterized source owners cover arrays, records, tuples, sums and calls, direct/field places, unchanged fixed-resource element-place checks, nested constructors and branch controls. This restores the existing one-owner rule, not a new provenance or interprocedural algorithm. |
| Move-out, nulling and Drop | `null_moved_source`, consuming-aggregate cleanup and `SlotZeroShape` must zero the entire fixed array, including N=0/1 and record fields. LLVM's iterative fixed-array cleanup reaches every String exactly once in ascending order without freeing inline backing. The native owner checks whole-array and enclosing-record transfer, by-value/generic/imported calls, normal/early exits, wrappers and repeated move rejection. |
| Replacement | Existing whole-local and field assignment stage the replacement before old Drop; exact self-assignment and branch-selected source transfer retain one owner. Native owner covers both paths, a failing RHS, calls reached through loop joins and enclosing-record replacement. Existing `owned_field_replacement` owners cover unchanged loop/arena assignment machinery. Source/HIR negatives retain direct indexed String writes, writes through views and extracting a nested Move field. |
| Indexed and range views | Extend String physical/logical index recognition consistently in sema, checked HIR, MIR validation and emission. Reuse plan44's String slice rules and fixed-storage provenance. Constant/runtime indices, direct/field/literal/parameter receivers, repeated borrows, range/default bounds, coercion and explicit clone positives run in both compilation modes. The unchanged bounds/termination path is covered by `move_record_slices::control_and_eager_operands`; new physical/logical type equations have dedicated malformed owners. |
| Non-indexed consumers and capture | Reuse canonical Move-element and capture guards without widening them. Extend plan44 consumer negatives with fixed String and derived-slice twins for `to_array`, `chunks`, `shuffle`, `sample`, `map_into`, pipeline callback arguments/results, generic by-value element returns and owning captures. Borrow-only slice capture controls retain their existing lifetime rejection/acceptance. |
| Lifetime and invalidation | New negative source table covers return of local index/range views, use after whole move/replacement, overlapping exclusive borrow, expired arena payloads and eager later-argument invalidation. Imported/generic borrowed-return and retention twins preserve the same owner roots. No general view-summary or producer algorithm expansion; pre-existing unsupported cases remain explicit refusals. |
| Inline backing versus owned payload | Fixed-array replacement preserves the inline storage generation but releases old String payloads. Extend the existing non-header owner fallback through String and nonempty fixed arrays so an indexed/range view also retains that release obligation. Dynamic collection headers keep their existing generation ownership. Zero-length inline arrays have no payload leaf. Local/field/containing-record replacement, indexed and range views, and the existing buffer/writer siblings must retain their corresponding refusal/acceptance controls. |
| Allocation mode | Current String producers (`str.clone()` and builder output) are free-standing even inside an arena; native controls cover that behavior both inside and outside arenas. A containing record that mixes those Strings with an arena-owned dynamic-array sibling rejects. No arena-owned String producer is introduced or claimed. Existing mode owners cover arena aggregate by-value rejection. Child failure must retain the exact mode of each staged element, with no array-level flag enabled until completion. Positive counted heap leaves prevent vacuous cleanup assertions. |
| Control flow | Parameterize reached if/match/else/?/map_err/branch joins/loop joins/break/early return across construction and whole replacement. A partial construction stays local and returns a scalar when testing existing producer limits; ordinary nonfallible array/record return remains in scope. Terminated operands must not publish the array, null untransferred sources or emit later actions. |
| HIR/MIR trust | Extend existing malformed array/index/drop mutation owners with physical String/logical Str positive and forged owned-String result negative, wrong element/length, pooled owned literal, wrong slot/type and unsupported Move sibling. Every checked lowering/emission entrypoint must reject malformed metadata before pointer/load/free actions. Existing producer certificates remain mandatory. |
| Interface/cache | New imported source fixture transports fixed String arrays and records, generic forwarding and borrowed String views in whole/per-unit modes. Cold/hit replay yields identical behavior and MIR; body change invalidates existing identity. Existing fixed-array interface codec goldens remain valid, with an owned-String semantic round trip. |
| Runtime provenance/resource parity | Generated child process links the counted runtime and witnesses positive payload allocations before exact frees and zero-live assertions at real function exits. Keep payload Strings alive through each borrow. Count no outer array allocation and inspect LLVM calls for builder/array materializers. No runtime ABI/export change. |

One capability must close formation, construction, views and cleanup together.
If it exceeds roughly 1,000 handwritten lines, that boundary is intentional:
publishing an owning type before its recursive Drop and invalidation proof would
create a dormant or unsound producer. A distinct dynamic-array-element or new
mutable-element projection feature can be deferred without weakening this
usable fixed metadata-key consumer. The author ledger-to-prose and matrix-to-diff
passes precede the one fresh full-diff implementation review and local gates.

## Author-side design pass

The ledger fixes the exact type/length identity, admitted expressions, physical
layout, source-order/termination rules, borrowed result types, mode and lifetime,
cleanup/transfer/replacement order, allocation and excluded mutation domain.
Each has a matrix owner. Existing serialized records and native signatures do
not change. The only array-level resource promise is absence of an outer
allocation, witnessed independently from positive element allocations. The
Category A classification and interaction with the deferred producer boundary
remain mandatory questions for the independent design review.

The old whole-local assignment gate still says fixed values are not materialized,
although the existing aggregate materializer now supports them. Remove that
implementation-only gate for every admitted fixed array, keeping exact
self-assignment a no-op. Copy scalar/view arrays, Move records and String arrays
share the same assignment machinery; no String-specific assignment exception.
The replacement owner includes Copy and Move-record controls and empty arrays,
while existing lifetime/move owners guard their unchanged exclusions.

## Independent design review

The inspection-only review accepted the Category A evidence and capability
boundary. Its three P2 corrections are incorporated above: empty literals keep
expected-slice coercion; non-indexed Move consumers and captures retain explicit
negative owners; and partial construction distinguishes deferred direct locals
from immediate transfer into a synthetic cleanup owner. Each completed payload
has exactly one authority in either path. No ownership-strategy redesign or
producer-certification widening is required.

## Implementation and author closure

The completed capability uses the existing representations throughout. Sema's
`drop_plan`, `fixed_array_type` and `check_index` agree with HIR formation and
physical/logical indexing. `store_array_elems` uses the existing consumed-argument
staging; `SlotZeroShape::Array` and iterative `drop_ty_at` cover direct and nested
String arrays. Empty String arrays carry whole Move authority without inventing
an owned element or entering the Copy-only producer normalization. The common
`reject_duplicate_action_sources` check closes deferred source overlap across
constructors/calls. `has_inline_owned_storage` and forwarded view fallback roots
retain payload release obligations alongside persistent fixed backing.

| Matrix obligations | Exact regression owners |
| --- | --- |
| Formation, generic substitution, N=0/1/2, direct/field/parameter views, control exits, replacement, wrappers, free-standing clones inside arenas, interface/cache and resource parity | `fixed_owned_strings::fixed_owned_strings_lifecycle_and_replay`: 68 positive String allocations, exact frees and zero live bytes at function cleanup; whole/per-unit/cache hit/body-change runs. Generic String element substitution, distinct Copy-view reuse and direct borrowed slice capture are positive controls. |
| Consuming duplicates, moves, stale direct/range/imported/generic views, exclusive access, eager later-argument release, mixed allocation mode, consumer/capture boundaries and invalid generic/cardinality shapes | `fixed_owned_strings::fixed_owned_strings_reject_invalid_transfers_and_consumers`, in whole/per-unit modes; source and derived-slice consumer twins. Existing fixed-resource transfer owners retain the separate element-place rules. |
| Forged physical/logical indexing, pooled/wrong-element/wrong-length literals, zero-array producer cycles/bad stores/duplicate SSA/wrong slots, no outer allocator | `fixed_owned_strings::fixed_owned_strings_reject_forged_hir_and_mir`: all four checked lowering forms, whole/partition producer certification, codegen rejection and generated call inspection. `hir_body_validator_storage_vector_array` independently constructs valid and invalid HIR. |
| Canonical recursive ownership, layout and source zeroing | `align_sema::tests::drop_plan_fixed_array_composes_through_every_wrapper`; `align_codegen_llvm::tests::sema_and_codegen_struct_layout_agree` at 0/1/2/4/37 and `move_handle_slots_are_zeroed_as_one_pointer`. Existing deep DropPlan and `fixed_array_ownership` owners cover the unchanged iterative graph machinery. |
| Existing Copy/Move-record arrays, field producers, bounds/termination and allocation modes | `fixed_array_fields`, `owned_fixed_record` (including zero-bit provenance mutations), `move_record_slices::control_and_eager_operands`, `owned_field_replacement` and `fixed_array_ownership`. New native replacement controls cover Copy scalar/view, Move record and empty array assignments through the removed obsolete gate. |

The author pass mechanically extracts normative obligations into retained review
evidence and checks them against these sites and owners. The generic-returned
slice/closure producer refusal above is an explicit existing boundary, not a
missing cleanup path. No runtime ABI, serialization tag or broad producer model
changes. Bounds and existing arena-resource behavior reuse their owning suites;
there is no unimplemented arena-owned String constructor promised by this slice.
