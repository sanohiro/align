# Test execution policy

The test suite has two distinct jobs:

1. prove on every change that the compiler still builds and its vertical path
   still works;
2. protect individual implementations with deep, expensive regression checks.

Those jobs must not share one mandatory command.

## Audit baseline

The 2026-08-05 audit found:

- 3,238 Rust test functions across the workspace;
- 167 `align_driver` integration-test binaries containing 2,395 tests and
  57,274 lines of test source;
- about 25 seconds of macOS startup per linked driver test binary on the
  audited machine even for `--list`, implying roughly 70 minutes before test
  work if all 167 binaries are launched;
- 10 differential tests that each compile and run 150–200 generated programs;
- two frontend fuzz loops with 12,000 seeds each and a formatter loop with
  10,000 seeds;
- 14 driver test binaries using real sockets and 32 using the filesystem;
- runtime tests that include TLS, timeouts, process control, fd-leak cycles,
  cryptographic cost cases and 10,000–100,000-iteration stress loops;
- measurement harnesses and wall-time assertions embedded in test targets.

Most of these tests have a valid regression role. Their accumulation into one
all-or-nothing `cargo test --workspace` result did not. The test-function count
grew from 936 on 2026-07-01 to 3,238 on 2026-08-05; growth at that rate requires
ownership and cost control rather than a larger universal gate.

## Ordinary PR gate

Run:

```text
scripts/test-pr.sh
```

This gate is deliberately fixed and bounded:

1. build the workspace, including the runtime archive;
2. run the explicit deterministic library-test list for the compiler crates and
   the small interface/formatter integration targets;
3. run `align_driver/tests/m0.rs`, which covers source checking, HIR-to-MIR
   lowering, native object emission, linking, execution, and a rejected
   program.

CI additionally builds release compiler/runtime artifacts and compiles and runs
`examples/hello.align` on Linux x86-64, Linux ARM64, and Apple Silicon. This is
the cross-platform packaged-command smoke path.

The ordinary gate does not run:

- the full driver regression corpus;
- differential fuzzers;
- real network, TLS, filesystem, timeout, process, or fd-leak suites;
- performance or scaling measurements;
- repeated concurrency and artifact-staging stress tests.

The compiler library list is intentionally explicit in `scripts/test-pr.sh`.
Adding a new workspace crate does not silently add its tests to every PR; the
new crate must first be given an intentional owner and gate classification.

## PR sequencing and review cost

The draft PR is a review-ready checkpoint, not the place to discover basic
correctness problems. Before opening a draft for a code change, finish the
coherent implementation, run the applicable self-review checklist, run a fresh
adversarial preflight on the local diff, and pass the focused owner target plus
the bounded PR gate. `scripts/pre-pr.sh` records those checks against the exact
HEAD, and `scripts/open-pr.sh` is the required agent path for opening the draft.
CI rejects a missing or stale attestation. This keeps obvious ownership,
malformed-input, ABI, and cross-stage omissions out of the external review cycle.

The preflight is the one required full-diff review. Do not assign another
reviewer the unchanged complete scope after opening the draft. Verify the
complete finding set and batch all valid fixes into one coherent follow-up.
`scripts/pre-pr.sh --findings-fixed` binds that reviewed candidate and the
later fix commit into one closure record after the focused owner check and
bounded gate pass. It does not claim that the final SHA received a CLEAN
verdict.

A second complete independent review is required only when the fix changes a
public contract or strategy, changes an IR shape, materially crosses three or
more compiler layers, or responds to a P1 by redesigning the implementation. A
small ownership, cleanup, FFI, or ABI correction receives a review of the
changed slice and root-cause class, not another reading of the unchanged full
diff. The normal completion cycle is review once, fix all findings once, run
the affected owner checks once, and finish.

