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
| 2b | DB CI changed-function scope | Implemented — direct DB/gate and dedicated DB-production paths remain unconditional, while mixed compiler sources provision PostgreSQL only when a changed zero-context hunk or its function header names the database boundary |
| 3 | Pipelined compilation | Shipped as #884. A dependent unit's frontend starts as soon as each dependency interface summary exists while already-ready codegen runs within the same `-j` budget; validation, publication, and retry follow the ledger below |
| 3a | Shared recursive-Drop codegen | Implementing for align-llm Request 19 — emit one private pointer-based destructor per reachable Move struct instead of cloning its recursive cleanup CFG at every Drop site |
| 4 | Prebuilt optimized cache distribution | Design settled below; implementation pending — ship warmed first-party `pkg` entries with each exact native compiler (compiler-provided `core`/`std` imports have no cacheable source unit) |
| 5 | Daemon / watch mode | Keep the in-process memo alive across builds; the main lever for AI-agent edit-compile loops. `align-repl` (`docs/impl/22-repl-plan.md`) is the first consumer of this lever: it is already a long-lived process, so it realizes memo residency with no daemon machinery |
| 6 | Function-level incremental compilation | Heaviest; requires its own design ledger before any implementation |

## Background

Measured on `pkg.db`: cold per-unit codegen ~9s versus frontend ~3.4s.
Caching (items 1–4) removes re-paying, parallelism (3) and residency (5)
shorten the wall clock, and granularity (6) shrinks the unit of re-payment.

## Item 3a: shared recursive-Drop codegen

### Measured problem and contract

align-llm Request 19 supplies the first real-client owner: a 1,573-line verifier
fixture with 39 wide Move-record types and many early exits. At Align
`f57b986bc9326ba8d75dad5dbe4c6531c0f872b6` on Linux x86_64,
`alignc check` completes in 2.23 seconds and raw LLVM emission completes in 7.80
seconds, but the fixture unit's `main` contains about 1.42 million raw IR lines.
The dominant repeated shape is not an aggregate load or store: every cleanup
site receives a fresh copy of the complete recursive Drop CFG, including loops
over owned dynamic arrays. A cold optimized build remains inside one LLVM job
after minutes while its resident set grows past 800 MiB.

This item changes compiler-internal lowering only. Language ownership,
source-visible diagnostics and their order, MIR, interfaces, runtime ABI,
package ABI, allocation, generated-program effects, cleanup eligibility, field
order, active tagged-arm selection, element order, and exactly-once Drop remain
unchanged. Codegen emits one private `nounwind void(ptr)` helper for each Move
struct whose destructor is reached in a module. Each ordinary struct Drop,
replacement Drop, fixed-array element Drop, and dynamic-array element Drop
passes the exact existing storage pointer to that helper. The helper contains
the existing canonical pointer-based **iterative** Drop plan once and returns
only after all children have been released in their current order. A generated
helper never calls another compiler-generated Drop helper: nested structs and
tagged/array children stay on that helper's compiler-emitted iterative CFG, so
generated-program call-stack depth is one regardless of nominal type depth.
Its symbol is module-private and its authoritative handle is indexed only by
the module-local struct id; it is neither an interface symbol nor a cross-unit
dependency.

The running `alignc` byte hash already participates in every codegen, prelink,
and backend cache key, so changed helper emission invalidates old objects
without a cache-format or manifest-format change. Frontend cache entries remain
valid because HIR, MIR, interfaces, and their formats do not change.

### Implementation closure matrix

