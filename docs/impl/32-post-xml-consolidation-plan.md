# Post-XML consolidation plan

> **Status:** SCHEDULED NEXT, conditional on the `std.xml` PR completing and
> its implementation owner stopping. If the owner proceeds with the intended
> release, qualification starts after that release completes.
>
> **First executable task:** V0 in
> [`33-consolidation-baseline-packet.md`](33-consolidation-baseline-packet.md).
> It uses shipped commands and existing fixtures. No compiler refactor or new
> benchmark harness is authorized by a position in this plan.

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
| CLI and reporting | Invoke existing commands; no `explain-opt` extension, JSON output, or new flag | `09-explain-opt.md`; S0B still needs its exact reviewed extension |
| Runtime, FFI, and native input | No new symbol, ABI, text/view boundary, process lifecycle, or global state | `20-runtime-abi-ledger.md`; new safety strategies require their own reviewed closure matrix |
| Build, cache, artifacts, distribution | No new input, cache format, profile, sidecar, release variant, or target | Existing build/cache/distribution plans; bind actual artifact identities |
| Evidence | Private Markdown notes and raw outputs from existing owners; no persisted interchange schema or automatic reader | Baseline packet; unavailable observations are explicit, not zero |
| Performance | No global speed, allocation, size, or compilation-time guarantee | A selected optimization preregisters its own benefit and guard thresholds under plan 31 |
| External consumer | Read provided align-llm evidence; no consumer code, pin, branch, or adoption work | Repository external-consumer boundary; consumer adoption is not a phase exit gate |

There is no new encoding, scalar-width, wire-order, or semantic-to-byte golden
contract to specify here. If an implementation introduces one, it belongs in
the narrow slice ledger before code. `draft.md`, `language-spec.md`, settled
decisions, and bilingual library designs do not change for this scheduling work.

## 4. Sequence and capability boundaries

| Step | Work and concrete output | Entry | Completion |
|---|---|---|---|
| V0 — baseline qualification | Bound release/source identities, existing corpus results, evidence gaps, and first AI pilot disposition using packet 33 | XML/release transition above | Each initial row has a supported result or an explicit blocker; no unmeasured cost is called a regression |
| S0A — startup observation | Parent-observed launch-to-reap benchmark, with untimed size/dependency companions | V0 identifies current artifacts; complete plan 31's exact fixture, measurement, and lifecycle ledger and its required design review | Plan 31 S0A acceptance; no codegen change or probe in timed children |
| S0B — current-decision observation | Exact records from existing selector owners through `explain-opt` | V0 names the decisions needed for the first consolidation; complete the exact `09-explain-opt.md` extension and closure matrix | Plan 31 S0B acceptance, including generated-code identity and located-mode isolation |
| C0 — first consolidation | One useful decision boundary with implementation, removed duplication, and owner coverage | Baseline/observations relevant to that boundary exist; fill the matrix below | One authority per decision, necessary fallback/validation preserved, selected correctness and resource guards closed |
| O0 — optional optimization | At most one admitted S1, S3, or S2 capability before reassessment | A named residual cost survives C0 and meets plan 31's admission gate | Adopt against preregistered evidence or record a measured deferral; an unprofitable candidate does not force a replacement project |
| V1 — reassessment and phase close | Reuse the same corpus and AI task set; record changed outcomes and dispositions | C0 complete; O0 complete or explicitly not needed | Exit criteria in §9 |

Default single-worker order is V0 -> S0A -> S0B -> C0 -> optional O0 -> V1.
S0A and S0B are independently useful and may proceed independently once their
own design gates close. S0A is not a prerequisite for reporting design. A
semantics-preserving deletion with sufficient existing evidence need not wait
for unrelated observation work. Do not bundle either observation capability
with the refactor or optimization it will later measure.

Only V0 is ready to execute directly from this preparation. S0A/B implementation
is not approved by this umbrella: their exact new harness/output contracts
remain the next bounded design work, informed by V0 rather than guessed now.
V0 can finish with a concrete missing-instrument finding; it need not implement
the instrument to qualify that finding. Conversely, C0 cannot claim a resource
improvement whose required observation is still unavailable.

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
