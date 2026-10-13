# Complete record-field assignment publication

## Existing contract and boundary

Plan65 row O requires a complete retained RHS, source transfer, old-value Drop,
then replacement publication. Plan73 carries the same rule through fixed-array
fields. The checked-HIR ledger's aggregate replacement row installs the selected
subtree only after RHS evaluation. This capability repairs those existing rules;
it changes no source admission, public type, IR variant, interface or runtime ABI.

Plan171 found a distinct `AssignField` defect. A direct `StructLit` currently
uses `store_value_at` on the destination: earlier members publish before later
members evaluate, and the old aggregate's Drop is skipped. A later member can
replace the entire root and overwrite the already-published prefix. The native
Copy-inner/Move-outer witness returns 1 instead of 0 on the Plan171 baseline.
Scalar/call RHSs also lack a post-evaluation root-shell liveness check.

Keep this as one capability: checking a final root without staging the literal
would leave intermediate writes unsafe; staging without final liveness checking
would still publish into a consumed root. No dynamic-array field admission,
indexed ownership extraction, partial tagged writes, K1 or general aggregate
provenance is included.

## Implementation closure matrix

| Invariant | Implementation and owner |
| --- | --- |
| Formation and validation | Preserve existing source/checked-HIR field types, mutability, path validation and nested Move-extraction restrictions. New `field_assignment_publication` owner pairs admitted scalar, Copy/Move record, fixed-array and borrowed-record targets with whole-root consumption negatives. Semantic replay must reject a forged consuming RHS before either checked lowering mode. |
| Final destination shell | Both recursive and transparent-spine `AssignField` walkers recheck immediately after successful RHS evaluation, before clearing the restored field marker or updating storage facts. `Whole(root)` rejects. A moved outer field rejects a strict descendant; the exact replaced outer field and independently moved siblings do not erase the destination shell. Keep existing pre-RHS conservative refusals. Whole root reinitialization during RHS may restore a valid destination. |
| Literal construction | Materialize a direct `StructLit` through the existing value-position `lower_expr` / `store_consumed_struct_fields` producer. Direct bound members remain readable until the complete constructor succeeds; fresh members retain their existing hidden cleanup owners. A whole load is available only after successful construction. Do not redirect fresh-local or native-descriptor `store_value_at` callers. |
| Move-in/out and source nulling | The canonical value producer commits retained direct bound sources only after successful construction and retires its fresh owners. A later rebind of a retained source remains a completion-snapshot error. Already-transferred child results retain their existing cleanup and path-local nulling rules; do not replay their transfer. Nonliteral RHSs retain their existing path-local source transfer after capture. Preserve exact self-assignment as a no-op and `pair.left = pair.right` as a transfer. Test later reads/clones and return/break of a retained direct source, wrapped-source cleanup, sibling transfers, retained-source rebind rejection and Copy-result root reinitialization. |
| Old Drop and publication | After complete RHS capture and source transfer, read/drop the old selected value under the root's existing ownership bit when its Drop plan requires it; then issue one complete `StoreField`. Preserve siblings. Fixed-array literals retain their existing complete materializer and owner retirement, including empty arrays. No new heap allocation or cloning is introduced. |
| Partial failure and return | Existing hidden owners release completed fresh members on reached return, break, failed `?`, and `map_err` failure; retained direct sources keep their own cleanup/return authority. The enclosing assignment does not touch the old destination on a terminating RHS. Hard traps/divergence have no successor action. Native allocation/free and live-byte counters cover success/failure, bound/fresh leaves, nested literals and fixed-array members. |
| Control and joins | Parameterized owners cross block/if/match/loop/else/`?`/`map_err`, normal and single-statement transparent-spine walkers, complete/partial root rebind, and mixed fallthrough/terminating members. Index assignments retain Plan171. Each member evaluates once in source order. |
| Runtime provenance | Reuse exact existing temporary flags and the root's uniform ownership authority. Heap and supported arena controls must preserve their current release path; no mixed-provenance admission or new whole-aggregate ownership inference. |
| Generics, imports and cache | Generic record forwarding and imported RHS helpers execute in whole-program and per-unit builds, including a cold/cache-hit pair. Source layout, summaries and serialized records are unchanged; compiler identity invalidates changed generated code. |
| Malformed HIR/MIR | Existing shape/type gates remain authoritative. Forged-HIR replay covers post-RHS moved root; the cumulative MIR producer suite checks staged whole loads and field payload proofs. Add a targeted structural owner only if the new staging path exposes a distinct construction invariant not owned there. |
| Allocation and resources | Runtime counters require every explicit heap payload allocated for the fixture to be freed exactly once, including replaced old aggregates and abandoned staged prefixes. Preserve supported arena behavior. The anonymous slot is inline compiler storage; no performance promise or benchmark is added. |

The new driver owner is `field_assignment_publication`. Reuse
`owned_field_replacement`, `owned_tagged_payloads`, `owned_structs_arrays`,
`fixed_array_fields`, `fixed_owned_strings`, and the MIR partial-construction
owners for their existing domains; do not manufacture a test per matrix cell.
`field_target_liveness_after_rhs` owns 26 post-RHS refusal rows across target
shapes, control forms and both walkers. `retained_source_rebinding_stays_rejected`
owns six snapshot negatives, and `forged_field_assignment_replay_rejects` owns
root/nested paths through semantic replay and both checked lowering modes.
`field_publication_lifecycle_and_replay` owns the named native scenarios above:
Copy publication/root rebind, direct source read/clone/return/break, partial
fresh construction, exclusive borrowed records, sibling/self transfer, nested
fixed arrays, wrapped-source cleanup and heap/arena allocation modes. It requires
positive allocation counts, exact free parity and zero retained bytes after
function exits, in whole/per-unit and cache-hit execution. All native probes use
the bounded `owned_fixture` lifecycle helper.

## Author consistency pass and review

The field-specific path is the only destination publication change. The root
check is shared by the two existing semantic walkers, with termination guarded
by their existing completion boundaries. No metadata producer or validator is
weakened. Independent adversarial plan review found that immediate bound-source
nulling
would repeat the existing later-member observation defect. The revised matrix
uses the reviewer-qualified canonical value-position producer, preserves
retained-source snapshot rejection and separates Copy-result root rebinding.
The direct-source read/clone and return/break owners close that finding. Complete
the matrix-to-diff pass, focused owners, one fresh full-diff review and exact-SHA preflight.

## Separate canonical aggregate transfer boundary

This field consumer uses the existing completed-value producer. It does not
change general constructor transfer timing. Review identified an existing
producer issue where `Some(source)`/`Ok(source)`/`Err(source)` or an arena-wrapped
bound source may be moved into a hidden owner before a later outer member reads
that source. The issue also exists in whole-value construction and the former
incremental field route. No new admitted stale-source clearing was identified
from reusing the canonical producer. Preserve current wrapped-source cleanup
parity here; Plan173 repairs the general deferred wrapper-transfer boundary as a separate
producer/consumer capability. This destination repair does not own that change.

The boundary is independently useful: it removes partial destination publication
and restores old aggregate Drop using an already-shipped value producer. It is
not a dormant producer/consumer split. K1 and general aggregate provenance remain
parked.