`scripts/review-bounded.sh` has no default wall-clock cutoff. Its watchdog stops
only after the configured stall interval has no log growth and no process
CPU/state progress. An optional user-supplied maximum duration bounds one
invocation. At elapsed checkpoints, inspect the process group, log growth, and
last completed action before deciding whether to continue. Time alone is not a
verdict. If no verdict exists after a stall or explicit bound, preserve
completed findings and continue only from the unfinished scope; do not restart
the same broad review or manufacture `CLEAN` from elapsed time.

Do not rerun the same broad review or broad test gate on an unchanged tree.
After an ordinary review fix, run the smallest owner targets that can detect a
regression in the changed lines. Preserve earlier successful broad results when
only documentation or review records change, and let CI provide the final broad
gate. Review automation does not run tests. Ordinary implementation work must
not promote `cargo test --workspace` into the PR path.

An acceptance-ledger row does not imply a distinct new test or invocation.
One parameterized owner may close many rows, and existing coverage is sufficient
when it would fail for the changed regression. Exhaustive field-by-field or
Cartesian mutation is reserved for public wire/ABI contracts, ownership or
memory safety, and malformed input capable of panic or miscompilation. Internal
representation-preserving work adds only discriminating coverage.

The final SHA-bound status records the original review cycle plus its bounded
finding closure. A later push cannot inherit that result.

A non-normative documentation-only PR uses `scripts/pre-pr.sh --docs-only`.
It retains the SHA/base guard but requires no code review. The PR wrapper marks
the required review status as docs-only exempt.
Broad normative design changes still use their design review gate.

## Feature qualification versus change regression

Load, stress, fuzz, resource, protocol, and cross-platform matrices qualify a
feature when that feature is implemented or changed. Their green result remains
evidence for that feature; unrelated changes do not expire it.

Rerun such a suite only when the change touches its implementation owner, a
shared invariant it consumes (for example ABI, allocator, scheduler, or
runtime ownership), its external dependency/toolchain/platform, or the release
surface that promises it. A type-inference change does not rerun HTTP load or
fd-leak tests merely because all three live in the workspace. Conversely, an
allocator or scheduler change may legitimately select several feature suites
because the impact reaches them.

This is distinct from unit-test growth. Cheap deterministic units may
accumulate in the bounded gate. Expensive feature qualification is selected by
impact, not accumulated into a universal regression tax.

## Change-specific verification

The author must run the narrow regression targets that own the changed
behavior. Examples:

```text
scripts/cargo.sh test -p align_driver --test par_map
scripts/cargo.sh test -p align_driver --test fuzz_differential
scripts/cargo.sh test -p align_driver --test m11_http_server
scripts/cargo.sh test -p align_runtime --lib http_client
scripts/cargo.sh test -p align_runtime --lib par_map
```

`fuzz_differential` is an owner check for its other cases only:
`result_question_chain_computes_the_oracle_value` hangs indefinitely (see
HANDOFF.md) and carries `#[ignore]`, so it is triage-pending, not part of the
running owner check, until that triage lands and the attribute comes off.

A correctness-only change that does not alter a performance path uses its owner
target and the bounded code gate. Network, filesystem, timeout, process, and fd
work runs the corresponding real-resource target in an unrestricted
environment. A test should be added to an existing owner target when possible;
do not create another cross-cutting integration matrix for a unit-level rule.

### Service-dependent required suites run locally first

The required `PostgreSQL integration` CI job gates its suites on
`ALIGN_DB_POSTGRES_REQUIRED=1`, so without a configured database they skip
locally and their first execution silently moves to CI. That made CI the
discovery loop: recent `pkg.db` waves paid repeated push→wait→read-log
round-trips for failures that reproduce locally in seconds.

`scripts/db-verify-local.sh` is the CI-parity local gate: it starts a
disposable Docker `postgres:16.4` with CI's exact credentials and environment,
runs the same inverted required-mode self-test and the same
`pkg_db_q2`/`pkg_db_q3`/`pkg_db_q5a` suites, and tears the container down. A
diff touching `apps/db` or a `pkg_db_*` test must pass it before push. The
same rule generalizes: a new env-gated required CI suite ships with a matching
local Docker script in the same PR.

