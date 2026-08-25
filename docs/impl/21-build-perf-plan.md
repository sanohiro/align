# Build performance plan (dedicated improvement track)

This is a dedicated improvement track outside the milestone roadmap
(`docs/impl/07-roadmap.md`). It consumes no mainline milestone. Each item
passes the normal design and implementation gates when work on it starts.

## Principle

Owner-settled 2026-08-12: **Output is always fully optimized. Build speed
comes from reuse and parallelism, never from lowering optimization.** A
reduced-optimization dev profile (`-O0` or equivalent) is a non-goal.

## Track items

Order is priority.

| # | Item | Status |
|---|------|--------|
| 1 | Persistent unit cache v1 | Shipped — in-process memo #757 (`docs/impl/10-cache-first-optimization.md` §6.6) and the persistent per-unit frontend cache #761 (§6.7, Slice C3) |
| 2 | lld linking on ELF | Shipped as #763 — see below |
| 2a | Required DB owner build-once/run-many | Shipped in #882 — exact-set concurrent execution across four isolated CI shards; required wall time fell from about 60 minutes to 15:25 while every shard kept the hard 30-minute budget |
| 3 | Pipelined compilation | Implemented. A dependent unit's frontend starts as soon as each dependency interface summary exists while already-ready codegen runs within the same `-j` budget; validation, publication, and retry follow the ledger below |
| 4 | Prebuilt optimized cache distribution | Ship warmed std/pkg cache entries with releases, once the v1 persistent format is settled |
| 5 | Daemon / watch mode | Keep the in-process memo alive across builds; the main lever for AI-agent edit-compile loops. `align-repl` (`docs/impl/22-repl-plan.md`) is the first consumer of this lever: it is already a long-lived process, so it realizes memo residency with no daemon machinery |
| 6 | Function-level incremental compilation | Heaviest; requires its own design ledger before any implementation |

## Background

Measured on `pkg.db`: cold per-unit codegen ~9s versus frontend ~3.4s.
Caching (items 1–4) removes re-paying, parallelism (3) and residency (5)
shorten the wall clock, and granularity (6) shrinks the unit of re-payment.

## Item 3: pipelined frontend and codegen

### Boundary and public contract ledger

The ordinary non-ThinLTO `build`/`run`/`size` path currently performs two
barriers: every unit completes frontend work, then every object-cache lookup
completes, then the codegen misses run. The first barrier is unnecessary. A
unit's dependents consume its finalized interface summary, not its object, so
the driver may continue the serial bottom-up frontend while an owned MIR from a
ready unit runs through LLVM on another worker.

This item changes scheduling only. It adds no language syntax, CLI flag,
environment variable, persisted format, cache-key field, diagnostic, output
artifact, or linker behavior. Existing library entry points remain available
and behavior-identical: `build_package`, `codegen_package_parallel`,
`build_per_unit`, and `build_thin_lto` do not change shape. The CLI switches its
ordinary package path to this exact additive driver surface:

```text
pub fn build_package_pipelined(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    cache: CacheContext,
    reuse: UnitReuse,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
) -> PipelinedPackageBuild

pub enum PipelinedPackageBuild {
    FrontendFailed {
        diags: Diagnostics,
    },
    CodegenFailed {
        diags: Diagnostics,
        error: PackageCodegenError,
    },
    Complete(PipelinedPackageComplete),
}

pub struct PipelinedPackageComplete {
    pub units: Vec<PipelinedBuiltUnit>,
    pub diags: Diagnostics,
    pub codegen: UnitCodegen,
    object_stage: ArtifactStage,
}

pub struct PipelinedBuiltUnit {
    pub unit: String,
    pub link_libs: Vec<String>,
    pub frontend: Option<CacheOutcome>,
    object: PathBuf,
}

impl PipelinedBuiltUnit {
    pub fn object(&self) -> &Path {
        &self.object
    }
}
```

`FrontendFailed` is present exactly when `diags.has_errors()` after the complete
walk; it exposes no unit or object record. `CodegenFailed` carries clean-or-
warning diagnostics plus the setup, stale-entry, or ordinary codegen error for
this one attempt; it also exposes no unit or object record. An internal worker
panic unwinds after cleanup and is not represented by this enum. `Complete` is
the only variant carrying units. Its units and codegen results use bottom-up
DAG order, never completion order, and every `object()` denotes a complete
object while the enclosing `PipelinedPackageComplete` remains alive.
Every variant's diagnostics use file ids allocated in the borrowed
`source_map`; the caller must render them before that same map is dropped and
must never render one attempt with another attempt's map.

