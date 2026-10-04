# Immediate array materialization before sum

This implements the check-time advice proposed in plan12 §8.5. It makes the
already-documented fused spelling easier to discover. No inference, execution,
ownership, allocation strategy, MIR shape or optimization decision changes.
Plan31 S6 profile inputs and hot/cold guidance remain unscheduled.

## Public diagnostic ledger

| Surface | Exact contract |
| --- | --- |
| Input and default | Existing source checking, with the ordinary standard lints enabled. No flag, environment input, runtime profile or threshold is added. |
| Admission | A finalized numeric `ArraySum` whose stages are empty and whose direct source is `ArrayToArray`. Integers and floats are admitted. Parentheses introduce no HIR node. No alias/use-history scan or additional HIR traversal is performed. |
| Diagnostic | Warning text: ``intermediate array before sum: consider removing `.to_array()` when only the sum is needed``. Anchor at the direct materializing source expression's exact existing span, including its source file identity. |
| Repetition | Emit at most one warning with this exact text and span within one Diagnostics owner, including repeated finalization or generic instantiation. Different source spans each warn. Existing diagnostics and their order remain; append at the existing child-first finalization point. |
| Silence | A named, returned, forwarded or otherwise indirect array source; intervening pipeline stages; another terminal; and a nonnumeric/error result do not match. An explicit materialization followed directly by sum inside such a larger program still matches at its own site. Scanner Result carriers are indirect and do not match. |
| Meaning and errors | Advice about requesting an intermediate array, not an automatic rewrite or a claim that a runtime allocation survives optimization. The original program remains valid; errors, effects, traps, integer wrapping, floating modes, evaluation, Drop and source inference are unchanged. A manual rewrite must preserve intended types and stage boundaries. |
| Ownership and allocation | Existing HIR/source owners are borrowed. Reuse existing diagnostic text/span inspection for deduplication; allocate the owned warning message only for a new warning and use the existing diagnostic vector. No language/runtime owner or allocation rule changes. |
| Owner, identity and prerequisite | align_sema's finalizer owns detection; the existing driver renders it. No runtime/package owner, ABI symbol, artifact, interface-format field or new cache-key component. Existing compiler-producer identity applies. Numeric sum and explicit array materialization are shipped prerequisites. |
| Acceptance | Parameterized sema library owners cover positive and silent shapes, exact severity/span, repetition and generic instantiation. Existing mmv2 owners preserve materialized execution; source checking pairs original and fused typed source, including ordinary effects. A driver owner checks whole/per-unit warning provenance. |
| Performance | No throughput, memory ceiling or compiler-speed promise. No benchmark is a correctness gate. |
| Source agreement | draft §16; language-spec lint list/count; design-notes; the Settled record; plan12 §8.5; this plan; guide26 English/ja; HANDOFF capability status. No library mirror or runtime ledger changes. |

The direct typed shape is deliberately finite. The warning does not erase a
callable, speculate a lane, donate storage or reopen deferred K1 authority.
Array materialization can remain intentional; the diagnostic offers the
existing fused alternative without changing the compiled original.

## Author implementation checklist

| Obligation | Implementation / owner |
| --- | --- |
| Final numeric type and exact direct source/stage shape | Separate ArraySum finalization arm; parameterized positive/silent typed programs |
| Existing child-first order and no topology change | Reuse finalize_expr/source/stage calls; existing exhaustive finalization and deep-HIR owners |
| One exact-span warning | Existing Diagnostics iteration; same-span and distinct-span controls plus generic source checking |
| Original source remains executable | Existing mmv2 materialized execution owners; paired original/fused impure-map source checking |
| Imports and per-unit source identity | mmv2 immediate_materialization_sum_lint_imported_source_provenance checks both source-checking paths with an imported numeric helper |
| Documentation consistency | Author ledger-to-prose extraction and paired guide pass |

This single diagnostic is a narrow public contract, not a cross-cutting language
or safety strategy. Perform the author checklist-to-diff pass, applicable
self-review and one fresh independent full-diff inspection before the normal
owner, bounded gate, Clippy and PR flow. Add no test binary or shared harness.