The D13 PostgreSQL streamed-delivery rail keeps server 16.4 as the compatibility
floor but requires libpq client/development files at version 17 or newer. Its
implementation PR must make both CI and `db-verify-local.sh` assert the client
version and run the new required direct-delivery suite; the prepared-parity PR
then adds its statement-resolver and prepared-delivery cases to the same local
and CI job. A newer server image must not hide an accidental client-version
dependency. The preceding libpq-consumer lease and result-status safety
prerequisites keep the current client floor. The same local database gate must
first run the catalog/EXPLAIN overlap suite and prove zero libpq calls on a
rejected live-stream overlap. The status prerequisite then injects every COPY
status, both pipeline statuses, and an unknown numeric status into every shipped synchronous,
timeout-completion, timeout-recovery, direct/prepared rows/one/command/prepare,
transaction, catalog/EXPLAIN, and silent-cleanup PGresult consumer. It requires
one current-result clear, physical close, zero subsequent
result/COPY/pipeline-exit/cancel/transaction-state/blocking-restore calls, correct first-error
or silent-Drop behavior, balanced owners/lease, and no reuse.
The Rust tool half uses one parameterized
`postgres_tool_results_fail_closed_before_followup_native_work` owner. It crosses migration
command/query and preparation command/query/prepare/describe consumers with null, all COPY,
`PGRES_SINGLE_TUPLE`, `PGRES_TUPLES_CHUNK`, both pipeline, and unknown numeric results. A non-null
result is cleared once, the connection is finished once and nulled before return, the first copied
diagnostic remains primary, and row access, rollback, deallocation, result retrieval, and every
later libpq call remain zero. Expected success and known complete error controls preserve their
current mapping and cleanup.
The same prerequisite retains Q5a's canonical PostgreSQL statement-screening owner. It crosses
Required/Forbidden top-level COPY, lowercase and comment-leading spelling, COPY after an ordinary
statement, quoted/comment/dollar-body near-misses, transaction control before later COPY, COPY
before later transaction control, Forbidden-count precedence, and unchanged SQLite behavior. The
two prohibited-order cases prove one source-ordered statement-classification pass. Every rejected
case proves zero URL-environment reads, target opens, locks, history publication, connection opens,
and libpq calls. A checked inventory
records that prepare uses `PQprepare`/`PQdescribePrepared`, migration is the only compiler-side path
that sends complete user SQL through `PQexec`, and all remaining tool SQL is fixed; that inventory
does not weaken the Rust tool fail-closed matrix above.

The direct-delivery suite crosses validation/decode/storage failure with no
timeout, time remaining, and deadline expiry on both connection and transaction
targets. It pins first-error retention, no further decode, completion-versus-
cancel SQL effects, the rows-v3 absolute-deadline/original-duration pair, and
recovery-budget use. Chunk owners report total transported rows separately from
maximum rows per `PGresult`; the latter is not asserted as a total-transport
bound. Every `PGRES_COPY_IN`/`PGRES_COPY_OUT`/`PGRES_COPY_BOTH`, both pipeline
statuses, and unknown-status injection is a fail-closed owner, including first-result, after-data, row/decode/storage error,
deadline recovery, mode failure, and Drop on connection and transaction targets.
Those owners assert one current-result clear and physical close, zero subsequent
result/COPY/pipeline-exit/cancel/transaction-state/blocking-restore calls, first-error retention,
balanced package-owner cleanup, and no reuse. Direct construction also retains a
pairwise phase-order owner with context allocation/free, lease, binder, and libpq
counters for live-state, generated-static-validation, and overlap failures. Its
option matrix checks Query payload-before-duplicate with both invalid orderings
and a valid duplicate, plus command tag rejection and the separate initial versus
post-release surface inventories. Its `one_native` region owner records zero
caller-region allocation before a valid first Row and the exact same one-clone
byte/alignment delta for singleton success, Cardinality, and every later error;
no case rewinds the arena or clones a second Row.
Direct and prepared explicit-delivery owners delay the test clock across
nonblocking enablement and require expired-before-send to produce zero
send/selector/cancel calls, exact Timeout, and blocking restoration or close.
Their result-sequence matrix also injects every ordinary error, invalid,
COPY, pipeline, and unknown status after a valid zero-row terminal. It asserts
that protocol state selects the invalid-sequence error before ordinary status
mapping, while status class still selects ordinary drain versus immediate
close. Whole/per-unit cache owners separately require the public `Delivery`
and rows/statement implementation changes to invalidate affected interface and
implementation keys once, with identical static descriptor semantics and no
runtime-mode-specific cache identity.

