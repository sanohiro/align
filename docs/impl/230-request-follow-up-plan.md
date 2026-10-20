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

## K1 scope and difficulty assessment (2026-10-20)

The owner selected independent AM-R16 element reads first and an assessment of
K1 before implementation. PR1398 delivers those reads. Retained fixed-field
slices and either parked K1 implementation remain unselected. This assessment
records current evidence and decision criteria; it adds no language contract or
implementation authorization.

Both frontends still accept Plan232's compile-only negative: a function returns
`row.shape[..]` from a shared `borrow row`, and its caller writes through the
returned slice. The reader control passes. Never execute this negative.
`ReturnBorrowSummary` contains parameter/capture indices, and
`summary_from_roots` drops `ReadOnly` and temporal observations. The existing
summary therefore proves neither writable backing nor continuing validation.
Plan210's text-byte return witness exercises the same missing transport.

| Remaining request | Useful result | Missing proof beyond delivered values |
| --- | --- | --- |
| AM-R16 retained fixed-field slice | Read a selected indexed field without a whole-owner binding or hidden copy. | Parent backing lifetime/reservation and read-only authority through calls, returns and mutable destinations. Independent element reads already ship. |
| R28 text and view-bearing builder reads | Inspect initialized builder data without materializing an owned copy. | Retire the selected generation on growth, writes, build, replacement and Drop; transport contained view origins. Length and independent Copy element reads already ship. |
| R137 retained owning-field projections | Pass an indexed owned-array field to ordinary readers/pipelines. | Preserve parent generation, selected field authority and both bounds across calls/returns. Length and independent Copy IDs already ship. |
| AM-R07 external writable read destination | Read directly into caller-owned native storage. | K1 is necessary but insufficient: exclusive external-view formation, initialization, bounds/overlap, native resource lifetime and synchronous short-read/error semantics need their own contract. No pinned-memory speed claim is established. |
| R132 exclusive owned subrecord | Pass one stable owning field to a mutating helper. | Separate cleanup-representation problem: a whole aggregate has one allocation-mode bit. Replacing one field must neither change sibling cleanup nor free arena storage as heap. Authority, replacement compatibility and allocation mode must close together. |
| R142 exclusive active sum payload | Mutate an active payload through a helper without consuming it. | Preserve active tag and exclusive parent authority, exclude conflicting replacement, and retain exactly-once cleanup; nested owning fields may also encounter R132. The retired Qwen2 design is not current product adoption evidence. |

At candidate `1e3dd74d`, reduced R132 and R142 packets reject in both frontends
with the current partial-Move-field and mutable-storage diagnostics. Their
whole-owner mutation and shared-reader controls pass. These are compile-only
observations, not evidence that removing either rejection is safe.

The smallest complete K1 repair crosses permission producers/projections,
callee effects, return values and mutable outputs, aggregate/tagged carriers,
branch/loop joins and recursion, imported generic/interface/cache transport,
and independent checked-HIR/MIR/native admission. A return-only read-only flag
would repair one witness while leaving hidden writes and output paths open.
Metadata production and its caller/validator consumers cannot ship as separate
dormant capabilities.

The main uncertainty is representation, not syntax or a missing method. Whole-
parameter roots lose independent sibling-field precision; exact selected paths
can grow through recursive slice graphs. A finite abstraction must preserve
permission/backing correlations and useful disjoint-field precision while
terminating. Mutation must end overlapping generation/validation facts without
invalidating unrelated destinations. Cached facts require independent replay,
not a trusted permission flag. Plan61's parked implementation and isolated models
are evidence to examine, not an accepted end-to-end solution or a cost estimate.

This is a substantial cross-compiler capability, materially larger than the
completed Copy indexing. A defensible implementation estimate needs a selected
finite representation and a closed interface protocol first. R132's allocation-
mode repair and R142's exclusive payload projection are related ownership axes,
not automatic benefits of K1, and must be estimated separately.

Recommended next decision: select a bounded feasibility investigation before
committing to full implementation. Its output must be (1) a finite-domain and
termination argument, (2) a small executable model checked against concrete
permission/alias/recursive-join counterexamples and independent-field controls,
and (3) one end-to-end producer/interface/caller/validator closure matrix with
remaining risks and a credible implementation boundary. If no sound, sufficiently
precise representation emerges, report the specific failed invariant and keep
retained views deferred. This investigation has no new public surface, no broad
benchmark claim and no consumer adoption work. Any selected implementation then
receives the ordinary independent strategy review before compiler changes.

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
