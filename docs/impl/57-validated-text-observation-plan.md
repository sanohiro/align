# Validated text observations

Status: implemented local observation boundary.
This closes a local validity boundary recorded by [plan 52](52-readonly-view-provenance-plan.md).

## Existing contract and capability

`draft.md` section 12 requires every `str` and `string` to remain valid UTF-8.
`slice<u8>.as_str() -> Result<str, Error>` validates the existing bytes and returns
a zero-copy, source-bound view; invalid input returns `Error.Invalid`. Plan 37
preserves mutation through retained native buffer byte views. This repair adds
no source type, qualifier, allocation, copy, runtime check or ownership rule.

At merged #1030 (`77f763744095638a3711710c83faac8a4f0f905f`), both source and
per-unit checking accept this invalid program:

```align
fn main() {
  owner := [(65 as u8), (66 as u8)].to_array()
  mut alias: slice<u8> := owner
  text := alias.as_str() else { return }
  alias[0] = 255
  print(text)
}
```

Never execute this negative witness. The same hole exists with an older alias,
a retained unexpanded Result, a text derivative, a later eager argument,
`vec.store`, `map_into` and `rng.shuffle`. Existing buffer reallocation and
explicit `borrow mut` lifetime invalidation already reject their stale views.
Source and per-unit baseline verdicts are recorded in the local investigation.

One capability contains local observation formation, alias-aware invalidation,
value-flow preservation, checked-HIR replay and positive/negative owners. It is
useful without a new exported effect: local validation and local writes are
currently accepted in one body. Plain by-value callee writes and validation
created inside a callee require the separate interprocedural access contract;
this plan neither certifies those calls nor exports a guessed validity summary.
Existing argument-derived provenance through ordinary identity/read helpers must
preserve caller observations. Native borrowed-buffer writes remain admitted when
no stale text is subsequently used.

## Analysis contract

| Surface | Exact rule |
| --- | --- |
| Formation | On completion of `BytesAsStr`, after its byte operand completes and is checked, create a fresh analysis observation of that completed byte backing. Attach it only to the `ResultOk` text leaf, never the error payload or source byte value. A runtime Err carries no observable text. |
| Identity | A distinct `StorageOrigin::TextValidation { expression }` uses the immutable HIR expression address. Existing `Current`/`Prior` recency distinguishes a fresh execution from retained prior executions of that site. No source span, local name, runtime counter or serialized identity is used. |
| Dependency | A validation record retains a snapshot of the selected byte backing: stable generation alternatives plus unresolved fallback roots and an explicit unknown alternative. Allocation identity is separate from its current release place and from contained-view provenance. Byte subranges conservatively share their backing identity. |
| Mutation | At an admitted write action, end every reaching validation observation whose backing may overlap the completed write target. Any unknown alternative prevents a disjointness proof. Known disjoint generation alternatives remain independent; unequal symbolic caller-storage keys alone do not prove disjointness. Fallback evidence uses the existing alias-root rules without flattening contained view owners into known backing. |
| Value flow | `BorrowRoot::Observation`/`EndedObservation` transport the validation identity through the existing projected `BorrowFact`, local/header/content facts, pipeline snapshots and eager completion snapshots. The distinct origin identifies a text observation for diagnostics and filtering; cursor observations keep their existing behavior. |
| Invalidation | Mark matching roots ended in all fact lanes and their diagnostic indexes. Do not end the backing allocation, its owner generation or unrelated raw-byte aliases. Mutation does not retroactively reject an earlier completed text use; subsequent text use and an enclosing operation with an invalidated eager operand reject. |
| Revalidation | A later validation creates a live Current observation; old values keep an ended Prior observation. Rebinding a byte descriptor before a write changes only that descriptor's target and cannot retarget an earlier validation. |
| Text derivatives | Borrowing, trimming, slicing and borrowed decoded text preserve input validity observations whenever the result still observes those bytes. Existing owned text copies detach source dependencies. A plain scalar result does not retain a text-use obligation. |
| Return to bytes | `str.bytes()` validates use of its text operand, then removes only text-validity observations from its resulting byte facts. Source lifetime, cursor observation and read-only authority remain. Bytes completed before a later write remain usable as bytes. Converting an already invalid text value still rejects at the operand use. |
| Diagnostics | A stale text local or completed operand reports that its validated bytes were modified and asks for revalidation or an explicit owned string copy. Keep source locations and existing one-root-error suppression. Do not describe the event as reader advancement, owner movement or reallocation. |
| Runtime and ABI | Compiler-only state. No allocation, copying, owner transfer, nulling, Drop, native ABI, emitted layout or evaluation-order change. Existing structural, lifetime, no-alias, bounds and ownership checks remain prerequisites. |
| Interfaces and cache | No HIR variant, type record, interface field or format version changes. Local bodies are recomputed after generic substitution and during checked-HIR replay. Existing compiler/artifact identity invalidates changed checker implementations; no new persisted format exists. |

