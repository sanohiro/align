# External request follow-up investigations

## Selection and completion rule

The owner's 2026-10-20 direction queues useful, policy-consistent deferred
requests for investigation. The follow-up instruction selects them in queue order,
starting with voice V006; complete each useful capability before taking the next. Reconcile all three request registers
before selection; overlapping requests share one capability assessment.

Establish consumer value and four-way alignment. Distinguish a missing
implementation from a deliberately settled restriction using the actual design
record. Apply [Plan23](23-friction-ledger.md) when reopening a real settled
restriction; do not infer a policy prohibition merely from a current validator.
Separate safety/ABI dependencies from grouping chosen by an earlier plan.
Recommend implementation when usefulness and policy fit are established; state
substantial difficulty and cost explicitly. Broad scope by itself is not a
sufficient reason to continue deferring a request.

An investigation finishes with a reproducible witness, the smallest complete
useful capability, affected compiler/runtime/library layers, exact unresolved
invariants and prerequisites, acceptance owners and an implementation recommendation.
A public-contract or safety-strategy change then follows the ordinary ledger,
closure-matrix and independent-review gates. Consumer adoption remains external.

## Later capability queue

| Requests | Value and investigation output |
| --- | --- |
| Voice V006 | Provider implementation: [Plan231](231-fixed-scalar-heap-fields.md) qualifies primitive Copy fixed fields through heap construction, growth, transfer, Drop and exact zero-size storage. `shape := values[i].shape` uses the existing complete-field Copy read and replaces scalar expansion/reconstruction. Independent design review accepted Category A composition under the original view-free relocation rationale. Consumer adoption remains pending. |
| Media AM-R16 | [Plan232](232-indexed-fixed-field-values.md) implements direct independent fixed-field indexing, including chained record fields, through the existing complete-field Copy snapshot. Outer/inner bounds and effects remain ordered; inner evaluation may retire the parent after the snapshot. Retained slices remain separate: the current helper-return witness loses read-only authority in both frontends, so that capability must close interprocedural authority and lifetime together. Plan231's shared-match fixed-field storage exclusion remains unchanged. Consumer adoption is pending. |
| Media AM-R17 | [Plan233](233-c-record-fixed-arrays.md) implements positive fixed arrays of numeric C scalars in C records: layout, Copy/raw storage and both supported native by-value ABI families. Whole/per-unit native C, physical ABI, malformed-domain and cache-change owners close the boundary. Zero-length and non-numeric C arrays, nested records and naked extern arrays remain excluded. Consumer adoption is pending. |
| Media AM-R12 and AM-R19 | [Plan234](234-cancellation-sensitive-math.md) implements `expm1`/`log1p` on scalar and explicit vector f32/f64 through width-exact native math, with signed-zero/domain semantics, independent sample goldens and honest scalar-fallback visibility. Standard `log1p` replaces the unshipped reservation; no entire E2 tier or SIMD-provider prerequisite is imposed. Consumer adoption remains pending. |
| Media AM-R14 and AM-R20 | Trigonometry and quadrant-aware atan2 serve rotary validation and guide orientation. Fix argument order, axes/signed zero, nonfinite cases, argument reduction, accuracy and target/scalar/vector behavior. Assess a bounded complete subset rather than automatically requiring the whole E4 tier. Shared guide vectors qualify concrete consumer behavior, not an unproved cross-target bitwise contract. |
| Media AM-R15 and AM-R07 | Separate fallible raw allocation with an owned error/cleanup carrier from positional reads into externally owned writable views. Establish allocation failure before side effects, size/alignment/null obligations, exclusive backing, native lifetime and cleanup. Identify shared prerequisites without inventing an opaque mutable view or hiding ownership. |
| LLM R28/R132/R137/R142 and related media view composition | Reconcile the current remaining portions against shipped builder Copy reads, owning-field extraction and indexed independent Copy observations. Use [Plan61](61-interprocedural-view-access-plan.md), [Plan196](196-indexed-array-field-length.md) and [Plan210](210-text-builder-byte-views.md) to identify the exact missing interprocedural read-only/writable authority and retained-view proof. Do not reopen already delivered portions or widen a readable header into writable/owned backing. |

## Evidence-dependent compiler triage

The 2026-10-19 LLM reconciliation names the following residuals. Their exact
`request-adoption-20261019` source/log packet was not present in the available
local consumer/cache or its GitHub main tree during the 2026-10-20 audit. The
register supplies descriptions, not a reduced reproducer for these cells.
Acquire shared source and compact receipts first; do not attribute them to the
historical request's original cause or claim a repair from an older provider PR.

| Residual | First bounded investigation and acceptance |
| --- | --- |
| R37 large-function build memory | Obtain `reconcile-builder-before.align` and `reconcile-builder-build-memory.json` with the exact imported-source revision. The reported Linux VmHWM is 83,309,188 kB, about 79.5 GiB. Reproduce under an explicit memory cap after a headroom check; separate checking, MIR certification and emission, then reduce the responsible phase. Retain source/toolchain/phase and peak-memory receipts. Compare the final shared-span helper control; do not restore the retired fourteen-function owner or infer a complexity bound from one sample. |
| R49 mutable-helper validated-text refusal | Obtain `reconcile-builder-mutable-helper.align`, `reconcile-mutable-helper-check.log` and the passing borrow/builder probes. Check an owned source copy with the original imports, reduce the exact invalidation of `name` after mutation of distinct destinations, and classify it under the current storage/validation summaries. A repair must admit the safe caller and retain actual validated-byte mutation negatives in whole/per-unit modes. |
| R80 FILE_SET branch replacement | Obtain `reconcile-composition-source-trust-branch-failing.align`, both check logs and the failed-command receipt. In an owned copy of the exact client revision compare mutable branch assignment with the qualified immutable conditional construction. Reduce the erroneous loop-iteration expiry in the loop-free function before choosing an ownership fix. Retain real expiry, alias and branch-join negatives; consumer source-trust execution remains external acceptance. |

These are pending investigations, not completed fixes or consumer-verification
claims. Source copies used for compiler reproduction belong to Align-owned
scratch; no consumer code, pin, build machinery or Git state is modified.
