# Fixed-array literals at admitted value consumers

Status: implementation candidate.

Plan 141. This repairs existing source-accepted consumers; it adds no
syntax, type, placement, borrow, or ownership admission. Plan73 and ledger19's
fixed-array construction/element records remain authoritative. K1/plan61,
standalone fixed-zero aggregate-builder canonicalization, pre-existing
bound builder-result aggregate call provenance, and indirect/borrowed
fixed-array temporary arguments remain outside this capability.

## Evidence and boundary

At main 8a6c6f0, a bound `[i64; 2]` or `[Row; 1]` can be pushed into its region
builder, but the equivalent direct literal passes `check` then panics in
`check-per-unit` and `emit-mir`: lower_expr_recursive assumes ArrayLit only occurs
as an initializer/pipeline source. A direct named function with a fixed-array
ByValue parameter has the same defect for Copy and in-place Move-record literals.
The snapshot's SHA-256 and exact sources/verdicts are in assessment.json,
sibling-probes.json and ownership-probes.json.

Sibling controls: a bound-returning helper used as a builder argument already
lowers, as does a record literal containing a fixed-array field. Whole-array
reassignment and bare literal function results are rejected in source. Function
values with fixed-array signatures and borrowed temporary fixed arrays are also
rejected. Preserve those boundaries; do not turn their rejections into admission.

## Existing contract and implementation

A fixed-array literal initializes its elements exactly once in source order and
materializes N consecutive inline elements with the already-selected fixed type.
Copy elements add no heap owner. An in-place Move record keeps each completed
owned leaf live until the complete literal is available; on early exit, only
completed owners are released. A consuming call transfers the completed literal
and its cleanup bit once. Later argument failure must release that completed
argument; a successful call owns it. Borrow-bearing Copy leaves retain existing
source provenance. No implicit deep clone or new native allocation is added.

Route the general ArrayLit value arm through the existing
materialize_array_literal/store_array_elems authority. Transfer any completed
synthetic owner into the returned SSA cleanup fact before the enclosing consumer
claims it, mirroring existing StructLit value lowering. Initializers and field
literal stores retain their existing direct destinations. Do not add a builder-
specific literal rule or a new IR variant. Checked HIR remains the authority for
which consumer contexts and element types may reach this operation.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Type formation/validation | No sema domain change. Exact literal type/count/element and permitted context remain checked by ledger19 ArrayLit. Reuse malformed literal/HIR owners. New `fixed_literal_values` owner checks forbidden bare return, branch tail, replacement, temporary borrow and indirect signature siblings. |
| Construction / Copy value | Shared materializer and existing per-element stores, then aggregate Load. Direct named-call and builder-push owners cover numeric, borrowed text, Copy record and fixed-record families; bind controls produce identical output. Include direct zero-length fixed function arguments where admitted, separate from deferred zero-length aggregate-builder descriptors. |
| Move-in / move-out / nulling / Drop | Existing in-place Move-record construction and temporary flags. New native owner checks normal consuming calls, later-argument early exits and later-element failure; exact runtime allocation/free deltas or delegated native allocation ledger prove no double free/leak. Array builder remains RegionPlain for fixed aggregates. |
| Ordered control / return / joins | Elements and later arguments cover if/match/else/?/map_err, loop and early-return paths using existing scalar/control expressions. No store or outer call follows a terminated element. Whole/per-unit observed output and MIR validity close the paths. Bare aggregate control-result admission stays rejected. |
| Generic / interface / cache / compilation modes | Direct imported generic instantiation with fixed literals follows existing source-template rechecking and type identity. Whole/per-unit and cache hit/edit/restore must preserve output and cleanup. No schema, canonical-byte version, runtime symbol or ABI shape changes. |
| Provenance / storage / allocations | Copy view payloads keep source dependencies; source-end/mutation negatives reuse the existing borrow owners and focused literal inputs. Inline scratch uses the exact existing layout. No benchmark: no latency or resource improvement claimed. |
| Depth / malformed input | Shared out-of-line materialization avoids enlarging recursive lowering frames. Run expr_depth and existing fixed-literal malformed-HIR owners, with no new panic on source-admitted literals. Existing scalar-Move and Move-enum prohibitions remain. |
| Test lifecycle | Use the parent-owned ArtifactStage and bounded process-group fixture shipped with plan140. All compile/link/run/cache and native allocation probes execute below that guard; retain its stalled-native cleanup control. |

This is one useful producer-to-consumer repair. Splitting Copy and Move literal
argument lowering would duplicate the same materialization and cleanup proof.
Implementation follows the already reviewed plan73/ledger19 construction
strategy; the author matrix-to-diff pass and one fresh preflight review will
check those boundaries. No separate public design or strategy is proposed.

## Owner closure

`crates/align_mir/src/lib.rs::lower_array_literal_value` calls the existing
`materialize_array_literal`/`store_array_elems`, carries the completed live bit
on the SSA value, and retires its internal owner. The surrounding call's existing
`lower_consumed_call_arg` owns the completed argument until call commitment.
No store or value load follows a terminated element.

`crates/align_driver/tests/fixed_literal_values.rs` closes the new consumer paths:

- `copy_literals_at_named_calls_and_builder_pushes`: scalar, live text view,
  Copy record, nested fixed field and empty literal; exact reached-element order,
  direct/bound builder content parity, whole/per-unit execution.
- `owned_elements_and_later_failures`: 32 iterations with exactly 800 allocations
  and 800 frees; complete transfer, bound-source nulling, later-element and
  later-argument return, if/match/else/?/map_err, inner-loop value and outer-loop
  break. Whole/per-unit execution uses the same independent expected counts.
- `cached_generic_literals_preserve_outputs_after_edit_and_restore`: imported
  generic body rechecking and two instantiations, cold/hit/edit/restore frontend
  states, MIR snapshots and actual native output.
- `existing_admission_and_view_lifetimes_stay_checked`: bare return/branch,
  whole-array replacement, temporary borrow and indirect signatures still reject;
  a literal-carried text view cannot escape its local owner.

Existing `fixed_array_fields`, `owned_structs_arrays`, `owned_temporaries` and
`expr_depth` owners close unchanged field, malformed producer, element-domain,
partial-cleanup and depth invariants. The MIR unit owner
`fixed_array_move_shapes_match_the_hir_gate` retains forged owned-view and
unsupported scalar-Move rejection. No new type, IR variant, runtime ABI,
serialization version or public contract is introduced.

The bound builder-result aggregate-call provenance failure reproduces on the
unchanged baseline with a bound literal: source checking succeeds, then LLVM
emission rejects the call's owned-leaf provenance. It is independent of literal
value materialization and remains deferred; builder result contents are observed
through the existing element-field access in this capability's owner.