The function creates one `ArtifactStage::temp("align-per-unit-obj")` itself.
Atomic unique-directory creation is the exclusive claim; neither a caller nor a
concurrent build can supply or share its `unit<N>.o` namespace. A stage-creation
error is held until the frontend finishes and becomes `CodegenFailed` only if
the frontend is clean. On either failure variant the local stage is dropped
before return and no path escapes. On success it moves into the complete record
as the private `object_stage` owner. The caller retains that record through the
deterministic library union, link, and same-directory atomic executable
publication; dropping it after the link removes the stage. Copying a path out
of `object()` does not extend its validity beyond the complete record.

Every other owned value lives until the returned record or the worker consuming
it is dropped. MIR moves into a work item; it is never cloned to cross the stage
boundary. Allocation is the unique stage, work queue, result slots, and at most
`jobs - 1` background thread stacks, replacing the existing post-frontend
stage/pool allocation rather than adding a second one.

The persisted frontend and object caches keep their formats and structural
keys. A cache hit still materializes the exact CAS bytes into its private object
path. A miss still runs the same optimized LLVM pipeline and, after the
validation boundary below, publishes through the existing best-effort atomic
CAS path. Cache write failure remains non-fatal. `--cache-stats` retains its
frontend-then-codegen blocks and DAG ordering.

### Schedule

`load_units` still reads and parses the complete import graph first. Sema,
summary formation, MIR lowering, static-input resolution, and persistent
frontend publication remain serial on the calling thread; sema has not been
audited for concurrent use. Within that loop, one unit becomes ready only after
its final interface and implementation hashes, link libraries, diagnostics,
and generated static data are fixed.

For each ready unit, in DAG order:

1. Build and look up its ordinary object-cache key immediately.
2. On an object hit, retain the materialized object path and continue the next
   dependent frontend. A frontend hit plus object hit never rehydrates MIR.
3. On an object miss with freshly lowered MIR, move the MIR into the codegen
   queue and continue the next dependent frontend.
4. On an object miss with a reused frontend, record a deferred rehydration and
   continue the dependent frontend immediately. Rehydration uses sema, so it
   remains serial and runs in DAG order after the primary frontend walk while
   already-queued codegen continues in the background.

The coordinator counts as one job while frontend or rehydration is active.
Therefore at most `jobs - 1` codegen workers run beside it. After serial work
finishes, the coordinator drains the same queue as the final worker, allowing
up to `jobs` codegen operations without creating another thread. `jobs == 0`
normalizes to one as today; `-j 1` performs no stage overlap. This makes `-j`
the maximum simultaneous compiler work, not `jobs` codegen workers plus a
hidden frontend worker.

The pool starts lazily at the first lowered object miss. An all-object-hit
build creates no queue thread and performs no rehydration. The target is
initialized once on the coordinator before the first worker can enter LLVM.
Every task owns one MIR, key, unit identity, DAG index, and destination path;
workers share no LLVM context or mutable MIR.

### Validation, publication, and failure order

Speculative codegen may finish before the complete frontend verdict, but its
object remains private. Object-cache misses are published only after all of the
following have succeeded:

```text
complete frontend walk
static-publication-lock validation
PGO snapshot/validation and codegen-key setup
every deferred frontend-cache rehydration and identity comparison
```

This commit point prevents a later frontend or metadata failure from making a
speculative object visible. Persistent *frontend* entries retain their current
per-unit immediate publication; only speculative object misses wait. Once the
commit point is crossed, every successful codegen task is published even when
a sibling codegen task fails, matching the current immediate-publish behavior
for codegen siblings. A cache-write failure is ignored exactly as today. The
requested executable remains unpublished until every object succeeds and the
final link succeeds.

The observable failure precedence stays:

```text
1  complete frontend semantic diagnostics, in DAG order
2  static-publication-lock validation
3  unique object-stage creation
4  PGO snapshot read and shallow path/header validation
5  first codegen-key/lookup setup error, in DAG order
6  first stale reused frontend entry, in DAG order
7  LLVM target initialization
8  lowest DAG-index ordinary codegen failure among claimed work
9  link and executable-publication failure
```