| Axis | Required closure | Owner |
| --- | --- | --- |
| Formation and construction | Copy structs emit no helper. Every reachable Move struct gets at most one private, defined `void(ptr)` helper, even when first reached from nested, tagged, fixed-array, or dynamic-array cleanup. A missing/out-of-range type record remains a diagnosed lowering error rather than a panic. | `align_codegen_llvm` helper inventory and malformed-id unit owners |
| Move-in, move-out, and source nulling | Moves keep the existing aggregate transfer and cleanup-bit behavior. The helper receives only the selected live storage pointer; moved or uninitialized storage remains zeroed before a possible call, so null-safe leaves stay null-safe. | existing Move struct transfer/nulling owners plus helper IR assertions |
| Normal Drop and replacement | Standalone struct Drop, reassignment, whole-field replacement, fixed Move-struct array element replacement, and dynamic Move-struct array element cleanup all call the same helper. Field and element order is byte-for-semantic identical to the former inline plan. | focused codegen IR owner; existing nested/owned-array runtime owners |
| Control exits | `if`, `match`, `else`, `?`, `map_err`, branch joins, loop back edges and breaks, return, and early error exits retain their existing cleanup guards and call the helper only on the same live paths. A terminating path manufactures no helper call. | existing ownership/control regression targets; Request 19 raw-IR call-count bound |
| Nested and tagged graphs | Nested Move structs, `Option`, `Result`, user sums, `array<string>`, `array<MoveStruct>`, handles, resources, and recursively owned record arrays retain active-arm selection, loop bounds, native thunk choice, and exact child-before-parent restoration order. Helper bodies use the existing compiler-owned iterative worklist/CFG and never call a generated Drop helper, so a 4,096-record valid acyclic graph executes with one helper stack frame rather than a type-depth call chain. | parameterized Drop-plan/codegen owners, an executable deep finite graph stack-bound owner, and runtime exactly-once controls |
| Direct, imported, generic, and function-value paths | Whole-program and per-unit compilation emit equivalent private helpers in each owning module. Generic instances follow their concrete module-local struct ids. Calls, returns, imports, and function-value ABI are unchanged. | whole/per-unit IR and executable parity owners |
| Runtime and allocation parity | Helpers allocate nothing and perform no artifact/source I/O. They call the same runtime free/handle/resource thunks with the same pointers and counts. Helper calls are `nounwind`; no unwind cleanup path is introduced. | IR call inventory and existing allocation/failpoint owners |
| Cache and artifact identity | The running compiler-byte hash invalidates every affected object/prelink/backend key; no persisted field changes. Same compiler and inputs remain byte-deterministic, including parallel per-unit builds. | cache edit/revert and deterministic-object owners |
| Resource promise | The Request 19 fixture's raw IR no longer scales with cleanup sites times the recursive Drop graph. Its optimized build completes within the consumer's per-target budget with peak memory well below the recorded 1,525,732 KiB, and output remains byte-identical. A representative small one-shot Move-record program is measured before and after for frontend/codegen work counts, wall time, peak memory, object size, and cleanup runtime; the optimization is not accepted if that unaffected path shows a material regression outside run-to-run spread. Counts come from actual compiler/cache outcomes and executed destructor counters, not an expected source/unit count. | local `bench/large_drop_codegen` pathological and unaffected controls plus align-llm `make prompt-verifier-smoke`; final consumer lane/fresh-worker proof belongs to align-llm |

The implementation boundary is one codegen capability because helper creation
and every consuming Drop site must agree in the same module. Splitting a dormant
helper producer from call-site conversion would add unreachable code without a
stable consumer and would duplicate the correctness proof.

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

## Item 4: prebuilt optimized cache distribution

### Boundary and public-contract ledger

This item makes an exact release compiler reuse work that the release runner has
already performed for the first-party package corpus. It does not add compiled
libraries, ambient package lookup, a package registry, or a binary interface
between independently built compilers. Consumers still vendor source under
`pkg/`; the ordinary source walk remains the only way a package enters a build.

The original track shorthand said “std/pkg”. That is too broad. `core` and
`std` imports are compiler-provided builtins and never become `LoadedUnit`s, so
there is no independent frontend or object-cache entry to warm for them. The
cacheable release corpus is the checked-in first-party source under
`apps/web/pkg`, `apps/jwt/pkg`, and `apps/db/pkg`. A consumer receives a hit only
for byte-identical reachable units from the same tagged source. The release
still does not embed or ambiently resolve those source trees.

