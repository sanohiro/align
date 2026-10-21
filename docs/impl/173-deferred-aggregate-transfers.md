# Deferred aggregate source transfer

## Existing contract and capability

MoveCheck evaluates eager operands in source order and commits an outer action only after every operand succeeds. Structural constructor children are fused into that action. A block/control arm's own constructor or call is a completed child action; a selected bound tail remains a retained source for the outer action. Source snapshot checks reject later invalidation of retained sources. Type formation, provenance precision, allocation mode, IR variants and runtime ABI remain unchanged; K1 remains excluded. Missing fallback Move/overlap checks now reject previously accepted unsound programs.

Executed baseline witnesses on Plan172 fail for a fresh record's later source.len(), Some(source) as a field/call argument, an inline String-array member, and arena/named-arena/task-group/if bound tails. A block wrapping Some(source) instead rejects a later source read: its constructor has already completed. Preserve that distinction rather than making every nested expression deferred.

This is one producer/consumer capability: explicit pending transfers, component cleanup, constructor publication and call commit must agree. Splitting these into dormant infrastructure and consumers would duplicate the ownership proof. If the change exceeds 1,000 handwritten lines, the additional control-result and nulling consumers belong to the same proof; removing any leaves admitted source observations or stale-source nulling incorrect. Test tables reuse the counted native lifecycle owner and cumulative owners.

## Proposed internal mechanism

A consumed value carries its operand, fresh cleanup owners, and exact pending source transfers recorded during evaluation. Pending transfers contain the existing MIR source-clear statements and optional path-selection flags, not a re-walk of source expressions at commit. A local, admitted record/tuple/resource-element projection contributes only its actual evaluated place. Fresh calls, completed child constructors and consuming unwraps contribute no pending original source.

Structural StructLit/Tuple/Enum/Some/Ok/Err/ArrayLit components share this protocol. Complete top-level constructors and consuming calls commit their collected transfers once and retire fresh owners. Fresh record locals use the same component staging into their final inline slot. The private partial-field stack becomes unnecessary; existing DropField MIR semantics/validation need not be removed or relaxed.

Scope/control result lowering needs a third result mode beside move and borrow: defer bound-source transfer. Statements and child constructor/call actions still complete normally. Selected bound tails attach pending transfers to the result. Each selected-arm transfer is guarded by a Boolean slot reset at the control discriminator's dominating block on every evaluation, then armed only by that continuing arm. Nested selections prepend outer guards and test them in short-circuit CFG order; a skipped nested discriminator must never cause a load of its uninitialized guard. Resource-element source capture retains the existing constant-index-only extraction limit. Commit clears only selected sources; reached return/break/failure executes no pending commit. Fresh selected results retain their exact temporary cleanup bit and hidden owner, while retained bound sources remain their own cleanup authority. No whole owner may alias an active bound source.

Retire source-expression replay for completed structural constructors from null_moved_source only after every constructor producer and consuming action uses the new finish protocol. Keep ordinary direct places, consuming unwrap/task actions and existing path-local ownership semantics. Gate eager pre-lowering with the same classifier used for deferred lowering so no child is evaluated twice or reuses an SSA definition from a non-dominating path.

## Implementation closure matrix

| Invariant | Implementation and discriminating owner |
| --- | --- |
| Formation and validation | No type-formation widening. Source negatives retain duplicate/overlapping Move places, invalidated snapshots, borrowed extraction, mixed modes and unsupported arrays. Existing checked-HIR replay and strict MIR construction validators remain unchanged. |
| Construction and move-in | One ordered component protocol for direct/nested records, tuples, enum payloads, Option/Result and fixed arrays, including empty owners. Parameterized native observations require later .len()/clone to see retained sources. Fresh Let, whole replacement, field replacement, direct/indirect/raw call and map_err receiver consume the same exact finish record. |
| Source transfer and nulling | Record evaluated source places once, commit only after success, never re-walk a completed child's original source. Native whole/field/tuple sources and source-rebind-after-completed-child controls distinguish this; snapshot negatives retain rejection of rebind-before-pending-commit. |
| Selected control result | if and all existing match discriminators retain selected bound sources and transfer only the selected place at commit. Scope wrappers preserve framing exactly once. Nested/loop-repeated selection, opposite-arm source survival and false-to-true iterations prove fresh guard reset and dominance. Constructors inside an arm/block still complete at that child boundary. |
| Replacement and return | Complete RHS before old Drop/publication, with Plan171/172 destination checks unchanged. Exact self/sibling transfers, returning/breaking a retained source after an earlier pending operand, and normal aggregate returns preserve one owner. |
| Partial cleanup | Fresh components have exact hidden owners immediately; bound components keep source ownership until commit. A later return, loop break, failed ?, map_err mapper failure or divergence cannot publish/clear pending sources. Native counters require positive allocation witnesses, exact frees and zero live bytes at actual exits. Inner-loop exits preserve outer pending components. |
| Control action boundaries | ElseUnwrap consumes its Option/Result container immediately and discards an Err payload before fallback evaluation. Its negative fallback follows the enclosing mode: a bound source remains pending until the outer action, while a fresh/completed child owns its result. Final source Move accounting and duplicate-source checks must include that fallback without consuming the positive container again. Try/map_err/loop retain their completed-action rules. Pair Option/Result positive and negative edges with later reads/clones, duplicate fallback/direct sibling, post-commit source use, interruption and nested/repeated selection. Completed block/arm constructor rebinding remains a snapshot negative, not a native success claim. Mixed fallthrough/termination and side-effect counts prove one source-order evaluation. |
| Runtime provenance | Preserve per-component heap/arena cleanup and selected runtime flags, free-standing String clones inside arenas, caller-backed borrowed roots and existing uniform aggregate mode checks. No new heap allocation/copy/clone, no performance claim. |
| MIR and optimization | Existing slots/statements express guards and pending clears. No IR variant or LLVM semantics change. Strict fixed construction/whole-load and StoreField payload proofs remain. MIR structural owners check late source clearing, conditional selection, partial cleanup and exactly-once effects; loop/native owners cover optimization. |
| Generic/interface/cache | Imported generic forwarding executes in whole-program, per-unit and cold/cache-hit builds. Layout, interface serialization and runtime ABI are unchanged; compiler identity invalidates codegen artifacts. |
| Malformed and depth | Cumulative HIR/MIR formation, expr_depth and borrowed-control owners retain fail-closed behavior, bounded worklist recursion and the independent borrowing protocol. No borrowing synthetic-owner classifier widening. |

