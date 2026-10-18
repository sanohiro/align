# JSON encoding from stable aggregate fields

Proposed capability for align-media AM-R11. All three consumer registers and
the VACE Guide.frames consumer were inspected. This closes source-place
composition of the existing encoder family; it does not widen JSON schemas.
The web/ws v0.10.2 release is verified; this capability preserves its callback and runtime boundary.

## Public-contract ledger

| Surface | Exact rule |
| --- | --- |
| API | Existing `json.encode(value: T) -> Result<string, Error>` and `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>` accept a named local or a nested record-field path rooted in a named local. Bound shared parameters and shared match bindings remain valid roots under their existing authority. No new syntax, default or configuration. |
| Domain | The selected type T must pass the same complete root/schema predicate as a direct local: records, dynamic scalar/record arrays, fixed record arrays and admitted sums. This includes mixed borrowed/owned JSON graphs. Primitive, fixed primitive-array, Option, SoA, slice and other currently excluded roots stay excluded. An enclosing record may have arbitrary nonselected fields, including resource owners; they are not encoded or schema-checked as part of T. Arbitrary temporary, indexed root and payload expressions remain outside this stable-record-place capability. |
| Ownership | Borrow the selected value; never move, clone, null or retain its owner or backing. Ordinary partial-move and lifetime checks apply to that exact place. The caller can reuse the selected field and surrounding owner afterward, including on encode failure. Output is one independently owned string, even inside an arena, and may outlive the entire source. No input allocation, descriptor allocation or hidden temporary owner is added. |
| Reservation | Preserve the encoder's existing conservative root reservation while evaluating the eager byte limit: neither the enclosing root nor its transitive backing may be moved, dropped, replaced or mutated until the limit completes and encoding finishes. This capability does not promise new disjoint-sibling mutation precision during the same call. Input must be valid before limit evaluation; divergence still validates and retires the reservation. Normal field reads after an earlier independent sibling move remain subject to the existing partial-move rules. |
| Errors/order | Validate arity, stable place, complete selected schema, then exact-i64 limit type. Ordinary source/alias/lifetime checking precedes code publication. Runtime limit evaluation occurs once before builder work; negative cap, selected nonfinite number or exceeded inclusive cap yields Error.Invalid, with no partial output. Exact fit succeeds. Allocation failure keeps the terminal policy. Existing declaration/element order, escaping, numeric spelling and Option omission are unchanged. |
| Compiler representation | Replace the JsonEncode base-only record with one canonical `JsonEncodeSource { root: LocalId, path: Vec<u32> }`; empty path denotes a local. Every segment must select an existing record field, with no redundant or arbitrary expression node. The selected type is independently reconstructed from local declarations. Each Pieces access must address that exact source plus its canonical schema projection; Owned graph root must equal the selected record or AoS element type. |
| Lowering/ABI | Ordinary borrowed Local/Field lowering supplies the owned-graph operand, preserving shared match projections. Fixed record-array fields use existing ElemField reads with the canonical field receiver and constant in-range i64 index, while direct fixed arrays retain their canonical local-index representation. Existing fixed-array field addressing is unchanged. Local records, shared parameters and materialized Copy match bindings use initialized slots. Shared Move payloads containing fixed arrays remain inadmissible under the existing recursive formation rule. No new runtime key, native signature, descriptor version or data layout is introduced. Existing JSON piece producer validation and builder cleanup remain authoritative. |
| Identity | Imported bodies are source text and rechecked. No interface wire field/version changes. Compiler executable identity invalidates derived code/cache. Exact field/schema identities remain in checked HIR and existing structural graph identity. No global mutable state or retained runtime inspection table. |
| Performance | No speed or resource reduction promise; no benchmark gate. |
| Mirrors | draft.md, language-spec, design-notes, Settled source-form entry, core-design/json.md and ja mirror, HIR ledger19, native boundary17 encoder/source-place text, Plan59 source-form amendment and HANDOFF capability entry. Prior JSON graph/descriptor contracts remain authoritative. |

## Implementation closure matrix and exact owners