| Public surface | Exact contract | Ownership, identity, and failure | Prerequisite and acceptance owner |
|---|---|---|---|
| release payload | Every native archive adds `share/align/cache/1/{cas,actions,index}` beside `alignc`, `align-repl`, and `libalign_runtime.a`; `1` is `CACHE_SCHEMA_VERSION`. Debian and Homebrew preserve that tree under the real executable directory, so the runtime path is always `<real-alignc-dir>/share/align/cache/<schema>`. Directly copying only the executable remains supported and simply has no packaged cache. | `.github/workflows/release.yml` owns creation after the final PGO-use binary exists. The archive checksum authenticates the payload as today; every manifest/key/blob is still decoded and digest-checked as untrusted input. The compiler never writes, removes, renames, or recursively scans this tree. Homebrew must suppress any cleanup/strip rewrite of the real compiler; re-keying after installation is forbidden because the published cache and archive compiler are one artifact. | Item 1 persistent cache and the final native release binary. Release layout tests enumerate the exact tree and reject staging files, unknown top-level namespaces, or writable publication into it. A generated local Homebrew formula is installed through Homebrew's real install/cleanup path on the native release runner; the installed real compiler must retain the warmed binary's SHA-256 and hit the packaged corpus. |
| enable and root selection | Unset `ALIGNC_CACHE` or `ALIGNC_CACHE=on` resolves the existing XDG writable root and additionally enables the adjacent packaged root when it exists. Empty or `off` disables both. Any other value selects only that explicit writable root and deliberately excludes the packaged root, preserving an isolated test/build cache. An unresolvable default root or an unidentifiable compiler disables both. The four measurement toggles continue to disable both. | `CacheContext::from_env()` owns two `PathBuf`s for one invocation: the existing writable primary and at most one read-only fallback. `CacheContext::at(root)` remains isolated and has no fallback. No environment variable, CLI flag, cwd lookup, source-tree search, network access, or process-global mutable state is added. | Existing cache-root matrix plus a complete unset/on/off/empty/custom × packaged present/absent × default resolvable/unresolvable owner. |
| lookup order | For both unit-frontend and codegen actions, lookup is exact key in writable primary, then exact key in packaged fallback, then producer. A packaged hit materializes/decodes directly and is not promoted into the writable root. A miss produces once and publishes only to the writable root. Primary publication therefore shadows a damaged or absent packaged entry on later processes. | Existing key equality is the authorization to reuse. Primary and packaged stores use the same codecs and lookup functions parameterized by read policy; there is no relaxed “trusted release” decoder. All returned summaries and objects retain their current owners and lifetimes; the extra allocation is one optional owned fallback path, while reads allocate the same bounded manifest/value buffers as an ordinary hit. | Parameterized frontend/codegen owners cover primary hit, fallback hit, both hit (primary wins), both miss, fallback hit after primary corrupt, and publish-after-fallback-miss. A producer counter proves a fallback hit invokes no frontend or LLVM producer. |
| slot diagnostics | If the exact action misses, `FirstDiff` consults the writable slot first and the packaged slot second. The existing fixed component precedence is unchanged. `--cache-stats` continues to print `hit`/`miss`; it does not expose store provenance. | Slot pointers remain observability-only and can never authorize a hit. A malformed/foreign packaged slot is ignored exactly like a malformed/foreign writable slot. | Owners pin primary-before-packaged selection and prove changing vendored source reports the existing `unit source`/dependency reason rather than a false hit. |
| corruption and I/O | Missing packaged root/file, permission denial, unknown format, key mismatch, or foreign schema is a clean miss. After an exact key match, malformed value bytes or a digest-bad blob print `alignc: packaged cache entry corrupt; rebuilding` at most once per process, leave the packaged bytes untouched, and fall through to production/publication. Writable corruption retains its existing unlink-and-rebuild behavior before fallback is tried. Producer, output-write, and link errors retain their current diagnostics and precedence. | The packaged tree is immutable even when filesystem permissions would allow writes. Exact-path reads may traverse installer-owned links, but every decoded key and content digest must match; runtime performs no directory walk. A packaged failure cannot fail an otherwise valid build. | Mutation owners cover every decoder stage, blob truncation/digest mismatch, unreadable paths, symlinked exact files, primary/fallback double corruption, and verify no packaged path changes. |
| compiler/cache identity | Unit keys remain `UNIT_KEY_FORMAT_VERSION=1` plus `align_interface::FORMAT_VERSION` and the running-`alignc` byte hash. Every codegen/prelink/backend key additionally gains `llvm_build_id: Hash128` immediately after the exact `llvm_version` string; implementation bumps `CACHE_KEY_FORMAT_VERSION` 3→4 and `MANIFEST_FORMAT_VERSION` 3→4 while `CACHE_SCHEMA_VERSION` stays 1. `llvm_build_id` is the loader-producer's nominal build identity, not a structural digest of all library bytes: `Hash128::of(tag || raw-id)`, where tag `0` is the ELF GNU build-id note bytes and tag `1` is the Mach-O `LC_UUID` 16-byte payload from the dynamic library containing `LLVMGetVersion`. `Hash128` is encoded as existing little-endian `lo: u64`, then `hi: u64`. No path, package-version, or release-version fallback exists. | A bounded fail-closed ELF/Mach-O parser resolves the loaded library through `dladdr`, accepts native non-UTF-8 paths, validates every offset/size/count before reading the declared header/command/note range, and memoizes the result once per process. `dli_fname`'s required terminal NUL is removed; supported OS paths cannot contain an embedded NUL, and a null path pointer is rejected. Missing symbol/path/file, unknown object/tag, absent/duplicate build id, malformed range, or I/O failure disables codegen cache lookup/publication with one note while leaving frontend reuse and uncached production available. No loader/global state is changed. Version and build id are compared before target/profile/source components; `FirstDiff::LlvmVersion` keeps its ordinal and reports `llvm version/build`. | Independent semantic→byte and byte→semantic goldens pin all three v4 key/manifest layouts and the new field order. ELF32/ELF64 × endian and Mach-O 32/64 parsing owners cover valid, every truncation, overflow, duplicate/missing ID, non-UTF-8 path, and changed-build-id/same-version. Same LLVM version with a different build id must frontend-hit and codegen-miss. Release final-layout smoke requires both compiler and LLVM identities to match. |
| warmed corpus and configuration | An empty private root is warmed once from a generated `module align_release_cache_warm` project with a trivial `main` that merges the three checked-in `pkg/` trees and imports `pkg.db`, `pkg.db.sqlite`, `pkg.db.postgres`, `pkg.db.pool`, `pkg.web`, `pkg.web.types`, `pkg.web.cookie`, `pkg.web.cors`, `pkg.web.multipart`, and `pkg.jwt`. The command is ordinary non-ThinLTO `alignc build`, `--profile release`, `--target-cpu baseline`, default runtime LTO on, PGO off. Only `actions/unit`, `actions/codegen`, their `index` slots, and referenced `cas` blobs enter the bundle. | The generated entry is training machinery, not a shipped API or source package; its exact cache entries are retained but cannot match an ordinary consumer entry. A manifest produced from the loaded-unit outcome stream records every expected first-party unit plus that one entry. The release fails if a checked-in first-party `.align` file is unreachable, any other unit is bundled, a referenced blob is absent, or an unreferenced blob remains. Descriptor/static-input application fixtures are excluded because their identities belong to consuming projects. | Release workflow owner compares source inventory, loaded outcomes, manifest inventory, and CAS references in both directions on every target. |
| applicability | On the ordinary non-ThinLTO build/run/size path, frontend entries may serve every profile/CPU/runtime-LTO/PGO choice because those inputs remain absent from `UnitKey`. Shipped codegen entries serve only the default release/baseline/runtime-LTO-on/PGO-off tuple. Other ordinary backend tuples may take a packaged frontend hit but must codegen-miss. `--thin-lto` continues to require every MIR through `build_per_unit`, so it uses neither packaged frontend entries nor packaged codegen/prelink/backend entries; item 4 adds no reusable-MIR format. | Exact existing keys and verb routing enforce every case; no option is normalized toward the warmed tuple. Package private-body edits invalidate that unit's codegen; public/interface edits also invalidate importers according to the existing structural dependency closure. Exact revert may re-hit the packaged entry. | Cartesian owner crosses package unchanged/private edit/public edit with frontend/codegen and default/dev/fast/native/no-rt-lto/PGO modes on the ordinary path, then separately proves ThinLTO performs zero packaged lookups and preserves its current ordered prelink/backend report. |
| clearing and upgrade | `alignc cache clear` removes only `cas`, `actions`, and `index` below the resolved writable root, exactly as today. It never touches packaged entries, so a later default build may still hit them. `ALIGNC_CACHE=off` is the single way to demand a genuinely cache-free build. Installing a new compiler selects its adjacent bundle and a new compiler fingerprint; stale writable entries remain unreachable under existing content keys until explicitly cleared. | User cache deletion remains bounded to its resolved root and keeps the existing symlink-safe removal. Package managers own removal of the packaged tree on uninstall/upgrade. | CLI owners prove clear → packaged hit, off → producer, custom root → no packaged hit, and uninstall/missing bundle → ordinary writable behavior. |
| source/package contract | The cache does not make first-party packages available. Imports still resolve only from the entry tree, and users still audit/update vendored source. A source tree from another tag or with any byte edit cleanly misses the affected entries. No source tree, package manifest, lockfile, registry metadata, or download operation is added to the release surface by this item. | Compiler owns import discovery; release machinery owns only derived cache bytes. Generic body slices already present inside an interface summary remain governed by the existing cache codec and are not a usable or resolvable package source tree. | Existing package-resolution negatives plus a packaged-cache smoke where absent source still yields the normal `cannot find module` diagnostic before any applicable cache lookup. |
| performance/resource evidence | Correctness requires exact eligible-unit hits and zero producer calls, not a wall-time threshold. Release evidence records archive-size delta and alternating cache-off versus empty-writable-plus-packaged-hit build time for each native artifact, with identical source, options, diagnostics, objects, executable, and stdout. | Measurements run after correctness on the final archive and do not enter the PR gate. No fixed speedup or bundle-size number is a public promise. | `bench/prebuilt_cache/run.sh` (implementation owner) and release summary artifact. |