Operations may execute early to permit overlap, but their errors are retained
and selected only by this logical order. If stage creation fails, no object
lookup or worker starts. Otherwise PGO path metadata is checked and the input is
read once with the existing missing/non-regular/unreadable/empty/short-or-bad-
magic precedence, then those bytes are snapshotted and digested before the
first object lookup, so every key and libLLVM see the exact same bytes. This is
deliberately shallow: a valid-magic profile with a corrupt or unsupported
version reaches libLLVM only on an object-cache miss and is an ordinary per-unit
codegen failure. An all-object-hit build runs no LLVM and therefore succeeds,
unchanged. Key construction proceeds in DAG order and stops logically at its
first error. Target initialization may run before the walk finishes, but its
error is reported only after every deferred rehydration has agreed. Thus a
stale entry still wins over target initialization as on the existing two-phase
path.

All setup errors are held while the frontend finishes and are reported only if
the frontend is clean. `pipeline_tests::setup_failure_order_is_total` covers
stage + unreadable/bad-magic PGO, shallow PGO + key setup, key setup + stale
entry, and stale entry + target initialization as multi-invalid pairs.
`pipeline_tests::valid_magic_profile_rejection_stays_on_miss_codegen` separately
pins a deep profile failure behind cache lookup and at the failing unit's DAG
index. PGO warnings and match counts are aggregated in DAG order and reported
only after successful codegen, unchanged.

An ordinary worker error stops new codegen claims but never truncates the
frontend walk. If frontend later fails and no internal panic occurs, in-progress
workers finish, queued work is discarded, no object miss is published, and only
the frontend diagnostics are reported. If the frontend is clean, every
deferred rehydration still runs before a speculative ordinary codegen error can
be returned. A mismatch invalidates the entry, cancels the queue, publishes no
speculative object miss, and returns
`CodegenFailed { error: StaleCacheEntry, .. }` for this attempt.

The CLI, not `build_package_pipelined`, owns retry orchestration. It creates one
`SourceMap`, calls one attempt, and formats that attempt's diagnostics while the
same map is alive. On the first attempt's stale-entry result it prints the
existing retry notice and starts exactly one new attempt with
`UnitReuse::Forbidden`, a new `SourceMap`, and a newly claimed stage. It formats
the retry diagnostics against that new map before dropping it. A successful
retry links its `Complete` result; a frontend, setup, or codegen failure on the
retry is returned normally and never retries again. The existing `All` first-
attempt / `ErrorsOnly` retry echo rule prevents successful-retry warnings from
being printed twice.

An LLVM emitter panic is deliberately outside the returned-error precedence.
No shared queue/result lock is held across the emitter, and a per-claim RAII
guard decrements the in-flight count and notifies the condition variable on
success, ordinary error, or unwind; when `std::thread::panicking()` is true the
guard also sets the cancellation flag before notifying. The coordinator joins
every remaining worker and resumes the original panic only after cleanup. It
does not replace or temporarily mutate Rust's process-global panic hook, so
concurrent compiler/library callers cannot race hook installation or
restoration. PL18 proves malformed or rejected user input reaches a diagnostic
or ordinary error rather than this internal-defect path. Background workers
and the coordinator's final-worker path use the same guarded task runner.

Before `build_package_pipelined` returns on any outcome it closes the queue,
joins every background worker, and drops the PGO snapshot after the last LLVM
consumer. A failure drops the function-owned `ArtifactStage` before returning;
a success moves it into `PipelinedPackageComplete`, which remains alive while
linking reads the objects and then removes the stage on Drop. There is no
detached compiler work after the function returns and no temporary stage after
the command returns.

### Scope

The first implementation boundary is one independently useful capability: the
ordinary non-ThinLTO `build`/`run`/`size` path, including cache on/off,
`--rt-lto`, instrument-PGO, profile-use PGO, and every `-j` value. It refactors
the existing walk through one ready-unit producer seam; it does not duplicate
the frontend.

The implementation plus the matrix owners may exceed roughly 1,000 changed
hand-written lines. It remains one capability because the consuming
coordinator is the only useful owner of the ready-unit producer seam, while the
publication commit and retry rules jointly span both. Splitting the producer,
queue, or CLI consumer would land dormant machinery and duplicate the same
ordering and cleanup proof without isolating a usable failure domain.

**Closure matrix reopened — `exclusive-artifact-ownership`.** The first API
candidate borrowed an arbitrary directory and returned public paths on both
success and failure. That missed the concurrency invariant: two callers can
borrow the same path, overwrite each other's deterministic `unit<N>.o`, and
publish the wrong bytes under a valid cache key. It also made partial paths
look consumable. The owning-result boundary above replaces that strategy: the
function alone claims the unique stage, failure variants expose no paths, and
only a complete result owns and lends usable object paths through link.