## State integration and finite closure

Keep the record in the existing generation directory, with no release place and
no owned storage header. Its text backing dependency is optional on ordinary
entries and present on validation entries. Joining two records unions generation
and fallback alternatives and ORs unknown; a missing or contradictory validation
record is unknown, never proved disjoint. Invalidation is a may-event and remains
sticky on a reaching old observation.

Generation renaming updates dependency keys and roots as well as all observer
facts. Pruning follows observation-to-backing dependencies before removing a
directory entry. An observation does not mint a releasable owner or extend the
runtime lifetime. `MoveControlEdge` differential renaming and the existing loop
worklists own recency and convergence. The state is finite over HIR sites,
Current/Prior alternatives, typed storage generations and existing root paths;
there is no iteration limit that produces an accepting verdict.

Formation belongs in `record_value_completion`, before publishing its final
facts and snapshots. Its Current-to-Prior transition must rename the completed
operand dependency and the enclosing local fact consistently; it cannot construct
a new observation and then accidentally rename that new root to Prior.
A marker cached in a walked fact must not be lost by recomputing the original
`BytesAsStr` syntax. `normalize_borrow_fact` keeps the ResultOk leaf explicit.

The directory and BorrowRoot already distinguish observation lifetime from
release ownership for XML cursors. The new origin must be handled exhaustively
by origin-path/renaming logic, summary conversion, pruning and diagnostics.
Do not broaden XML's opaque replacement special case to ordinary byte owners.
For public lifetime-summary conversion, a validation marker contributes its
underlying source lifetime roots, not an exported validation identity. Direct
caller-origin substitution still preserves an already-created caller observation.
The owner must prove that distinction rather than infer it from an empty summary.

## Mutation inventory

| Existing action | Required integration |
| --- | --- |
| `AssignIndex`, `AssignElemField`, `AssignElem` | `update_mutable_collection_contents` snapshots the selected backing before any content transition, then ends overlapping text observations. A descriptor-field replacement alone is not a write into that field's old pointed-to allocation. |
| `VecStore`, `ArrayMapInto`, `RandShuffle` | `invalidate_collection_mutation_target` uses operand-completion headers and fallback evidence, before visible mutation. The later syntactic descriptor cannot replace the snapshot. |
| `ProcessLive` `OutBytes` | Classify every argument declared as OutBytes as a byte write after all operands complete; share the target invalidator with the read-only destination check. Retain the existing native validation/error and no-alias rules. |
| Direct explicit `Out` arguments | End observations against each pre-call completed destination, whether or not the mutable-retention result contains borrowed content. An empty retention set is not evidence of no byte writes. Source formation continues to reject first-class functions with Out parameters; do not widen that domain. |
| Explicit `BorrowMut` arguments | Preserve current lifetime invalidation and additionally close alias-visible byte writes against pre-call destinations. Existing declared mode remains conservative; no new claim that a header replacement is an exact write effect. |
| `ReaderRead`, `ReaderReadLine`, `HttpReadStreamRead`, `FilePread`, `UdpRecvFrom`, `CryptoRandom`, `BufferPut`, `BufferAppend`, `HttpSseStreamNext` | Existing storage invalidation already rejects dependent text when storage can be replaced. Retain the owner tests and ensure the new record does not suppress that existing ending. |
| Builder contents, RNG state and XML cursor advancement | Preserve existing action semantics. They do not write arbitrary byte backing merely because they retain views. XML keeps its independent cursor-observation mechanism. |
| Ordinary calls, callbacks and unsafe writes | Ordinary hidden writes require interprocedural effects and remain deferred. Explicit unsafe writes retain their unsafe contract. Do not claim these are closed by local root unions. |