The additive library surface is exact:

```text
align_codegen_llvm::loaded_llvm_build_id() -> Option<Hash128>
CacheContext::codegen_is_enabled(&self) -> bool
```

`loaded_llvm_build_id` owns the once-per-process loader/path/object parsing and
returns `None` on every identity failure above. `CacheContext::is_enabled`
continues to mean frontend/root availability;
`CacheContext::codegen_is_enabled` additionally requires the LLVM identity.
The existing public codegen-key builders retain their signatures and return
their existing `Result`; every production caller checks `codegen_is_enabled`
before key construction, while a direct caller under an unidentified LLVM gets
the exact error `cannot identify loaded LLVM build for codegen cache`. Lookup
then behaves as disabled and publication is a no-op, so the error never turns a
valid CLI build into a failure. Both functions borrow no caller data and return
no native handle or retained path.

There are no detail levels, verification states, user-visible unavailable
fields, retained native handles, connection-global operations, or user-visible
record ordinals in this contract. The one native text input is `dladdr`'s
borrowed NUL-terminated path; its encoding, NUL, ownership, validation, and
pre-side-effect rules are fixed in the identity row. The applicable products
are the environment matrix, lookup matrix, object-format parser matrix, and
build-configuration matrix enumerated above; their orders and first failures
are fixed there rather than delegated to an installer.