**Closure matrix reopened — `failure-precedence`.** The owning-result redesign
initially placed the bounded stale-entry retry inside the library attempt,
separating its raw diagnostics from the fresh `SourceMap` that owns their file
ids. It also grouped independently fallible setup phases and tried to convert a
worker panic without accounting for Rust's process-global panic hook. The final
boundary keeps one map per one library attempt, leaves retry and diagnostic
rendering in the CLI, totally orders shallow setup failures while leaving deep
valid-magic PGO rejection on cache-miss codegen, and treats internal panic as
cleanup-then-unwind rather than a returned diagnostic.

`check`, `check-per-unit`, `emit-mir`, `emit-llvm`, `emit-obj`, `explain-opt`,
and export-root tooling retain their existing serial or two-phase paths.
`--thin-lto` also stays unchanged in this boundary: its frontend cache is
deliberately disabled, every prelink artifact feeds one global thin-link, and
its backend already has a separate three-phase scheduler. Pipelining ThinLTO
prelink is a later measured extension only if its own frontend/prelink overlap
is material; it is not required to ship the ordinary-path win.

### Implementation closure matrix

The matrix is authoritative for item 3. One parameterized owner may close
several rows when it discriminates every listed state.

| Cell | Axis | Required behavior | Owner |
|------|------|-------------------|-------|
| PL1 | dependency readiness | a dependent frontend starts after every direct dependency summary is final, without waiting for their objects | `pipeline_tests::dependent_frontend_does_not_wait_for_dependency_codegen` over a chain and diamond |
| PL2 | frontend serialization | sema, rehydration, static resolution, and summary mutation never overlap each other | `pipeline_tests::frontend_work_is_serial` |
| PL3 | job budget | `frontend_active + codegen_active <= max(jobs, 1)` for `0`, `1`, `2`, and `N`; `-j 1` has no overlap | `pipeline_tests::job_budget_is_global` |
| PL4 | move boundary | a lowered MIR has one owner and is moved, not cloned or retained in a second package body | `pipeline_tests::queue_accepts_a_non_clone_payload` plus the producer's destructive `UnitBody::Lowered` extraction |
| PL5 | cache-state product | frontend hit/miss × object hit/miss selects no-rehydrate, enqueue-lowered, or deferred-rehydrate exactly once | `pipelined_compilation::cache_product_selects_exact_work` |
| PL6 | rehydration | deferred misses rehydrate serially in DAG order; every existing identity component is checked; the first mismatch unlinks and returns stale for the CLI's one reuse-forbidden whole-package retry | `pipelined_compilation::stale_entries_retry_once_after_speculative_work`, reusing the `unit_cache` tamper fixtures |
| PL7 | diagnostics | warnings and errors are byte-identical and DAG-ordered; a later ordinary codegen failure never hides or truncates frontend diagnostics | `pipelined_compilation::frontend_diagnostics_precede_speculative_codegen_failures` |
| PL8 | failure precedence | frontend, lock, stage, shallow PGO, first DAG key, first DAG stale entry, target initialization, lowest claimed DAG-index ordinary codegen error (including deep valid-magic profile rejection), and link form the total order above regardless of execution timing | `pipeline_tests::failure_precedence_is_deterministic`, `pipeline_tests::setup_failure_order_is_total`, and `pipeline_tests::valid_magic_profile_rejection_stays_on_miss_codegen` |
| PL9 | cancellation and Drop | frontend failure, stale entry, setup failure, ordinary codegen failure, worker panic, and normal return all close the queue and join workers; panic resumes only after notification/join without touching the global hook; the PGO snapshot outlives LLVM but not the function; a failed build drops its unique stage before return, while a complete result owns its stage through link and no command exit | `pipeline_tests::every_exit_joins_workers` and `pipeline_tests::panicking_worker_notifies_joins_then_resumes` |
| PL10 | publication commit | no object miss publishes before frontend, lock, setup, and rehydration validation; after that point successful siblings publish despite a codegen sibling failure | `pipelined_compilation::object_publication_waits_for_validation_commit` |
| PL11 | cache identity | keys, first-difference reasons, hit/miss outcomes, cache-off bypass, corruption recovery, and DAG-ordered stats equal the existing path | `pipelined_compilation::pipelined_cache_matches_two_phase_cache` |
| PL12 | static descriptors | codegen may overlap static resolution only after that unit's MIR is fixed; lock-validation failure publishes neither object nor executable | `pipelined_compilation::metadata_race_publishes_nothing` |
| PL13 | PGO product | Off/Instrument/Use use the same snapshot digest and optimized pipeline as today; all-hit Use performs shallow validation but no deep libLLVM validation, while a valid-magic malformed profile fails only on a miss; warnings and tallies remain DAG-ordered | `pipelined_compilation::pgo_modes_preserve_cache_and_diagnostics`, reusing `pgo_cache` and `pgo_sv` fixtures |
| PL14 | runtime LTO | off/on keys and object bytes remain isolated and `--rt-lto` still merges before the unit's single optimization run | `pipelined_compilation::runtime_lto_preserves_key_and_object_parity`, reusing `rt_lto` fixtures |
| PL15 | output identity | cache off/on, `-j 1`/`-j N`, cold/all-hit/private-edit, and retry produce byte-identical per-unit objects, executable, stdout, and link-library order to the existing driver | `pipelined_compilation::pipeline_is_byte_identical_to_two_phase_build` |
| PL16 | all-hit cost | an all-object-hit build starts no worker and rehydrates no MIR | `pipeline_tests::all_hit_starts_no_worker` |
| PL17 | unchanged verbs | all excluded verbs and `--thin-lto` call the pre-existing entry points and retain output | `pipelined_compilation::excluded_verbs_keep_their_existing_driver` plus the existing verb suites |
| PL18 | malformed/error input | cyclic imports, parse/sema failure, rejected HIR lowering, invalid profile, and codegen refusal return diagnostics/errors rather than reaching the internal panic path or publishing a partial executable | `pipelined_compilation::invalid_inputs_publish_no_executable` |
| PL19 | worker-independent order | reversing completion order does not change units, outcomes, PGO report, chosen error, link inputs, or bytes | `pipeline_tests::reverse_completion_preserves_every_ordered_result` |
| PL20 | performance promise | cold multi-unit build performs the same frontend and codegen counts and demonstrates actual stage overlap; wall time is lower under the same compiler/profile/cache state and `-j` budget | local `bench/build_pipeline/run.sh`, not CI |
| PL21 | legacy library projections | the ready-unit seam preserves every `PerUnitArtifact` field (`unit`, `is_entry`, MIR, summary bytes, dependency hashes, file, static descriptors, static inputs, static artifacts), every `BuiltUnit` field (`unit`, `is_entry`, summary bytes, dependency hashes, link libraries, static inputs, frontend outcome), package diagnostics, reuse state, and materialization result | mutation-verified `pipeline_tests::ready_seam_preserves_per_unit_projection` and `pipeline_tests::ready_seam_preserves_package_projection`, over cold, frontend-hit, and rehydrated units |
| PL22 | exclusive artifact availability | concurrent invocations always claim distinct stage directories; failure variants expose no unit/path and remove their stage; complete paths all exist, are immutable for their record lifetime, cannot be overwritten by a sibling build, and disappear only after the complete record drops following link | `pipeline_tests::concurrent_pipelines_own_distinct_stages` and `pipeline_tests::only_complete_results_lend_objects` |
| PL23 | retry map identity | each attempt renders diagnostics against the exact `SourceMap` that produced them; stale attempt 1 prints the notice and retries once; clean retry warnings do not repeat; retry errors use retry-map paths and never recurse | `pipelined_compilation::stale_retry_keeps_each_diagnostic_map_alive` |

