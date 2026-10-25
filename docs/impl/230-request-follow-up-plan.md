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

## Completed selected batch (2026-10-25)

Plan240/#1409 delivers K1's selected call-boundary repair, with all three
consumer adoption receipts. The resumed batch delivers AM-R22 in Plan241/#1412,
R28 and the authentic R49 precision repair in Plan210/#1413, AM-R16/R137 in
Plan242/#1414, AM-R23 in Plan243/#1415, and AM-R07 in Plan244/#1416. AM-R24/#1410
preceded this batch. Consumer adoption remains separate; R132/R142 were excluded.
The source-packet investigations below are complete at the evidence/reproduction
boundary. R37 resource growth and both R80 precision witnesses remain unfixed.

## Authenticated request investigation (2026-10-25)

The previously missing source packet is now available privately and verified:
archive SHA-256 `38125adc4a41127f2e348d2688f3b850d84d7459da2511a16e69d26a073e3402`,
832,473 bytes, all 379 internal checksums. The 362-file source snapshot binds to
consumer commit `33e4d3ff40f45f48a4edf2a2f155a9e7966aeaed`; two missing imported
sources were restored from that exact commit, with separate hashes. Private
source and raw logs remain under `.git/handoff`, outside Git/PR publication.

| Request | Reproduced boundary and completed investigation result | Remaining work |
| --- | --- | --- |
| R49 mutable-builder helper precision | The authentic helper previously rejected an unchanged text owner after an independent builder mutation. Plan210/#1413 repairs lost identity for statically headerless owners without weakening unknown collection proofs. Original helper and immutable control pass both check modes on the final Plan210 compiler; mutation negatives still reject. | Consumer verification remains external. |
| R37 compiler resource growth | On macOS arm64 with opt-level-1 compiler and a supervised 3 GiB RSS cap, the complete inline-builder program passes whole-program check in 19.783 s (sampled peak 292,656 KiB), but MIR emission reaches the cap in 18.621 s. A reduced imported module reaches the same cap in 7.372 s for emission and 8.719 s for per-unit check; the shared-helper control emits in 2.970 s / 114,688 KiB. Samples place the reduced frontend cost in checked-HIR body analysis/MoveCheck loop replay. Whole-program control emission has a separate 240 s bounded stop with active CPU at 605,232 KiB in native producer analysis. | This closes the missing-evidence/reproduction task, not the compiler complexity defect. Separate frontend replay growth from whole-program native-producer cost before changing a solver; preserve the cap and owner comparison. No speed or asymptotic repair is claimed. |
| R80 optional owned-array assignment | The authentic old FILE_SET branch witness rejects in both frontends while its immutable control passes. A synthetic loop-free reduction also rejects: assign a Result-match payload array into a mutable Option local and return it. Scalar/string controls, immutable conditional binding and else-unwrap assignment pass; a record containing an array fails. The reduced matrix still reproduces on the final Plan242 compiler. | Investigate branch-local active payload/release ownership before any precision fix. Merely ignoring a staging release when a joined Local release also exists is not a sound repair. The separately authenticated nested-MTP witness is reproduced below; its loop-depth defect must not be conflated with this branch-local Option assignment. |

The R37 figures are bounded observations, not benchmark claims: RSS was sampled
at 100 ms, cap overshoot is possible, and the compiler/source bindings
and per-run hashes are preserved in the private result manifests. R49 final
replay and R80 final reduction have their own compiler hashes; an earlier R37
measurement is not silently relabeled as a final-batch measurement. Historical
high-RSS R37 logs justify the bounded supervisor, not reproducing that allocation
on the current 16 GiB machine. No receiving application was changed or adopted.

### Additional R80: authenticated nested-loop MTP witness

The owner confirmed reproduction base `2be36e7669df822c6ddd8516c1b926fe6687197a`
from the dated manifest and evidence index. The original log predates that commit:
execution is inferred from Git history/timestamps as
`8fb5e72ba94bf1ce313846e7f48c16f3c804e3fc` plus uncommitted changes, using compiler
`b39d9ee88d42ddd47966ce8c6b0c070ea4efd56c`. That inference is recorded separately
from the authenticated reproduction base; it is not an exact historical checkout.

