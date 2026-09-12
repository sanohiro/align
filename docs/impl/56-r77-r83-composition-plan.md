# R77–R83 owned and borrowed composition

Status: provider implementation complete on branch `feat/r77-r83-composition`.
R77/R79/R80/R81/R82/R83 are implemented in one provider wave with focused owner
coverage; R78 is assessed and deferred under plan 23. The branch is not merged,
and no align-llm consumer adoption or end-to-end client acceptance is claimed.

## 1. Scope and evidence

The owner requested one design for every additional align-llm composition
request, including additions to existing rows. The assessed provider baseline
is `21b3151e` (2026-09-11). Requests R77–R83 were reported against `6ca79fee`.
The self-contained reproductions below are baseline evidence, checked again using
an optimized compiler built from the assessed baseline. Every reported failure
still occurs at that baseline; all seven examples pass syntax-only formatting.

| Request | Source check | Per-unit check | Required outcome |
| --- | --- | --- | --- |
| R77 | Pass | Checked-HIR body validation rejects `build` | A projected owned string supplies a shared text parameter without moving its owner. |
| R78 | Reject | Reject | Preserve the current enum-field exclusion. Reopening is deferred; this is not an existing-contract regression. |
| R79 | Pass | Native `SignalNumber` producer rejects `inspect` | Nested borrowed optional/sum projections preserve the existing nominal Copy signal. |
| R80 | Reject | Reject | A fresh scalar copy survives its loop-local source; owned branch replacement does not retain an ended/self generation. |
| R81 | Pass | Call-argument producer rejects `inspect` | A borrowed record-array field supplies a shared slice reader returning an independently owned result. |
| R82 | Reject | Reject | Shared matching admits `process.user_namespace` through the existing borrowed-payload grammar. No owner extraction or new namespace API. |
| R83 | Pass | Return-leaf producer rejects `wrap` | Owned document/template returns compose through calls and active Result/Option paths. |

R81 blocks complete task-source collection. R83 blocks complete evaluator
document admission and measurement finalization. R77–R80/R82 remain nonblocking
for the client's selected current data flow. Nonblocking does not remove a row
from this assessment. R78 has the separate decision prerequisite below; the other
six form the implementation wave. A passing application control does not
discharge an admitted composition failure.

Plan 55 moved producer certification into `align_mir::producer`; old diagnostics
still contain "XML" for the shared resource analysis. Do not repair these cases
in the LLVM emitter or classify them as XML-library defects. Plans 52 and 53
continue to own read-only backing and independently founded initialization.

On this branch the focused owner `owned_borrowed_composition` passes all 22
whole/per-unit cases, including the seven-field R83 provider witness. The current
optimized compiler also accepts the align-llm evaluation-inputs source in
per-unit mode (`17 unit(s)`, warnings only). These are provider/compiler checks;
the blocking consumer smoke owners, source-collection witness, full evaluator
admission and measurement digest remain align-llm-owned and pending.

## 2. Authoritative public-contract ledger

These records describe the six selected targets and the R78 deferral. Existing acceptance remains
as reported above until the implementation and its owners pass. No new syntax,
user-visible type qualifier, implicit clone, runtime descriptor, native helper,
ambient option, or public function signature is introduced.

