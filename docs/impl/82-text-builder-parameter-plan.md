# Text builder parameters

## Capability boundary

Request 24 needs one shared decode-and-accumulate helper in align-llm. The text
builder already has an opaque Move representation, append operations, ownership
checking, and Drop. Its source type spelling and declaration validation are
missing. This capability makes that existing type nameable; it adds no IR variant,
runtime operation, allocator, or view-access analysis.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Type | `builder` is a zero-argument builtin type spelling for the existing opaque Move text accumulator. It is valid in local annotations and direct named-function parameters and results. |
| Construction | Existing `builder()` and `builder(capacity: i64)` retain their existing allocation, capacity, abort, and Drop rules. Type annotation and parameter passing allocate nothing. |
| Mutable borrow | `fn append(borrow mut output: builder, text: str)` may call `output.write(text)` and the existing `write_int`, `write_bool`, `write_char`, and `write_float` methods. Calls use ordinary positional syntax. The caller supplies a stable mutable binding. |
| Shared borrow | `borrow output: builder` is read-only. Existing writer/file/log builder-source operations may read it. Every append method rejects a shared-borrowed receiver. `shared_reader_keeps_caller_owner_live` exercises the ordinary writer-source path. |
| Ownership | By-value parameters and results transfer the existing sole owner. A borrowed parameter cannot be moved into another binding, passed by value, returned, or consumed by `to_string()`. Nested exclusive forwarding is non-consuming. `borrow mut` retains ordinary whole-owner replacement: Drop the old builder and install the new owner in the caller's slot. Shared replacement rejects. |
| Retention | Append copies text bytes during the call; it retains no input view or caller stack address. The borrowed builder cannot be stored or captured. Existing mutable-call alias exclusion applies. |
| Placement | There is no `Scalar::Builder`. Struct fields, tuples, arrays, `Option`, `Result`, function-value parameter types, closure captures, and task results remain outside the admitted type grammar. The existing function-type result grammar admits a `fn() -> builder` annotation, but named function-value and lambda formation still reject builder results through `fn_value_ret_ok`. No callable, aggregate, or capture admission is widened. |
| Effects | Existing builder allocation and append remain Pure. I/O operations reading a builder remain Impure. |
| Interface | The existing canonical named-type record carries `builder` with zero arguments. It is an opaque builtin with no contained borrowed views. Existing parameter modes, effect, and ownership metadata remain authoritative. No format tag, width, order, or policy changes. |
| ABI | Reuse existing `Ty::Builder`, mutable borrowed-place ABI, move-source nulling, and type-aware Drop. No runtime ABI inventory entry changes. Whole-program and per-unit calls must agree. |
| Owner | Sema owns name resolution and shared-borrow append rejection; checked-HIR owns declaration admission and independently checks append/finish receiver modes; interface validation owns the builtin spelling; MIR/codegen/runtime retain the existing implementation. |
| Prerequisite | Existing borrowed-parameter and builder implementations on main. K1 is not a prerequisite. |
| Acceptance | `text_builder_params.rs` owns exact-byte output, nested forwarding, ownership transfer, negative borrow/placement/alias cases, and whole-program/per-unit parity. Existing interface builtin-domain parity and checked-HIR type-placement owners close registry/header agreement. |
| Performance | No new speed or resource-count promise. No benchmark required. |
| Sources | `draft.md` §12, `docs/language-spec.md` Strings, `docs/design-notes.md`, Settled in `docs/open-questions.md`, and `core-design/string.md` plus its Japanese mirror. |

## Implementation closure matrix

All rows reuse the established opaque-handle and borrowed-parameter strategy.
No new source authority or provenance representation is introduced.