Audit this inventory against `source_visible_mutation_action`, explicit call
modes, process input descriptors and every assignment arm before requesting the
implementation review. Any additional safe byte-output sink found by that sweep
belongs in the same capability and matrix.

## Implementation closure matrix

The owners below bind the local observation boundary to source and replay checks.
Existing owners are reused where their current assertions detect a regression.

| Axis | Exact owner and required cases |
| --- | --- |
| Formation and independent byte reuse | `align_sema::validated_text_observation_local_matrix`: array and retained native buffer views; owner/preexisting alias/range write; last text use before mutation; fresh revalidation; raw byte reuse; byte conversion completed before mutation; read-only authority remains. |
| Typed selection and carrier flow | `align_sema::validated_text_observation_projection_matrix`: retained Result before unwrap, Option/record/fixed-array carriers where admitted, unrelated sibling, descriptor rebind, trim/range and borrowed JSON text; owned string clone detaches. |
| Every write sink | `align_sema::validated_text_observation_sink_matrix`: indexed actions, SIMD store, map_into, shuffle, process OutBytes, direct Out and admitted direct/indirect BorrowMut; pair each with independent backing or use-before-write. Unsupported indirect Out remains rejected at formation. |
| Joins, exits and recency | `align_sema::validated_text_observation_control_matrix`: if/match/else/Try/map_err, loop backedge/break, early exit, fresh validation on repeated site, retained prior observation and mutation in a later eager operand. Existing `storage_generation_control_edge_join_matrix` and `storage_generation_repeated_control_routes_keep_generation_recency` protect shared recency. |
| Move, replacement, return and cleanup | Local matrix moves the owner without changing alias identity, replaces/drops a source to retain existing lifetime errors, and uses identity/read helper transport without inventing callee-created observations. Existing buffer invalidation and native borrowed-buffer owners must pass unchanged. No new cleanup implementation. |
| Generic/imported and replay | `align_driver::borrow_liveness::validated_text_observation_whole_unit_parity` covers local generic bodies and imported read/identity helpers in source/per-unit mode. `align_mir::validate_hir_tests::validated_text_observation_checked_hir_replay` substitutes a same-typed stale view use into a structurally valid candidate and requires rejection before lowering. |
| Safe execution | Driver owner executes byte reuse, fresh validation and independent string-copy controls with exact bytes/text. Compile-only invalid-UTF8 witnesses never execute. |
| Neighbor strategy | Existing `std_xml` cursor/eager observation owners, `m12_read_line`, native borrowed-buffer `struct_handle_fields` and `consumer_borrow_boundaries` remain valid. No regression in ordinary mutable byte APIs. |
| Cost and malformed input | Finite directory/CFG reuse and same structural validation. No new public performance/resource promise, so no benchmark gate. No silent accepting fallback for a malformed validation record. |

Complete one author matrix-to-diff pass before the fresh implementation review.
This capability may exceed 1,000 handwritten lines: the observation producer,
alias-aware sink transitions, recency/summary consumers and source/replay owners
must agree before any stale use is rejected correctly. Separating these into
dormant changes would repeat the same alias and control-flow proof while leaving
the demonstrated local defect open. The runtime and persisted interfaces remain
outside this boundary. No milestone,
versioned release, align-llm adoption or broad language redesign belongs here.

## Author strategy pass

The existing implementation provides `MoveGenerationEntry`,
`BorrowState::rename_generations`, `retain_reachable_storage_with`,
`record_value_completion`, `completed_headers`, and shared
`mark_matching_roots_ended`/`invalidate_roots` transitions. These are the selected
owners for the record, recency, reachability, creation, backing snapshot and
ending rules above. `borrow_fact_one` already preserves walked completion facts;
the new observation must use that path instead of adding a second expression
walker. `summary_roots` currently converts only cursor caller origins and must
explicitly expand validation dependencies. `StrBytes` currently preserves all
storage roots and needs the narrowly typed observation removal described above.

