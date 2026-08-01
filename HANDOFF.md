# Session handoff

Current continuity note for a fresh Claude Code or Codex session. Keep this file
about the present state, the next decision, and operational facts. The former
per-PR journal is preserved in
[`docs/archive/HANDOFF-2026-07-25.md`](docs/archive/HANDOFF-2026-07-25.md).

_Last updated: 2026-08-01. `main` includes the shipped wave through #686; the current final
implementation PR carries the am-u lexical extern invocation correction. After that merge, the
next slice is am-p placement validation.
#667 adds the canonical recursive Drop plan and sound `Option<string>` fields;
#668 admits one direct recursively Move payload per tagged arm; #669 admits multiple Move payloads;
#670 completes nested tagged payload representation and the exact pkg.db L1b acceptance shape.
#672 carries L2a parameter modes and explicit empty return-provenance facts through AST, HIR, MIR,
interfaces, caches, and separate compilation without enabling the borrow ABI.
#673 infers named direct/imported parameter-root provenance; #674 refines product projections; #675
closes MIR continuation after every terminating eager child; #677 validates the global HIR type
domain; #678 fixes and closes the am-r public-contract ledger and implementation order.
#653 adds stable compaction for callable primitive-scalar `where` stages before
`par_map`; #654 adds a measure-first task-group record probe without changing
production behavior. The width/stride probe now covers scalar fused and
materializing maps without changing production behavior. #657 adds a runtime-only
aggregate-like stride probe without changing production behavior. #658 widens the
compiler-generated AoS range path to field projection and `where(.field)` stages;
#660 extends the same range path to `chunks` source slices and direct integer
chunk transform-reduce. #662 records a bounded, measure-only probe for the retained
`chunks` header allocation. #664 admits the already-recognised invariant
`str.contains` filter to the stable range path; SoA, richer string-search, and
unsupported layouts remain sequential._

## Start here

1. Read `CLAUDE.md` for repository rules, sources of truth, and the required
   review flow. `AGENTS.md` is the Codex compatibility link to the same file.
2. Read the design or audit directly governing the requested work.
3. Use the archive only when historical implementation detail is material.

Do not rely on Claude's per-machine memory or a previous conversation. Durable
facts must live in this repository.

## Current baseline

- **Release:** v0.4.0 was tagged at `88ee798` and published for the completed
  align-llm R1/R2/R3 batch. `RELEASE_NOTES_0.4.0.md` is the release record.
- **Post-release portability:** v0.4.0's macOS artifact failed to link because
  `clearenv(3)` is unavailable on macOS/BSD. #636 fixed this on `main` with a
  platform-specific portable implementation; macOS CI is green and Linux
  behavior is unchanged. The v0.4.0 tag predates the fix, so the next versioned
  release must be cut from current `main`.
- **Compiler roadmap:** M0-M15, the LLVM 19-to-22 checkpoint, separate
  compilation, the default-on per-unit object cache, parallel codegen,
  ThinLTO, and instrumented PGO are complete. The roadmap retains the
  implementation evidence; it is not the live backlog.
- **pkg.web:** F0-F3 and W1-W7 are complete. The current contract is
  `docs/impl/pkg-design/web.md`; `docs/impl/15-pkg-web-plan.md` is the completed
  execution record. The framework is general-purpose REST infrastructure, not
  an LLM-gateway-specific subset.
- **align-llm requests:** all filed requests are complete and answered in
  `../align-llm/docs/align-requests.md`.

## Latest shipped wave

```text
#630  std.process captured output + cwd
#631  process timeout + Error.Timeout
#632  process env / env_clear
#633  std.net connect/read/write timeout substrate
#634  std.http client/request I/O timeouts, including TLS and pooled reuse
#635  core.json array<str> struct-field decode
#636  portable env_clear for macOS/BSD
#637  unified Claude/Codex guidance + compact handoff
#638  Copy-struct array materialization
#639  Unit-call values + aggregate call-ownership hardening
#640  cold/cache build-result parity via complete structural MIR identity
#641  remove redundant JSON schema summaries from MIR
#642  whole-range par_map kernels with direct vectorizable element loops
#643  Copy-capturing par_map range contexts + integration-test hardening
#644  par_map range-threshold retune from measured crossover data
#646  separate bounded and full test suites
#647  direct integer par_map reduction fusion and ABI documentation
#648  batched parallel pool job publication
#649  focused test selection policy
#650  low-lock task-group claims and completion
#651  fuse primitive scalar `par_map` map stages
#652  bounded body and byte-aware `par_map` grain hints
#653  stable callable scalar `where` compaction
#654  measure-first task-group record layout probe
#655  relevance/cost test-selection policy
#656  scalar par_map width and output-stride measure-first probe
#657  runtime-only aggregate-like par_map stride measure-first probe
#658  compiler-generated AoS projection/field-filter par_map range stages
#659  bounded pre-PR attestation, review watchdog, and focused verification policy
#660  chunk-source range kernels and direct integer chunk transform-reduce
#662  measure-only chunks header allocation probe
#664  parallelize compiler-recognised invariant string filters
#667  recursive Drop plans and Option<string> owned fields
#668  direct recursive Move tagged payloads
#669  multiple Move payload partial construction
#670  nested tagged payload representation and exact pkg.db L1b shape
#672  L2a parameter modes and return-provenance representation
#673  named/direct/imported parameter-root inference
#674  product return-provenance refinement
#675  eager MIR continuation closure
#685  path-complete task-wait dominance
#686  native output-buffer local/mutability validation
current PR  lexical extern invocation permission and non-escaping extern-call closure (am-u)
```

#639 fixes Unit-call values across direct, indirect, pipeline, and per-unit
lowering. Its final review also hardened call ownership: temporary aggregate
arguments keep per-member cleanup until the call is reached, and arena-owned
Move values cannot transfer to a callee. Bound aggregates require one uniform
owned allocation mode so their single path-local cleanup bit remains exact.
The same transfer rule covers `Result.map_err`; fused pipeline functions reject
Move source and result elements until explicit per-iteration cleanup exists.
The review follow-up carries `map_err` and one-owner struct runtime provenance
through their result slots, guards partial direct struct and fixed-array initialization, and
rejects mutation when aggregate ownership is path-dependent. `map_err` also
retains its receiver during mapper evaluation and tracks mapper-capture borrows.
Move values leaving a `task_group` forward the tail local's cleanup bit and
clear that inner source before return or call transfer.
Early exits join open task groups before dropping captured frame or arena storage.
The task-group runtime region is reserved for spawned environments and result slots:
ordinary owned values inside the block retain individual cleanup, and general arena-only
allocation still requires an explicit nested `arena {}`.
Both `reduce` and materializing `scan` require a Copy accumulator until MIR has
explicit per-iteration transfer and error-path cleanup for Move values.

