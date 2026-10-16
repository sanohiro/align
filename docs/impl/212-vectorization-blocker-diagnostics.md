# Honest vectorization blocker diagnostics

At `af679ad7`, `explain-opt` tells a loop implementing
`xs[i] = xs[i - 1] + 1` to write into a distinct output array. The real loop
carries updates between iterations; changing to an independent destination can
change its result. The translator classifies array-bound discovery failures and
all memory-operation refusals as aliasing, and classifies any reason containing
`call` or `reduction` as a more specific cause than LLVM establishes.

This repair implements Plan09's existing honesty rule. It changes diagnostics
only: no optimization, source evaluation, ownership, ABI, interface or cache
record changes. Match the known LLVM22 refusal messages, not isolated keywords.
Distinguish unknown access bounds, unsafe or unproved memory ordering, an
unvectorizable call, an unrecognized reduction, floating-point ordering,
unknown trip count and profitability. A memory-ordering refusal does not prove
aliasing; a call refusal does not prove that no vector implementation exists.
Neither receives a speculative source rewrite. Unrecognized reasons retain the
complete raw reason in the existing neutral diagnostic.
Dependency remarks can append a newline and a conflicting-access location;
their exact first line owns classification while the complete detail is retained.

Acceptance owners:

- `explain::tests::vectorization_blockers_do_not_invent_causes_or_remedies`
  parameterizes known LLVM22 memory, bounds and call refusals, exact source
  location/raw-detail preservation, default/verbose visibility and unknown
  reasons that contain misleading keywords. Existing FP/reduction/cost owners
  preserve the neighboring cases.
- `explain_opt::dependent_memory_loop_does_not_suggest_independent_output`
  drives the real LLVM22 stream in both report modes. The fixture retains an
  in-place recurrence and checks the source-located memory-ordering explanation
  without proposing a separate output. Its process and scratch are owned by the
  existing bounded fixture harness.

The focused gate is `align_driver --lib explain::tests` and the `explain_opt`
CLI target. Self-review, one independent inspection and final code preflight
apply. There is no performance claim or need to repeat SIMD qualification.
