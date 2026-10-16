# Stable captures in sequential par_map lowering

Plan221 found temporary HIR capture addresses entering the eager-expression
cache in sort lowering. Its sibling audit identified the sequential `par_map`
fallback, which clones every source stage and appends a cloned terminal map.
This capability removes that remaining synthetic-HIR boundary. It preserves
Plan11's execution selection and capture contract, Plan12's ordinary pipeline
semantics, and Plan19's checked-HIR/termination boundary.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and validation | No source/HIR/type/IR variant or admission change. Reuse checked-HIR and `par_map` owners. Original stages and terminal captures remain borrowed HIR throughout lowering. |
| Evaluation and identity | Extend the internal borrowed `CollectKind` with a terminal map, beside collect and scan. Prepare original stage captures in order, then original terminal captures once, before allocation/loop entry. Native `par_map_capture_identity` covers distinct sibling branch captures in terminal and prior stages, nested projection and SoA fallbacks, with an unchanged parallel control. |
| Complete values and storage | Apply the prepared map to the surviving current element before append, using the ordinary named-call ABI and output type. Native owners vary input/output scalar types, empty/nonempty filtered results and branch results. Source copies, output allocation/header and physical storage remain existing collect behavior. |
| Ownership and cleanup | No new owner, transfer, source nulling or Drop rule. Existing source setup and temporary retirement remain shared. Native owners cover heap/arena, temporary/bound sources, return, replacement and discard. Reuse the collection allocation owners for unchanged storage rules. |
| Control and termination | Source, stage and terminal captures retain source order and stop immediately on return. Extend the existing `current_plan_capture_termination_preserves_only_reached_decisions` owner to sequential stage/terminal captures; no later map, allocation or selector record may follow termination. Branches, match/loop joins and early return use the same existing lowering machinery. |
| Donation and current plans | A terminal map is donation-safe exactly like the formerly appended map stage; removing that stage leaves `donation_stages_ok` unchanged. No selection/reason/ordinal changes. Reuse `current_plan_par_map_rows_follow_the_consumed_execution_form` and donation owners. |
| Generic/import/replay | No interface or cache encoding change. Native owner exercises whole/per-unit calls and frontend cache replay. Existing generic/lifted callback resolution is unchanged. |
| Verification | Focused native owner, existing `par_map`, scan/collect and current-plan owners, bounded gate and separate-target Clippy. No runtime/FFI change or performance promise; no benchmark. |

This is one representation-preserving repair, not a new ownership or ABI
strategy. The existing reviewed stable-HIR invariant applies unchanged; the
author matrix pass and one preflight adversarial code review own the boundary.
No public contract or language mirror changes. The expected diff is below
1,000 handwritten lines. A regression must distinguish the old cloned-capture
path; a passing minimal CLI experiment alone is not proof of its safety.

## Author closure

A 16-arm nested-record projection fixture failed before the fix with LLVM's
`Instruction does not dominate all uses`: every later branch reused the first
branch's capture load. The committed native owner retains that witness and
executes original map/filter captures, hoisted string needles, full record map
arguments, narrower integer and float results, SoA output inside an arena,
source-buffer donation through a temporary scalar producer, and return/replace/
discard/match/loop control. Whole-program execution and both cold/hit two-unit
frontend executions check exact output. A simpler three-arm CLI probe happened
to pass; it was not accepted as a safety proof.

`CollectKind::Map` and `PreparedCollectKind::Map` apply the terminal callback
after unchanged original stages. Preparation borrows original HIR for both
prior-stage and terminal expressions; there is no remaining `stages.clone()`
or `captures.clone()` in MIR expression lowering. Source setup, donation, output
storage, call ABI and cleanup remain shared. Sequential source/stage/terminal
termination reuses the current-plan owners, with two new stage/terminal early
return cases asserting no collection allocation or later execution decision.
No new public source form is admitted: arbitrary record-returning call roots
and aggregate `par_map` results keep their existing restrictions. Temporary
source coverage uses an already-supported scalar producer and nested-record map.
The native owner also runs with needle hoisting and buffer donation disabled;
its replay children pin ordinary cache-enabled settings in either invocation.