#640 replaced the function-only implementation hash with the complete structural
per-unit MIR program consumed by codegen. Type tables, declarations, linkage,
alignment, and located metadata now participate automatically, so a warm cache
hit cannot skip a cold codegen failure caused by an omitted backend input.

#641 then removed the recursive JSON schema strings that had existed only to
perturb the former incomplete implementation hash. JSON MIR nodes now retain
their target ids; the structural program hash owns cache identity.

#642 replaced the per-element indirect `par_map` callback with one generated
typed kernel per claimed range. The element loop and direct Pure body call now
share an LLVM function, allowing cheap arithmetic bodies to inline and
vectorize while preserving the existing worker-pool scheduler and ordered
output.

#643 carried direct-source Copy captures through a synchronous immutable context
so they use the same range kernel; filtered and unsupported aggregate forms remain
sequential. It also hardened malformed-expression cleanup, the macOS `fcntl` ABI,
and the web integration-test startup retry.

The #644 threshold probe moved the caller-only `PAR_MIN_CHUNK` floor from 32768
to 65536 on native Apple Silicon. It uses alternating paired timings and median
ratios for cheap and heavy bodies; the value remains host- and body-sensitive, so
rerun `bench/par_map/run.sh threshold` before another retune.

#651 carries the first primitive-scalar length-preserving `map` stages into the
same ordered range kernel. #652 adds a compiler-generated 1/2/4 body hint and
input/output byte-aware floor. An aggressive reduction of the common `i64` floor
was measured at the boundary and regressed pool/caller by about 7%, so the first
model keeps that floor conservative until width and aggregate measurements justify
a broader retune.

#653 adds callable primitive-scalar `where` stages to the same ordered
range pipeline. It counts survivors per range, prefixes byte offsets, then scatters
in source order into an exact-sized owned array. Pure map and predicate stages are
rerun by the two kernels.

#658 extends that path to direct and dynamic AoS struct inputs, compiler-generated
field projection and `where(.field)` stages, and ABI allocation-size validation for
padded rows and JSON struct-array descriptors. #660 extends the typed range
kernel to direct and materializing `chunks` maps and to direct integer chunk
transform-reduce; #664 admits the compiler-recognised invariant `str.contains`
filter to the same stable count/scatter path. Move aggregates, SoA, richer
string-search, and unsupported layouts remain on their safe existing paths.

#659 establishes the operational rule that self-review and an adversarial
preflight happen before a draft PR, review processes have a bounded watchdog,
and related review fixes are batched instead of committed one finding at a time;
see `CLAUDE.md` and `docs/impl/16-test-policy.md`.

## Delivery retrospective: 2026-07-27 through 2026-07-30