On candidate `32b99f385716ae427895ce109fe32e3a325eaa6a`, an Align-owned copy of
the confirmed commit passes both check modes with its `run_case` comparison.
Replacing only `src/native_cuda_mtp_probe.align` with the retained failing source
reproduces the expired `expected` diagnostics in both modes. Whole check takes
6.955 s / 120,208 KiB for control and 10.512 s / 126,480 KiB for the witness;
per-unit takes 14.465 s / 254,464 KiB and 31.643 s / 221,216 KiB respectively.
These are sampled bounded observations, not performance promises. No cap fired.
Compiler/source/log hashes and every base source hash are retained privately.

Synthetic isolation removes GPU/native dependencies entirely. An array created
inside an outer loop is incorrectly expired when borrowed inside two further
nested loops. One/two loop levels pass, three fail in both frontends. No conditional
owner construction, optional payload, fallible producer, mutable session or fixed width table
is required. Moving the array outside all loops passes; scalar and owned-string
controls also pass. Simplifying the real baseline and replay bodies independently
and together retains the refusal. This is a separate reproducible loop-generation
precision boundary, not proof of the earlier Option-assignment root cause.

```align
fn read(borrow expected: array<i64>) { print(expected.len()) }
fn main() {
  mut i := 0
  loop {
    if i >= 2 { break }
    expected := [1].to_array()
    mut j := 0
    loop {
      if j >= 2 { break }
      mut k := 0
      loop {
        if k >= 2 { break }
        read(expected)
        k = k + 1
      }
      j = j + 1
    }
    read(expected)
    i = i + 1
  }
}
```

Recommended implementation boundary: loop-scope generation advancement and release
ownership in sema, independently replayed checked HIR, without weakening actual
iteration-owned temporary or escaped-view rejection. A closure matrix must cross
one/two/three/deeper loops, owner declaration depth, normal/back/break/return/`?`
edges, owning array versus string/scalar controls, helper extraction, imported
whole/per-unit paths, and real local/temporary expiration negatives. First prove
which scope owns each release before adjusting `invalidate_iteration_drops`,
iteration release handling or loop-head joins. R37 and both R80 reductions remain
unfixed; their selected evidence/reproduction investigations are complete.

## Historical non-deferred reconciliation (2026-10-21)

The owner's follow-up selects every remaining request except deferred work.
All three registers were reread. Plans231–236 below are already merged;
their former provider-deferred assessments are superseded by those deliveries,
not reasons to reimplement them. Upstream consumer qualification remains external,
including voice V006's separate adoption receipt for an unpublished candidate.
LLM R157 is delivered by [Plan229](229-out-slice-read-certification.md)/#1394,
media AM-R21 by [Plan205](205-indexed-array-field-copy-reads.md)/#1395, and
LLM R158 by [Plan237](237-json-doc-encoding.md)/#1404. Provider receipts belong
in the affected request registers; consumer code and pins are outside this pass.

R86's retained optional-record witness now prints the complete digest on main.
[Plan173](173-deferred-aggregate-transfers.md)/#1320 already defers structural
source transfers until the outer action completes. The exact JSON and completed-
move negative controls extend its existing lifecycle/replay owner; accepting the
fused constructor is no longer proof of the former empty-digest bug. This is
provider regression evidence, not the consumer's task-attempt qualification.

The K1/view and exclusive-owner boundaries below remain unselected. The R37,
R49 and R80 source-packet investigations remain on hold for evidence, and R131
has no reproducible current defect in its retained constructors. No unavailable
witness, retired consumer, platform-only acceptance or unselected policy trial
is converted into a completed repair. Other already adopted/qualified register
entries retain their existing receipts and limits.

## Later capability queue