| Surface | Exact inputs and result | Ownership, allocation, errors and ordering | Implementation and identity owner | Prerequisite and acceptance |
| --- | --- | --- | --- | --- |
| R77 shared text call | An existing admitted indexed record field of type `string`, used as the argument of `fn f(borrow text: str) -> T`; dynamic slice/AoS views use a checked runtime index, while a source-formed fixed `StructArray` uses an integer-literal index. Existing by-value `str` readers remain controls. `T` keeps its declared ownership and summaries. | Read one `str` descriptor from the exact field. Evaluate receiver/index once and retain the source generation through all later arguments and the call. Bounds failure and argument termination preserve existing order. No string move, shallow owner load or extra allocation. An explicitly cloned result is independent; a returned view retains its roots. The existing fixed-resource exception remains unchanged. | Sema argument coercion and borrowed-place metadata; checked-HIR body/replay; existing MIR borrowed place/indexed field lowering. Exact nominal field and call types remain authenticated. `BorrowedElementPlace.field_path` is internal MIR metadata; the interface/cache schema is unchanged. | Plans 28/30/44/54. R77 direct/named controls, local/imported readers, source-expiry and later-argument invalidation twins. The fixed-index HIR/sema parity owner is included in the focused target. |
| R78 heap-record enum field — DEFERRED | Retain the existing `array_builder<R>()` field grammar and rejection of direct or nested enum fields. The request seeks existing payload-free nominal enums as Copy record leaves; that request is not selected for admission. | No ownership, allocation, layout, Drop or rejection-order change. The client's boolean representation is an ordinary data-layout choice, not a counted mechanical workaround. | Plan 17 §7.6 remains authoritative. Plan 23 row B4 owns reopening evidence; no compiler/interface/ABI change is authorized for this row. | Still PROPOSED. Reopening requires the recorded five mechanical sites across two independent real programs, or a subsequent explicit owner instruction overriding that prerequisite. The request register currently establishes neither. Preserve the rejection/control evidence. |
| R79 nested Copy native value | Existing `process.signal` projected through admitted borrowed `Option<Report>`, a record field and a user-sum payload, then passed to `process.signal_number(signal: process.signal) -> i64`. | Copy the active nominal signal value; preserve root, ordered projection and exact active-arm proof before the read. The native mapping remains Pure with its existing valid-signal contract. No mutation, namespace/process operation, allocation, owner transfer or additional native call. Inactive/forged paths and wrong nominal signal types remain rejected. | Existing borrowed-match HIR/MIR representation and `align_mir::producer` native input certification. Plan 50's nominal type, runtime key, tag/layout and mapping remain unchanged. | Plans 26/28/50/53. None/Some, all signals, nested/direct/helper twins, wrong root/variant/type and uninitialized-input mutations. |
| R80 fresh copy and replacement | Existing `slice<T>.to_array() -> array<T>` for its currently admitted Copy element types, and existing consuming builder freeze/owned assignment. No new element domain. The reported scalar case is `T = u8`. | A new array has fresh backing. Copy only the element's actual retained view/region facts; primitive scalar elements retain no input owner. Builder freeze transfers backing, rather than borrowing the consumed builder header. Owned replacement retires the old destination and transfers the new value under existing cleanup rules. No additional copy/allocation, lifetime extension, global fact clearing or new loop semantics. | Sema projected value/storage-generation facts, MoveCheck/EscapeCheck and checked-HIR replay; existing MIR allocation/move/Drop. Interface lifetime summaries retain their existing encoding. | Plans 38/44/52. Separate owners for loop scalar copy and non-loop nested owned Option replacement; genuine borrowed-element, arena, stale-generation and read-only/writable-copy twins. |
| R81 shared call with owned output | Existing `borrow value: Observation` with `tree: array<Row>`; a selected field is supplied to `fn changes(left: slice<Row>, right: slice<Row>) -> array<string>`. The example explicitly clones strings and freezes an owned builder. Generalize by typed path and declared mode, not these names. | Authenticate the field's readable backing and both independent source roots. A Copy slice argument needs shared read authority, not ownership of the original record/array. Each call input must be initialized. The owned result has independent backing/strings; no source lifetime is attached solely because an argument was borrowed. A declared borrowed result still retains its actual sources. No hidden copy or source consumption. | Sema/HIR descriptor formation where necessary; MIR producer argument, buffer and return-leaf relations. No new interprocedural writable-view authority or persisted summary. Whole/per-unit publication uses the same MIR owner. | Plans 28/44/52/53/55. Minimal original, named-view and consuming controls; two roots, caller source reuse, owned-result expiry, real source collection and borrowed Config consumers. |
| R82 shared namespace carrier | Existing stable `borrow Option<process.user_namespace>` and stable record-contained equivalents; `Some(ns)` may supply existing `c.inherit_namespace(borrow ns: process.user_namespace, slot: i64) -> Result<(), Error>`, with the existing mutable command receiver. | Matching observes the tag and projects the owner; it neither duplicates nor closes an fd. The explicit inheritance call retains plan 50's typed descriptor duplication, slot validation/error order, command ownership and transactional failure. Source namespace remains unchanged and reusable. No implicit behavior for None: the written arm decides. A projected namespace cannot escape or be moved/replaced through the shared binding. | Extend the one shared borrowed-payload classifier with `ProcessUserNamespace`, uniformly through its already-admitted recursive carriers. Recompute HIR eligibility/active roots and preserve projection-only cleanup. Existing ABI, interface, native receiver and Drop remain unchanged. | Plans 28/37/44/50. Portable None/type/error owners; Linux real Some/duplicate/source-expiry owner; macOS preserves existing unavailable native acquisition/launch behavior. |
| R83 owned call/return leaves | Existing local/imported `bound(...) -> Result<Document, Error>`, `json.decode(...) -> Result<Template, Error>` and `wrap(...) -> Result<Option<Template>, Error>`, including source-owned and borrowed optional path inputs. Also the reported `Result<TaskMeasurement, Error>` final owned digest. | Certify every present owned leaf independently at the exact typed Result/Option/field path. Inactive None/Err leaves need no owner but do not create READ/OWNED evidence. Input readiness, return ownership and borrowed lifetimes remain separate proofs. An independently owned decoded/cloned result survives input expiry; a borrowed leaf does not. Existing error precedence, cleanup and explicit allocations are preserved, including late error/early return. | MIR producer founded equation/call-component analysis, existing source return summaries and HIR replay when the minimal witness identifies a producer mismatch. No optimistic cycles, blanket owned-call seed, XML bypass, JSON schema change or new wire record. | Plans 25/38/45/50/53/55. Original wrapper/direct controls and all active/error arms, local/imported and whole/per-unit/ThinLTO; both actual document admission and 32nd-field measurement digest owners remain separate mandatory consumers. |

