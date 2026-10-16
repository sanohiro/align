# Vectorization profitability diagnostics

Request116 / Plan68 G10 requires a source-located explanation when LLVM declines
loop vectorization. At `56f26a8b`, a counted ordered-f64 loop produces the LLVM22
message `the cost-model indicates that vectorization is not beneficial`, but
normal `explain-opt` buckets it as an untranslated remark. Only `--verbose`
reveals the decision. The existing prefixed refusal already maps to CostModel.

This repair implements the existing Plan09 diagnostic contract. It changes no
optimization, source semantics, ownership, ABI, interface or cache record. The
complete unprefixed message joins the existing loop-vectorize miss path, with
CostModel reason, original file/line/column and retained raw detail. Default and
verbose reports each show the same one diagnostic for that input remark.
Interleaving and SLP profitability messages stay separate untranslated remarks;
unknown locations remain compiler-internal. Unrecognized message extensions
remain raw rather than guessing their meaning.

Both spellings say only that the compiler's cost model found no benefit. They
must not invent a short trip count, cheap body, aliasing/FP cause or advice that
nothing in source could help. This reports an optimizer decision, not proof of
the final machine loop shape: later unrolling, SLP or elimination may occur.
No blanket claim that every source loop has an LLVM reason is introduced.

The parameterized driver unit owner
`vectorization_cost_refusals_are_actionable_without_invented_causes` covers both
spellings, exact location/reason/raw-detail transport, default/verbose visibility,
interleaving/SLP/unknown-message negatives and unknown-location suppression.
The existing FP, reduction, vectorization-success and bucket owners retain the
neighboring behavior. A provider-local counted-loop CLI witness records the
real LLVM22 message before and after the repair, without running the program.
The focused gate is `align_driver --lib explain::tests` plus the existing
`explain_opt` CLI target; ordinary code preflight and independent review apply.

This closes the observed profitability-reporting gap in G10. Request116's
broader corpus and consumer measurements remain separate; current owner plans,
not the original audit's historical status table, determine other guarantees.
