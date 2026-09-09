# Bound owned values in if results

Status: implementation plan accepted after assignment-axis review, 2026-09-09.

## Capability and exact contract

Close the Category A implementation gap recorded in `23-friction-ledger.md` and
Open Questions' "A value-carrying if expression cannot move a bound owned local".
This is the existing Move model, with the same behavior already supplied by match
results. No language restriction is reopened and no type, syntax or API is added.

A consuming `if condition { then_value } else { else_value }` may select an
already-bound Move local. Evaluate the condition once, then only the chosen arm.
On a continuing arm, capture its value and allocation provenance, transfer that
value into the result, and clear only that arm's source ownership before leaving
it. The unselected source remains owned and is dropped normally. A diverging arm
publishes no result and contributes no post-join move/borrow state. Both-diverging
arms have no reachable join value. A use after a possible move still rejects;
assignment to a moved local reinstates it using ordinary replacement rules.

The same rule applies to bindings, assignment, function arguments, returns,
Option/Result/enum construction, loop breaks and nested control-flow tails.
A borrowed conditional remains a borrow: neither selected bound source is moved,
and any fresh temporary follows existing synthetic-owner cleanup. Every existing
Move type follows its canonical Drop plan; arrays keep their path-selected
heap-versus-arena ownership bit separately from conservative escape lifetime.
A borrowed parameter or borrowed payload cannot be moved; region escape, live
resource-dependency, mixed-owner and unsupported projected-move checks remain.
No additional clone, allocator, Drop rule, runtime symbol or implicit side effect
is introduced. Original source evaluation order and inferred effects are unchanged.

## Implementation closure matrix

`MoveCheck::expr_if_value_worklist` and `MoveCheck::move_if` currently mark each
consuming arm tail as an indirect move site and reject a bound source. Both must
recognize the same direct transfer at the arm's own join store. MIR's existing
`lower_if` already uses `store_control_result`: it captures the ownership bit,
stores the selected value, and calls `null_moved_source` on that edge. Reuse that
single implementation; do not add a second nulling walk after the join. No new
HIR/MIR discriminator or layout is planned.

The independent plan review found a pre-existing whole-local replacement hole:
`x = if c { x } else { y }` would suppress old-x cleanup on both paths because
sema's may-moved union is mistaken for a definite move. Close the class together.
Both ordinary-block and wrapper-worklist assignment analysis set `drop_old` from
the canonical destination Move classification. MIR assignment captures the RHS
value and its ownership bit, clears the selected RHS source, then conditionally
drops the still-live destination, then installs the captured value/bit. The
existing path-local drop flag decides whether the old value remains owned; the
static may-moved set cannot make that runtime decision. This is the same ordering
already used by supported field replacement. Exact `x = x` and transparent
self-assignment capture and clear x before cleanup, preserving the original
payload rather than freeing it. Assignment from a function that consumed x sees
its already-cleared flag. Existing borrow-mut destination cleanup remains live.
The repair covers if, match, else, nested wrappers and a destination already
possibly moved before assignment; no second full-range review is used to find
sibling cells.

All new integration cases belong to the existing `value_control_flow` owner.
The matrix is an invariant product, not one new test per table cell.

| Axis | Implementation/proof | Exact owner |
|---|---|---|
| Formation, construction, consumed local and selection | Both MoveCheck walkers mark continuing consuming arm tails as direct; existing type unification/Move classification is unchanged | `bound_if_result_formation_matrix`, sema `move_owned_local_through_if_arm_checks` |
| Move-in/out, source ownership clearing, Drop, replacement, return | Existing MIR `store_control_result`, selected drop flag captured before source clearing, ordinary `null_moved_source` and slot/aggregate Drop | `bound_if_result_execution_matrix` for both conditions, selected/unselected source, same source in both arms, reinitialization and result replacement |
| Conditional self-replacement, both sema assignment walkers | Capture RHS/flag, clear selected source, conditionally drop old destination, store value/flag; remove static may-moved suppression in both writers | `bound_if_result_flag_transfer` deterministically asserts selected-edge clearing and destination cleanup ordering; `bound_if_result_execution_matrix` covers both conditions, direct/transparent self-assignment, match/else/nested wrappers and prior possible moves |
| Type families | string, owned scalar/string/record arrays, array slice descriptors, owned record, tuple, Option/Result/user sum, buffer use existing canonical owner/Drop families; arena-backed arrays retain their dynamic flag | `bound_if_result_formation_matrix`, `bound_if_result_execution_matrix`; existing `owned_temporaries` and `owned_tagged_payloads` cover unchanged per-family temporary/carrier machinery |
| Control/consumer paths | Pure-tail iterative walker and statement-bearing recursive walker; if/match/else/?/map_err, branch/loop joins, early return/break, nested conditionals, function argument/return and wrapper construction | `bound_if_result_execution_matrix`; baseline `function_return_completeness_matrix`, region and flag rows in `value_control_flow` |
| Borrowed conditional and synthetic owner | Keep consuming=false separate; reuse `lower_expr_for_borrow` and the one borrow-mode classifier; bound branches retain ownership and fresh branches use existing hidden owner | `bound_if_result_borrow_and_rejection_matrix`; existing `owned_temporaries` |
| Refusal and malformed input | Possible-move reuse, borrowed parameter/payload move, live dependent and arena escape still refuse. Checked HIR has no direct-site bit; existing structural/type/visibility validation and move-source contracts continue to gate lowering | `bound_if_result_borrow_and_rejection_matrix`; existing checked-HIR control and ownership owners |
| Generic and imported boundaries | Generic substitution, interface borrow summaries and by-value returns carry the same type and provenance; no serialized field or cache identity change | `bound_if_result_execution_matrix`, whole/per-unit × Dev/Release, including a separate imported generic/helper module |
| Allocation provenance and parity | No allocation is added by selecting a bound value. Structural MIR evidence pins ownership-bit capture and clearing to the selected branch; native runs prove both heap and arena paths survive selection and Drop | `bound_if_result_execution_matrix`, `bound_if_result_flag_transfer`; existing allocation owners remain authoritative for allocator implementation |