### Implementation closure

The implementation is one capability PR because the release producer and the
runtime fallback are dormant without each other: landing either alone creates
no stable consumer. Before coding, map these cells to the diff and owner tests:

| Cell | Required closure |
|---|---|
| C1 root formation | Resolve the real executable directory once; append `share/align/cache/<CACHE_SCHEMA_VERSION>` without canonicalizing or scanning; preserve every existing `ALIGNC_CACHE` branch exactly. |
| C2 frontend read | Parameterize unit-manifest lookup by writable versus immutable policy; preserve key-before-value validation, diagnostic `FileId` reattachment, stale-entry retry, construction, move-out, and return. |
| C3 codegen read | Parameterize action/CAS lookup by policy; preserve digest verification, object materialization, primary corruption unlink, and no packaged mutation. |
| C4 publication | Every miss/retry/error/early exit publishes only complete entries to the primary root; fallback hits, failures, and Drop publish nothing. |
| C5 control paths | Cover build/run/size, cache-stats, package retry, malformed source, producer error, link error, `ALIGNC_CACHE=off`, custom roots, and `cache clear`; check/emit/explain paths remain unchanged. |
| C6 artifact graph | Warm only after final PGO compiler/runtime production, copy the exact bundle into tar/Debian/Homebrew layouts, and verify every manifest/blob/source-inventory edge in both directions. |
| C7 identity parity | Whole-program/per-unit construction, generic interface serialization, runtime-LTO digest, target resolution, compiler provenance, and allocation behavior reuse existing keys/codecs; the one v4 codegen-family codec adds the loaded LLVM build id to ordinary/prelink/backend keys and nowhere else. |
| C8 output parity | Primary hit, packaged hit, cold miss, source edit, and exact revert produce byte-identical diagnostics, object bytes, link inputs, executable, and stdout for the same key. |
| C9 installed identity | Native tar, extracted Debian/Homebrew layouts, and a real local-formula Homebrew installation retain the final compiler bytes, resolve the intended adjacent bundle, identify the exact loaded LLVM build, and hit before publication; any post-warm executable mutation fails the release. |

