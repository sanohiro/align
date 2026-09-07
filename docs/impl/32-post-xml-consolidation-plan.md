# Post-XML consolidation plan

> **Status:** ACTIVE. V0, S0A (PR #955), S0B (PR #959), C0 (PR #964), and the
> V1 corpus/AI replay are complete. V1 selects one bounded O0 implementation in
> §11; the phase closes after its evidence disposition.
>
> **First executable task:** implement §11's direct explicit-parallel virtual
> chunks source, run its preregistered candidate/control probe, and adopt it or
> revert it from the measured result.

## 1. Purpose and authority

The owner selected this pause on 2026-09-05. Finish `std.xml`, allow its owner
to complete the intended release, then qualify and consolidate the capabilities
already shipped before adding another library. `std.time` named formatters,
cloud packages, and additional transports remain deferred during this phase.

`HANDOFF.md` owns the live task. This document owns the phase sequence and exit
conditions. The baseline packet owns the initial corpus and execution procedure.
[`31-execution-storage-startup-plan.md`](31-execution-storage-startup-plan.md)
continues to own optimization admission, semantic boundaries, and S0–S7.
The narrow HIR, runtime ABI, cache, parallel, and build plans retain their
contracts. A scheduling decision does not reopen those contracts.

The objective is to give each semantic decision one authoritative producer,
remove demonstrated unnecessary paths or dependencies, and prove that ordinary
programs preserve their behavior and resource properties. Deleted lines, new
tests, and completed S numbers are not success metrics.

## 2. Release and concurrent-work boundary

The XML owner retains its implementation, review, tests, PR, and release work.
Preparation here changes English internal planning documents only. It does not
change the XML design or Japanese mirror, compiler/runtime/package sources,
fixtures, Cargo versions, release notes, tags, or distribution workflows.
Use a separate worktree and branch; do not switch or reset the XML checkout.

The transition is:

```text
std.xml PR merged and required verification complete
  -> XML owner stops adding capabilities
  -> if releasing: release commit/tag and intended artifacts complete
  -> bind baseline identity and start V0
```

The user's anticipated release is not permission for a preparation agent to
publish one. Do not select a version, bump Cargo files, tag, or wait for an
invented version. Record the actual release chosen by its owner. A failed or
unfinished intended release leaves release-artifact qualification pending;
preparation may continue. If the owner explicitly elects not to release, use
the completed XML merge revision as a source baseline and record that no
release artifact is being qualified. Elapsed time never chooses that branch.

Resolve shared-document merge conflicts by preserving the XML owner's factual
implementation/release status and this post-XML scheduling decision together.
Do not restore an old `DESIGNED` status or the old automatic next-library queue.
Planning-only commits after the release do not rename its baseline.

Bind release assets to their actual target and digest; source-built compiler
and runtime artifacts form a separate lane. A local `--release` build is not
equivalent evidence for a distributed dist/PGO build. A corrective compiler or
runtime commit starts a new candidate identity; retain the old baseline and
rerun only the affected qualification. Never edit recorded baseline results.

## 3. Contract boundary ledger

This is an internal work plan, not a new public language contract.

| Surface | Decision | Authority and acceptance |
|---|---|---|
| Language and library API | No new type, signature, default, syntax, or removed API | Existing specification and English library ledgers; source oracles remain unchanged |
| Effects, errors, ownership, lifetime, allocation | Existing semantics, source-visible boundaries, and Drop rules remain authoritative | HIR/MIR and native-boundary owners; compare defined behavior, not all internal allocation counts |
| CLI and reporting | Invoke existing commands; S0B supplies the current-plan surface and O0 adds one exact selected chunks strategy tuple, with no JSON output or new flag | Exact tables in `09-explain-opt.md` |
| Runtime, FFI, and native input | No new symbol, signature, text/view boundary, process lifecycle, or global state. O0 revises only A39/A46 so `in_buf` may name a compiler-certified immutable source whose kernel-owned extent differs from logical `count * in_stride`; A89 and ordinary sources retain the physical-span rule | `20-runtime-abi-ledger.md`, parallel plan §7.6, and the reviewed §11 matrix |
| Build, cache, artifacts, distribution | No new input, cache format, profile, sidecar, release variant, or target | Existing build/cache/distribution plans; bind actual artifact identities |
| Evidence | Private Markdown notes and raw outputs from existing owners; no persisted interchange schema or automatic reader | Baseline packet; unavailable observations are explicit, not zero |
| Performance | No global speed, allocation, size, or compilation-time guarantee; O0 has only the local adopt-or-revert threshold in §11.4 | A selected optimization preregisters its own benefit and guard thresholds under plan 31 |
| External consumer | Read provided align-llm evidence; no consumer code, pin, branch, or adoption work | Repository external-consumer boundary; consumer adoption is not a phase exit gate |

There is no new encoding, scalar-width, wire-order, or semantic-to-byte golden
contract to specify here. If an implementation introduces one, it belongs in
the narrow slice ledger before code. `draft.md`, `language-spec.md`, settled
decisions, and bilingual library designs do not change for this scheduling work.

## 4. Sequence and capability boundaries

| Step | Work and concrete output | Entry | Completion |
|---|---|---|---|
| V0 — baseline qualification | Bound release/source identities, existing corpus results, evidence gaps, and first AI pilot disposition using packet 33 | XML/release transition above | Each initial row has a supported result or an explicit blocker; no unmeasured cost is called a regression |
| S0A — startup observation | Parent-observed launch-to-reap benchmark, with untimed size/dependency companions | Implemented in PR #955 against plan 35's accepted exact fixture, measurement, and lifecycle ledger | Plan 31/35 S0A acceptance; no codegen change or probe in timed children |
| S0B — current-decision observation | Exact records from existing selector owners through `explain-opt` | V0 names the decisions needed for the first consolidation; complete the exact `09-explain-opt.md` extension and closure matrix | Plan 31 S0B acceptance, including generated-code identity and located-mode isolation |
| C0 — first consolidation | One useful decision boundary with implementation, removed duplication, and owner coverage | Baseline/observations relevant to that boundary exist; fill the matrix below | One authority per decision, necessary fallback/validation preserved, selected correctness and resource guards closed |
| O0 — optional optimization | At most one admitted S1, S3, or S2 capability before reassessment | A named residual cost survives C0 and meets plan 31's admission gate | Adopt against preregistered evidence or record a measured deferral; an unprofitable candidate does not force a replacement project |
| V1 — reassessment and phase close | Reuse the same corpus and AI task set, select or defer O0, then record its final disposition | C0 complete | O0 complete or explicitly deferred and exit criteria in §9 satisfied |

Default single-worker order is V0 -> S0A -> S0B -> C0 -> V1 reassessment ->
optional O0 -> V1 close.
S0A and S0B are independently useful and may proceed independently once their
own design gates close. S0A is not a prerequisite for reporting design. A
semantics-preserving deletion with sufficient existing evidence need not wait
for unrelated observation work. Do not bundle either observation capability
with the refactor or optimization it will later measure.

S0A is implemented within plan 35's exact harness/output contract and closure
matrix. S0B implementation is approved only within its accepted exact
`09-explain-opt.md` extension. C0 cannot claim a resource improvement whose
required observation is still unavailable.

If V0 finds no justified consolidation or optimization boundary, record that
result and close through V1's disposition review without inventing a refactor
or implementing observations with no current consumer. Existing correctness
defects may be repaired through their normal owner flow immediately; they need
not wait for S0. Preserve the failing baseline and bind the repair as a candidate.

## 5. Selecting the first consolidation

Inspect the routes exercised by the initial corpus, not the entire repository
in an open-ended cleanup audit. Name each candidate by the decision it owns.
Record the producer, all known consumers, the repeated rule or residual cost,
the discriminating existing owner, and the paths proposed for deletion.

Priority is:

1. Conflicting semantic rules or a correctness defect affecting the corpus.
2. Repeated authoritative reasoning that causes repair omissions or measurable
   compiler work. Consumers should use the same producer's fact.
3. Proven unused paths, abstractions, code, or native dependencies.

A shared producer does not remove validation at trust boundaries. Whole-program
and per-unit compilation may need distinct consumers; serial and explicit
parallel operations may need distinct algorithms. A conservative fallback,
target implementation, malformed-IR rejection, or independently necessary
runtime guard is not legacy merely because it resembles another path.

The conceptual source -> HIR -> semantic MIR -> decision -> lowering structure
does not authorize a universal planner pass/crate or a summary with dormant
fields. Move producer and consumer together only when that closes a useful
capability. Prefer the smallest abstraction that removes the repeated proof.

## 6. Implementation closure and semantic checks

Before C0 code, replace the applicable cells below with exact implementation
locations and tests in its narrow owning plan. This is a selection matrix, not
a claim that every cell already has complete coverage. Existing tests can close
many cells when they would fail for the changed defect. Run tests by impact.

| Axis | Required closure | Starting owners, not an automatic suite list |
|---|---|---|
| Formation, validation, unknown variants | Checked facts have one authority; unknown fails closed; validators still reject malformed input | `19-hir-validation-ledger.md`, affected sema/MIR owners |
| Construction, move-in/out, nulling, replacement, return, Drop | Preserve runtime provenance and exactly-once cleanup, including borrowed and owned results | `borrowed_params`, `resource_ownership`, `value_control_flow` |
| `if`, `match`, `else`, `?`, `map_err`, branch/loop joins and early exits | Preserve defined evaluation/error precedence, selected-owner state, and cleanup | Affected existing control-flow and operation owners; explicitly locate any missing case |
| Generics, imports, function values and serialization | Producer facts survive real supported routes; no second semantic summary | `core_codec`, `pkg_frame`, `pkg_csv`, interface and cache owners |
| Whole-program, per-unit, caches, target/profile | Same defined behavior; cache identity reflects actual changed inputs; located reports stay ephemeral | `unit_cache`, `cache_codegen`, `function_thin_lto`, affected located-report owner |
| Runtime/FFI and ownership provenance | Exact registry, argument validation, allocator/global-state ownership, cleanup and failure order | `20-runtime-abi-ledger.md`, operation's native owners |
| Fusion, donation, chunks and parallelism | Preserve legality, rejection/fallback, explicit parallelism and stable results | `deep_pipeline`, `buffer_donate`, `chunks`, `par_map`, `task_group` |
| Allocation and resource behavior | Ownership balance and source-visible allocation rules hold; internal allocation/copy reductions remain allowed | Changed operation's allocation owner plus a benchmark only for an explicit resource claim |

Optimization-on/off differential testing supplements specification-derived
oracles; shared bugs can survive both arms. Use the existing owner-controlled
toggle where one exists, with explicit cache isolation. Do not add a public O0
mode, general optimization-disable switch, or a second evaluator for this phase.

Compare only specified observables. Preserve specified floating-point and
parallel result semantics without requiring unspecified worker schedules to
match. Do not require internal allocation counts to equal when eliminating an
allocation is the intended change. Physical bytes moved, estimated byte work,
allocation count, allocation balance, and elapsed time are distinct measures.

If the matrix changes a public or safety strategy, complete the fresh independent
design review before implementation. Otherwise use the existing reviewed
strategy and author-side matrix pass, then the normal one-review/one-fix code
flow. No compiler builds or tests run inside review automation.

## 7. Optional optimization admission

S1 is the first default generated-code candidate only if a named nonescaping
consumer still materializes chunk headers. S3 takes priority if current closure
evidence instead identifies deployment, unnecessary dependency, or startup cost.
S2 follows only for a measured eligible fixed-scratch allocation cluster.
S1, S2, and S3 are not prerequisites of one another.

S3 must inspect the emitted link request as well as final image dependencies;
historical binary-size tables are not measurements of the new baseline. First
try the narrowest partition that can remove the observed closure. Keep one ABI
registry, allocator owner, and process-global state owner.

S4 stays deferred until an actual mixed CPU/blocking workload identifies a
problem in the current policy. S5 belongs to its first repeated-parse consumer.
S6 requires an observed selector decision that static facts cannot make well.
S7+ remains individually deferred. No source tuning annotation, automatic
parallelism, or implicit ownership/layout change is introduced.

Before O0, record the workload, primary metric, minimum useful improvement,
guard metrics and maximum tolerances, identical-code/noise control, sample
schedule/statistic, target/toolchain/link/profile identity, and fallback.
Do not invent a universal 3% rule or retrospectively choose the winning metric.
Ordinary CI owns structural/correctness invariants; timing remains local evidence.

## 8. AI authorability and public surface

Packet 33 defines a small fixed pilot at V0 and a same-task repeat at V1.
Task/oracle/model inputs stay fixed; baseline and candidate compiler/runtime
identities are recorded as separate execution arms. Replay saved source on
both arms to distinguish compiler changes from fresh model variation.
Correctness rate and performance among correct solutions are separate. An
unavailable model/provider is an explicit missing lane, not a reason to block
compiler qualification or to buy/connect an unrequested service.

Use failures to distinguish documentation/diagnostics, model misunderstanding,
implementation gaps, missed optimization, and deliberate language restrictions.
Inspect runtime performance only with runtime-input workloads and matching
semantics; small correctness fixtures do not establish vectorization rates or
Rust/C speed ratios.

Do not remove explicit allocation, ownership, errors, or parallel intent to
improve an AI score. Reconsidering a settled restriction still follows
`23-friction-ledger.md`: five mechanical workarounds across two independent
real programs permit review, not automatic widening. Generated pilot exercises
alone are not counted as independent real consumers. Public API removals
require their own contract review and actual consumer evidence.

## 9. Exit and return to library work

Close this wave when:

- V0/V1 identities, corpus, raw results, and comparison limits are preserved;
- the chosen C0 boundary has one authoritative decision source, required
  validators/fallbacks, and its applicable closure cells are tested, or V0
  records why no boundary merits a change;
- no new correctness defect remains in any changed boundary; existing unrelated
  failures have named owners rather than silently becoming new exit gates;
- selected resource guards have passed or the proposed change was not adopted;
- O0 is adopted or explicitly deferred with a reason; S1–S6 completion is not
  required;
- AI pilot changes or its unavailable-lane disposition are recorded; and
- remaining candidates each name an owner and an evidence-based next action.

Write one capability-level result and the next selected task to `HANDOFF.md`.
Retain detailed measurements and finding history outside the live handoff.
Do not keep the phase open for a zero-warning corpus, zero duplicate code,
100% AI score, architecture freeze, or unrelated consumer adoption.

At exit, select the next concrete consumer. `std.time` named formatters remain
the first paused library candidate if cloud work is selected; they do not
automatically start because this phase ended. A new request can change that
selection explicitly. Internal architecture remains changeable under the same
semantic and evidence rules.

## 10. C0 — chunks-representation decision consolidation

### 10.1 Evidence and exact boundary

V0 selected chunks representation as the conditional C0 candidate. S0B now
reports all five shipped outcomes from the selector owner. The fixed examples
confirm both materializing corpus routes: `examples/chunks.align` reports
`materialized-headers / stored-or-boundary`, while
`examples/chunk_parallel.align` reports
`materialized-headers / parallel-consumer`. The complete S0B owner additionally
pins `virtual-count / direct-len`, `virtual-index / direct-index`, and
`materialized-headers / pipeline-consumer`, with the selected representation
matched against the generated MIR.

The implementation audit found one bounded repeated-rule class in
`align_mir`: direct length, direct index, and ordinary materialization construct
their own decision tuples, while pipeline and parallel consumers first publish
the ordinary materialization tuple and then mutate its reason through
`mark_chunks_consumer` and `PlanCollector::replace_chunks_reason`. A sequential
`par_map` fallback may pass through two such rewrites. That mutable reporting
repair is a second account of a representation decision already implied by the
consumer lowering path and can drift from the MIR it describes.

C0 introduces one private exhaustive `ChunksConsumer` classification with
exactly `DirectLen`, `DirectIndex`, `Pipeline`, `Parallel`, and
`StoredOrBoundary`, and one private selector that maps it to the existing
`PlanDecision`. Each existing lowering consumer supplies its classification
before lowering the chunks expression. The selected tuple is consumed both by
the existing virtual/materialized lowering path and by located reporting.
Materialized source lowering receives its final classification up front and
records exactly once. Delete `mark_chunks_consumer`,
`replace_chunks_reason`, and the default-then-rewrite path.

This is consolidation only. It does not admit another nonescaping consumer,
virtualize a currently materialized header array, add a HIR/MIR variant, alter
evaluation order, change ownership or cleanup, or move a decision across a
module/interface boundary. S1 remains measure-first and unscheduled. There is
no public-contract or safety-strategy change, so this exact matrix is folded
into the normal single implementation review rather than receiving a separate
design-review loop.

### 10.2 Implementation closure matrix

| Axis | Exact C0 closure | Owner evidence |
|---|---|---|
| Formation and validation | `ChunksConsumer` is private and exhaustive; its selector is the only state/strategy/reason mapping; existing plan-tuple validation remains fail-closed | Unit table over all five classifications; existing current-plan enum/selector tripwire and malformed-record matrix |
| Construction and consumption | Direct length/index and materialized boundary/pipeline/parallel paths obtain one decision before emission; located mode records that same decision once; unlocated mode allocates no report table | `current_plan_chunks_rows_follow_the_consumed_representation`; normal/located plan-storage owner |
| Move-in/out, source nulling, replacement, return, and Drop | Existing `lower_chunks_source`, synthetic-owner creation, borrow-owner inheritance, temporary release, and stored/returned materialized ownership remain unchanged; C0 removes only plan-record replacement | `direct_chunks_consumers_are_semantically_equivalent`, owned-source chunks, borrowed-liveness replacement/loop owners, resource ownership owners |
| Evaluation and control exits | Source, width, then index order remains exact; nonpositive width preserves the canonical empty result and direct-index bounds failure; termination before a selector publishes no row, while a reached chunks decision survives a later terminating pipeline/parallel capture | `index_receivers_evaluate_before_indices`, `direct_chunks_zero_size_index_aborts`, `current_plan_emits_no_row_before_a_selector_decision_is_reached`, `current_plan_capture_termination_preserves_only_reached_decisions` |
| Pipelines and explicit parallelism | Synchronous pipeline, direct range reduction, range materialization, and sequential `par_map` fallback all classify the chunks source before lowering without changing their current algorithms or par-map decision | Current-plan five-row fixture, `chunks`, `par_map`, and `direct_chunks_consumers_are_semantically_equivalent` owners |
| Generics, imports, whole-program, per-unit, and checked-HIR replay | The private classification is reconstructed from each concrete checked HIR body; no interface field is added; equal visible routes retain identical tuples and imported source unavailability | Current-plan generic/import fixture, whole/per-unit parity, replay/catalog owners, `per_unit_surface` |
| MIR, LLVM, cache, and artifact identity | Ordinary MIR statements, runtime keys, implementation hashes, LLVM IR, objects, and cache eligibility remain unchanged; located records stay ephemeral | Current-plan normal-versus-located MIR comparison, existing side-table implementation-hash/LLVM/object identity owner, cache owner |
| Runtime ABI and allocation | Materialized paths retain `Rvalue::Chunks` and the existing runtime materializer; virtual paths retain no header allocation; no runtime symbol or ABI row changes | LLVM direct-versus-stored structural owner, runtime ABI inventory equality, allocation balance owner |

### 10.3 Acceptance and continuation

C0 is complete when the code contains one exhaustive selector and no post-hoc
chunks plan mutation, every matrix owner above passes, the normal code-tier gate
and independent review close, and the existing examples produce the same S0B
rows. Because C0 makes no performance or resource-improvement promise, it adds
no benchmark threshold. After merge, perform V1 against the preserved V0 corpus,
decide whether any residual cost admits one O0 candidate, and either execute
that one bounded candidate or record its measured deferral before closing this
phase.

## 11. O0 — virtual chunks for direct explicit parallelism

### 11.1 V1 evidence and exact boundary

The private V1 record is `/home/hiro/prj/align-v1-20260907/V1.md`. The candidate
at C0 merge `6a645c413b6dabdd04682620de3db2d6da43ed97` replayed R0–R5 without a
specified-output change. Every saved V0 AI answer and every fresh V1 answer had
the same compile/oracle outcome under the v0.7.2 and C0 compilers; the fresh
1/6 versus 5/6 authoring difference therefore remains unpinnable model
variation rather than a compiler claim.

The fixed `examples/chunk_parallel.align` route still reports
`materialized-headers / parallel-consumer` immediately before the selected
`range-reduce` decision. It is a named direct, synchronous, nonescaping
consumer. The saved runtime probe independently shows that allocating and
writing the header array costs work, but cannot justify a source lowering on
its own. This admits exactly one O0 candidate under plan 31: virtualize an
immediate, stage-free `ArrayChunks` source only when the existing `par_map`
form selector chooses `range-materialize` or the existing direct integer-sum
specialization chooses `range-reduce`.

An immediate `par_map` with prior stages, a rejected form using the sequential
collector, and every synchronous non-parallel pipeline remain materialized.
So do chunks values bound to a name, returned, passed, captured, stored in an
aggregate, or carried through control flow. O0 adds no source syntax or type,
no implicit parallelism, and no new runtime call, symbol, signature, or
declaration row. It does revise the compiler-private safety interpretation of
the existing A39/A46 `in_buf` and `in_stride` arguments as specified below; A89
is outside this slice. It does not admit the remaining S1 consumers or any
S2–S7 capability.

### 11.2 Exact MIR and LLVM boundary

`align_mir` replaces the parallel rvalues' unclassified `src: Operand` with one
exhaustive internal `ParallelSource`:

```text
Materialized(view)
VirtualChunks { base, width, elem }
```

`Materialized` retains every current source and lowering. `VirtualChunks`
contains the already-lowered borrowed base slice, the evaluated `i64` chunk
width, and its primitive element type. It is legal only for a stage-free direct
chunks source whose logical parallel input is `slice<elem>`. There is no caller-
supplied chunk count or byte offset for malformed MIR to forge. Both
`ParMapParallel` and `ParMapReduce` consume this same source classification;
there is no dormant producer and no separate virtual collection value.

Lowering evaluates the underlying chunks source, then width, then stage and
terminal captures in the existing order. After source and width succeed it
records `selected / virtual-range-views / parallel-consumer`; a later
terminating capture preserves that reached chunks row while suppressing the
unreached `par_map` row. The base view inherits the same hidden owner used by
direct virtual length/index. Every edge after that owner becomes live either
reaches the synchronous runtime call or transfers it to the existing
error/return cleanup path. The call consumes all borrowed views before
returning, after which normal cleanup releases a fresh hidden base owner
exactly once. A terminating callable or terminal capture releases that owner
once without publishing a call or output. Bound, arena, and static bases remain
borrowed and are not freed.

Codegen validates the source variant before generated-kernel lookup. For
`VirtualChunks`, `base` must be `slice<elem>`, `width` must be `i64`, logical
`elem_in` must be `slice<elem>`, and `stages` must be empty. Any mismatch is a
malformed-MIR error before LLVM publishes an object. Generated-kernel identity
continues to use the canonical physical source and logical terminal types: the
`slice<elem>` physical base plus `slice<elem>` terminal input distinguishes the
virtual kernel from both an ordinary scalar-slice source and the existing
`array<slice<elem>>` materialized source. No generated-id format field or
interface field is added.

The caller extracts the base pointer and element count, then derives the chunk
count with the runtime materializer's exact precedence: null base, nonpositive
element count, or nonpositive width selects zero before division; otherwise
`(element_count - 1) / width + 1`. It passes that count and the logical
16-byte slice-header stride to the signature-unchanged `align_rt_par_map` or
`align_rt_par_map_reduce` ABI, preserving the materialized control's scheduling
input and byte/work floor.

For A39/A46 only, O0 makes `in_buf` a compiler-certified immutable source
identity passed through unchanged to the generated kernel. It need not cover
`count * in_stride` bytes. It must remain valid for every source byte the
certified kernel can derive until the synchronous call joins. `in_stride`
remains the logical scheduling width; O0 supplies the positive 16-byte slice-
header width, and `count * in_stride` must be representable in `isize`. The
runtime never dereferences `in_buf`; its existing multiplication becomes a
logical-work-span validation rather than a physical-extent proof. The generated
kernel may read only the base element ranges derived from its assigned logical
chunk indices after checking them against the context's `element_count`, and
retains nothing. Output-range and context validity rules remain unchanged.
Ordinary A39/A46 sources still
satisfy the stronger physical-span form, and A89 retains it as a requirement.
The implementation updates the Rust safety comments, the parallel plan, and
the runtime ABI ledger together; there is no unrecorded unsafe-call exception.

The synchronous call-scoped context prefixes the existing Copy captures with
`width` and `element_count`. A generated kernel derives each borrowed slice in
SSA from its assigned chunk index. It checks multiplication before pointer
formation, proves `start <= element_count`, computes
`remaining = element_count - start`, and selects
`length = min(width, remaining)` without forming `start + width`. It checks the
element-byte offset before a byte GEP, then constructs `{ptr, length}` for the
existing Pure callable. Arithmetic failure reaches the existing generated-code
allocation-size abort boundary; it never wraps into a pointer. Empty and
nonpositive-width inputs schedule no kernel and form no pointer.

The current-plan table in `09-explain-opt.md` gains the exact
`virtual-range-views / parallel-consumer` tuple. The old
`materialized-headers / parallel-consumer` tuple remains valid for all explicit-
parallel fallbacks outside this slice. Located and ordinary lowering consume
the same `ParallelSource`; reporting adds no second legality predicate.

### 11.3 Implementation closure matrix

The implementation exceeds roughly 1,000 changed hand-written lines because
the MIR classification, generated-callable preflight, LLVM range kernel,
ownership owners, and measurement gate form one strict producer-to-consumer
chain. Splitting that chain would leave a dormant safety-sensitive MIR shape
without a useful consumer and would duplicate the source-identity, ownership,
and unchanged-ABI proof across capability boundaries.

| Axis | Exact O0 closure | Owner evidence |
|---|---|---|
| Formation, validation, unknown variants | One exhaustive `ParallelSource` is visited by embedded-type collection, tagged-type remapping, operand/value sweeps, canonical hashing, generated-callable preflight, and LLVM lowering. Virtual source/type/stage mismatches fail before kernel lookup or output. | MIR selector table and variant tripwire; malformed virtual base, width, element, logical input, stage, result, callable, and capture matrix; existing malformed materialized nodes remain rejected. |
| Construction and move-in/out | Only a syntactically immediate, stage-free chunks expression reaches `VirtualChunks`; its base/width are moved into the two existing parallel rvalues without constructing an `array<slice<T>>`. No source-level chunks value exists to move, return, or store. | Range-materialize and range-reduce MIR positives; bound/source-stage/rejected-form negatives retaining `Rvalue::Chunks`. |
| Source nulling, replacement, return, and Drop | The virtual source inherits and consumes only the base's hidden temporary owner. A fresh heap base is freed once after the synchronous call or by the existing exit cleanup when a later capture terminates first; named/arena/static bases are never freed there. Materialized header ownership and every escaping fallback are unchanged. | Exact guarded MIR Drop topology for fresh and returned bases; distinct named, arena-owned, and static execution owners; fresh-base width- and later-capture-termination Drop owners for both parallel rvalues; existing chunks replacement, return-provenance, and borrowed-liveness owners. |
| Evaluation and control exits | Source, width, prior-stage operands, and terminal captures retain order. Source/width termination emits no chunks row; a later capture termination retains only the reached chunks row and runs hidden-owner cleanup before propagating. Zero/negative width and empty/null base produce the canonical empty source before division or pointer work. | Direct virtual-source and width termination fixtures for both parallel rvalues, paired fresh-base width/later-capture cleanup topology, nonpositive/empty cases, current-plan reached-row owners, and unchanged fallback diagnostics. |
| Parallel correctness | Caller-only and shared-pool range materialization/reduction derive identical exact-multiple and partial-tail slices, preserve stable output order and wrapping integer reduction, and never retain base/context/views. | `chunk_parallel`, materializing and direct-sum owners at below-floor and pooled sizes, one-worker/multi-worker runs, partial-tail and captured callable fixtures. |
| Generics, imports, whole/per-unit, checked-HIR replay | Concrete local/imported callables reconstruct the private source variant from checked HIR; no interface summary is added. Equal visible routes select the same tuple; unavailable imported source anchors remain unavailable. | Generic imported callable, whole/per-unit current-plan parity, replay/catalog owners, and `per_unit_surface`. |
| MIR identity, cache, and generated functions | The changed semantic MIR changes implementation hashes and object/cache identity normally. Kernel identity distinguishes physical base from logical chunk-view input without changing the canonical generated-id format. Located collection remains identity-neutral. | Canonical MIR/hash delta, cache miss-then-hit, two virtual element types with no kernel collision, materialized/virtual collision negative, normal/located LLVM/object identity. |
| Runtime ABI and safety | Existing par-map symbols and signatures are unchanged, but A39/A46 explicitly accept a compiler-certified opaque immutable source. Runtime validates a logical work span and never dereferences it; generated code proves and derives every source view. A89 and ordinary-source physical-span safety remain unchanged. Checked count/start/byte-offset arithmetic precedes all pointer formation and malformed MIR cannot supply a forged count. | Runtime ABI inventory equality; Rust ABI owner passing a source smaller than logical `count * in_stride` through A39 and A46 to certified range kernels; optimized-LLVM structural owner; maximum count/width/stride malformed matrix; and no-call/no-pointer empty controls. |
| Allocation and fallback | Selected routes contain no `Rvalue::Chunks`, `align_rt_chunks`, header allocation, or header free. Every ineligible route retains all four. Output/partial allocations remain balanced. | LLVM symbol absence/presence, `alloc-count` candidate/control probe, and materialized fallback structural owner. |
| Reporting | Selected virtual rows use the one new valid tuple and exact explanation; staged/rejected forms retain the materialized parallel row. Row order, source anchoring, and no-partial-output validation remain unchanged. | Updated selector table covering both parallel outcomes, default/verbose golden, whole/per-unit and normal/located owners. |

This changes a MIR shape, a generated-kernel safety strategy, and a documented
current-plan tuple. Receive one fresh adversarial design review before code.
Implementation then follows the ordinary one-review/one-fix code cycle; the
design review is not repeated as a patch-discovery loop.

### 11.4 Preregistered evidence and disposition

The baseline compiler/runtime are the C0 artifacts preserved at
`/home/hiro/prj/align-v1-20260907/o0-baseline`, with compiler SHA-256
`926a1f29f5d837c587cef764edfc3632c00e0a79243523b918a43386bf1a457a`
and runtime SHA-256
`44c00aa01a65caaec203e3f33d55c0f57537e8a1be08c094f84734ca47bb7ed0`.
The candidate is the final reviewed O0 commit built by the same Rust 1.96,
LLVM 22.1.8, system linker, release profile, and native target on the V1 host.

Add one manual `bench/par_map` source-consumer mode. It compiles one source
template with distinct equal-length baseline/candidate export prefixes and
co-links both objects into each measurement binary. The source exports both
direct `chunks(width).par_map(chunk_sum)` materialization with a full-output
checksum and direct `.sum()` reduction. Input length is 1,048,579 to cover
partial tails; widths are 1, 8, 64, and 1,024. The driver links the timing
binary against the ordinary production runtime, completes it, then links a
separate resource binary against a freshly built `alloc-count` runtime. Thus
the baseline's extra allocation/free cannot receive instrumentation overhead
in the timing comparison. Each timing arm warms once, then runs 21 paired
samples in alternating AB/BA order. Retain every sample; report median
candidate/base ratio and p10–p90 without post-hoc outlier deletion.

The separate resource process performs no timed sample. Its probe invocations
are serialized: after warmup and with no parallel call active, reset the
requested-live probe and sample counter deltas around exactly one synchronous
exported call. The primary resource gate is exact: for each invocation the
candidate removes one successful allocation and one non-null free, remains
balanced, and reduces peak requested live bytes by exactly
`16 * ceil(input_length / width)` relative to the matching baseline arm.
Results must be byte/value identical for every row. The primary timing gate is
the width-1 reduction median candidate/base ratio at most 0.90. Guard timing
for every other row is at most 1.05, and the candidate object may grow by at
most the larger of 5% or 65,536 bytes. Startup is excluded because both arms
run in one warmed process; R0/S0A remain the startup owners.

Adopt O0 only if the exact resource gate, primary timing gate, all timing/size
guards, semantic owners, and structural no-materializer proof pass. Any miss
reverts the optimization, preserves the raw evidence, records measured
deferral, and closes the phase without choosing a replacement project.

#### Disposition — adopted 2026-09-07

`bench/par_map/run.sh source-consumer` passed on the V1 Linux x86-64 host with
the pinned baseline identities above. The harness printed all 21 sorted ratios
for every row; the summary was:

| Consumer | Width | Median candidate/base | p10–p90 | Limit |
|---|---:|---:|---:|---:|
| materialize | 1 | 0.3527 | 0.2998–0.4095 | 1.05 |
| reduce | 1 | 0.3081 | 0.2839–0.3530 | 0.90 |
| materialize | 8 | 0.7824 | 0.7159–0.8513 | 1.05 |
| reduce | 8 | 0.7695 | 0.6926–0.8900 | 1.05 |
| materialize | 64 | 0.9004 | 0.8157–0.9160 | 1.05 |
| reduce | 64 | 0.8992 | 0.8014–0.9268 | 1.05 |
| materialize | 1,024 | 0.9670 | 0.8787–1.0548 | 1.05 |
| reduce | 1,024 | 0.9852 | 0.9361–1.0969 | 1.05 |

All eight resource rows were value-identical and balanced at zero requested
live bytes. Each candidate removed exactly one allocation and one free. Peak
requested-live reductions were 16,777,264, 2,097,168, 262,160, and 16,400
bytes at widths 1, 8, 64, and 1,024 respectively, exactly
`16 * ceil(1,048,579 / width)`. The candidate object was 3,624 bytes versus
3,176 bytes for the baseline, a 448-byte increase within the 65,536-byte
guard. Timing used the production runtime without `alloc-count`; resource
measurement used a separate instrumented binary. Semantic owners and the
optimized-LLVM no-materializer proof also passed, so O0 is adopted; no
replacement optimization is selected.