R78 changes a deliberately closed grammar, so it is Category B, not a compiler
regression. [Plan 23 row B4](23-friction-ledger.md#b4--enum-fields-in-heap-built-records)
records zero qualifying mechanical occurrences in the supplied evidence. Its
boolean representation is a design choice and does not meet the protocol's
counting rule. The owner's instruction to aggregate the requests is not an
explicit instruction to override that prerequisite. Keep R78 PROPOSED and retain
the enum-field rejection; no classifier, layout or Drop implementation work for
R78 is part of this wave. Its requested scope and diagnostic remain recorded so
a future decision does not need to repeat this assessment.

R82 completes a shared carrier of an already-owned native handle and its existing
shared operation. It retains the structural shared-only projection policy and
adds no second optional/resource model, owned extraction or new native behavior.

The public native text/byte validation, numeric widths, encoding and embedded-NUL
rules are unchanged: R77–R81 add no native input; R82 uses plan 50's exact input
transaction; R83 composes existing read/hash/decode operations in written order.
No serialized format or reflection table is added. Reuse existing nominal and
reachable-definition identity; do not invent a new cache salt to mask stale
analysis. Any actual new persisted field or IR shape requires a revised ledger
and independent strategy review before implementation.

## 3. Implementation strategy

### 3.1 Preserve four distinct facts

Use the current analyses and representations. Do not add a second solver or a
new source qualifier. Every affected operation must keep these questions separate:

1. Is the selected value/path initialized on the active control path?
2. May this operation read that value, or does it require an owning transfer?
3. Which storage generation owns the descriptor/backing, and which roots occur
   in its contained view values?
4. Does the operation produce fresh ownership, transfer existing ownership, or
   merely copy a view descriptor?

A shared descriptor read answers neither owner-transfer nor writable-backing
questions. A fresh outer array does not make its contained views independent.
A copied scalar has no view lifetime merely because its input container was
borrowed. An absent payload is not an initialized present value. A native or
call-result ownership guarantee applies only after its inputs and exact contract
are authenticated. These rules also preserve plan 52's explicit unresolved
interprocedural writability boundary.

### 3.2 Normalize projections before changing certification

For R77, compare the produced direct-call HIR with the passing named-`str`
control. Reuse the existing owned-string-to-view representation. Keep the physical
field type `string` and the consumer's logical `str` requirement distinguishable;
do not falsify an expression's stored type to make a validator pass. Authenticate
the indexed base, index, exact field, root generation, active payload path and
later-argument preservation. The body validator and replay must reconstruct the
same conversion. The existing `BorrowedElementPlace` carries an internal
`field_path` for the dynamic AoS path; fixed `StructArray` literals use the
existing `BorrowedFixedElementPlace`. Neither representation changes a public
signature, interface field or runtime ABI.

At an explicit `borrow mut` boundary, preserve the physical owner contract. An
owned `string` or dynamic array cannot be relabeled as a mutable `str`/`slice<T>`
place because a whole-view replacement would bypass the owner's cleanup bit;
sema and checked-HIR replay reject that shape. Fixed arrays are inline storage,
so MIR materializes a `{ptr,len}` descriptor in a call-local Copy slot before
passing the view. Element writes still reach the fixed backing, while a view
header replacement remains confined to the descriptor. The producer path accepts
only the complete canonical view-retype predicate for a root descriptor.

For R79/R81/R83, trace the first failing typed producer equation, not the final
diagnostic's application field name. The current `value_equation` handles
`Rvalue::Use(Operand::BorrowedPlace)` through a string-view-only branch. Its
closed Copy-read coverage is a required inspection point, not proof that all
three reports have one root cause. An admitted Copy projection may produce its
typed value only after exact source/arm/readiness checks. A Move projection may
remain a shared operand but cannot use that rule to materialize an owner.

Reuse `xml_borrowed_path`, the descriptor-path checks, the distinct ordinary and
buffer nodes and the shared producer solver in `align_mir::producer`. Preserve
exact static type, logical view conversion and nominal native argument checks.
Do not fix R79 by accepting arbitrary enum bits as `process.signal`, or R81 by
treating a by-value Copy slice header as an owned source array.

### 3.3 Fresh and transferred storage

For R80, audit `pipeline_element_fact`, `borrow_fact_inner`,
`completed_value_fact`, `assign_completed_borrow_fact`, projected header contents
and generation renaming together. `ArrayToArray` currently derives retained
facts from the selected element; repair the typed selection if it carries the
source container identity into a scalar result. Clearing all facts for every
owned type is unsound: an owned container can contain borrowed views.

On assignment, capture the RHS's completed value once, detach only storage
identities transferred by that operation, retire the previous destination's
ownership, and install the new generation/content facts. Preserve the existing
runtime selected-edge cleanup and source nulling. Self-assignment, branch joins,
empty/nonempty loops and consuming freeze must use the same rule. A destination
name must not become its own expired lifetime source merely because its former
generation was replaced. The non-loop optional FILE_SET case is a distinct
mandatory witness; do not declare it fixed from the scalar-copy result.

Read-only markers remain storage properties, not lifetime/ownership roots.
Copying bytes creates writable new backing; copying a descriptor preserves the
selected backing's restriction. Keep the #1026 static-descriptor owners and
the genuine borrowed-element escape negatives.

### 3.4 Calls, returns and founded evidence

For R81, authenticate each argument at its declared type/mode and selected path.
Shared slice/record readers require readable initialized data. A by-value Move
parameter requires an actual transfer-capable value and its cleanup contract.
Do not demand owning array authority just to copy a read-only slice descriptor.

For R83, follow the complete selected return path through calls, local stores,
`?`, `Some`, `Ok` and record construction. Independently certify every present
owned leaf, including sibling strings and nested record fields. Propagate absence
only for the matching inactive discriminator. Imported declarations and copied
call signatures retain exact modes, return cleanup and borrow/region summaries;
an empty/unavailable summary cannot mint ownership or erase a real lifetime.

Keep plan 53's input-readiness fixed point separate from access propagation.
An uninitialized call argument, out-slot or cyclic local store must not certify
itself through the result of the same action. Seeded cycles/joins remain accepted;
unseeded cycles remain rejected. Reuse cached evidence only for a fully certified
Present node in the same immutable graph. Value access, backing access and header
replacement remain different queries. Test cached/uncached and query-order
equivalence; do not cache unresolved/conditional state as a completed success.

R81 and R83 may share a repair, but their successful direct/consuming controls,
failing ownership shapes and all real clients remain separate acceptance cells.
The measurement digest at field 31 is not discharged by Template field 6.

### 3.5 Complete the shared namespace carrier

R82 adds only `ProcessUserNamespace` to the existing shared-payload leaf set.
Apply that leaf through all already-admitted carriers using the same predicate;
do not special-case `Option`, a particular application type, or one match arm.
This includes an existing record/AoS-record shared projection when every other
reachable leaf already qualifies. It does not add other opaque handles, new
collection shapes, mutable indexed access, temporary-place admission or generic
owner extraction. Source checking, replay, indexed/shared call selection and
cleanup exclusion must agree.

The existing namespace constructor opens a retained Linux user-namespace fd;
inheritance uses slots 3..1023. It checks command/source/slot/duplicate-slot
admission before allocation/duplication. The command owns its CLOEXEC duplicate
and retains no source lifetime. This batch changes no native part of that
transaction. Linux positive ownership tests can acquire `/proc/self/ns/user`;
they need neither a newly created namespace nor sandbox/container integration.
On macOS, test portable None/type paths and existing native unsupported results;
do not claim actual namespace inheritance there.

## 4. Implementation closure matrix

The matrix remains the closure record for this implementation. The focused driver
target `owned_borrowed_composition` now covers 22 whole/per-unit cases; existing
native and malformed-IR owners are reused where they detect the same defect. The
remaining cells below are consumer-owned or require platform-specific provider
execution; the faithful provider witnesses are included in the focused target. A
table-driven owner may close several rows, and no fixture is added per Cartesian
product cell.

| Axis | Required cases and implementation evidence | Owner |
| --- | --- | --- |
| Type formation and validation | R82 exact handle / still-excluded handle; finite/deep/invalid nominal graphs and deterministic rejection. R78 enum-field rejection remains unchanged. | Extend sema `borrowed_payload_classifier_admits_only_ordinary_dynamic_array_graphs` and its HIR mutation twins; reuse the existing heap-record rejection owner as an unchanged-domain control. |
| Construction and move-in | R82 None/Some and record-containing optional owner through already-admitted recursive carriers. Existing builder zero/one/multiple pushes remain controls; no element-domain widening. | `owned_borrowed_composition::admitted_carrier_domains`; existing builder owners. |
| Read and argument projection | R77 physical String/logical Str, R79 every signal, R81 two owners, R82 shared namespace; direct/nested/indexed and local/imported calls; the original owner remains reusable. `StrBorrow`/`ArrayToSlice` call wrappers normalize to their physical place through `align_sema::borrow_argument_source` in sema, checked-HIR replay and MIR lowering. Fixed-array slice calls use a materialized descriptor slot, and root view retypes use the canonical producer predicate. | `owned_borrowed_composition::shared_reads_and_calls`; `borrowed_config_forwarding_returns_independent_argv`; `borrowed_fixed_array_to_slice_materializes_a_descriptor`; `borrowed_string_root_view_retype_is_certified`; `borrowed_params`, `move_record_slices`, `m11_process_live` controls. |
| Mutable view and generic argument boundaries | An exclusive call may not relabel an owned `string` or dynamic array as a view; an existing view remains writable under the ordinary generation rules. A fixed array may be passed through the materialized descriptor, and an unbound generic `borrow T` argument routes `arr[i].field` through the same indexed-field checker as concrete parameters. Sema, checked-HIR replay, MIR and producer validation agree on these shapes. | `owned_borrowed_composition::mutable_owned_view_retypes_are_rejected_before_lowering`; `owned_borrowed_composition::generic_borrow_accepts_an_indexed_move_field_chain`; `return_provenance` mutable-slice controls; checked-HIR mutation owners. |
| Move-out, source nulling and Drop | Move records transfer once; projected Move bindings have no owning local/expression cleanup; Copy signal loads add none. Partial builder, source expiry, normal return and early Err release every actual owner once. | Existing builder/resource cleanup counters and malformed cleanup owners plus `admitted_carrier_domains`. |
| Replacement and joins | R80 scalar copy and the non-loop nested Option assignment; branch replacement, self-assignment, zero/one/multiple iterations, old generation invalidation, still-live sibling roots and consumed builder freeze. | `owned_borrowed_composition::scalar_to_array_replacement_does_not_retain_the_loop_iteration_owner`; `owned_borrowed_composition::owned_option_branch_replacement_keeps_nested_members`; sema generation invariant owner, `borrowed_replacement`, `move_return_cleanup`. |
| Every control carrier | `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early return/break, absent/error arms and non-fallthrough operands. Reuse admitted source forms; do not introduce new syntax or broaden unrelated carrier formation. | Parameterized driver variants and existing `return_provenance`/owned-tagged owners. |
| Call and return composition | R81 borrowed field/slice parameter/owned argument controls, including Config and task-source expansion; R83 bound/direct/local/imported wrappers and independent owned versus genuinely borrowed results. The full-evaluator Document metadata/digest path and the 32-field measurement final digest are distinct owners from the seven-field Template witness. | `owned_borrowed_composition::borrowed_config_forwarding_returns_independent_argv`; `owned_borrowed_composition::borrowed_task_source_expansion_keeps_owned_rows_and_source_reusable`; `owned_borrowed_composition::faithful_document_digest_and_template_return_path_is_admitted`; `owned_borrowed_composition::owned_measurement_final_digest_field_is_certified_after_canonicalization`; MIR call-component and return-leaf mutations. |
| Malformed source/IR | Wrong same-shaped root, stale generation, inactive valid variant, invalid ordinal/type, missing output/cleanup, forged owned read, uninitialized argument/out slot, unseeded and seeded cycles. No compiler panic or optimistic fallback. | Existing checked-HIR owners and MIR producer mutation owners; run each producer negative at publication, whole-program emission and ThinLTO entrypoints. |
| Read-only and mutable authority | Literal/static/mapped read-only twins, fresh byte-copy writable twin, descriptor copy retaining read-only backing, plain by-value slice versus BorrowMut/Out header/backing, excluded borrowed-owner writes/transfers. | `constants_aggregate` #1026 owners, plans 52/53 producer mutations and composition escape negatives. |
| Generic and interface transport | Concrete generic admitted records/nested views, indexed Move-field chains with an unbound `T`, imported non-generic summaries, imported generic source rechecking, renamed same-shaped types and source definition mutation/restoration. Exact nominal identity/modes/cleanup survive. | Composition driver whole/per-unit twins and existing interface canonical-graph owners. No new codec fields. |
| Whole / per-unit / ThinLTO | Check and execute reachable positive programs in both ordinary modes; R83 also uses ThinLTO. Negative producer mutations fail before lowering/native execution in all three entrypoints. An unused/public-only function is insufficient native-execution evidence. | Composition target plus existing three-entrypoint producer helper. |
| Native provenance and allocation | R82 repeated independent commands, duplicate/invalid slot failure leaving source and command unchanged, source Drop after duplication, None/no hidden syscall, descriptor counts. R77/R80/R81/R83 add no allocation beyond the written clone/copy/build/decode; R79 adds none; R78 changes nothing. | Extend/reuse `m11_process_verified` and runtime `process_verified` ownership controls. Linux actual fd owner, macOS portable/refusal owner. |
| Termination and cached proof | Iterative finite type/equation traversal; no recursive rediscovery loop, success on founded joins, rejection of unseeded cycles; equivalent results for query orders and cached/uncached proof. | MIR invariant owner with bounded/deep graphs and existing plan 53 cycle twins. No new performance claim or benchmark gate. |

## 5. Complete client acceptance map

The request register is the source for these clients. This design does not read
or modify their implementation, pin, branches or acceptance machinery. Provider
owner tests establish the language contract; the align-llm owner separately
records real-client acceptance after one combined adoption. Do not replace a
consumer's schema, safety check, digest or ownership with a convenient control.

The provider now includes the seven-field R83 witness and the faithful
Document/measurement witnesses in
`crates/align_driver/tests/owned_borrowed_composition.rs`. The same shape was
checked with the baseline `21b3151e` compiler and still fails in `wrap` with
`producer return leaf String at [ResultOk, OptionSome, StructField(6)]`; the
branch passes it in whole and per-unit modes. The source-collection and borrowed
Config witnesses exercise direct field-to-view forwarding, while the full
Document and 32-field measurement witnesses exercise the distinct field1 and
field31 paths. Real consumer smoke, managed-pin adoption and platform namespace
execution remain external acceptance work.

| Request / additional witness | Required client acceptance |
| --- | --- |
| R77 direct projected text | `src/prompt_repair_evidence_smoke.align` after direct-call adoption. |
| R78 enum record — deferred | Existing boolean representation stays valid. `scripts/run-prompt-worktree-smoke` would qualify a future enum admission only after its separate design prerequisite is met. No adoption or implementation is required in this wave. |
| R79 nested optional Report | `src/prompt_fixture_setup_smoke.align` after direct inspection. |
| R80 scalar loop copy | `src/prompt_git_observation_smoke.align` after direct copy. |
| R80 non-loop FILE_SET Option replacement | `scripts/run-prompt-source-trust-smoke`; exact member rows survive source expiry. This is not assumed to have the scalar case's root cause. |
| R81 original borrowed observation | `src/prompt_git_observation_smoke.align` with borrowed inspection; `src/prompt_task_validation_smoke.align`, including candidate reuse. |
| R81 borrowed sandbox Config | `src/prompt_sandbox_plan_smoke.align` if the borrowed interface is selected. The consuming Config remains a valid application choice. |
| R81 blocking task-source collection | `scripts/run-prompt-source-collection-smoke`: multiple tasks, last-declaration failure, original-manifest membership, limits and source expiry. |
| R82 optional namespace | Final sandbox command/probe owner after shared optional matching; real Some is Linux-only. |
| R83 reduced document/template | The standalone appendix fixture and direct-call control; source-owned/borrowed path selectors, None/Some/Err and source expiry. |
| R83 blocking evaluator admission | `scripts/run-prompt-task-inputs-smoke` and `src/prompt_evaluation_inputs_smoke.align`, including actual repair-template success/refusal. |
| R83 blocking measurement finalization | `src/prompt_measurement_assembly_smoke.align`: version-4 canonical goldens, final digest at field 31, cleanup/containment precedence, unchanged edit evidence and completion bounds. Do not discharge it with the reduced Template case. |
| Consolidated cutover | Client's final A2 functional integration after the provider batch is adopted. This does not move application supervision, measurement assembly, publication or Python retirement into native Align. |

The standalone appendix does not reproduce the appended application graphs.
The provider witnesses below retain each selected typed call and ownership path:

- R80 non-loop FILE_SET owned Option replacement and final Evidence transfer.
- R81 borrowed Config field forwarding into validation/argv builders.
- R81 blocking `collect_task` declarations → expansion → owned file-row assembly.
- R83 full evaluator admission, including the two-phase Document.sha256 leaf at
  `[ResultOk, OptionSome, StructField(1)]` and the explicit-admission call mismatch.
- R83 measurement finalization, including the owned digest at
  `[ResultOk, StructField(31)]` and the actual field/call dependencies.

- `owned_borrowed_composition::owned_option_branch_replacement_keeps_nested_members`
  preserves nested members through the non-loop Option replacement.
- `owned_borrowed_composition::borrowed_config_forwarding_returns_independent_argv`
  and `borrowed_task_source_expansion_keeps_owned_rows_and_source_reusable` keep
  the borrowed source reusable after independent output construction.
- `owned_borrowed_composition::faithful_document_digest_and_template_return_path_is_admitted`
  retains the resource-backed Document and nested optional selectors.
- `owned_borrowed_composition::owned_measurement_final_digest_field_is_certified_after_canonicalization`
  retains the exact 32-field result shape and final `content_sha256` replacement.

These close the provider-side composition cells. The real align-llm smoke suites,
managed-pin adoption, canonical measurement goldens and platform-specific native
execution remain consumer-owned. They gate ALIGN_LLM_VERIFIED and CLOSED after the
provider commit is merged; no provider result claims that later client work passed.

## 6. Implementation and remaining handoff

1. The request register and all seven appendix sources were refreshed before
   coding. Its consumer repository remains read-only except for the provider
   answer/status update permitted by `AGENTS.md`.
2. The focused driver owner records the selected positive/negative paths,
   including the Config, source-collection, full Document and 32-field measurement
   witnesses named in §5. Provider-side composition acceptance is complete; real
   consumer adoption remains external.
3. Shared projection/read authority (R77/R79), fresh/transferred storage facts
   (R80), shared call arguments and owned return leaves (R81/R83), and the R82
   namespace carrier are implemented together. The MIR producer remains the
   certification owner; LLVM only lowers validated places.
4. The fixed-array admission was narrowed consistently across sema, checked-HIR,
   MIR and the specification: dynamic slice/AoS views use runtime indices, and
   fixed `StructArray` Move-field call places require integer literals.
5. The author matrix-to-diff pass and focused owner checks are complete for the
   implemented surface. The view-wrapper source is normalized once by
   `align_sema::borrow_argument_source` and replayed by checked HIR and MIR, so
   direct field forwarding cannot diverge between stages. Repository preflight
   remains the publication step for the final pushed SHA.

Deliver one consolidated capability PR, not one PR per request or a dormant
producer/consumer chain. More than roughly 1,000 handwritten lines is plausible:
source facts, body replay and producer certification span three compiler layers,
and the namespace carrier consumes that same proof. One closure matrix
and one coupled regression envelope avoid duplicated ownership proof and repeated
integration/pin rounds. Internal commits may follow the order above; none is a
claim of complete client delivery before the matrix closes. Any strategy/IR
redesign gets the repository's required revised-matrix review.

Suggested first owner after implementation:

```text
scripts/cargo.sh test -p align_driver --test owned_borrowed_composition
```

Add the narrow sema/MIR invariant owners for the exact change and existing native
namespace owners when its carrier is implemented. Do not run an unrelated DB
service gate or whole workspace suite just to author this design. Code publication
still follows the then-applicable repository gate and explicit owner direction.
A request batch implementation requires the normal final optimized workspace
build; it is still required before this branch is published.

## 7. Documentation and completion

This plan owns the exact new target and closure matrix. `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md` and the Settled design record
describe the implemented R82 carrier and the R78 deferral. The core
array/pipeline and Option/Result design notes and their Japanese mirrors carry
the same selected/deferred boundary. Existing plans 17/28/44/50 retain their
evidence and point to this extension; the HIR ledger records the implemented
replay obligations. Runtime ABI/JSON codec ledgers need no new record because
their contracts do not change. HANDOFF records this capability boundary once.

R77/R79/R80/R81/R82/R83 are `IMPLEMENTING` on this unmerged provider branch;
they become `ALIGN_MERGED` only with a merged provider commit and become
verified/closed only with recorded client acceptance. R78 remains `PROPOSED`
under plan 23. Do not advance a row merely because a control or unrelated real
client passes. Recheck the register before publication so additional numbered
requests or appended acceptance are handled together.

## 8. Author validation

The baseline compiler rebuild succeeded. The seven original standalone sources
have the check/per-unit outcomes in §1; all seven syntax-format successfully.
The seven-field R83 provider witness was also run through the baseline binary and
reproduced the `StructField(6)` producer rejection before the branch repair.
On this branch, `owned_borrowed_composition` passes all 22 whole/per-unit cases,
the producer and codegen malformed Store/Load owners pass, and the current
optimized compiler accepts the align-llm evaluation-inputs source in per-unit
mode (`17 unit(s)`, warnings only). These checks do not claim consumer execution,
DB verification or align-llm adoption.

The author ledger-to-prose and matrix-to-diff pass covers the implemented rows
against §§3–6. The appended provider witnesses now pass in whole/per-unit modes;
consumer adoption and platform-specific runtime owners remain explicitly pending.
The appendix matches the seven baseline inputs, local links resolve, and
`git diff --check` passes. A final register rescan still ends at R83. The fresh
inspection of the `409defb0` candidate found four implementation-closure issues:
mutable owning view retypes, fixed-array slice descriptors, root view-retype
authentication and generic indexed-field dispatch. The follow-up repairs close
those four classes with focused owner cases. The final candidate's full and
changed-slice host inspections reported no actionable defects; release build,
preflight and publication remain.

| Finding | Closure |
| --- | --- |
| R78 admission was still scheduled despite the unmet reopen prerequisite. | §2 records Category B/B4 deferral; §§3–7 select only the other six requests. Specifications and mirrors retain the enum exclusion, and the register keeps R78 PROPOSED. |
| Only two unreduced additions were named, and provider/consumer completion could become circular. | §5 enumerates FILE_SET, Config, collect_task, full evaluator and measurement witnesses. Provider owners now close those cells; real client smoke/A2 follows the single merged adoption and stays externally owned. |

The initial reviewer inspected all seven contracts, added clients, sema/MIR
seams, readiness/cache rules and native namespace boundaries without builds or
tests. Its findings and the follow-up provider closure are recorded above; the
final candidate received a clean host inspection, and the owner suite uses the
existing whole/per-unit/ThinLTO rejection helpers.

## Appendix A. Exact reported standalone sources

These are diagnostic inputs, not passing normative examples. Each is a separate
module/file. The baseline outcomes and control limitations are recorded in §1
and §8. The selected forms and faithful provider witnesses are retained by the
provider owner; the appended consumer graphs still require real client adoption
and runtime acceptance listed in §5.

### R77

```align
module repair_evidence_borrow_min
Record { body: string }
fn redact(borrow text: str) -> string { return text.clone() }
fn build(blocks: slice<Record>) -> string { return redact(blocks[0].body) }
fn main() {
  blocks := [Record { body: "x".clone() }]
  print(build(blocks))
}
```

Passing per-unit diagnostic control: Bind `blocks[0].body` to a named `str` before calling the reader.

### R78

```align
module heap_enum_record
Kind { Regular, Link }
Entry { kind: Kind, path: string }
fn main() {
  mut entries: array_builder<Entry> := array_builder()
  entries.push(Entry { kind: Kind.Regular, path: "file".clone() })
  values := entries.build()
  print(values.len())
}
```

Passing per-unit diagnostic control: Use a boolean discriminator; this is a different application representation.

### R79

```align
module fixture_setup_nested_signal
import std.process
Stop { Cancelled(process.signal) }
Report { stop: Stop, data: string }
Attempt { report: Option<Report> }
fn inspect(borrow attempt: Attempt) -> i64 {
  return match attempt.report {
    None => 0,
    Some(report) => match report.stop {
      Cancelled(signal) => process.signal_number(signal),
    },
  }
}
fn main() {
  value := Attempt { report: Some(Report { stop: Stop.Cancelled(process.signal.Terminate), data: "".clone() }) }
  print(inspect(value))
}
```

Passing per-unit diagnostic control: Pass the contained Report to a separate borrowed helper.

### R80

```align
module scalar_copy_loop
Report { stdout: array<u8> }
fn report() -> Report {
  mut values: array_builder<u8> := array_builder()
  values.push(65 as u8)
  return Report { stdout: values.build() }
}
fn collect() -> array<u8> {
  empty: array_builder<u8> := array_builder()
  mut output := empty.build()
  mut index := 0
  loop {
    if index == 2 { break }
    value := report()
    bytes: slice<u8> := value.stdout
    output = bytes.to_array()
    index = index + 1
  }
  return output
}
fn main() {
  result := collect()
  print(result.len())
}
```

Passing per-unit diagnostic control: Copy through an owned-return helper taking the input slice.

### R81

```align
module borrowed_array_return_probe
Row { path: string }
Observation { tree: array<Row> }
fn changes(left: slice<Row>, right: slice<Row>) -> array<string> {
  mut result: array_builder<string> := array_builder()
  if left.len() != 0 { result.push(left[0].path.clone()) }
  if right.len() != 0 { result.push(right[0].path.clone()) }
  return result.build()
}
fn inspect(borrow value: Observation) -> Result<(), Error> {
  mut rows: array_builder<Row> := array_builder()
  rows.push(Row { path: "b".clone() })
  right := rows.build()
  actual := changes(value.tree, right)
  if actual.len() != 2 { return Err(Error.Invalid) }
  return Ok(())
}
fn main() -> Result<(), Error> {
  mut rows: array_builder<Row> := array_builder()
  rows.push(Row { path: "a".clone() })
  value := Observation { tree: rows.build() }
  return inspect(value)
}
```

Passing per-unit diagnostic control: Consume the Observation in the diagnostic helper instead of borrowing it.

### R82

```align
module borrowed_namespace_command
import std.process
fn attach(borrow mut command: command, borrow namespace: Option<process.user_namespace>, slot: i64) -> Result<(), Error> {
  return match namespace {
    None => if slot == 0 { Ok(()) } else { Err(Error.Invalid) },
    Some(value) => command.inherit_namespace(value, slot),
  }
}
fn main() -> Result<(), Error> {
  mut command := process.command("/bin/echo", ["echo", "namespace absent"])
  absent: Option<process.user_namespace> := None
  attach(command, absent, 0)?
  return Ok(())
}
```

Passing per-unit diagnostic control: Use a direct shared namespace parameter; this control only checks types.

### R83

```align
module product_cutover_owned_document_return

// R83 compiler regression fixture, not product code or a validation bypass.
// Align 6ca79fee6eb221702652c2bb131839708db55eda:
//   scripts/alignc check FILE                         PASS (5 functions)
//   scripts/alignc check-per-unit FILE                FAIL in wrap
// MIR producer return leaf String at [ResultOk, OptionSome, StructField(6)]
// is not certified by its body. Only shipped standard/core APIs are imported.
// Direct-call control: copy this file to /tmp and replace the marked bound call
// with `source := read_document(root, path.bytes(), remaining, deadline)?`.
// The control passes check-per-unit. It intentionally omits bound's checks and
// is diagnostic evidence only; it is not an application repair.
// Template decoding is reduced to json.decode: digest normalization/restoration
// is unnecessary to trigger the failure. There is no borrowed record array.

import core.json
import std.crypto
import std.encoding
import std.fs
import std.time

pub Document { text: string, sha256: string, metadata: fs.metadata }

fn same(before: fs.metadata, after: fs.metadata) -> bool {
  regular := match after.kind { Regular => true, _ => false }
  return regular && before.device == after.device && before.inode == after.inode &&
  after.links == 1 && before.mode == after.mode && before.size == after.size &&
  before.modified_seconds == after.modified_seconds &&
  before.modified_nanoseconds == after.modified_nanoseconds &&
  before.changed_seconds == after.changed_seconds &&
  before.changed_nanoseconds == after.changed_nanoseconds
}

fn read_document(borrow root: fs.directory, relative: slice<u8>, maximum: i64, deadline: i64) -> Result<Document, Error> {
  if maximum < 1 || maximum > 2097152 || time.instant() >= deadline { return Err(Error.Invalid) }
  input := root.open_read_single_link(relative)?
  before := input.metadata()?
  if before.links != 1 || before.size < 0 || before.size > maximum { return Err(Error.Invalid) }
  mut data := buffer(0)
  mut total := 0
  loop {
    if time.instant() >= deadline { return Err(Error.Invalid) }
    if total == maximum {
      mut probe := buffer(1)
      if input.read(probe)? != 0 { return Err(Error.Invalid) }
      break
    }
    remaining := maximum - total
    mut chunk := buffer(if remaining < 65536 { remaining } else { 65536 })
    count := input.read(chunk)?
    if count == 0 { break }
    data.append(chunk.bytes())
    total = total + count
  }
  after := input.metadata()?
  if time.instant() >= deadline || total != before.size || !same(before, after) { return Err(Error.Invalid) }
  text := data.bytes().as_str()?.clone()
  digest := crypto.sha256(text)
  return Ok(Document {
      text: text,
      sha256: encoding.hex_encode(digest[0..digest.len()]).clone(),
      metadata: after,
    })
}

pub Profile { Diagnostics, EditSet, Policy }
pub Headers {
  STATUS: string,
  POLICY: Option<string>,
  EDITSET: Option<string>,
  SUMMARY: string,
  STDOUT: string,
  STDERR: string,
}

pub Template {
  schema_version: i64,
  artifact_kind: string,
  template_id: string,
  preamble_text: string,
  section_headers: Headers,
  closing_text: string,
  content_sha256: string,
}

fn decode_template(source: str, profile: Profile) -> Result<Template, Error> {
  return json.decode(source)
}
pub Manifest { path: Option<string>, hash: Option<string>, kind: Option<string> }
fn bound(borrow root: fs.directory, path: str, hash: str, deadline: i64, borrow mut remaining: i64) -> Result<Document, Error> {
  source := read_document(root, path.bytes(), remaining, deadline)?
  remaining = remaining - source.metadata.size
  if source.sha256 != hash { return Err(Error.Invalid) }
  return Ok(source)
}
pub fn wrap(borrow root: fs.directory, borrow manifest: Manifest, deadline: i64, borrow mut remaining: i64) -> Result<Option<Template>, Error> {
  return match manifest.path {
    None => Ok(None),
    Some(path) => {
      hash: str := match manifest.hash { None => { return Err(Error.Invalid) }, Some(value) => value }
      // Replace only the following call for the direct-call control.
      source := bound(root, path, hash, deadline, remaining)?
      profile := match manifest.kind {
        Some(kind) => if kind == "PROVIDER_EDIT" { Profile.Policy } else { Profile.Diagnostics },
        None => { return Err(Error.Invalid) },
      }
      value := decode_template(source.text, profile)?
      return Ok(Some(value))
    },
  }
}
```

Passing per-unit diagnostic control: Call `read_document` directly. This omits `bound` checks and is not an application-equivalent repair.