The recent pkg.db prerequisite wave delivered nontrivial compiler foundations
but did not deliver a user-visible database package or SQLite/Postgres driver.
The merged capability sequence is recursive Drop and `Option<string>` fields
(#667), direct and multiple Move sum payloads (#668-#669), nested payload
representation (#670), parameter-mode and empty return-provenance
representation (#672), named return provenance (#673), and product return
provenance (#674). PR #675 closes eager MIR continuation lowering, but remains
compiler infrastructure rather than a usable pkg.db surface.

GitHub's earliest-commit-to-merge wall-clock proxy shows the throughput failure
clearly:

```text
PR    observable wall time    production / test / docs changed lines
#666  8h17m                   0 / 0 / 10,746
#667  7h13m                   1,736 / 1,397 / 158
#668  15m                     808 / 447 / 155
#669  24m                     68 / 313 / 82
#670  47m                     3,889 / 496 / 156
#671  14m                     0 / 0 / 169
#672  8h42m                   4,468 / 2,068 / 72
#673  23h27m                  7,380 / 3,057 / 549
#674  2h20m                   1,150 / 620 / 113
#675  4h14m                   4,595 / 295 / 258
```

These wall times exclude work before the first commit and may include idle
review or CI waits. Changed-line counts also overstate implementation progress
when formatting moves dominate, as in #675. They are evidence of delivery cost,
not productive coding hours.

The failure was operational: cross-cutting compiler slices were too broad,
contract and matrix authoring expanded before a small executable checkpoint,
ordinary findings caused repeated full-diff review, broad gates were repeated
after narrow changes, and document or formatting churn was allowed to resemble
implementation progress. The fast #668-#671 sequence proves that narrow,
mergeable slices do not have the same failure mode.

The canonical correction is in `CLAUDE.md`: one complete review pass, one
coherent all-findings fix, no ordinary re-review, narrow owner checks after the
fix, no repeated broad gate on an unchanged tree, a 60-minute implementation
checkpoint, and a 500-line target for implementation PRs. Exceptions are
limited to materially risky redesigns or explicit user direction.

### PR #679 delivery retrospective

PR #679 was squash-merged as `b194703`. Its earliest commit-to-merge proxy was
16h12m across 58 commits; the public PR was open for 3h31m. The atomic am-d
vertical changed 11 files by +14,242/-5,033 because every already-active
checked-HIR body and type-DAG consumer had to become stack-safe together; the
plan of record documents why splitting that consumer closure would have merged
an accepted graph that could still overflow in a later phase.

The avoidable delay was proof closure, not the iterative implementation alone.
The initial deep fixtures did not fully reproduce sema-owned allocation facts,
exact Result/Error identities, owned endpoints, or proportional control-flow
evidence. That caused the high-risk repeated-review exception, including P1
findings and one matrix reopen. The corrected closure matrix and exact owner
tests now make those producer-validity facts author-side obligations. The final
local gate also spent about 35 minutes on macOS test-binary startup and locked
workspace all-target Clippy; that was slow but continuously progressing and is
not a reason to narrow required verification.

One separate operational error was generalizable: a normal `gh pr edit` after
the preflight refresh deleted its hidden markers and failed CI. `CLAUDE.md` now
requires all ordinary PR-body edits before `scripts/update-pr-preflight.sh`;
only the marker-preserving post-review recorder may mutate the body afterward.
If a later full-body overwrite occurs after post-review recording, both the
preflight updater and post-review recorder must be rerun because each restores
only its own marker family.
No additional rule was added for the implementation findings because the
existing cross-cutting closure-matrix gate already states their durable fix.

### PR #681 delivery retrospective

PR #681 was squash-merged as `f3bb666`. The predecessor-merge-to-merge proxy was
2h22m; the public PR was open for 8m14s. The independently mergeable am-e vertical
changed 11 files by +729/-31 and closed the source producer, malformed-MIR preflight,
whole/per-unit LLVM ABI, optimized ABI, ThinLTO parity, and exact exit behavior together.
It stayed below the 1,000-line split-proof threshold.

The avoidable delay was acceptance-matrix translation. The first implementation had
the correct producer and backend strategy, but its tests sampled return categories,
build paths, and malformed builtin `Error` fields where the ledger promised every
category and an exact Cartesian product. Preflight therefore added the complete
source return/parameter matrix, raw/optimized whole/per-unit signatures, direct-i32
and argv ThinLTO cases, and one-field-at-a-time argv/Error mutations. The durable rule
is now in `CLAUDE.md`: before coding an exact boundary slice, translate every
`every`/`exact` acceptance phrase and every named build path into an explicit owner-test
closure checklist; representative samples do not close such a contract.

Required verification was slow but not stalled: locked all-target Clippy took 13m42s
before the final lint fix and 13m45s in the HEAD-bound preflight. Test binaries also
reported seconds of measured work after longer tool-output waits. Those commands kept
making progress and are not a reason to weaken the gate. The post-open host review
completed cleanly but put its verdict marker inline; its preserved result was normalized
instead of rerunning the full review. No extra rule is needed for that formatting-only
tool failure because the existing bounded-review continuation rule already applies.

### PR #683 delivery retrospective

PR #683 was squash-merged as `c304512`. The predecessor-merge-to-merge proxy was
3h48m14s; the public PR was open for 53m30s. The independently mergeable am-f vertical
changed 12 files by +884/-89 and closed source completion, HIR publication, MIR/LLVM
termination, whole/per-unit code generation, execution, and object-cache parity together.
It stayed below the 1,000-line split-proof threshold.

The avoidable delay was an incomplete control-flow/type-inference Cartesian matrix. The
initial implementation treated completion mainly as a body-tail property, while the exact
contract also depended on whether a discriminator falls through, whether each alternative
produces a value or completes directly, whether its expected type is already known or inferred
later, arm source order, and whether a subtree already emitted an error. The broad owner gate
first exposed an eager-termination compatibility gap; post-open review then found unreachable
alternatives receiving an outer expectation and mixed branches typing differently by source
order. The coherent fix delayed only the necessary branch reconciliation, and the focused
closure found and removed one resulting diagnostic cascade on malformed children.

The durable rule is now in `CLAUDE.md`: before coding a control-flow/type-inference slice,
cross reachability, completion kind, expected-type availability, source-order permutations,
and subtree validity in the owner matrix. Runtime joins use only fallthrough alternatives;
discriminator-unreachable alternatives are structurally checked without an enclosing expectation;
reachable eager-diverging typed wrappers receive required late reconciliation without contributing
a runtime value. Delayed constraints retain the same diagnostic guard as immediate ones.
Required verification was slow but progressing: the final locked all-target Clippy took
11m56s, and several test binaries had their known long startup delay. Those timings do not
justify weakening the gate.

### PR #685 delivery retrospective

PR #685 was squash-merged as `257c704`. Its earliest-commit-to-merge proxy was 5h17m;
the public PR was open for 4h18m. The implementation changed 4 files by +1,042/-18,
including the complete compiler-only task-wait proof checker and its owner tests. The
vertical remained intentionally atomic because splitting proof state, control joins, or
TaskGet diagnostics could authorize an uninitialized task result or reject an outer proof.

The avoidable delay was review execution and checkpoint churn, not the final compiler fix.
The first host review spent its bound reading the large repository context and repeatedly
hit macOS `xcodebuild` cache errors. A later host pass found a real P1: join tokens were
keyed by incoming generation/epoch, so loop headers could allocate a new token on every
state change and miss the required fixed point. The fix made join generation/epoch tokens
stable by syntax site, then replaced an invalid owner fixture that tried to move a Task
directly out of a conditional expression with a legal branch/loop reassignment twin.

Durable rules: a review that has no verdict is never a clean result; inspect process state,
log growth, and the last completed action, then stop at the hard bound and rerun against a
small explicit complete patch. For control-flow token interning, keys that define a join
identity are syntax-site-only; incoming state may key Spawn/Wait/Err events but must not
create fresh identities for the same loop or branch join. Finally, every owner fixture must
be legal under the language's own Move/arena rules before it can test compiler-only proof
transport.

### Current am-u delivery retrospective

The am-u implementation interval exceeded the two-hour checkpoint target because the callback and
lambda lexical-depth matrix had to be expanded after independent review, while macOS test startup
and compilation runs intermittently produced no harness output. The implementation was narrowed to
one sema producer gate plus the existing resolver consumers; direct execution of the built owner
binary was used to distinguish a completed test from a stalled Cargo wrapper. The durable rule is
to preserve the stalled transcript, inspect the actual child process, and continue with the smallest
complete owner target instead of restarting a broad suite. The matrix now includes exact one-error
precedence, rejected-HIR absence, branch/loop/early-exit lambda depth, and qualified whole/per-unit
parity.

#660's final verification records 48/48 `align_driver` `par_map` tests,
including 65,537-element worker-range tests for both materializing chunks and
direct chunk reduction, a chunk filter, a cross-worker i8 wrapping fold, and
backend-independent MIR-shape assertions. Codegen also has malformed chunk
`ParMap`, `ParMapReduce`, and filter-stage tests; the full bounded preflight,
workspace build, `scripts/test-pr.sh`, and locked all-target Clippy passed.
Review attempts are part of the record: a broad host review produced no verdict
within the 15-minute bound, and a narrower input attempt was stopped after its
process/log showed no useful progress. A compact diff review then found missing
worker-range materialization, filter-type, wrapping, backend-independent, and
malformed-MIR coverage; those findings were fixed and the final adversarial
review was CLEAN. At elapsed review checkpoints, inspect the process, log growth,
and last completed action first. Meaningful review work may continue until the
hard 15-minute watchdog; otherwise record the elapsed time and last action,
then narrow or rerun the review rather than treating elapsed time as a verdict.

After #660 opened, the final pushed SHA's broad host-native review also reached
the 900-second watchdog without a verdict; its last completed inspection was
the `align_rt_chunks`/`ParPool::submit_many` runtime path. The required narrower
host rerun and independent adversarial diff review found no actionable issues.
The final SHA's preflight attestation, Linux x86_64/ARM64 and macOS CI, and
post-open review status passed; #660 was squash-merged as `6153847`.

#662's measure-only probe was squash-merged as `f8ef4c7`. Its broad host review
did meaningful inspection for about five minutes but the wrapper emitted no
machine-readable verdict, so elapsed time was not treated as a clean result.
The review state and last useful inspection were checked, then a bounded narrow
host rerun and a fresh independent adversarial review both returned CLEAN. The
two SHA-bound logs were recorded before the `Post-open review` status was set;
the initial attestation failure was only because that post-open marker was not
yet present, and the rerun passed after the marker was recorded. This is the
operational rule to retain: when a review interval elapses, inspect process
state, log growth, and the last completed action first; allow meaningful work
to continue while the hard 15-minute bound has not been reached. At that bound,
stop the review, record the elapsed time and last completed action, and then
narrow or rerun it; never infer a verdict from elapsed time.

#664's invariant string-filter slice was squash-merged as `f775889`. The recognised
`where(fn s { s.contains(NEEDLE) })` form now lowers to a compiler-generated
`FilterStrContains` stage and calls the existing `str_contains` ABI in both stable
count/scatter passes. The focused `align_driver` `par_map` suite passed 49/49;
the new driver test pins survivor order, MIR shape, both kernel names, and the
ABI call, while the malformed-MIR codegen test pins a `CodegenError` for a
callable-bearing string stage. Workspace build, `scripts/test-pr.sh`, and
workspace all-target Clippy passed; required Linux x86_64/ARM64 and macOS CI
also passed.

The review record is intentionally explicit. The broad preflight reached the
15-minute watchdog without a verdict, and bounded commit-scoped follow-ups also
ended without a verdict after meaningful capture/ABI inspection; the final
narrow independent preflight returned CLEAN. After opening #664, the broad
host-native review reached its 900-second watchdog after inspecting the sema
parallelizability boundary; a narrower host attempt also reached its bound after
checking the LLVM branch, `AlignStr` ABI, HIR recognition, and stage matches. A
final narrow host check returned no findings, and a fresh independent post-open
review returned CLEAN. These are not elapsed-time verdicts: at each checkpoint
the process/log/last action was inspected, meaningful work was allowed to
continue, and only the hard bound caused a rerun or narrowing.

The task-group record probe in `bench/task_group/` compares the shipped split env/result/error
allocations with one-record tight and cache-line-separated padded controls over the same registration ABI.
The recorded native Apple Silicon command was `LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib:/opt/homebrew/opt/llvm/lib TRIALS=30 REPS=7 bench/task_group/run.sh`;
the harness reported eight runtime workers and covers the one-task caller-only path plus
worker-derived scheduler/batch boundaries. Packed-tight is primarily allocation-call/record-shape
evidence because the split bump-arena allocations may already be physically adjacent; the padded
control measures total padding/alignment and cache-footprint cost rather than isolated false sharing.
Error-slot rows use successful task bodies; the separate smoke exercises the actual error return.
The run improved large groups but did not meet the median 10% gate across small and error-slot
groups;
repeatability still requires multiple fresh invocations. The harness drains late pool helpers between
repetitions outside the clock. Padding was materially slower on large groups. Zero-task groups are
rejected because they contain no record to measure. The production
task-group ABI and allocation shape remain unchanged until that gate is met.

The last recorded full workspace run before #636 was 2748 passed / 0 failed,
with clippy clean. #636 then passed focused Linux runtime/process tests, clippy,
and the macOS release-build CI path. A local `cargo build --release --workspace`
was rerun after #636. #637-#644 passed their focused and PR CI gates.

## Next work

The query-centered `pkg.db` design and its general library-boundary prerequisites are specified in
`docs/impl/pkg-design/db.md` and `docs/impl/17-library-boundary-prerequisites.md`; the feasibility
review and revised delivery gates are `docs/impl/18-pkg-db-review.md` (#666). L1a is complete: the
canonical recursive Drop plan and sound `Option<string>` struct fields are implemented with
tag-tested Drop, move/null handling, focused analysis coverage, and an alloc-count benchmark.
Partial Move-leaf replacement fails closed without an exact drop-old lowering, and fixed-array
field reads admit only Copy leaves or a borrowed `string` view; replace the whole struct/element for
larger Move leaves. L1b is complete through the authoritative §8.3 closure matrix: L1b-a (#668)
admits one direct existing-`Scalar` Move payload per tagged arm; L1b-b (#669) adds multiple Move
payload partial construction and uniform ownership; L1b-c (#670) adds tagged-in-tagged type
representation plus the exact `Result<Option<Output>, DbError>` acceptance. Dynamic
path-selected return cleanup bits remain L2; L1b accepts only values proven free-standing by the
current ABI rules. L2a is complete in #672. L2b-a1 named/direct/imported parameter-root inference
with conservative aggregate and indirect unions is complete in #673. Its hand-written diff exceeds
1,000 lines because producer inference, consumer import
validation, whole/per-unit serialization parity, source-order reachability, and their owner tests
form one compatibility boundary: splitting them would either publish unvalidated provenance or
consume provenance whose producer semantics are not yet closed. The closure matrix therefore
keeps this as the smallest independently correct vertical slice. The final local provenance
preflight also carries checker-owned break acceptance into HIR so region-rejected recovery syntax
cannot manufacture an effect, escape, ownership, provenance, or MIR loop-exit edge. The final local
review then reopened the closure matrix for termination inside an accepted break payload:
effect inference now stops every eager/source-order boundary action after termination, and MIR
emits an outer result/cleanup edge only from a reachable continuation. Copy `str`, owned `string`,
mixed control-flow, process termination, and malformed-HIR owners cover the correction. The final
independent review found one further pipeline timing gap, so the matrix was reopened before more
code: pipeline sources now form first, stage Copy captures snapshot once at their written
positions, explicit terminal arguments follow, terminal captures snapshot last, and callback
actions begin only after every operand falls through. Sequential, parallel, zip, and JSON-scanner
lowering reuse the preheader operands; an owned temporary source is hidden-owner guarded so a
later terminating operand still cleans it exactly once. MoveCheck retains captured-view owner
dependencies across `init`, EffectScan separates formation from action, and structural
EscapeCheck/finalization still visit dead HIR without joining reachable state. Focused sema/MIR,
lambda, borrow-liveness, map_into, zip, and JSON scan-reduce owners are clean. The refreshed local
provenance benchmark on Apple Silicon reports 1.916 ms/check and 22,848 interface bytes for
summary inference, plus 1.878 ms/import for semantic import validation. The final owner review
also closed source snapshots across later terminal arguments: direct, zip, scanner, and
borrow-preserving `if`/`match` sources retain selected owner roots until action; a Move-`else`
success payload and a value-producing loop instead transfer and null their old container/source.
Terminating return and outer-break paths remove the analysis snapshot from current and saved loop
states.
L2b-a2-s is complete in #674, L2b-a2-ac in #675, L2b-a2-am-g-t in #677, and the am-r design gate in
#678. Am-g-t validates only
the global type domain, concrete roots, references, template reachability, and inline cycles before
direct handcrafted-HIR lowering. The am-r ledger's first
fresh adversarial review was not clean. The revised draft addresses those findings: all 239
expression and every helper
discriminator have exact rows in `docs/impl/19-hir-validation-ledger.md`; native ABI,
generated-identity, corrected source/header origin facts, and the dependent PR order are recorded.
The pre-PR author consistency pass and adversarial preflight were clean. Post-open review of PR
#678 reopened the am-w closure matrix: a failed drained Wait could be hidden by a later empty
successful Wait, and seven adjacent ledger contradictions also required one coherent follow-up.
The first revised-HEAD review then found five closure details: depth preflight had to precede every
body pass, differing control states needed an exact Task-proof remap, task-error mirrors retained
the obsolete i32-code ABI, two extern cross-class collision rows were absent, and optional probe
names were over-reserved. The next independent pass found one final closure omission: a differing
am-w join allocated fresh generation and proof-epoch tokens without explicitly assigning them as
the active state. That assignment and its post-join Wait/TaskGet owner evidence are now authored;
the following host continuation then reopened the matrix for a missing loop-header/backedge fixed
point and a conflicting Wait-coverage sentence. The ledger now uses stable syntax-site tokens,
computes loop headers to a monotone fixed point before joining breaks, and keeps established
completion across a later unhandled no-task Wait. The final review, repository gate, and merge
completed in #678. PR #679 completes am-d with exact producer-valid depth owners and
stack-safe checked-HIR/type-DAG consumers. PR #681 completes am-e by restricting
the entry producer to Unit, signed i32, or exact `Result<(), Error>`, preserving exact whole/per-unit
and ThinLTO C entry behavior, and rejecting malformed MIR entry ABI before LLVM construction.
PR #683 completes am-f by rejecting bare non-Unit returns and reachable fallthrough
before HIR publication/MIR lowering while preserving typed and proven non-fallthrough paths. Its
focused sema, MIR, LLVM, and whole/per-unit driver matrices pass, including raw/optimized IR,
execution, and cold/hot object-cache parity. The reviewed order is am-w
outcome-sensitive task-wait dominance (#685), am-v native output-buffer local/mutability (#686),
am-u lexical extern invocation (current PR), then am-p placement, am-n nominal/link, am-h
declarations/headers, dormant am-b1/am-b2/am-b3 plus activating am-b4 total-body validation, then
am-c typed callable namespaces.
Am-c follows am-b4 because it consumes body-validated callable facts. #678 fixes the
twenty-three/twenty-seven counts as project truth.
The final author pass found one additional hidden dependency before review convergence: imported
effect bits previously arrived only through the sema call's out-of-band map and did not survive in
checked HIR, so a later handcrafted-HIR preflight could not replay parallel purity independently.
Am-h now owns one normalized imported `FnEffect` header fact (an absent compatibility-map entry
normalizes to fail-closed `Impure`), and am-b4 replays body/call-graph effect inference, every
concrete function-value/projection/join cell, and parallel eligibility before am-c consumes callable
facts. Am-h converts that validation-only fact to a new effect-free MIR imported-declaration record
with the same six existing fields and structural Debug bytes before codegen, so MIR `impl_hash`,
interface bytes/hash, and cache behavior stay unchanged. This is an internal checked-HIR transport
change only; the interface already carries the same effect fact.
The closing review also found five semantic producer holes that must land before header/body
validation. Am-e restricts no-arg `main` to Unit, exact i32, or
`Result<Unit,builtin Error>`: other returns were accepted by sema but emitted an invalid external
C `main` ABI. Am-f rejects bare return and reachable absent tails in non-Unit functions; before #683
both reached MIR/LLVM as `ret void` under a value-returning signature. Am-w replaces task
wait-state traversal order with generation- and Wait-id-sensitive successful-wait
dominance across `if`, direct `match wait()`, direct `wait() else`, every reachable loop break,
early exits, nested groups, Spawn reset, infallible Wait, and direct fallible `wait()?`; its
fallible-Wait Result carries compiler-only group provenance through bare locals, copies,
reassignment, `map_err`, and value-producing control joins. Every earlier Wait for the same drained
task generation must also resolve Ok: a later empty Wait cannot hide an unresolved or failed first
result. Err invalidates every Task/Wait proof it covered. Every Spawn advances the current
generation and stales prior Wait proofs; with an unresolved Wait it also invalidates covered Tasks,
while after success one later Wait reauthorizes old and new handles. The settled stored-result form that exhaustively handles every
covering Wait Result before reading a task remains valid. Calls, returns,
captures, imported values, and aggregate reconstruction do not synthesize provenance, and every
Spawn stales all WaitProof aliases for that group. Each Move Task handle separately carries its originating
group through transparent local/control transfer. Nested entry preserves outer facts; inner Wait
cannot authorize an outer Task, while handling an outer Wait Result inside the inner group updates
the outer fact. Exit clears proofs naming the inner group, including proof on its Result block
value, but preserves outer proofs; handling that cleared Result outside cannot authorize an outer
Task. Current primitive `TaskGet` is non-consuming and repeatable, and its rejection diagnostic
uses the originating group's fallibility rather than the innermost group. The current
match/loop gaps can otherwise admit `TaskGet` from an uninitialized result slot. The final
post-review closure assigns stable tokens per syntax site, computes each loop header from entry and
reachable fallthrough to a fixed point before joining breaks, and preserves an established
completion across a later unhandled no-task Wait. This prevents an earlier iteration's unresolved
or failed Wait from disappearing while keeping initialized slots readable. The next independent
pass found the same distinction missing in join remapping: a TaskProof now survives an unresolved
Wait only when that predecessor already completed its current generation; otherwise coverage
clears the proof. The TaskGet discriminator and straight-line/branch/loop owner rows carry the same
rule. Checkpoint `7247fc6` now contains the first compiling vertical implementation and has passed
`cargo check -p align_sema --lib` plus the existing 188 sema unit tests. The am-w implementation is intentionally one vertical: splitting group state, proof
transport, joins, or TaskGet diagnostics would leave an intermediate compiler that can authorize
an uninitialized task slot or reject a valid outer proof. If the implementation exceeds the
500-line target, the split-proof exception is justified by this single safety invariant; the
formation, control, ownership, and whole/per-unit rows must land together. Am-v (#686) required a
bound `mut Buffer` local at ReaderRead, ReaderReadLine, FilePread, UdpRecvFrom, and CryptoRandom;
those five producer paths now reject equal-typed temporaries and immutable buffers even though the
runtime writes through them.
The LLVM 22 toolchain is available at `/opt/homebrew/opt/llvm`, and focused task-group tests pass
with `LLVM_CONFIG`/`LIBRARY_PATH` set. The ordinary `scripts/test-pr.sh` gate remains blocked by
the `align_codegen_llvm` unit-test binary hanging in macOS dyld startup before listing its zero
tests; this is an environment/toolchain execution blocker, not a compiler test failure. Am-u rejects
extern declarations as first-class function values and requires direct or non-escaping named
pipeline/reducer/sort invocation to be lexically
inside `unsafe`; current HIR function values cannot carry visible unsafe-call permission. These are
independently correct producer fixes, not malformed-HIR heuristics.
The same review found a separate stack-safety closure. The constructor/parser audit fixes 259 as a
proved conservative checked-HIR producer ceiling; it does not claim a syntactic program reaches the
minimum possible ceiling. This avoids relying on apparent maximum-depth shapes that the shared
parser recursion counter or template-hole grammar cannot construct. Am-d accepts handcrafted depth
259, rejects depth 260 before semantic consumption, and converts producer replay to explicit
enter/exit worklists. All four MIR-lowering paths use a heterogeneous worklist for strict eager
spines, immediate parent-owned continuations for multi-child ownership boundaries, and small
out-of-line structured-control and specialized-operation frames bounded by the fixed 259 ceiling.
The outer dispatcher reaches the existing file, reader, array-builder, process-command, path,
regex, and HTTP helpers without retaining the giant recursive frame; strict string wrappers stay
on the eager worklist. Operation-specific MIR assertions cover mixed eager/call, string trim,
producer-valid path/regex chains whose individually owned temporary strings end in an owned return,
self-buffering reader plus file/command/HTTP producers whose every Move-valued node carries its
individual-allocation fact, a producer-valid `Result<str, Error>` `StrBytes`/`BytesAsStr`/`Try` cycle,
template spines cloned from their hidden views into an owned return, file create, array-builder push,
process-command construction, HTTP request construction, block/statement sentinel reachability,
proportional `if` and binary/wildcard `match` CFG evidence, independently counted `else` and
short-circuit branches, loop, and arena/task-group boundary owners on the 2 MiB test stack. Result
construction and inspection instructions are checked against the exact Ok/Error payload and bool or
unwrap destination type, and the bytes/string owner checks the exact builtin Error declaration.
Reachability replay propagates strict-child divergence
through transparent stage/template records before visiting later siblings and preserves the
non-fallthrough status of `process.exit`/`process.abort`. The final raw/optimized
LLVM owner uses the same accepted record boundary, and the deep owned-leaf codegen owner emits the
actual recursive Drop on that same stack.
Am-g-t intentionally
admits every finite header-mediated type DAG with no depth cap, so
am-d also establishes one common iterative type traversal. Am-p placement, am-n shape comparison,
am-h signature/summary validation, am-b1–b4 body type relations, and am-c canonical codecs each
inherit deep valid and deep malformed-sibling owners. Am-c preserves depth-first first-visit bytes
with explicit work items. Am-d first inventories and closes every already-active recursive type
consumer from HIR through LLVM: Drop/Move, borrow/region/escape/ownership predicates, MIR
type/layout conversion, and LLVM struct body/layout. Deep acyclic-inline and header-mediated graphs
run through stored headers, parameters, locals, returns, aggregate fields, Drop, borrow/region, and
raw/optimized LLVM roots on the 2 MiB test stack. Am-d may exceed 1,000 hand-written lines because
splitting any current body/type consumer or the common type visitor would merge an accepted graph
that can still overflow a later consumer; the atomic vertical and its reason are recorded in the
plan.
L2b-a2-af then adds validated
fixed-array formation and exact/dynamic element and element-field
selection/replacement on that completed substrate. L2b-a2-ar closes retained storage across
non-fixed index/range, `ArrayChunks`, and `HttpRespHeader` actions. L2b-a2-ap separately closes
pipeline `Project`/`WhereField` under an explicit stage/terminal state machine. L2b-a2-t completes
user-sum/`Option`/`Result`, `match`, `else`, `?`, and `map_err`. The public interface remains the L2a
parameter-index summary, so a single aggregate actual deliberately remains conservative. Unknown
extern and indirect targets retain the all-compatible-input fallback.
The ac implementation uses one required-child continuation protocol across every eager MIR parent:
a terminating child may not feed a typed operation, lower a later sibling, append an action, build
a helper CFG, allocate, or transfer cleanup. Direct tail delegation is permitted only when the
caller performs no later work, and its first non-tail parent must apply the guard. The owner matrix
requires every recursive child-lowering entrypoint (`lower_expr`, borrow/block wrappers, consumed
argument lowering, and delegating helpers) to be classified, plus representative
fixed/non-fixed/index/native/call/aggregate/control/pipeline no-action tests and whole/per-unit
parity. Normal driver input is semantically checked HIR; exhaustive handcrafted-HIR action
metadata validation is the following am boundary.
Block reachability is an O(1) `BuilderCtx` bit maintained by `new_block`/`terminate`, not a CFG scan
after every child. Pre-child synthetic-owner, cleanup-bit, explicit-region setup, and infallible
checked-HIR type/layout fact derivation remain permitted when the terminating edge owns any
required cleanup. Fallible compiler-table/path lookup and every parent action wait until all
required children fall through; no post-child transfer/action may use non-fallthrough as success.
The L2b-a2-ac implementation is complete on its branch: the caller-local required-child protocol
covers expression, statement, call, aggregate, native, index/range, and pipeline lowering; all
value-carrying joins reject an unterminated zero-predecessor block. Missing indirect-function
signatures and invalid element-field paths terminate without a parent action as narrow defense in
depth, not as an exhaustive malformed-HIR contract. The author-side matrix-to-diff pass classifies
every recursive lowering entrypoint as an immediate required child, explicit predecessor-backed
control continuation, or side-effect-free tail delegation. The focused whole/per-unit MIR and
codegen owners pass. Positive MIR/codegen owners also preserve origin-compatible indirect and
`map_err` callback signatures and the settled owned-`string` field to borrowed-`str` read. The
final revised adversarial preflight is clean. The post-open independent review found that dynamic
and SoA element-field lowering still emitted `SliceLen` before its narrow malformed-path check;
the follow-up moves that check before the first length/field action and adds fixed/dynamic/SoA
negative owners. The refreshed Apple Silicon provenance benchmark
records 3.136 ms/check and
22,848 bytes for summary inference, 1.835 ms/import, and
`mir-continuation-lowering` at 1.105 ms/lower over 2,561 basic blocks. The passing ac depth owner is
the end-to-end `within_limit_chain_compiles_and_runs` test; the broader `expr_depth` binary
currently reproduces the same sema-time `deep_within_limit_expression_is_accepted` stack overflow
on unchanged `origin/main` under local Rust 1.96.1 and is recorded as a baseline failure rather than
a clean ac gate. The ac debug `lower_expr` frame is 75,808 bytes versus main's 77,168 bytes, so the
slice improves rather than consumes MIR-lowering headroom.
The hand-written ac diff is roughly 1,700 lines. It cannot split safely by expression family:
landing any subset would retain the same reachable placeholder operand while knowingly leaving
another eager parent able to append after termination. Reachability state, the caller-local guard,
every recursive parent family, and their whole/per-unit tests are therefore one compatibility
boundary and the smallest independently correct vertical. A fresh adversarial preflight found
that the original matrix also promised fail-closed checks for every handcrafted-HIR action family
while the implementation only hardened the two lookups touched by ac. The matrix was reopened:
the broad contract was first split as L2b-a2-am-g/am-b. A second revised-matrix review found that
action-only validation still omitted callable symbol uniqueness and universal
`Expr.ty`/operator consistency. Implementation then proved the global half itself too broad: the
combined type plus nominal/link checkpoint measured 1,535 changed hand-written lines. Type-domain
validation has no atomic dependency on nominal/link validation, so am-g-t was the only
authorized next slice at that checkpoint. It returns a canonical all-empty MIR program only for an invalid global type
domain and leaves every placement, nominal, namespace, declaration/header, and body predicate
unchanged.

The broader remainder was deliberately reopened rather than patched again. Reviews found that a
body-free validator must not claim body-derived Drop facts; inline-layout cycles must be separated
from valid header-mediated nominal recursion; generic-template roots must remain producer-compatible;
and current HIR cannot distinguish source functions from generic monomorphs. Am-h therefore replaces
the ambiguous `lifted_capture_count`/`exportable` pair with one required `FnOrigin` record for source,
monomorph, and lifted functions; exportability is derived from source entry/visibility flags.
Post-open review then
found two more missing invariants: graph-valid types still need exact per-position producer
admissibility, and callable validation must cover logical runtime lookup keys plus body-generated
`$fnval`, `$clos`, task-trampoline, and parallel-kernel identities. It also found that rejecting
source-accepted exact compiler/runtime spellings as malformed HIR would be a hidden semantic
change. The completed am-r ledger preserves those spellings by separating typed program, runtime, and
generated call registries; proposes injective compiler-owned identities for non-exported Align and
private generated helpers; inventories the 277 existing runtime lookup keys, all 239 `ExprKind`
variants, and every helper discriminator. The revised native ledger promotes the four previously
codegen-selected AEAD symbols to typed keys, for 281 keyed records. Five always-built unkeyed
runtime records make the fixed compiler registry 286 entries. Four `alloc-count` and four distinct
`par-map-probe` exports extend only the verification-time maximum export table to 294; their names
remain ordinary program/extern/export spellings, and probe-feature runtime fixtures never link user
artifacts. Runtime feature selection changes no compiler input, source acceptance, or cache
identity. `task-group-probe` adds no unmangled export. Exact LLVM
types/attributes and verification presence policy live in
`docs/impl/20-runtime-abi-ledger.md`. The completed ledger
also records every placement predicate and gives every body discriminator an envelope/child/type/
ownership row in `docs/impl/19-hir-validation-ledger.md`; any body failure returns the same
canonical all-empty program as a global failure. Concrete MIR call-target types, structural
generated-identity bytes, and semantic/byte goldens are now recorded in the am-r ledger. Body
construction remains scheduled as three dormant exhaustive validator PRs and one atomic activation
PR so no partial malformed-HIR claim is exposed. #678 fixed the twenty-three/twenty-seven counts and
strategy after the clean reviewed ledger passed the repository gate.

Am-g-t's type-domain implementation is preserved separately. The split applies the existing
review-size and closure-matrix rules; it does not justify a new process rule.
The final L2b-a2-s vertical is approximately 1,900 lines because its adversarial review required
malformed constructor/read/write fail-closed validation, common eager-child source-order snapshots,
snapshot-generation invalidation, checked-expression identity, action-boundary validation, and
discriminating deferred-array/liveness owners in the same PR; separating any of them would publish
an under-approximating or dangling fact.
Its final local provenance benchmark reports 3.147 ms/check, 22,848 interface bytes, and
1.844 ms/import on Apple Silicon.
Do not begin a
SQLite/PostgreSQL driver or add database-named compiler
variants before L1a–L7 are complete. The reviewed part of the L2 sequence is L2a
parameter-mode and provenance-summary representation plus
L2b-a1/a2-s/a2-ac/a2-am-g-t plus the completed am-r design gate, a2-am-d, a2-am-e, and a2-am-f. #679 completes
am-d, #681 completes am-e, and #683 completes am-f; the remaining sequence is a2-am-p/a2-am-n/a2-am-h/a2-am-b1/a2-am-b2/a2-am-b3/a2-am-b4/a2-am-c/
a2-af/a2-ar/a2-ap/a2-t/b
return-provenance slices, L2c cleanup-ABI record plus dynamic Move-return bit, L2d shared borrow,
then L2e
mutable borrow/out and all-peer
exclusivity, for twenty-three L2b and twenty-seven L2 implementation PRs. The counts are fixed by
#678; after the current am-u PR, am-p is the next implementation slice. The required milestone order
is L2,
L3 package-defined/dependent
resources, L4 named region capability, L5 deterministic static inputs/Query/command artifacts, and
L6 the region plain-struct builder, then L7 nested generic package APIs and the closed
`RegionPlain` bound. No safe driver begins before L1a–L7 are complete. L2 includes contextual
parameter parsing, all-peer mutable-borrow alias checking, drop-old replacement, target-relative
capture provenance, and the dynamic Move-return cleanup ABI. L3 includes a producer-owned linkable
Drop thunk and root-only raw transfer. L5 permits exactly one whole-body static constructor per
uniquely named descriptor item and uses tagged file/inline source identity plus exact per-driver
checked-metadata Missing/Present manifest state. D3's migration-backed SQLite prepare uses the canonical
validated numeric catalog. Query and command share the L5/D1 statement artifact and generated
binder. Checked metadata is per permitted driver. Option API ownership is D1/D2/D4/D6/D7/D12, with
D9 completing deadline enforcement/native cancellation cleanup without a v1 public cancel
resource. PostgreSQL Text `bytea` uses exact hex while raw bytes are Binary-only; cancellation must
resynchronize or close before connection reuse; and Query-less validation uses no fabricated Query
identity. `resource.borrow` is public safe ownership-only access while raw forms stay
declaring-subtree privileged. `db.exec_result` is exactly the Copy affected-row record. Every D11
live command has explicit entry/catalog/driver/target inputs. D12 uses exact `SchemaRef`/`TableRef`
and complete flat Column/Key/Index/Query records, with exact detail/discriminator projection,
Unknown-state and Summary→Parameter→Column ordering, ordinal/digest semantics, and declaration-order
U+0000 rejection precedence for identifier components. Duplicate constraint names use a canonical
`key_ordinal`; its complete key signature includes action/deferral/validation fields. The L5/D1
artifact has an exact top-level/nested canonical codec, checked-in Query/command byte+digest
goldens, structural reachable-definition Params/Row fingerprints, every binder/decoder ABI version,
and a producer-owned QueryMeta plan/thunk. D2 owns SQLite's single active-execution lease. D3/D5 use
the exact derived metadata path, fail-closed canonical JSON/identity codec, explicit schema ID for
mutable targets, and independent goldens; D3/D11 share the versioned migration/schema-identity
codec. D0 records the actual
engine/version origin/nullability evidence; D3/D5 own fail-closed driver matrices, ambiguous evidence
remains `Unknown`, and runtime NULL guards remain mandatory. The first public database release gate is
driver-relevant D1–D12: D4 merge/release requires non-skippable provisioned PostgreSQL CI; D11 uses
the exact atomic-default/dirty-forbidden migration contract; and D12 returns flat metadata/plan
records into an explicit region. D13/D14 remain committed additive work. Native feasibility probes
may run independently but create no public API.

The capture-context, threshold, test-policy, direct integer transform-reduce, queue-publication,
focused-verification, low-lock task-group, staged-map, and body/byte-aware grain slices are
shipped in #643, #644, #646, #647, #648, #649, #650, #651, and #652. #653 shipped stable
callable scalar `where` compaction, and #654 shipped the task-group record probe. The probe
measured packed-tight and padded controls without changing production behavior; the cross-size
gate was not met. The scalar width/stride measure-first probe covers `i8`, `i32`, and `i64`
fused/materializing maps around the runtime floor; #657 also records runtime-only aggregate-like
16/32/64/128-byte record strides with full-output checksums, without changing production behavior.
Other-host aggregate cost sweeps and any broader width/aggregate retune remain
separate follow-ups. The merged #660 chunk-source
range path intentionally retains the `chunks` header allocation. The measure-only
`bench/par_map/run.sh chunks` probe on Linux x86_64 used symmetric validation in both
timed arms and showed 1.249x–1.336x versus the allocation-free cursor control across
two final invocations; an earlier one-sided-validation result was rejected as a timing
artifact. This earns an end-to-end no-header design measurement, but not a production
allocation-removal change by itself. Review of the harness also found that a failed ABI
assertion could leak the runtime-owned header buffer; the follow-up uses an RAII cleanup
guard, explicit count bounds, checked pointer arithmetic, and reran the probe plus
probe-feature Clippy clean. The no-header design remains consumer-gated, and no
new compiler widening is justified until the cross-host/aggregate measurements
earn a concrete consumer.

Consumer-gated deferrals that remain intentional:

- Fully escaping function values wait for a consumer and a settled heap-owned
  environment/drop model.
- `std.process` binary capture (`run_bytes`) waits for a binary-output consumer;
  see `docs/impl/std-design/process.md`.
- Top-level `array<str> := json.decode(...)` waits for a result representation
  that carries the input region. Struct fields of `array<str>` already ship;
  see `docs/impl/core-design/json.md`.
- The first pkg.web consumer application remains a separate, owner-scheduled
  task.

The integration-test execution-policy review is implemented on `main` in #646 and its
focused-test-selection clarification shipped in #649.
`scripts/test-pr.sh` is the bounded ordinary gate: workspace build,
deterministic non-runtime library tests, and the M0 compile/link/run smoke.
CI no longer runs the full workspace corpus or the pkg.web performance gate on
each PR. Deep driver regressions, differential fuzz, runtime network/filesystem,
and performance suites remain explicit change-specific checks;
`scripts/test-full.sh` retains the full corpus for unusually broad work and
versioned-release preparation. `docs/impl/16-test-policy.md` records the audit,
commands, suite-growth rule, and the relevance/cost rule: every add-on target
must name the changed boundary, plausible failure, non-duplicate information,
and reason its cost is justified. Meaningful expensive checks remain allowed;
unrelated or duplicative suites do not become mandatory by proximity.

## Build and test notes

On this Apple Silicon machine, use:

```bash
export LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm
export LLVM_CONFIG=/opt/homebrew/opt/llvm/bin/llvm-config
export LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib

cargo build --workspace
scripts/test-pr.sh
cargo clippy --workspace --all-targets -- -D warnings
```

Run `scripts/test-full.sh` only when the change scope or release preparation
requires the retained full regression corpus.

Operational rules:

- After modifying `align_runtime`, run a plain workspace build before driver
  tests or `alignc run`; user programs link the runtime static archive.
- Do not edit runtime sources while a workspace test run is in progress; that
  can produce a stale-archive cascade in driver tests.
- Do not pipe test output through a command that hides the original exit code.
- Use `ALIGNC_CACHE=off` when a test specifically requires a cold build.
- Network, TLS, filesystem, and fd tests may need an unrestricted local
  environment rather than a sandbox.

## Durable records

```text
Language semantics and surface       draft.md
Current decisions and open items     docs/open-questions.md
Milestone implementation evidence    docs/impl/07-roadmap.md
Current pkg.web contract             docs/impl/pkg-design/web.md
Current pkg.db contract              docs/impl/pkg-design/db.md
Native library boundary prerequisites docs/impl/17-library-boundary-prerequisites.md
pkg.db feasibility review            docs/impl/18-pkg-db-review.md
Cache architecture and parity resolution    docs/impl/10-cache-first-optimization.md
Test execution policy                docs/impl/16-test-policy.md
Closure/memory/I/O/SIMD audit        docs/impl/12-pipeline-closure-memory-io-simd-audit.md
Allocation and short-input audit     docs/impl/13-string-array-allocation-short-input-audit.md
Source-correctness fixes             docs/impl/source-correctness-fixes-2026-07-13.md
Historical session journal           docs/archive/HANDOFF-2026-07-25.md
```

## Maintaining this handoff

- Update the current baseline and next-work sections in place.
- Do not append a full PR narrative. Put durable design facts in the relevant
  spec/audit, and rely on the PR and Git history for implementation chronology.
- When historical context is still worth retaining, add a dated archive rather
  than growing the live handoff indefinitely.
- Keep release and review procedures in `CLAUDE.md`; link to them instead of
  duplicating them here.