New owner: deferred_aggregate_transfers, using bounded owned_fixture native execution. Reuse field_assignment_publication, assignment_target_liveness, owned_field_replacement, owned_tagged_payloads, owned_structs_arrays, fixed_array_fields, fixed_owned_strings, fixed_scalar_borrows and the relevant MIR ownership/control/depth owners. The author must map every applicable row to actual code/test before full code review.

The R86 consumer residual is another instance of the structural-wrapper row:
`Envelope { value: Some(value), digest: Some(value.text.clone()) }` retains
`value` through the clone and then commits its transfer. The native owner now
requires the exact nonempty JSON digest for this source and the original
clone-before-move, reordered-field source in whole/per-unit/cache-hit modes.
Requested-live accounting includes the JSON output and must return to zero;
the realloc-grown output's free event has no matching counted allocation event.
The completed-child `value: { Some(value) }` and post-statement reads still
reject in both frontends. The old consumer rejection-only oracle for the fused
constructor predates this capability; acceptance alone no longer demonstrates
the historical empty-digest defect.

## Source-of-truth consistency

Plan164 currently describes immediate transfer of wrapped bound operands into synthetic owners. Correct that implementation promise to the already-enforced semantic action boundary. Plan172's separately deferred producer gap becomes closed by this capability. No language decision or new public API is introduced. Record status once in HANDOFF. Independent adversarial strategy review is required before implementation because pending selected-source transfers change the internal ownership proof.

## Independent plan resolution

The fresh plan review found one P2: ElseUnwrap has an immediate container action and a potentially deferred negative fallback. The matrix now names both MIR timing and semantic finalization/overlap owners for that edge. The author verified the executed read witness and semantic post-use witness; no post-use native execution is claimed. Nested guard short-circuiting and constant resource-element extraction are explicit. The remaining strategy was qualified by the same review; complete this matrix before code review.

## Author matrix-to-diff closure

`PendingConsumption` keeps fresh owners and evaluated source-clear records together;
`source_move_statements` is shared by direct and recorded transfers. Constructor,
array, call/raw/function-value and map_err consumers finish those records. Fresh
records retain final-slot construction but use component owners, so the private
partial-field stack is removed. Existing `DropField` MIR semantics remain.
`ControlResultMode::Deferred` and `store_control_result` propagate selected records
through existing joins, with per-evaluation resets and outer-first short-circuit
guards. The separate borrowing classifier and all strict producer validators stay
unchanged. `consumed_mode_differs` also filters eager-worklist children.

`deferred_aggregate_transfers` owns structural-wrapper/source-place observations,
all admitted match discriminator families, scope framing/effect counts, repeated
nested selection, Option/Result fallback edges, retained return/break, fresh early
exits and inner-loop preservation. Its native owner requires positive allocations,
exact frees and zero retained bytes after each function, with imported generic
forwarding in whole/per-unit/cache-hit modes. Source and forged checked-HIR owners
reject missing fallback Move accounting/duplicates. Existing `field_assignment_publication`
retains snapshot/replacement/arena/borrowed-root and map_err/try controls;
`fixed_owned_strings`, `owned_tagged_payloads`, `owned_temporaries`, fixed-array,
resource and MIR owners cover the unchanged domains. The complete MIR library
suite and accepted/rejected expression-depth owner pass.

Existing formation limits remain explicit: tuple-valued record fields and
Option-valued function-pointer parameters reject; the indirect owner uses the
admitted String signature. Named arenas inside the repeated-loop probe are already
rejected by the checked-HIR Bodies gate on the pre-change release compiler, so the
named-scope owner runs outside that shape. Generic forwarding of a fixed String
field retaining its source also keeps the existing lifetime refusal; the generic
owner uses an ordinary owned record. These are unchanged admission boundaries,
not weakened validators or silently skipped test rows. Plan173 does not own their
expansion. No throughput/resource-performance claim requires a benchmark.