## Benchmarks are not tests

A benchmark measures one named performance path against a baseline or control.
It is never part of the ordinary gate, a feature integration suite, a broad
refactor gate, or a versioned-release gate. Run it locally only when the change
touches that performance path, an audit is explicitly `MEASURE-FIRST`, or the
PR makes a performance/resource claim.

Benchmarks live under `bench/` or as manual `#[ignore]` probes with an exact run
command. A normal `#[test]` must not invoke a benchmark harness or assert
wall-clock throughput/ratios. Correctness prerequisites used by a benchmark —
output parity, IR validity, allocation balance, protocol behavior — belong in
separate deterministic owner tests. The benchmark may check them before timing,
but its measurement result is not a general correctness verdict.

The 2026-08-05 audit moved the deep-pipeline and clang-IR harness invocations,
and the ThinLTO/PGO compile-time ratio checks, out of default test execution by
marking them manual. Other existing runtime measurement probes were already
ignored.

## Selection procedure

Choose the smallest test set that covers the changed boundary, then expand only
when the change crosses another boundary:

1. Identify the owner: documentation, one private implementation helper, a
   public FFI/ABI surface, compiler lowering, or a resource boundary.
2. For code changes, run the focused owner target first. For a private runtime
   helper, use a filtered library test such as
   `scripts/cargo.sh test -p align_runtime --lib par_map` rather than the entire runtime
   library test binary. For documentation-only changes, run `git diff --check`,
   then at most one directly relevant consistency or render check when the
   changed normative surface has one. Non-normative status, wording, or
   operational notes need no adversarial review and no code-test target.
3. Run the ordinary PR gate required for the change. Rust code changes use
   `scripts/test-pr.sh` as the bounded test gate, with the standard workspace
   build and applicable Clippy checks still required. Documentation-only changes
   need only `git diff --check` plus any relevant consistency or render check.
4. Add a broader target only when the changed behavior is not exercised by the
   owner target, crosses crate/ABI/linker boundaries, changes scheduling or
   resource semantics, or is unusually broad.
5. For a broad change, name and run each affected owner/resource suite. Breadth
   is not a reason to substitute an undifferentiated workspace-wide command.

Do not run a whole crate or the full workspace by reflex after a narrow change.
Do not repeat a target already covered by `scripts/test-pr.sh` unless the
focused invocation selects an additional behavior. Record the reason for every
expanded target and distinguish a product failure from a host permission,
network, toolchain, or dependency-linking limitation.

## Relevance and cost rule

Test cost is a reason to narrow the selection, not a reason to skip a meaningful
check. An additional target is meaningful when all of these are true:

- it executes the changed boundary or a direct consumer of its contract;
- it can fail for a plausible regression introduced by the change;
- it adds information not already supplied by an earlier target; and
- its result has a clear interpretation and stopping point.

If one of those conditions is false, do not run the target merely because it is
nearby in the same crate or suite. If all are true, run it even when it is
expensive: soundness, ABI/FFI, scheduler, resource-boundary, cross-platform,
and release checks can justify their cost. Benchmarks use the separate
local-only rule above.

Classify verification by scope rather than by a machine-specific wall-clock
threshold:

```text
bounded   the ordinary PR gate or a small deterministic owner target
focused   one owner regression, filtered library test, or named probe
expanded  multiple owner/resource targets needed for a crossed boundary
audit     a non-blocking workspace health inventory, never a PR or release gate
```