Before review, the implementation author maps every applicable row to the
ready-unit producer, coordinator, worker, publication step, and a regression.
A later finding audits the whole matrix for its root-cause class rather than
patching one schedule edge.

The implementation closure pass maps the rows as follows:

| Cells | Diff owner | Regression owner |
|------|------------|------------------|
| PL1-PL2 | `walk_inner`'s single ready-unit callback runs only after the unit summary, MIR, diagnostics, static inputs, and artifacts are final; all frontend and rehydration calls remain on the coordinator | `pipeline_tests::ready_seam_preserves_per_unit_projection`, `pipeline_tests::ready_seam_preserves_package_projection`, and the measured overlap corpus |
| PL3-PL4 | `PipelineWorkers` lazily creates at most `min(jobs - 1, outstanding work)` fallible background workers and `UnitBody::take_lowered` destructively transfers MIR into one task | `pipeline_tests::job_budget_is_global`, `pipeline_tests::oversized_job_count_is_capped_by_available_work`, and `pipelined_compilation::pipeline_is_byte_identical_to_two_phase_build` |
| PL5-PL6 | the callback selects hit, lowered miss, or deferred reused miss; the coordinator materializes deferred indices in DAG order through the existing complete identity checker | `pipelined_compilation::cache_product_selects_exact_work` and `unit_cache::a_stale_entry_makes_the_cli_retry_once_and_succeed` |
| PL7-PL8 | the coordinator completes the walk before selecting retained setup/key/stale/target/ordinary errors; the single PGO file handle validates its bounded header before allocating or reading the tail, while valid-magic deep rejection remains in `emit_unit_object` | `pipeline_tests::bad_profdata_header_is_rejected_before_any_tail_read`, `pipelined_compilation::frontend_diagnostics_precede_shallow_setup_failure`, `pipelined_compilation::shallow_profile_failure_is_codegen_failure_with_no_paths`, and `pgo_sv::gate_sv2c_corrupt_profile_valid_magic_hard_errors` |
| PL9-PL10 | `PipelineClaimGuard`, `PipelineWorkers::finish`, and the post-validation publication loop own every exit, resumed unwind, and cache commit | `pipeline_tests::panicking_worker_notifies_joins_then_resumes`, `pipelined_compilation::object_publication_waits_for_validation_commit` |
| PL11-PL15 | the existing key, lookup, emitter, PGO/runtime-LTO, and link inputs are reused unchanged; only their schedule and publication point move | `cache_parallel`, `unit_cache`, `pgo`, `pgo_cache`, `pgo_sv`, `rt_lto`, and `pipelined_compilation::pipeline_is_byte_identical_to_two_phase_build` |
| PL16-PL19 | pool construction is lazy, excluded verbs never call the new API, every malformed-input variant exposes no object, and results sort by DAG index | `pipeline_tests::all_hit_starts_no_worker`, `pipelined_compilation::excluded_verbs_keep_their_existing_driver`, `pipelined_compilation::only_complete_results_lend_objects`, and `cache_parallel::parallel_dag_build_is_deterministic` |
| PL20 | `bench/build_pipeline/run.sh` alternates revisions, derives each revision's frontend/codegen invocation counts from a fresh cold-cache outcome stream, checks work/output identity, and samples coordinator/worker overlap | the local measurement below |
| PL21-PL23 | the no-hook projections remain field-complete, complete results alone own unique stages, and the CLI keeps one map per one attempt/retry | both `ready_seam_preserves_*` owners, `pipelined_compilation::concurrent_pipelines_own_distinct_stages`, `pipelined_compilation::only_complete_results_lend_objects`, and `unit_cache::a_stale_entry_makes_the_cli_retry_once_and_succeed` |

