# Session handoff

Current continuity note for a fresh Claude Code or Codex session. Keep this file
about the present state, the next decision, and operational facts. The former
per-PR journal is preserved in
[`docs/archive/HANDOFF-2026-07-25.md`](docs/archive/HANDOFF-2026-07-25.md).

_Last updated: 2026-07-28. `main` includes the shipped wave through #666.
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
larger Move leaves. The next implementation slice is L1b only: Move tagged payloads through
Option/Result/user sums. Do not begin a SQLite/PostgreSQL driver or add database-named compiler
variants before L1a–L7 are complete. The remaining required order is L1b Move tagged payloads
through Result, L2 borrow
summaries, L3 package-defined/dependent
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