For every focused, expanded, or audit target added beyond the ordinary gate,
record four facts in the PR description or handoff: the changed boundary, the
plausible failure it protects against, why a smaller target is insufficient,
and why the result is worth its cost. If a target is omitted because it is
duplicative or unrelated, record that briefly when the omission could otherwise
look surprising. A host limitation is evidence about the environment, not a
product result, and must be reported separately.

### Scope matrix

| Change scope | Minimum verification | Add only when |
| --- | --- | --- |
| Documentation or policy | `git diff --check` | add the relevant consistency or render check when one exists |
| Private helper or local analysis | one filtered owner test; Rust changes also use the bounded gate `scripts/test-pr.sh` (including its workspace build) and applicable Clippy | the helper crosses a crate, ABI, linker, scheduler, or resource boundary |
| Compiler, runtime, or FFI code | focused owner test, `scripts/test-pr.sh`, and applicable Clippy | the focused target does not exercise the changed contract or the change is broad |
| Optimization or concurrency | owner correctness test and `scripts/test-pr.sh`; separately run one named local benchmark only for a changed performance path, `MEASURE-FIRST` audit, or performance/resource claim | add fresh-process, cross-platform, stress, or repeatability coverage only when that behavior is part of the changed contract or claim |
| Broad refactor or versioned release | the bounded gate and every affected named owner target | add platform/package smoke checks required by the changed release surface |

Run each selected target once per unchanged environment. Repeat only when the
target is inherently statistical, tests fresh processes or hosts, is
nondeterministic by design, or follows a code/environment change. Do not rerun
a command solely to obtain a more comforting green result, and do not use a
full-workspace command as a substitute for identifying the owner target.

## Workspace health audit

`scripts/test-full.sh` is retired. `cargo test --workspace` mixes deterministic
units, 167 separately linked driver targets, generated-program fuzzing, real
resources, stress loops, and measurement probes. Its long elapsed time and one
aggregate verdict do not identify which product boundary was verified, and a
single environmental failure invalidates the entire run.

The individual tests remain available and meaningful through their crate,
target, and filter. Run named affected targets for code changes. A human may
still request `scripts/cargo.sh test --workspace --locked` as a background health audit,
but its result is informational: it does not block a PR, release, or milestone,
and it must not cause already-green owner targets to be rerun.

## pkg.db owner-test harness

The `pkg.db` end-to-end suites (`pkg_db_a1`, `q2`, `q4a`, `q4b`, `q5b1`, `q5b2`,
`q6`) share one harness at `crates/align_driver/tests/db_harness/`. The
Rust-API suites (`q1`, `q3`, `q5a`) drive `align_driver` directly and are out of
its scope.

`db_harness` is a shared module, not a test binary: Cargo auto-discovers only
`tests/*.rs` and `tests/*/main.rs`. It deliberately does not live in
`tests/common/`, which every one of the ~167 driver binaries compiles; only the
E2E suites include it. It depends on `common` being declared first in the
including file and refers to it as `crate::common::…`.

### What it replaces

Four blocks were hand-written per suite, ~1,319 duplicated lines in total: the
eight-module package layout (72 declarations across 9 files, plus 13
`package_files` builders), 56 tool-availability guards, 55 exit-code assertions
in **four incompatible shapes**, and 87 trailing stub-counter checks encoded as
hand-numbered sentinel exit codes. Twelve assertion sites printed only stderr
and eighteen dropped the exit status, so a signal-killed child reported nothing
useful.

### Layers

```text
Layout      the 8 package modules; module() REPLACES by path; materialize()
            creates parent directories (common::Proj::write does not)
Run         check_exit / check_payload return a Mismatch; expect_* are the
            panicking wrappers; every report carries the RAW status, stdout,
            stderr, and the run label
gate        Needs::{Backend, BackendAndCc, LivePostgres}; LivePostgres ASSERTS
            under ALIGN_DB_POSTGRES_REQUIRED=1 rather than skipping
counters    the stub prints `#db-counters-begin` / name / value / … and the
            Rust table reports EVERY mismatching counter by name
