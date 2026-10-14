# Indexed assignment target liveness after eager operands

## Existing contract and reproduced defect

Assignment evaluates its index, then its value, and only then accesses the live
destination and checks its bounds. The checked-HIR ledger and MIR lowering
already implement this order. A destination consumed by either operand must not
be used by the final write. No syntax, allocation, ABI, purity or ownership rule
changes.

On the Request148 baseline, both of these programs pass `alignc check`:

```align
fn consume(values: array<i64>) -> i64 = 0
fn main() {
  original := [1, 2]
  mut values := original[..].to_array()
  values[consume(values)] = 8
}
```

```align
fn consume(values: array<i64>) -> i64 = 8
fn main() {
  original := [1, 2]
  mut values := original[..].to_array()
  values[0] = consume(values)
}
```

MoveCheck checks the target before either child and never rechecks at the write.
The same timing occurs in `AssignElemField`, `AssignElem` and the
transparent-spine single-statement fast path. MIR selects the destination after
`lower_required!` completes the children, so an early fact cannot authorize it.

## Implementation closure matrix

The capability is one post-operand destination check shared by both MoveCheck
walkers for the three indexed statement kinds. Keep existing pre-operand
diagnostics; recheck the selected backing and required root shell immediately
before the final write's analysis transitions.
Do not reserve an old header across evaluation: a completed whole replacement
may establish a valid new destination. A terminating child performs no final
check or content transition. No new provenance seed or summary is introduced.

| Cell | Implementation and regression owner |
| --- | --- |
| Formation and admission | Existing source types, mutability and HIR shape validators remain unchanged. `assignment_target_liveness::eager_moves_reject` crosses scalar dynamic arrays, fixed fields of Move records, dynamic struct element fields and fixed Move-record whole elements. Each index/value position consumes the root. |
| Borrowed target | Inspect the outer selected header's lifetime roots after children, without resolving stored element contents. `eager_owner_invalidation_rejects` consumes or replaces a slice backing owner from an eager operand, including a helper and a conditional. The original owner cannot authorize a now-stale descriptor. |
| Root shell versus replaced payload | `MovedKey::Whole(base)` invalidates every indexed shell; a moved outer field invalidates only a selected path under that field. An unrelated sibling or element payload is not the destination shell. Fixed arrays keep their inline backing; dynamic arrays, slices and SoA require their outer header generation to remain live. `valid_replacements_execute` covers consuming an independent record sibling while writing a fixed field, fresh whole-root replacement, ordinary fixed Move-record replacement, and both SoA indexed store forms. Indexed Move extraction and fixed String element replacement remain excluded. Preserve existing initial partial-move refusals. |
| Control and evaluation | `eager_control_paths` crosses direct, block, if, match, loop, else, `?` and `map_err`; index before value, each once. `valid_replacements_execute` owns `terminate`, `break_child` and `fail_child`: return/break/failed `?` perform no later write/bounds action. Nested single-statement blocks force both transparent-spine child-size choices. |
| Move, nulling, cleanup | Existing consuming operands, source nulling, old-payload Drop and return cleanup remain unchanged. Native replacement controls and existing fixed-owned owners verify these paths. No runtime or MIR operation is added. |
| Checked HIR | `forged_assignment_replay_rejects` starts from valid checked HIR, replaces an eager operand with a consuming call and requires checked-HIR semantic replay to reject it. Replay runs the same MoveCheck and must not trust stale body metadata. |
| Imports and artifacts | `imported_assignment_liveness` checks the same consuming-helper negatives whole/per-unit; `valid_replacements_execute` runs replacements whole/per-unit and cold/cache-hit. Interface summaries and function signatures are unchanged; current compiler identity owns cache invalidation. |
| Sibling audit | Ordinary whole-local assignment intentionally reinitializes a moved slot and is not a read of its old shell. Vector lanes contain no Move/borrow storage. Expression reads and call/native mutation actions already use their own snapshot/check authorities and receive no new admission. Ordinary record-field assignment has a separate incremental literal-lowering boundary, recorded below; do not claim it fixed by this indexed capability. Audit these distinctions against the diff. |

### Exact backing check

An exact-header post-operand check must not call whole-value `check_borrow_use`
or recursively resolve the target's stored element values. Type formation stays
with source checking and the checked-HIR shape gates; MoveCheck owns lifetime. Refactor the existing outer-header
owner calculation in `BorrowState::resolve_headers` into a shared helper: each
referenced generation supplies its current release/caller roots, marked ended
using the directory's `ended` state; header fallback roots remain separate.
Retain the existing anonymous ended marker when a generation has no named owner.
The ordinary resolver still recursively joins contents; the new indexed check
uses only the selected outer header's roots. No new generation or lifetime is
inferred, and unknown/fallback lifetime markers retain their current semantics.
Some established projected/callback views have only a conservative local fact,
without an outer projected header. Recheck that existing local borrowing fact
in this case; absence of newer header metadata is not itself an error.
`conservative_view_backing_survives_without_header_metadata` pairs live and
consumed owners through materialized record arrays, builders, reductions and
returned views, in both eager operand positions. The existing readonly-origin,
validated-text and deep-HIR owners retain their original acceptance rules.

For a fixed inline array, no payload-owned header supplies the destination bytes;
check only the exact root/ancestor Move state. For a dynamic array, slice or SoA,
select its root header and reject an ended lifetime root. Shared/exclusive write
admission and readonly checks retain their existing authorities. Both transparent
spine completions call the same final check only after both children complete.
The parameterized owner includes borrowed `str` contents whose old payload owner
ends during replacement: live backing must not be confused with dead contents.

### Separate ordinary-field failure domain

Plan review found that `AssignField` has an additional boundary:
`StructLit` RHSs go directly to `store_value_at`, which evaluates and publishes
members incrementally. A final root check cannot repair an intermediate store
before a later RHS member reinitializes the root, nor a partially published
assignment before a later terminating member. Ordinary field assignment needs
its own RHS staging/publication and cleanup closure, with
`outer.inner = Inner { a: consume(outer), b: { outer = fresh(); 2 } }` and later
termination owners. That producer/publication problem is deferred to a distinct
capability; the three indexed forms already lower complete children before any
destination access and can be repaired independently. No new field admission is
part of this PR.

New owners belong to `crates/align_driver/tests/assignment_target_liveness.rs`;
their native/imported cases use the existing bounded `owned_fixture` helper.
The focused regression set also includes `fixed_owned_strings`,
`fixed_dynamic_arrays`, `fixed_array_fields`, and `owned_borrowed_composition`.
The bounded gate and checked-HIR owners cover replay. No benchmark is needed:
the change makes no performance or allocation claim.

One independent adversarial matrix review precedes the safety change. Complete
the author matrix-to-diff pass and one fresh inspection-only full-diff review
before final preflight and publication. Keep this as one useful repair even if
the Request147 field-admission work remains deferred.

## Request147 boundary discovered during the audit

At this audit's boundary, dynamic-array field admission remained closed. Plan194
now closes its source checker, both checked-HIR validators and selected-header
lowering together with the selected-backing lifetime and retention proof.
The existing indexed element family includes borrowed `str`, but EscapeCheck's
backing-lifetime table is keyed by collection locals, not selected record fields.
Even a static `str` store through an existing fixed-array field then rejected.
An aggregate's contained-value region cannot substitute for the selected buffer's
allocation lifetime. The complete field extension required Plan194's separate
reviewed lifetime/retention closure; neither repair resumes the parked K1 or
general aggregate-provenance work and does not introduce a numeric-only special
case for Request147.