| Axis | Closure and owner |
| --- | --- |
| Formation and all schema routes | Reuse stable resolve_place and existing route selection; prepend selected path to record pieces; use canonical selected Local/Field access for scalar arrays, dynamic record arrays and unions. Fixed record-array fields use canonical ElemField, including nested element fields. `json_field_encode::stable_field_schema_matrix` covers each existing selected root route, both encoders, nested selected paths, enclosing unencodable resource siblings, empty/nonempty values and unchanged root exclusions. Compare bytes to direct-local controls. |
| Exact checked-HIR authority | Update envelope and semantic validation, independently resolving root/path and reconstructing pieces or Owned graph. Reject invalid local/path/type/root, forged sibling/wrong-root access, extra/missing/reordered pieces, wrong index/element/schema and malformed limit. `validate_hir_tests::json_field_source_and_piece_identity_matrix` extends `json_array_root_checked_hir_requires_exact_source_and_element` with field and owned-route mutations. Include fixed record-array field index/path controls. |
| Move-in/out, partial moves, replacement | No new value or owner. Canonical source read runs ordinary field move/lifetime validation without consuming it; source/root/backing snapshots precede limit. `json_field_encode::source_reservation_and_partial_move_matrix` covers moved source/root, already-moved unrelated sibling, replacement/drop/exclusive borrow through a limit call or alias, expired mixed text and shared-match field sources. Sibling mutation during limit is explicitly rejected. |
| Output/Drop/control | Existing independently owned Result lowering and runtime builder guard. `json_field_encode::field_output_lifecycle_and_control` covers repeat encode, exact/under/negative caps, nonfinite failure followed by reuse, source scope exit, arena output escape, if/match/else/?/map_err/loop/early-return and divergent limit. Native requested-live accounting proves a positive peak and zero retained bytes (the allocation-call counter excludes realloc-grown JSON output) for success, failure and unused output; input owners drop once. |
| Whole/per-unit/generic/cache | `json_field_encode::imported_generic_field_whole_per_unit_cache` uses concrete generic record fields and imported shared helpers, cold/hit execution and byte equality. No consumer code/pin/test is edited. |
| Compiler sweep and malformed producer | Update all JsonEncode construction, clone/remap/canonical consumers and body walkers; path is data, not an unvisited expression. Existing MIR typed JSON pieces authenticate their selected operand/graph and prohibit ownership laundering. Retain `align_mir::tests::owned_json_encoders_use_stack_safe_dispatch`; extend `producer::tests::producer_json_borrowed_root_requires_initialized_exact_place` with field-selected shared-match sources; `validate_hir_tests::json_field_source_and_piece_identity_matrix` supplies malformed borrowed-source twins. Retain `r63_json_sequence_is_checked_by_every_body_emitter` and `malformed_borrowed_place_type_fails_before_llvm_call_emission` as existing MIR/LLVM authority; add a field-selected operand mutation to the former only if its current cases cannot discriminate the new route. |
| Fixed-array formation and addressing | Preserve the existing initialized-slot lowering. `json_field_encode::borrowed_fixed_record_array_field_matrix` covers local records, shared parameters and Copy match payloads, including a sibling admissible Move payload, nested element fields, both encoders and whole/per-unit native execution. Retain formation negatives for shared Move payloads containing fixed arrays in Option, Result and user sums. The unchanged checked-HIR borrowed-binding validator independently enforces that formation boundary. `producer::tests::fixed_record_field_slice_producer_matrix` closes exact initialized root/path/type/length and nested element ownership forwarding. |
| Existing family | Run `m5_json_bounded`, `mixed_json`, and `client_admission_composition` focused JSON source/reservation/cleanup owners alongside new `json_field_encode`. Existing descriptor/runtime ABI owners need no rerun unless their production code changes. |

Author extraction must bind every unchanged claim to current code before the
fresh strategy inspection. In particular fixed record arrays are valid record
fields, so they cannot be silently omitted from source-place composition.
One complete capability is preferable to separate scalar-array and owned-graph
admissions: those splits would duplicate reservation and exact-source proofs.
If the diff exceeds 1,000 handwritten lines, the larger boundary is justified
by closing both encoders and every existing route with one source authority.
Follow normal self-review, fresh full-diff inspection, owner tests and final-SHA
preflight before publication. Consumer adoption is separate from provider merge.

## Author source-to-ledger pass

At release candidate 9ebc8bf3, `json_encode_parts` uses `place_local` and
`json_object_parts` owns declaration-order Local/Field/IndexField pieces.
`resolve_place` already resolves nested record paths. `is_field_ok` admits
fixed record-array fields; `lower_index_field` already lowers them through
`array_source_slot` and MakeFieldSlice. This justifies the explicit fixed-array
cell instead of silently leaving a route out. `owned_json_piece` deliberately
uses ordinary local lowering to preserve borrowed match bindings; its field
replacement must preserve that same authority, not make a synthetic owner.
`json_encode_parts_match_source` and `owned_json_source_ok` independently bind
source/schema in the body validator; both envelope and semantic passes need the
new source. MoveCheck reserves base/backing/header generations before the eager
limit, and its Finish action retires the reservation even on divergence.
The field extension preserves that order and conservative root scope while
checking the selected field's move state. MIR JsonEncode already contains only
pieces, bound and out slot; it remains unchanged. No fixed-array MIR record or lowering change is required. The native return
owner exposed an existing producer-equation omission: MakeFieldSlice admits
StructArray structurally but its ownership equation recognized only scalar
arrays and only a bare element projection. Match MakeSlice by authenticating
the exact record array and forwarding nested element paths into the selected
slot field. The native fixed-field return owner and a malformed MIR matrix
close type/length/path/initialization authority; no borrowed payload formation
is widened. Existing
R63 and mixed-JSON owners pin failure cleanup and source invalidation. Every
new public domain and safety cell above has a named owner; no performance or
schema widening is claimed.

## Fresh strategy inspection and closure

The independent json_field_strategy_review initially raised a P2 about fixed-array
fields beneath shared Move match projections. A native fixture demonstrated that
this source shape is rejected before JSON selection. The reviewer re-inspected
formation and withdrew the finding: `borrowed_sum_payload_is_admissible`
recursively excludes fixed arrays, `check_match` creates projections only for
admissible Move payloads, and checked-HIR validation repeats both predicates.
Copy payloads instead receive initialized locals. The matrix therefore covers
all admitted fixed-array sources and retains the existing formation negatives;
no MakeFieldSlice redesign or payload-domain widening is needed. The reviewer
found no remaining P1/P2 strategy issue. Native execution, not the structural
dispatch guard alone, proves the selected addresses.