The local assignment and shared source-visible mutation inventory have been
checked against these seams. Process OutBytes and explicit parameter modes are
separate action consumers and remain mandatory matrix rows. The strategy changes
no public representation or runtime permission; ordinary call effects remain
explicitly deferred. The independent preimplementation review assesses this
complete observation strategy and its capability boundary before code changes.

The baseline owner also accepts `probe(alias, alias)` when `probe` has two plain
slice parameters and validates one before writing through the other. Entry
parameter ordinals therefore cannot certify separate backing allocations.
CallerStorage paths without an applicable no-alias mode guarantee are may-alias;
a body-local fresh allocation remains disjoint from its incoming arguments.
`validated_text_observation_backing_identity_matrix` owns this case beside owner
transfer, retained prior executions and borrowed JSON text. Native OutBytes keeps
its existing writable-root admission. Existing local-backed loop-break rejection
is unchanged; the new break owner observes a write followed by a unit break and
a subsequent stale text use.


## Strategy review resolution

The independent inspection identified six obligations. The author checked each
against the selected consumers before implementation. No second analysis lane,
public validity summary, or ownership strategy is introduced.

1. **Typed carrier facts.** `BorrowFact.projected` remains the authoritative
   path-aware lane; `sources` and `invalid` are whole-value diagnostic indexes.
   A whole Result read observes every possible payload, as existing borrowed
   carrier reads do. Reading a selected independent field does not read its
   sibling. A statically constructed Err contains no validation observation;
   an unresolved validation Result can contain Ok and must remain valid when
   read as a whole. `flatten_lifetimes` must preserve text-observation paths,
   as it already preserves read-only paths. Projection, Try, else and map_err
   use the existing typed selectors. A second validity lane would duplicate
   those selectors and their joins without adding a different source contract.
2. **Exact state.** Add `text_validation: Option<TextValidationBacking>` to
   `MoveGenerationEntry`. `TextValidationBacking` contains finite sets
   `generations: BTreeSet<StorageGeneration>`, `fallback_roots: BorrowRoots`,
   `lifetime_roots: BorrowRoots`, and `unknown: bool`. Join unions each set and
   ORs unknown. One missing side joins as unknown. Renaming maps both generation
   sets and all roots. Pruning follows the dependency generations and observation
   roots in both root sets. A write scans validation entries and all reaching
   observation roots; an absent entry is an unknown record. No reverse index or
   second loop state is added.
3. **Certainty.** A known selected root header supplies allocation generations;
   a header with `known == false` stays unknown unless independent completed
   mutable-backing evidence authenticates the complete alternative set.
   `BufferBytes` publishes the existing native owner roots as known backing;
   plain view forwarding preserves that evidence. An arbitrary call or mixed
   unresolved branch cannot acquire certainty merely from fallback lifetime
   roots. Generation content is never used as backing identity. Distinct
   symbolic caller alternatives may alias; a fresh body allocation is separate
   from incoming storage. Subranges retain allocation identity.
4. **Derivatives.** Completed value/storage snapshots carry text roots through
   the existing borrowing-derivative selectors. The derivative matrix proves
   trim, ranges, typed fields and borrowed JSON text. `StrBytes` removes only
   roots whose generation origin is TextValidation. It preserves cursor,
   lifetime and read-only roots and still checks its input before conversion.
5. **Calls and summaries.** Existing return-origin selection substitutes the
   completed argument fact, retaining its caller-created observation; it does
   not mint one from the callee's exported parameter root. Existing aggregate
   summary precision remains conservative, while local typed field selection
   stays exact. Summary conversion expands each validation's captured lifetime
   roots through a visited worklist, including ended markers, and never exports
   the validation identity. Apply this conversion to parallel-transfer records
   when the roots are collected, before pruning can discard their dependencies.
   Callee-created validation and hidden writes remain plan 56 work.
