# Loop-relative generation advancement

## Evidence and invariant

Plan230 authenticates R80's nested MTP witness and a GPU-independent reduction:
an array created in an outer iteration is incorrectly expired inside two further
loops. The extracted `run_case` control passes. This is separate from R80's
loop-free Option-array branch assignment and R37's analysis resource growth.

The existing finite Current/Prior generation model and language remain unchanged.
A control edge's advance set is relative to that control construct's entry.
For a `break`, collect advances within the innermost active loop, including its
nested branch/expression frames, but exclude advances preceding that loop.
When that loop completes, propagate its reached exit's advances into its parent
frame normally. An outer iteration's producer must not be aged merely because
an inner loop exits. Actual regeneration, moves, replacement, iteration cleanup,
temporary cleanup and escaped-view rejection retain their existing semantics.

`active_storage_advances` currently collects every enclosing frame. A nested
break therefore reports an outer producer as an inner-loop advance. Subsequent
joins can age the still-live Current generation into the ended Prior generation
from a previous outer iteration. Limit the collection at the already-existing
innermost Loop marker; add no state, generation kind, annotation or exception.

This is a correction within the reviewed generation strategy, not a new safety
model or public contract. The author-side matrix below precedes implementation;
the fresh preflight review will inspect the boundary and complete diff together.

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Formation and construction | Keep all existing type/provenance formation. `loop_generation_scope::depth_and_owner_matrix` crosses one through four loops and owner declaration depth, arrays and string/scalar controls. |
| Move-in/out, source nulling, Drop, replacement and return | No runtime/MIR ownership change. `loop_generation_scope::actual_expiration` and existing `borrow_liveness`, `owned_temporaries`, `loop_expr` owners preserve genuine dropped/consumed local and hidden-temporary refusals and transferred loop results. |
| Break and fallthrough | Both statement and iterative value-bearing Break walkers use the same loop-relative collector. `loop_generation_scope::control_edges` crosses plain/value breaks, multiple exits and fallthrough, with real regeneration and stale-view negatives. |
| Branch and early-exit composition | Preserve advances inside if/match/else/?/map_err expression frames and nested loops; exclude frames before the target loop. `loop_generation_scope::control_edges` and existing `owned_borrowed_composition` owners cover returned arrays and loop-free branch replacement's current boundary. |
| Generic/imported and whole/per-unit | `loop_generation_scope::imported_native` exercises imported generic array readers and helper extraction in both compilation modes, including cold/hit interface replay and exact native values. The authentic private MTP control/witness are replayed under the existing bounds. |
| Independent HIR replay and malformed inputs | Existing checked-HIR body replay runs the same MoveCheck; source acceptance must agree with MIR validation. No HIR/MIR shape or trusted flag changes. Existing sema generation/control owners and the bounded gate retain malformed-input coverage. |
| Runtime provenance/allocation | No runtime, allocation, ABI, storage layout or emitted cleanup changes. Native nested-loop values and existing temporary/loop cleanup owners close this unchanged boundary. No performance promise or benchmark gate. |

## Scope and completion

One small capability PR owns this scope correction and its regressions. Do not
relax invalidation, suppress diagnostics, reset joined facts or redesign the
solver to accept the witness. If the scoped change does not close the complete
matrix, identify the missing invariant before widening implementation. R80's
Option assignment and R37 remain separate subsequent work.
