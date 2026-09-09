# Fixed-array length receiver evaluation

## Contract and defect

A method call evaluates its receiver once before producing its result. A fixed
array's length is statically known, but that fact cannot erase receiver effects,
control flow, local declarations, or cleanup. The existing semantic checker
replaces every fixed-array `.len()` with an integer literal after checking and
discarding the receiver HIR. For example, `({ print("evaluated"); xs }).len()`
checks successfully but loses the print; a receiver-local binding instead leaves
an orphan local and fails the checked-HIR boundary.

Preserve the receiver in the existing borrowed `Len` expression. Extend that
record's validated receiver domain to the already accepted `Array` and
`StructArray` types. MIR uses the existing guarded slot initializer for a direct `ArrayLit`, including
synthetic-owner registration before element evaluation and partial-prefix cleanup
on return or `?`. On a continuing edge it drops that literal's synthetic owner
before returning the constant length. It never sends a literal through generic
expression lowering. Other fixed receivers use the existing borrowed
owner/control-flow path and release its temporary owners before returning the
constant length. Existing internal aggregate slots/SSA values are permitted;
this adds no source-level Copy operation or ownership transfer and makes no
optimization/performance promise. Dynamic/view lengths
retain their existing lowering. No new HIR/MIR variant, runtime symbol, array
ownership rule, supported method receiver type, or allocator is introduced.
The checked-HIR ledger's `Len` row must describe the retained fixed-array child.

## Implementation closure matrix

| Axis | Closure and exact owner |
| --- | --- |
| Formation and validation | `Checker::check_len` retains fixed receivers; `BodyValidator` validates the complete child and exact i64 result. `fixed_length_receiver_control_matrix` and MIR `fixed_array_len_child_and_result_contract`. |
| Evaluation and result | Existing borrow lowering runs receiver effects once before returning constant n (direct literals use the guarded slot initializer). `fixed_length_receiver_control_matrix` exact native output/count witnesses, whole-program/per-unit, Dev/Release. |
| Type families | Integer/float/bool/char/str fixed literals and owned-field `StructArray` are covered by both native matrices. Empty, nested and owned-string scalar literals retain their existing rejections in `fixed_length_preserves_array_formation_rejections`; no fixed-array field annotation or zero-length source formation is added. |
| Scope and control | Direct local, block, unsafe, arena, named arena, task_group, both if/match branches, else inside the receiver, Copy-array loop break, early return, and successful/failing `?` are covered by `fixed_length_receiver_control_matrix`. Fixed-array Option/Result payload formation and named field types are not widened. |
| Construction and lifetime | Receiver-local arrays/owners remain represented; existing function/loop local cleanup and synthetic owner machinery own construction, Drop and early exit. Length borrows a bound source and does not null it. `fixed_length_owned_receivers_keep_and_release_exact_owners` covers source reuse after if/match borrowing, named-arena receiver-local lifetime and enclosing-function cleanup and direct literal owners. |
| Replacement and return | Receiver statements may replace an owner or return early; retain existing statement execution/cleanup. No length result on a terminated edge. Both native matrices cover receiver replacement and early exits. |
| Generic and interface paths | Concrete fixed lengths remain producer-owned types after instantiation. Imported helper and generic scalar consumer checks through whole/per-unit owner. No interface record layout change. |
| Provenance and allocation | Borrowed fixed receiver uses the existing owner/provenance path. No new allocator or ABI; source-mandated owned element allocations now execute instead of being erased. Existing aggregate slots/SSA may materialize a borrowed receiver. `fixed_length_owned_receivers_keep_and_release_exact_owners` enables the requested-live probe, requires a positive allocation witness, and compares live bytes after each of 32 literal evaluations and partial-prefix exits. No performance benchmark claim. |
| Malformed HIR/source | Wrong Len receiver/result types still reject; malformed receiver descendants still reject before MIR. `fixed_array_len_child_and_result_contract` checks scalar/struct positive records and three mutations (result type, child length and unsupported receiver) through whole/per-unit rejection. |

The independent plan review found one P1 strategy gap: generic expression
lowering panics on `ArrayLit` and bypasses guarded element initialization.
The literal-specific existing slot initializer and explicit cleanup above close
that finding without widening first-class fixed-array expressions. The owner
must include effectful direct scalar/owned-record literals, partial owned-prefix
initialization followed by return/`?`, and bound-owned-array source reuse across
control joins. Perform one author matrix-to-diff pass and the normal
committed-candidate preflight review. The expected capability is well
below 1,000 hand-written changed lines.

## Review closure — free literal placement

The committed-candidate review found one P2: the expanded Len receiver domain
admitted a forged `Len(Block(ArrayLit))`, although the source checker rejects a
bare block-tail literal and generic MIR expression lowering cannot materialize
it. Preserve the existing source placement rule at the shared block, match-arm,
else-fallback and break validators, rather than special-casing Len's descendants.
This covers every scope and both if arms through their existing block validator.
The same audit found bare literal expression statements lacked a producer guard;
they now use `reject_bare_array_value` and the matching statement validator.
Direct `.len()` literals and existing initializer/collection-source literals remain
admitted, including guarded partial-owned initialization.

`fixed_array_len_rejects_free_literal_placement` starts from producer-valid scalar
and struct receivers, mutates only the free literal placement in nine scope/control/
statement positions, and requires rejection from all four MIR entrypoints.
`fixed_length_preserves_array_formation_rejections` owns the source diagnostics.
The literal strategy and ownership boundary remain unchanged; this local validation
correction closes against the original review and owner checks.