parity      one program, both drivers, one compile, one child per case
fingerprint a case digest over label, runner, files, env, argv, expected exit
```

Counter checks move to the table only when they sit in the trailing epilogue of
`fn main`, which is where 87 of the corpus's 92 are. The other five are mid-run
deltas against a value captured earlier — a different assertion — and stay in
Align.

### The dual-driver parity owner

`FINDINGS.md` records `operation-matrix-completeness` at four events across
three PRs, past the three-event threshold at which CLAUDE.md requires a
mechanism rather than another checklist line. The mechanism is one parity owner
per wave, built on `db_harness::parity`:

- both drivers return `Result<pkg.db.conn, pkg.db.Error>` and both query
  constructors produce the same `pkg.db.query<P, R>`, so the driver is a runtime
  branch **inside one program** and the shared body cannot drift;
- selection is by environment variable, because `align_sema` rejects an `-> i32`
  argv `main` (argv requires `Result<(), Error>`) and argv would therefore force
  every matrix program off the exit-code protocol;
- Align `match` has no string-literal pattern, so case dispatch is an `if`/`else`
  chain whose final `else` returns the unknown-case sentinel `99`; the engine
  asserts no case reaches it;
- `ALIGN_DB_CASE=__list__` makes the program enumerate its own cases, and the
  engine diffs that list against the Rust table **in both directions, on every
  driver** — listing from one driver only would leave the other side unchecked;
- the engine sets both selectors explicitly and then derives the removals from
  the keys it just set, clearing every other inherited `ALIGN_DB_*`. There is no
  implicit default and no second hardcoded exclusion list to drift;
- divergence is declared, never implicit: `Expect::Same`, `Expect::Differs
  { why }`, `Expect::DriverOnly { why }`. The number of `Differs` rows is pinned
  by a per-table budget constant that must be raised in the same diff.

`DriverOnly` is for a row inside an otherwise shared matrix. A surface with no
cross-driver counterpart at all keeps its conventional standalone owner.

The parity engine lives in `db_harness`; each wave instantiates its table inside
its existing owner file. No new test binary and no change to
`scripts/db-verify-local.sh` or `ci.yml`.

### Pipeline retention

A parity program must use ONE compile pipeline for both drivers, and only the
per-unit + C-fixture pipeline can link the libpq stub. Converting a pair whose
SQLite member used `build_and_run_multi_with_static_descriptors` therefore drops
the whole-program static-descriptor path for that matrix. That path must be
**retained by a separate owner** in the same change; `FINDINGS.md` records
`owner-test-topology` ("Retain the whole-program execution owner") for exactly
this mistake. `pkg_db_q4b`'s
`sqlite_full_matrix_retains_the_whole_program_static_descriptor_path` is the
worked example.

### Migration equivalence

A migration must prove the same defect still fails:

1. **Backward** — the new layout builder is asserted equal to the retired
   `package_files`, path by path and source by source
   (`layout_reproduces_the_pre_harness_package_files_exactly`), and the migrated
   suite passes unchanged.
2. **Forward** — a case-fingerprint golden over label, runner, test-owned files,
   environment, and expected exit. Hashing sources alone is not enough: a
   refactor can preserve the file set while changing the runner, environment, or
   expected code, and each of those changes what the case proves. The eight
   package sources are excluded from the digest — they are product code with
   their own owners, and including them would break the golden on every
   `apps/db` edit.

   The golden must be **observed, not restated**. A case is one record (`Case`,
   or the built `ParityProgram`), and the executor and the fingerprint both read
   that record: `Case::run` runs exactly what `Case::fingerprint` hashes, and
   `ParityProgram::fingerprint` reads the pipeline from `runner_id()`, the child
   environment from `case_env()`, and the modules from the ones actually
   compiled. A golden that rebuilds those inputs by hand agrees with itself and
   notices nothing. Each covered axis is confirmed by mutation: swapping a
   runner, adding a child variable, or changing an expected code must move a
   digest.
3. **Fail-open probes** — every converted assertion class has an owner that
   deliberately breaks it and requires the exact report: off-by-one counter,
   absent counter, wrong parity class, mis-declared divergence, over-budget
   divergence, one-character case-name typo, signal termination, accumulated
   multi-row failure, missing package module, and required-mode gating.
4. **Test inventory** — the post-migration name set must be a superset of the
   pre-migration set *minus explicitly folded names*, with a correspondence
   table in the PR body mapping each folded name to its new owner and case.

### Counter registry

One registry (`PG_COUNTER_NAMES`) names every counter the Align dump module
prints. The parser rejects a name outside it, `CounterExpect::eq` rejects an
expectation naming one, and a string-only owner compares the registry against
the names actually printed by the Align module. That owner needs no C compiler
and no LLVM, so a counter added on one side and forgotten on the other fails in
milliseconds rather than surviving until someone happens to assert on it.

### Deferred cells

Recorded so the gap is explicit rather than implied:

- **Per-case timeout owner.** The timeout path (kill, record, keep running) is
  implemented and its partial-result behaviour is exercised by every table, but
  no owner drives a deliberately hanging case. Deferred to PR-2, which needs a
  hanging fixture anyway.
- **Live-PostgreSQL identity and cleanup** (unique per-run object names, drop
  what a case created, assert no survivors). No live parity case exists yet;
  deferred to PR-3 with the `q2` scalar table.
- **SQLite storage-mode declaration** (`:memory:` versus file-backed). No table
  currently needs cross-connection state; deferred until one does.
- **Layer-1 fingerprint coverage** is complete for all twelve migrated call
  sites in `pkg_db_q4b`. The two `#[ignore]`d measurement probes are excluded:
  they parse stdout numbers rather than asserting an exit code, so they are not
  `Case`-shaped. They were checked by hand during review; mechanising them is
  PR-2 work.