### Measurement

The benchmark uses a release `alignc`, the repository's multi-unit `pkg.db`
fixture, `ALIGNC_CACHE=off`, the same target/profile/linker, and the same `-j`
value for both revisions. It alternates baseline and pipelined builds, discards
one warm-up pair, and reports at least seven paired wall-time samples plus
frontend/codegen invocation counts and the observed overlap interval. The
acceptance condition is a lower median paired wall time with identical work
counts and output bytes. No fixed percentage becomes a correctness gate, and
the benchmark never enters `scripts/test-pr.sh`.

The 2026-08-25 implementation measurement compared optimized compilers at exact
base `79e68944` and the final item-3 candidate on x86-64 Linux, using the 14-unit
`apps/db` corpus, `ALIGNC_CACHE=off`, `--profile release`, `--no-rt-lto`, and
`-j 4`. Seven alternating pairs after a warm-up produced a 12.51 s baseline
median and 10.11 s pipelined median (19.2% lower). Separate fresh cold-cache
builds derived 14 frontend and 14 codegen invocations from each revision's
actual outcome stream, and the timed builds produced byte-identical
executables. Linux `/proc` task sampling observed the coordinator and LLVM
workers both advancing in 115 ten-millisecond buckets. No PostgreSQL service
ran; the corpus only supplied compiler work.

## Item 2a: required DB owner build-once/run-many

PR #881's first required PostgreSQL run spent roughly 80 seconds on service
setup, packages, cache restore, and the workspace build, then roughly 58 minutes
running fourteen owner suites through separate Cargo invocations. Cargo retains
the target-directory lock while a test binary runs, so the job paid the serial
sum even though the suites own independent test processes and the services are
disposable per job.

`scripts/run-db-suites.sh` selects the exact fourteen binaries from one
authoritative partition, checks the observed set in both directions, and
executes them through the bounded gate's shared concurrent runner. The local
Docker gate consumes the complete set in one invocation. CI balances the set
across four isolated service shards and aggregates their results; each four-core
runner uses two binary processes with two libtest threads. Output optimization
and test coverage are unchanged.