| Invariant / path | Implementation | Owner |
| --- | --- | --- |
| Type formation, reserved name, zero arity | Sema `BUILTIN_SPELLING_TYS` and existing type resolver | `text_builder_params::type_and_placement_matrix`; interface builtin-domain parity |
| Header formation / malformed HIR | Checked-HIR declaration type validation admits `Ty::Builder` alongside opaque handles; other body-only types stay rejected; body replay independently checks append/finish modes | `align_mir::text_builder_parameter_modes_are_revalidated` and existing declaration type-placement owner |
| Construction, local annotation | Existing BuilderNew and checked-HIR expression validator | `text_builder_params::borrowed_append_all_methods` |
| Shared vs exclusive append, all five methods | Existing exclusive opaque-handle receiver gate after operand type checking; one shared `builder_receiver_sources` inventory follows scope tails and if/match selected values; loop breaks retain their consuming transfer gate | `text_builder_params::shared_append_matrix`, including block/scope and selected-value negatives and fresh-owned positives; `align_mir::text_builder_parameter_modes_are_revalidated` independently forges shared append and borrowed finish |
| Move-in, move-out, source nulling, return | Existing BuilderToString and by-value call/return lowering | `text_builder_params::owned_transfer` and moved-source negative twins |
| Borrowed consumption, return, local retention | Existing MoveCheck and non-scalar placement checks | `text_builder_params::borrowed_owner_cannot_escape` |
| Exclusive whole-owner replacement | Existing borrowed-place assignment and Drop-before-store lowering | `text_builder_params::imported_builder_helpers_match_whole_program`; shared replacement negative |
| Caller header allocation across borrowed calls | Existing fail-closed stack-header escape proof rejects every place operand passed to a call; caller and callee retain the boxed ABI for a possibly replaced header | `align_codegen_llvm::borrowed_builder_calls_retain_boxed_headers`, isolated text/array and shared/exclusive cases; `text_builder_params::isolated_borrowed_replacement` native heap/arena caller cases |
| Completed receiver before a later eager argument | Text builder values participate in the existing completion snapshot frontier; replacement, move, or exclusive call through a later argument invalidates the completed receiver before append | `text_builder_params::receiver_argument_invalidation_matrix`, all five writes with direct/scope/selected receivers, owning and borrowed parameters, replacement/consumption negatives, shared-read and distinct-owner positives |
| Drop, normal and early exit | Existing builder Drop and borrowed-parameter cleanup exclusion | `text_builder_params::borrowed_append_control_flow`; existing `m5` builder owners |
| Borrowing receiver joins / fresh temporary cleanup | `BuilderWrite` uses existing `lower_borrowed_owned`, preserving bound branch sources and dropping a fresh receiver only after the append; required-argument termination keeps the registered owner under ordinary exit cleanup | `text_builder_params::borrowed_receiver_values_preserve_owner`, including both selected arms and fallible argument termination; existing `owned_temporaries::borrowed_control_flow_temporaries_lower_exactly_once` |
| `if`, `match`, loop joins, return | Existing control-flow lowering and borrow lifetimes | `text_builder_params::borrowed_append_control_flow` |
| `else`, `?`, `map_err`, early failure | Existing Result control-flow and borrowed-parameter cleanup | `text_builder_params::borrowed_append_control_flow` |
| Aliasing, nested forwarding, input lifetime | Existing call reservations and copied append input | nested forwarding positive and duplicate exclusive argument negative in `text_builder_params` |
| Generic monomorphization | Generic direct named helpers resolve concrete builder parameters through the existing template mechanism | generic forwarding case in whole-program/per-unit owner |
| Interface serialization, import, per-unit/native ABI | Zero-arity opaque builtin registration and existing named-type codec | `text_builder_params::imported_builder_helpers_match_whole_program`; interface builtin-domain parity |
| Allocation parity / runtime provenance | Passing only an existing handle/place; append keeps its runtime copy and growth behavior | exact-output owner exercises heap and arena construction; runtime unchanged |
| Aggregate, callable, worker placement | Existing absence of Scalar::Builder, `fn_value_ret_ok`, and capture/task restrictions | `text_builder_params::type_and_placement_matrix` |

## Pre-implementation review closure

The independent inspection identified a shared append bypass through a transparent
block receiver and a mismatch between the planned placement/replacement prose and
existing admission. Receiver checking follows the transparent returned value rather
than only the outer expression. Negative owners sweep shared block/scope/selected
receivers; owned expression twins remain accepted. The ledger retains whole-owner
exclusive replacement and distinguishes function-type annotation from function-value
formation. These corrections reuse the existing safety strategy.

## Author consistency pass

The ledger has one new named spelling and no second builder representation.
Parameter modes retain their existing semantics; only an owned receiver may
finish. A shared receiver cannot append. Generic type arguments, unsupported
placements, malformed declaration types, and borrowed transfers fail before
lowering. Interface records use the already-specified named-type encoding; a
new wire format or cache-policy field is unnecessary.

The author matrix-to-diff pass maps the new resolver/ownership bridge, opaque
interface registration, declaration validation, and shared receiver inventory to
the owners above. The source owner discriminated missing resolver admission;
the branch receiver negative exposed and closes the non-consuming `if` bypass.
Native selected-receiver execution also exposed ordinary move lowering nulling a
borrowed branch source. The borrowing consumer now uses the established borrow
lowering and temporary cleanup strategy, with native bound/fresh and terminating
argument twins. No new ownership representation or cleanup strategy is required.
Consumer adoption stays in align-llm; Align records its
provider result in the external request register without modifying consumer code.

## Code review finding closure

The first full-diff review found two missing cells: a borrowed call was invisible
to the stack-header escape proof, and the header-free text builder did not enter
the eager completion frontier. Compile-only probes confirmed both: an isolated
reset emitted stack initialization followed by boxed callee Drop, and a write
whose argument replaced its receiver checked successfully. Neither unsafe probe
was executed. The matrix above now names both axes. The fixes reuse the existing
boxed-call boundary and completion invalidation strategy; they add no new IR,
ABI, ownership authority, or public contract. The owner tests keep the isolated
allocation case separate from returned-builder and generic import cases.

| Verified finding | Fix / class sweep | Discriminating owner |
| --- | --- | --- |
| Borrowed call exposes a stack header to boxed Drop | `stack_header_plan::reject_header_operand` exhaustively handles all Operand forms and rejects borrowed-place roots across every existing call consumer; both text and array headers retain the boxed ABI | `borrowed_builder_calls_retain_boxed_headers` fails before the fix and passes afterward for shared/exclusive text/array calls; isolated replacement executes successfully after the fix |
| Later eager argument invalidates a completed builder receiver | `MoveCheck::value_snapshot_needed` includes `Ty::Builder`, using existing local/parameter roots and the shared source-ordered completion frontier | `receiver_argument_invalidation_matrix` fails before the fix and passes afterward for all five methods, three invalidation actions, direct/scope/selected receivers, and borrowed parameters; stable-owner positives remain accepted |
