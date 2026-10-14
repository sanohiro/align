# Tuple payloads in existing tagged carriers (voice V003)

## Evidence and scope

Voice's rotary-table helper returns two i64 handles through Result using a named
Rotary record. Its equivalent `Result<(i64, i64), Error>` witness is rejected by
payload formation. All three request registers were reconciled; the other two
consumers have no selected tuple-payload adoption. Consumer code/pins remain
external. This is existing anonymous-product composition, not new syntax, a
second error model, or permission to relax ownership checks.

Implement one complete owning-carrier capability for the already admitted tuple
element grammar. Option, both Result positions and ordinary user-sum payloads
share the same payload representation; no Result-only exception. Do not widen
tuple elements, collection elements, boxes, source globals or FFI merely because a new
internal Scalar discriminator exists. Direct flat-tuple record fields are included
with tagged tuple fields: the existing recursive field grammar must stay uniform,
without a wrapper-dependent admission exception. Existing nested owned-field
move-out and exclusive projection restrictions remain. Tuple function-value parameters/returns
are included through the same normal Align ABI: map_err on a tuple error needs
that complete callback path, including named functions and lifted lambdas. Shared
Move-payload projection remains subject to Plan26's closed grammar, which excludes
tuples; owning extraction and Copy payloads remain the existing modes.

## Public-contract ledger

| Surface | Exact contract | Owner and acceptance |
| --- | --- | --- |
| Type formation | Existing `(T0, T1, ...)`, arity at least two, in Option<T>, Result<T,E> or a user sum payload. The existing tuple element whitelist applies unchanged, including its generic instantiation and specialized-handle restrictions. No defaults or new syntax. Invalid arity/element/recursive type emits an ordinary diagnostic before lowering. | sema formation and checked HIR; scalar/view/owned-string/array/resource positive and forbidden-element negatives, generic whole/per-unit twins. |
| Representation | Internal Scalar::Tuple(id) refers to the exact existing ordered TupleDef. Conversion to/from Ty::Tuple is reversible. Tuple payload fields retain positional order and natural element layout. Every union uses its existing tag and inactive-payload rules with size/alignment computed from the actual tuple graph. No runtime ABI symbol or allocation is added. | one contextual layout authority and LLVM payload construction, independently checked mixed-width/empty/Move payload layouts and values. |
| Ownership and cleanup | Copy iff every element is Copy; otherwise Move. Carrier construction transfers the tuple once after successful eager evaluation. Active payload extraction moves it and nulls the source. Drop recursively releases only active initialized elements, in existing tuple order. Replacement releases the old initialized value exactly once. Free-standing/arena allocation provenance remains uniform; no hidden clone or allocation. | tuple-aware shared DropPlan used by classification and codegen, counted native-resource lifecycle owner and negative use-after-move/borrow/arena tests. |
| Control and errors | Existing match, else, ?, map_err, if/loop joins, explicit/implicit/early returns and eager-diverging operands preserve their control/evaluation order. Result else discards its error according to the existing cleanup rule; ? converts no error type. No additional recoverable error or abort is introduced. | source control matrix exercises both active variants, success/failure and partial construction with exact resource counters. |
| Record fields | Ordinary records may contain the same flat tuple directly or under an admitted carrier. The existing recursive field predicate, inline-cycle/alignment restrictions, replacement, provenance and recursive cleanup apply uniformly. Complete direct owning field extraction follows Plan192; deeper consuming paths and exclusive partial-Move borrowing remain restricted. | direct/tagged field twins in layout, generic formation, replacement, cleanup and lifetime owners; forbidden deeper-path owned extraction remains a negative. |
| Function values | Existing fn types may carry the same tuple parameters/return through the normal Align ABI, including map_err converting from/to a tuple error. Param modes and purity/return-borrow/cleanup summaries retain their existing rules. No escaping closure environment or foreign tuple ABI is introduced. | direct named and lifted-lambda callbacks, by-value Move parameter transfer, borrowed parameter and return lifetimes, indirect returned tuple/result and both map_err tags. |
| Views and authority | A tuple's str/resource-reference/array-contained views retain their real source lifetime through a carrier. Caller-owned borrowed origins remain caller-owned; fresh owned contents do not retain unrelated inputs. Owning extraction does not mint mutable authority. Plan26's shared Move tuple match remains rejected, and new scalar admission cannot bypass specialized handle storage restrictions. | provenance/replay tests and retained-view/owner replacement/escape negatives in whole/per-unit modes. |
| Interface and cache | Existing IType::Tuple encodes the complete ordered structural element graph inside Named Option/Result/user variants. The interface format and IType codec remain unchanged. The existing canonical MIR v3 codec gains scalar tag 62:u8 followed by the canonical tuple node ordinal:u32 little-endian; definition node tag 2 already encodes ordered tuple elements. Old unknown-tag rejection remains fail-closed. Nodes are structural equivalence classes in first-visit order, not raw ids; existing truncation/trailing/invalid-reference/cycle validation applies. Compiler identity invalidates internal artifacts; generic import and canonical MIR type/ABI identities include tuple members transitively. | independent existing-format tuple-carrier bytes/round-trip, element-type change invalidation and cold/hit frontend/imported executable parity. |
| Prerequisites and sources | Existing tuple, recursive tagged ownership, return placement and borrowed-origin ledgers. No new milestone or performance promise. This ledger owns the admitted capability; draft.md, docs/language-spec.md and core-design/option-result.md plus its ja mirror document the newly admitted composition. Plan17 producer-placement and canonical codec tables and Plan19 checked-HIR inventory gain the exact tuple discriminator/record obligations. Unchanged design rationale and locked semantics are not rewritten. | author ledger-to-prose pass, independent plan review before production edits, one preflight full-diff review after implementation. |