The first single-runner candidate proved that build-once/run-many alone was not
enough: the owner step reached 29 minutes and the job was cancelled at 30
minutes 17 seconds. The timeout stays fixed. Parallel service shards supply the
additional CPU needed to reduce wall time without weakening the detector; every
shard retains the hard 30-minute budget.

The shard closure matrix is authoritative:

| Shard | Exact owners | #881 serial time | Isolation and acceptance |
|-------|--------------|------------------:|--------------------------|
| `catalog-stream` | `q3`, `q4b`, `q5b1` | 15:17 | disposable PostgreSQL + pgvector; required-mode configuration owners also run here |
| `delivery-callbacks` | `a1`, `callbacks`, `q6`, `q5a` | 13:14 | disposable PostgreSQL + pgvector |
| `vector-static` | `vc1`, `q1`, `q5b2` | 13:58 | disposable PostgreSQL + pgvector |
| `portable-pool` | `q2`, `q4a`, `pool`, `a2` | 16:17 | disposable PostgreSQL + pgvector |

`scripts/test-db-ci-scope.sh` compares the union to the canonical fourteen-name
set, rejects duplicate membership, pins every matrix name, and proves unknown
shards fail closed. `scripts/run-gate-binaries.sh` separately rejects a missing
or extra Cargo artifact inside each shard. The required result aggregates the
matrix job, so one cancelled, failed, missing, or timed-out shard is red.

## Item 2: lld linking on ELF

`alignc` links through the system C driver (`cc`). On ELF it now additionally
tells that driver to run LLVM's `ld.lld`; on Mach-O nothing changed.

**lld, not mold.** `ld.lld` ships inside the LLVM 22 toolchain `alignc`
already requires, so selecting it adds no dependency to any build, developer
machine, or CI job — the Linux CI package list gains only `lld-22` from the
same apt.llvm.org repository the rest of the toolchain comes from. `mold` is
a separate third-party install on every host that wants the speedup, and it
has no macOS support at all, so it would buy one platform a faster link at
the cost of a second toolchain dependency and a platform-shaped fork in the
driver.

**macOS is unchanged.** Apple's linker (`ld-prime`, Xcode 15+) is already
fast, and running a second linker there would be a behavior difference for no
measured gain. Mach-O never selects lld, and an explicit `ALIGNC_LINKER=lld`
on Mach-O is a hard error rather than a silent no-op.

**Output.** The linker choice is optimization-neutral: the same objects, the
same hygiene flags (`--gc-sections`, `--as-needed`), the same per-profile
strip, the same fully optimized code. The track's "output is always fully
optimized" principle constrains the optimization level, and the linker does
not touch it. Only the link stage gets faster.

The image is not byte-identical, because two linkers lay out and prune
differently. Measured on the `pkg.db` application and a `link("z")` FFI
program: `.text` and `.rodata` came out marginally *smaller* under lld while
`.debug_str` and `.gcc_except_table` came out larger, and lld's `--as-needed`
is more precise — the `pkg.db` binary whose db code is entirely dead records
no `DT_NEEDED` for `libpq`/`libssl`/`libcrypto`/`libsqlite3`, where GNU `ld`
recorded all four. A library that is actually called is always recorded.

Two consequences for tests. No ELF test may assert an exact binary size or an
exact section layout, and **no ELF test may treat a dynamic-dependency list as
proof of what the driver passed** — `--as-needed` precision decides whether an
over-linked library is visible at all, so an over-linking regression can hide
there. What the driver asks for is asserted directly on the pure
`link_command_args` argv (`capability_linking`), with `DT_NEEDED` kept as the
corroborating check that the request actually reached a real image. Comparing
two links produced by the *same* linker on one host is unaffected by either
rule and remains fair game.

**Selection.** `align_driver`'s `select_linker` owns the policy in one place:

```text
ALIGNC_LINKER unset   ELF    ld.lld when one is found, else the system linker
ALIGNC_LINKER unset   MachO  the system linker
ALIGNC_LINKER=lld     ELF    ld.lld, or a hard error when none is found
ALIGNC_LINKER=lld     MachO  a hard error
ALIGNC_LINKER=system  any    the system linker
anything else         any    a hard error
```