### Resource and isolation rules

Every case spawns its own process, which also isolates stub counters. Each spawn
has a wall-clock cap (default 120 s); on expiry the child is killed, the case is
recorded as a timeout, **and the remaining cases still run**. Outcomes are
collected before any assertion, so a mid-table failure never hides later rows. A
table declares `Limits` for case and child-run counts so a generated Cartesian
product cannot silently become a many-thousand-spawn suite; `run_parity`
applies the default ceilings and a table needing others calls
`run_parity_with_limits`. Every declared row must execute at least once: a
`DriverOnly` row whose driver is absent from the run set is a failure, not a
skip, because a table whose coverage silently shrinks to nothing is the failure
mode the table exists to prevent. Case names are also checked for capture-file
collisions, since the sanitiser that turns a name into a filename is not
injective.

Decisions that depend on process-global state — the live-PostgreSQL gate and the
`ALIGN_DB_*` clearing rule — are pure functions (`live_postgres_decision`,
`should_clear_env`) so they are testable without `set_var`; mutating the
environment from a parallel test is the recorded `test-global-state-isolation`
finding.

A diff touching these suites still runs `scripts/db-verify-local.sh` before
push. When Docker is unavailable that script exits 2, and the change must not be
pushed: CI is the final guard, never the discovery loop.

## Growth rule

Every new integration test must name the boundary or regression it protects.
Prefer, in order:

1. a unit test beside the implementation;
2. one focused regression in the existing owner target;
3. an end-to-end test only when the failure crosses crate, ABI, linker,
   runtime, process, or protocol boundaries.

Load, throughput, scaling, repeated-race, differential-fuzz, and resource-leak
checks are explicit change-specific tests, never ordinary smoke tests. Do not
add a new top-level integration-test binary when an existing owner target can
express the regression. Parameterize repeated semantic cases instead of
copying fixtures. A new cross-process target must justify its separate link and
startup cost. New large seed or iteration loops must be ignored by default and
document the exact owner command that enables them.

Consolidating the existing corpus is useful maintenance, but it must not
interrupt a product milestone merely to improve a test-count metric.