## Implementation closure matrix

| Axis | Required implementation and discriminating owner |
| --- | --- |
| Formation / validation | Scalar/Ty round-trip, inference/defaulting, explicit annotations and generic substitution; exact tuple table id/arity/element grammar, no recursive tuples or forbidden scalar use. Checked-HIR and MIR/codegen validators reject missing ids, malformed elements, stale payload and mismatched constructors before any unsafe lowering. Extend enum-sweep tripwires. |
| Construction / move-in | Scalars/views copy; owned elements transfer once, source nulling follows publication. A later tuple element or carrier operand can return/propagate before publication: earlier owned temporaries are cleaned. Native counted tuple resources and independent source values own these paths. |
| Move-out / source nulling / Drop | Contextual DropPlan traverses tuples under tagged/record/enum wrappers and uses their exact table, rather than top-level tuple special cases. Direct destructuring, indexed local extraction, wildcard binding, unused carrier, active/inactive variants and nested carriers preserve exactly-once cleanup. |
| Replacement / return | Local and record-contained carrier replacement, self-transfer rules, direct/imported and explicit/implicit return use the same existing cleanup ABI. Uniform arena/free-standing mode is checked recursively, including new tuple edges. |
| Control paths | if/match/else/?/map_err, branch and loop joins, early return, partial eager construction and malformed input. Test both active tags and all materially different cleanup outcomes; reuse existing owner suites for unchanged carrier control. |
| Views / borrowing | Region, return-borrow, MoveCheck/EscapeCheck, origin projections, retention, byte invalidation and checked replay traverse Scalar::Tuple via the existing tuple Ty path. Reject expired input/arena/resource owner, overlapping mutable authority, and unsupported shared Move tuple projection. |
| Generic / interface / whole/per-unit | Existing structural Tuple interface, monomorph substitution, canonical type graph and call ABI have the same exact ordered members. Imported generic/helper and cache cold/hit owners compare values and lifetime rejection; no raw internal id enters a persisted identity. |
| Runtime / allocation / ABI | No new runtime path. Direct/indirect Align calls and map_err callbacks use identical tuple parameter/return types and cleanup flags. Flat LLVM tuple types must be complete before any enclosing record/tagged body refers to them; natural size/alignment and payload lowering use the same type. Counted resource fixtures distinguish leaks/double frees and unchanged source allocation provenance. |

A single capability may exceed 1,000 hand-written changed lines because the new
payload discriminator reaches contextual type tables, Drop and backend layout.
These are a strict producer-to-consumer chain: splitting dormant classification,
cleanup and layout changes would duplicate proof without a useful safe consumer.
Keep the complete path in one implementation PR. No benchmark is required because
there is no performance or allocation-count improvement claim.