Discovery is a total, deterministic order, memoized once per process:
`$LLVM_SYS_221_PREFIX/bin` (the compile-time prefix, so the linker matches
the LLVM the compiler was built against), then `llvm-config --bindir`
(versioned name first). Each step requires an **executable regular file**
named exactly `ld.lld` in that directory: that is the only name
`-fuse-ld=lld` can address, so apt's suffixed `/usr/bin/ld.lld-22` is not a
match, and a present-but-unrunnable file must not be selected — doing so
would turn the fail-open default into a fail-every-link default.

**`PATH` is never searched.** Both steps resolve an LLVM installation this
compiler is version-matched to, which is what makes `-B` safe (see below) and
keeps the answer independent of ambient environment — a conda/nix/toolbox shim
or a relative `PATH` entry can never become the project's linker.

The unset default is **fail-open**: a host without lld links exactly as it did
before, correctly and at the old speed. That is safe only because the two
linkers produce equally valid images — there is nothing for the user to act
on, so a diagnostic would be noise. An explicit request always fails loud
instead.

**Flag spelling.** The driver passes `-B<dir> -fuse-ld=lld`. `-fuse-ld=lld`
alone is not enough and is actively unsafe with GCC: `collect2` then searches
`COMPILER_PATH` and `PATH` for `ld.lld` and, when only `ld.lld-22` exists,
dies with `collect2: fatal error: cannot find 'ld'` instead of falling back.
`-B<dir>` puts the resolved LLVM `bin` directory on `COMPILER_PATH`, which
both GCC and Clang honor. Clang's `--ld-path=<abs>` would be more direct but
GCC rejects it outright, and `cc` is GCC on the Debian/Ubuntu hosts this
targets.

**Visibility.** The flags are ordinary argv on the `cc` command rather than a
mutated child environment. A failed link always names the linker that ran
(`link failed (cc exit code …, linker: lld (/usr/lib/llvm-22/bin/ld.lld))`),
and a failed lld link additionally names the `ALIGNC_LINKER=system` escape
hatch. `alignc`'s usage text documents the variable. A successful link stays
silent, matching every other stage.

**Cache identity.** Linking is not a cached stage — the unit cache stores
frontend results, objects, and ThinLTO bitcode, never the linked executable —
so no cache key changes and no entry is invalidated.

**CI fails closed.** Every Linux job (both `ci.yml` jobs, `nightly.yml`,
`release.yml`) sets `ALIGNC_LINKER=lld`, and the macOS legs set `system`. A
disappearing `lld-22` is then a red build rather than a silent return to the
slow linker, which the fail-open default would otherwise hide — the one
failure mode of fail-open is that nobody notices.

**Instrument-PGO.** The one link with an unusual shape is `--pgo-instrument`:
it appends the clang profile runtime and forces `__llvm_profile_runtime`
undefined so the archive's atexit `.profraw` writer is pulled in. Dropping
that anchor still links, and only shows up as an empty PGO corpus much later,
so it is checked directly. On the container above, `examples/hello.align`
built with `--pgo-instrument` wrote a 248-byte `.profraw` that
`llvm-profdata-22 merge` turned into 664 bytes of `.profdata` — byte-identical
counts under lld and under GNU `ld`. This matters for `release.yml`, whose PGO
training phase links the whole corpus under lld.

**Measured** on `ubuntu:24.04` (aarch64, GCC 13.3.0, GNU ld 2.42, LLD 22.1.8),
five warm-cache rebuilds each so the link stage dominates, timing the `cc`
invocation itself:

| Target | Link stage (GNU ld) | Link stage (lld) | Whole build |
|--------|--------------------|------------------|-------------|
| `apps/db` (12k lines, 17 MB image) | 288–331 ms | 56–59 ms | 1.75 s → 1.52 s |
| `examples/hello.align` | 274–340 ms | 60–74 ms | 0.81 s → 0.58 s |

The link stage is roughly 5× faster, and it was 15–35% of a warm rebuild, so
a warm rebuild lands 13–28% shorter. The saving is per link and independent
of program size in this range — the runtime staticlib dominates both links.

**Why the argv layer exists**, measured rather than argued. Adding one
unconditional `-lssl` to the driver's argv and rerunning `capability_linking`
on that same container under lld: all four `DT_NEEDED` tests still passed —
lld's `--as-needed` pruned the unreferenced library, so the over-linking was
invisible in every produced image — while both argv owners failed with the
exact extra flag named. On macOS the same mutation did trip the `DT_NEEDED`
tests, which is precisely the trap: the weaker layer looked sufficient on the
platform it was written on.