| Requests | Value and investigation output |
| --- | --- |
| Voice V006 | Provider implementation: [Plan231](231-fixed-scalar-heap-fields.md) qualifies primitive Copy fixed fields through heap construction, growth, transfer, Drop and exact zero-size storage. `shape := values[i].shape` uses the existing complete-field Copy read and replaces scalar expansion/reconstruction. Independent design review accepted Category A composition under the original view-free relocation rationale. Consumer adoption remains pending. |
| Media AM-R16 | [Plan232](232-indexed-fixed-field-values.md) implements direct independent fixed-field indexing, including chained record fields, through the existing complete-field Copy snapshot. Outer/inner bounds and effects remain ordered; inner evaluation may retire the parent after the snapshot. Retained slices remain separate: the current helper-return witness loses read-only authority in both frontends, so that capability must close interprocedural authority and lifetime together. Plan231's shared-match fixed-field storage exclusion remains unchanged. Consumer adoption is pending. |
| Media AM-R17 | [Plan233](233-c-record-fixed-arrays.md) implements positive fixed arrays of numeric C scalars in C records: layout, Copy/raw storage and both supported native by-value ABI families. Whole/per-unit native C, physical ABI, malformed-domain and cache-change owners close the boundary. Zero-length and non-numeric C arrays, nested records and naked extern arrays remain excluded. Consumer adoption is pending. |
| Media AM-R12 and AM-R19 | [Plan234](234-cancellation-sensitive-math.md) implements `expm1`/`log1p` on scalar and explicit vector f32/f64 through width-exact native math, with signed-zero/domain semantics, independent sample goldens and honest scalar-fallback visibility. Standard `log1p` replaces the unshipped reservation; no entire E2 tier or SIMD-provider prerequisite is imposed. Consumer adoption remains pending. |
| Media AM-R14 and AM-R20 | [Plan235](235-trigonometric-math.md) implements sin/cos/atan2 on scalar and explicit vector f32/f64 through exact LLVM intrinsics. Signed axes, quadrants, nonfinite cases, native helper compatibility and vector disposition have independent owners. Three-target numerical qualification establishes concrete samples, not a cross-target bitwise contract. Consumer rotary/guide adoption remains pending. |
| Media AM-R15 and LLM R34 | [Plan236](236-fallible-raw-carriers.md) implements unsafe `raw.try_alloc(i64) -> Result<raw, Error>` and non-owning raw transport through Option/Result, sums, flat tuples and ordinary fields. Invalid/zero/refused/success outcomes, allocation provenance, explicit free or resource transfer, native ABI and partial resource cleanup have deterministic owners. Raw remains Copy/Static; no automatic pointee cleanup or new access authority is inferred. Consumer adoption remains pending; retired ggml examples are not revived. |
| Media AM-R07 | External writable read destinations remain separate. K1 plus an explicit exclusive-view formation, initialization, bounds/overlap, native lifetime and synchronous I/O contract are prerequisites. Fallible allocation does not create these permissions or establish a pinned-memory performance claim. |
| LLM R28/R132/R137/R142 and related media view composition | Reconcile the current remaining portions against shipped builder Copy reads, owning-field extraction and indexed independent Copy observations. Use [Plan61](61-interprocedural-view-access-plan.md), [Plan196](196-indexed-array-field-length.md) and [Plan210](210-text-builder-byte-views.md) to identify the exact missing interprocedural read-only/writable authority and retained-view proof. Do not reopen already delivered portions or widen a readable header into writable/owned backing. |

## Historical K1 scope and difficulty assessment (2026-10-20)

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

The 2026-10-19 LLM reconciliation names the following residuals. Its exact
`request-adoption-20261019` source/log packet was unavailable during the
2026-10-20 audit and is now retained in the private transfer packet above.
Reproduce those authenticated sources on current main; do not attribute them
to the historical request's original cause or claim a repair from an older
provider PR. The investigation/acceptance boundaries below remain applicable.

| Residual | First bounded investigation and acceptance |
| --- | --- |
| R37 large-function build memory | Obtain `reconcile-builder-before.align` and `reconcile-builder-build-memory.json` with the exact imported-source revision. The reported Linux VmHWM is 83,309,188 kB, about 79.5 GiB. Reproduce under an explicit memory cap after a headroom check; separate checking, MIR certification and emission, then reduce the responsible phase. Retain source/toolchain/phase and peak-memory receipts. Compare the final shared-span helper control; do not restore the retired fourteen-function owner or infer a complexity bound from one sample. |
| R49 mutable-helper validated-text refusal | Obtain `reconcile-builder-mutable-helper.align`, `reconcile-mutable-helper-check.log` and the passing borrow/builder probes. Check an owned source copy with the original imports, reduce the exact invalidation of `name` after mutation of distinct destinations, and classify it under the current storage/validation summaries. A repair must admit the safe caller and retain actual validated-byte mutation negatives in whole/per-unit modes. |
| R80 FILE_SET branch replacement | Obtain `reconcile-composition-source-trust-branch-failing.align`, both check logs and the failed-command receipt. In an owned copy of the exact client revision compare mutable branch assignment with the qualified immutable conditional construction. Reduce the erroneous loop-iteration expiry in the loop-free function before choosing an ownership fix. Retain real expiry, alias and branch-join negatives; consumer source-trust execution remains external acceptance. |

These are pending investigations, not completed fixes or consumer-verification
claims. Source copies used for compiler reproduction belong to Align-owned
scratch; no consumer code, pin, build machinery or Git state is modified.