The expected change is below 1,000 handwritten lines and is one useful capability.
No standalone producer seam or benchmark is warranted: it makes no performance or
exact allocation-count claim. The focused owner command runs `value_control_flow`,
`owned_temporaries`, `owned_tagged_payloads` and `borrowed_replacement`; final preflight runs the bounded
compiler gate and Clippy. Audit both sema walkers and all shared control-result
consumers before review. If a probe exposes a missing ownership strategy, reopen
this matrix before expanding the code boundary.

## Documentation closure

Remove only the now-obsolete if-result restriction from `draft.md` §6.3 and
`docs/language-spec.md`; mark the Category A row and Open Questions item complete.
Record this implementation of the existing model in `docs/design-notes.md` and
one capability-level HANDOFF paragraph. Update adjacent sema/MIR comments that
still claim MIR lacks branch-local transfer. The checked-HIR ledger clarifies replacement metadata; runtime ABI
and serialized representation contracts are unchanged. English compiler/internal
documents need no language mirror.

## Author-side closure evidence and separate follow-ups

The four `bound_if_result_*` owners pass. The symbolic MIR owner follows each
condition through both assignment walkers and direct/wrapped self-assignment,
asserting one Drop of the unselected payload and none of the returned payload.
Native buffer counters additionally pin cleanup across if/match/else replacement,
wrappers, previous possible moves and final scope exit, in both compilation modes
and both profiles. Borrowed results retain both sources; negative owners retain
possible-move, borrowed parameter/payload, live-view and region-escape refusals.

Two existing gaps were reproduced while forming the original matrix:

- A bound local in an `else`-unwrap fallback hit the indirect-source guard
  (`optional else fallback`). Earlier prose incorrectly called this sibling fully
  working. The follow-up is closed by the bound else-fallback matrix below.
- A program that only constructed and dropped `http.client()` could omit `ssl`
  from inferred link libraries and fail on `SSL_shutdown`/`SSL_free`. The
  Drop-capability closure in `20-runtime-abi-ledger.md` repairs constructor and
  carrier detection; the else matrix now includes the client family too.

The surrounding `owned_temporaries` target has three pre-existing empty-MIR
failures, reproduced unchanged on baseline `ab34b044` in an isolated checkout:
`borrowed_control_flow_temporaries_lower_exactly_once`,
`borrowed_scope_temporaries_lower_their_scope_exactly_once`, and
`match_and_try_preserve_temporary_ownership`. Its other nine tests pass. The
capability owner command skips exactly those three baseline failures, while
running all `value_control_flow`, `owned_tagged_payloads`, and
`borrowed_replacement` tests. Producer rejection investigation remains separate.

## Bound else-fallback closure (implemented 2026-09-09)

The separately reproduced `optional else fallback` gap uses the same existing
selected-edge MIR transfer as the accepted if-result capability. The sole
ElseUnwrap move worklist must mark its consuming fallback as a direct transfer
site. `lower_else_unwrap` already routes the negative edge through
`store_control_result`; the positive edge consumes the container and transfers
its payload. A Result's discarded error is dropped before the fallback runs.
Borrowed fallback results retain the fallback source; the input container keeps
its existing consuming behavior. No MIR/ABI, ownership strategy, type admission,
allocation, effect or serialization rule changes.

| Closure axis | Implementation and exact owner |
|---|---|
| Formation and type families | One ElseUnwrap worklist, canonical Move classification and existing MIR join; `bound_else_fallback_matrix` covers strings, supported arrays, records, Option/Result/enums, buffers and clients; tuple and array-of-slice Option payloads retain their existing formation refusal |
| Selection, move, Drop, replacement, return and allocation provenance | Existing negative-edge result store and path-local cleanup; `bound_else_fallback_matrix` covers both Option and Result alternatives, discarded owned errors, self-replacement and heap/arena paths using live buffer counts |
| Wrappers, nested control, generic/imported boundaries, arguments and exits | Existing worklist dispatch and control joins; `bound_else_fallback_matrix` covers nested if/else, block fallback, generic helper, argument/return and diverging fallback in whole/per-unit Dev/Release; existing `bound_if_result_execution_matrix` owns unchanged `?`/`map_err` paths |
| Borrow and rejection | Preserve consuming=false for the fallback and ordinary container consumption; `bound_else_fallback_rejections` pins possible-move reuse, borrowed input/payload consumption, live views and arena escape; matrix execution retains a borrowed fallback source |
| Malformed HIR, serialization and runtime ABI | Representation and validators are unchanged; existing bounded gate and the original plan's owners retain these cells |

This follows the reviewed selected-edge strategy; boundary inspection belongs to
one fresh preflight code review. The owner target is `value_control_flow`, with
`owned_tagged_payloads` for the unchanged discarded-error cleanup. No performance
promise or benchmark is added.