## Exact owner map and plan-review resolution

- `align_driver --test tuple_carriers`: `tuple_carrier_values_and_transport`
  owns scalar/view/owned cases, direct/tagged record fields, generic substitution,
  all control edges, direct/indirect/lambda callbacks and map_err, whole/per-unit
  executable parity and cold/hit frontend imports. `tuple_carrier_lifecycle`
  uses counted native resources for eager partial construction, transfer,
  replacement, early return, wildcard/unused active payloads and exactly-once Drop.
  `tuple_carrier_rejects_invalid_authority` owns use-after-move, source/arena escape,
  shared Move projection, deeper field extraction, forbidden tuple-element/collection/box/global/FFI
  placement and incompatible branch/return types in both compilation modes.
  The shared bounded owned-fixture harness owns subprocess retirement.
- `align_sema --lib tuple_carrier`: `tuple_carrier_layout_and_drop_plans` owns
  exact mixed-width layouts, recursive Copy/Move/drop classification and invalid
  ids/graphs; `tuple_carrier_provenance_replay` owns carrier/member origin and
  cleanup facts plus stale checked-record rejection. These close formation,
  ownership, representation and replay rows together.
- `align_mir --lib tuple_carrier`: `tuple_carrier_checked_hir_validation` owns valid source twins followed by
  single-axis malformed tuple/payload/table mutations in checked HIR.
  `tuple_carrier_validation_and_identity` owns canonical type graph remapping, exact
  tag-62 bytes, round-trip and malformed byte/reference rejection.
- `align_codegen_llvm --lib tuple_carrier`: `tuple_carrier_layout_validation`
  checks independent semantic/LLVM size-alignment and pre-emission rejection of
  malformed tuple payloads; the driver lifecycle owner verifies actual cleanup.
- `align_interface --lib tuple_carrier`: `tuple_carrier_interface_round_trip`
  pins the existing IType tuple bytes under generic tagged arguments and changed
  member identity. No interface-format bump or compatibility path is introduced.
- Reuse the complete existing driver `tuples`, `owned_tagged_payloads` and
  `else_result` targets, and the relevant existing `return_provenance` tuple and
  borrowed-sum owners. They own unchanged control/drop/borrow restrictions; new
  focused owners discriminate the newly admitted edges rather than duplicating
  every existing fixture. Run canonical-graph/type-layout owner modules for the
  shared classifiers, plus ordinary final-SHA preflight.

Independent preimplementation review identified two P2 plan gaps: direct tuple
fields and exact owner bindings. The public ledger now includes direct/tagged
field twins under the existing uniform field grammar, preserving nested owned
projection restrictions. The owner map above closes every matrix row. The author
also made the existing canonical codec's new tuple scalar tag/ordinal explicit;
the interface tuple format itself is unchanged. No production implementation
preceded resolution. This plan and the new codec record will be checked against
the implementation in the one preflight full-diff review.

## Author-side closure notes

Tuple positional projection now preserves the same caller-owned place root as a
named field when computing borrowed-storage caps. By-value tuple parameters,
local owners, and call/temporary results retain the frame cap. Direct and indirect
borrowed tuple callbacks have a positive execution twin and local/by-value escape
negatives. This reuses the existing owner-generation and return-root strategy;
it does not admit shared Move sum projection or exclusive partial-Move borrowing.

The native lifecycle owner pins exact explicit resource-allocation counts as well
as equal free counts and zero remaining requested bytes. It includes both active
variants, partial construction, replacement, generic transfer and both map_err
directions. Option propagation retains explicit `else { return None }`; `?`
remains the existing Result-only operator.

Checked HIR enforces the unchanged source tuple-element grammar. Private MIR
retains flat handle leaves used by the existing parameterized storage/nulling
owners; it rejects missing references and inline aggregate members before layout.
This internal test domain does not admit new source tuple elements. Constructor
correlation is checked for tuple, user-sum, Some/None and Ok/Err producers even
when unused or in a partition peer body. The demand-driven provenance walk alone
cannot cover an unused constructor; `tuple_carrier_constructor_preflight` owns
wrong/missing result types, operand types and arity across that family.