6. **Consumer sweep.** Discriminate text and cursor observations by origin in
   mutable-call filtering and all three diagnostic paths. Pre-action completed
   backing snapshots own every local byte sink, even with empty retention.
   Existing XML owners verify that cursor ending and replacement stay intact.

The review's absence-of-implementation observations in items 2, 4 and 6 are
bound to the exact new fields and consumer edits above. Item 1's proposed
independent lane is unnecessary because the existing projected fact and
projection-use checker already separate whole reads from selected leaves;
keeping text roots projected across materialization is the required repair.


## Implementation closure

The author extracted the `must` / `exact` / `every` / `before` / `reject` /
`required` lines from this plan and checked them against the implementation and
owners below. Source types, runtime code, wire formats and interface records
have no diff; their unchanged contracts require no new ABI or benchmark owner.

| Extracted obligation group | Implementation | Regression owner |
| --- | --- | --- |
| UTF-8 formation; ResultOk only; completion before publication; cached fact preservation | `record_value_completion`, `normalize_borrow_fact`, `borrow_fact_one` | `validated_text_observation_local_matrix`, `validated_text_observation_state_closure` |
| Backing identity; aliases; unknown alternatives; movement and generation recency | `TextValidationBacking`, `text_backing`, `completed_text_backing`, generation entry join/rename | `validated_text_observation_backing_identity_matrix`, `validated_text_observation_state_closure`, existing storage-generation control owners |
| Dependency pruning; missing records fail closed; no owner release | `BorrowState::retain_reachable_storage_with`, `invalidate_validated_text` | `validated_text_observation_state_closure`, local byte reuse and owner-transfer controls |
| Selected carriers and derivatives; old Result; owned copies; source errors remain | `BorrowFact::flatten_lifetimes`, existing projection/unwrap/derivative selectors, `StrBytes` filtering | `validated_text_observation_projection_matrix`, `validated_text_observation_completion_matrix`, backing-identity JSON case, existing borrowed-buffer owners |
| Local indexed and collection writes use completed backing | `update_mutable_collection_contents`, `invalidate_collection_mutation_target` | `validated_text_observation_local_matrix`, `validated_text_observation_sink_matrix` |
| Every native OutBytes and explicit mutable mode; empty retention is a write; no indirect Out widening | `apply_builtin_mutation_action`, `apply_mutable_call_effects`, `completed_text_call_backing` | `validated_text_observation_sink_matrix`, `validated_text_observation_mutable_call_contents`, existing read-only destination owners |
| Earlier completed uses and scalar results; eager writes; joins, exits, and repeated validation | `finish_child_staging_frontier`, existing eager action/control walkers, generation renaming | `validated_text_observation_control_matrix`, `validated_text_observation_completion_matrix` |
| Caller-created observation transport; public lifetime and parallel summary conversion; diagnostics | existing return-origin selection, `expand_text_validation_roots`, `summary_roots`, three invalid-use diagnostics | state closure, local identity, whole/unit parity, and eager owners; existing XML owner suite |
| Generic substitution; structurally valid replay rejects before lowering; safe execution | unchanged body-fact replay entry point consumes the new MoveCheck facts | driver `validated_text_observation_whole_unit_parity`, `validated_text_observation_safe_execution`, MIR `validated_text_observation_checked_hir_replay` |
| Ordinary hidden writes and callee-created validation | Explicitly deferred to the interprocedural access boundary | No claim that local observations close those effects |

The storage directory and content table keep identical key sets. Each validation
entry therefore has an empty `MoveValueFact` content row, without an owned header
or release place. The state owner and array-carrier source owner protect this
invariant: omitting the row makes a later producer formation fail.

A local element store writes its collection backing. An explicit mutable call
can also write through a byte view stored in an argument's elements. Its target
snapshot follows reachable header facts separately. Current materializers can
retain element lifetime roots without a complete element header graph; a
collection containing storage-bearing elements therefore contributes an unknown
write alternative. An outer known allocation does not certify disjointness of
those contained views. Precise composite access effects remain interprocedural
work. The mutable-call-content owner proves this boundary with both Out and
BorrowMut. Existing whole-carrier read policy is retained, including whole-array
receiver reads; independently selected values remain independent.