The author-side matrix pass must point each cell to implementation and a
regression before the implementation review. A finding in root selection,
immutability, decoding, identity, or release inventory triggers a class-wide
audit across both frontend and codegen stores.

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

## Item 2b: DB CI changed-function scope

The database scope classifier keeps dependency, workflow, shared harness,
`apps/db`, `pkg_db_*` owners, `db_*` production modules, and the dedicated
static-artifact/input/runtime/query-metadata modules unconditional. For mixed
compiler source files it inspects only the zero-context changed hunks,
including Git's nearest function header, instead of searching the complete
base and head files. A marker-free edit in a dedicated module and a body-only
edit inside a PostgreSQL-named mixed-file function therefore provision the
service, as do additions, deletions, and renamed DB sources, while an
unrelated function in a monolithic mixed file no longer inherits a dormant
`pkg.db` marker elsewhere in that file. Unreadable ranges still fail closed.

The direct regression is #884: its pipeline-only changes to
`crates/align_driver/src/lib.rs` previously classified as `database-source`
because unchanged DB code shares that file, starting four service shards whose
longest took 15:35. The changed-hunk classifier returns
`required=false` for that exact implementation range. Synthetic owners pin
both directions in one monolithic source: an unrelated function edit skips,
and a marker-free body edit under `fn postgres_owner` provisions PostgreSQL.
A dedicated `db_prepare_native.rs::pq_text` analogue additionally proves that
a generic-named, marker-free helper edit remains in service scope by path.

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
