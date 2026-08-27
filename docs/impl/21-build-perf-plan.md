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
| 3a | Shared recursive-Drop codegen | Implemented for align-llm Request 19 — one private pointer-based destructor per Move struct reached as a Drop-site root replaces cloned recursive cleanup CFGs; merge and consumer lane restoration remain |
| 4 | Prebuilt optimized cache distribution | Shipped as #893 — exact native releases carry an adjacent immutable cache warmed from 19 first-party `pkg` modules plus one generated entry unit (20 total); compiler-provided `core`/`std` imports have no cacheable source unit |
| 5 | Foreground watch builds | Design settled below; implementation pending. `align-repl` (`docs/impl/22-repl-plan.md`) already realizes memo residency for interactive sessions; `alignc build FILE --watch` extends the same explicit long-lived-process model to editor and AI-agent loops without a detached daemon |
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
struct reached as a Drop-site root in a module. Each ordinary struct Drop,
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
| Formation and construction | Copy structs emit no helper. Every Move struct reached as a Drop-site root gets at most one private, defined `void(ptr)` helper, including roots reached from fixed-array and dynamic-array cleanup; nested and tagged children stay inside that root helper's iterative CFG. A missing/out-of-range type record remains a diagnosed lowering error rather than a panic. | `align_codegen_llvm` helper inventory and malformed-id unit owners |
| Move-in, move-out, and source nulling | Moves keep the existing aggregate transfer and cleanup-bit behavior. The helper receives only the selected live storage pointer; moved or uninitialized storage remains zeroed before a possible call, so null-safe leaves stay null-safe. | existing Move struct transfer/nulling owners plus helper IR assertions |
| Normal Drop and replacement | Standalone struct Drop and reassignment, fixed Move-struct array Drop and element replacement, and dynamic Move-struct array element cleanup all call the same helper. Field and element order is semantically identical to the former inline plan. | focused codegen IR owner; existing nested/owned-array runtime owners |
| Control exits | `if`, `match`, `else`, `?`, `map_err`, branch joins, loop back edges and breaks, return, and early error exits retain their existing cleanup guards and call the helper only on the same live paths. A terminating path manufactures no helper call. | existing ownership/control regression targets; Request 19 raw-IR call-count bound |
| Nested and tagged graphs | Nested Move structs, `Option`, `Result`, user sums, `array<string>`, `array<MoveStruct>`, handles, resources, and recursively owned record arrays retain active-arm selection, loop bounds, native thunk choice, and exact child-before-parent restoration order. Helper bodies use the existing compiler-owned iterative worklist/CFG and never call a generated Drop helper, so a 4,096-record valid acyclic graph executes with one helper stack frame rather than a type-depth call chain. | parameterized Drop-plan/codegen owners, an executable deep finite graph stack-bound owner, and runtime exactly-once controls |
| Direct, imported, generic, and function-value paths | Whole-program and per-unit compilation emit equivalent private helpers in each owning module. Generic instances follow their concrete module-local struct ids. Calls, returns, imports, and function-value ABI are unchanged. | whole/per-unit IR and executable parity owners |
| Runtime and allocation parity | Helpers allocate nothing and perform no artifact/source I/O. They call the same runtime free/handle/resource thunks with the same pointers and counts. Helper calls are `nounwind`; no unwind cleanup path is introduced. | IR call inventory and existing allocation/failpoint owners |
| Cache and artifact identity | The running compiler-byte hash invalidates every affected object/prelink/backend key; no persisted field changes. Same compiler and inputs remain byte-deterministic, including parallel per-unit builds. | cache edit/revert and deterministic-object owners |
| Resource promise | The Request 19 fixture's raw IR no longer scales with cleanup sites times the recursive Drop graph. Its optimized build completes within the consumer's per-target budget with peak memory well below the recorded 1,525,732 KiB, and output remains byte-identical. A representative small one-shot Move-record program is measured before and after for frontend/codegen work counts, wall time, peak memory, object size, and cleanup runtime; the optimization is not accepted if that unaffected path shows a material regression outside run-to-run spread. Counts come from actual compiler/cache outcomes and executed destructor counters, not an expected source/unit count. | local `bench/large_drop_codegen` pathological and unaffected controls plus align-llm `make prompt-verifier-smoke`; final consumer lane/fresh-worker proof belongs to align-llm |

### Candidate evidence

Measured on 2026-08-26 on Linux x86_64 with release compilers. The Request 19
raw-IR lens fell from 1,517,324 lines / 113.6 MB to 109,992 lines / 5.96 MB.
Its three-unit default-runtime-LTO cold build fell from 471.074 seconds with an
observed resident set above 832,704 KiB to 13.555 seconds and 266,400 KiB peak
RSS. Fresh cache outcomes reported exactly three frontend misses and three
codegen misses. The resulting executable prints the exact required PASS line.

The one-shot Move-record control retained one frontend miss, one codegen miss,
a 1,240-byte release object, and an executed allocation/free count of 1 / 1.
Its single-run wall/RSS observation changed from 0.283 seconds / 87,144 KiB to
0.294 seconds / 86,852 KiB, within run-to-run noise. The focused codegen owner
pins one private helper across functions and all struct-root Drop-site shapes;
the 4,096-record executable owner pins stack-bounded generated cleanup. Seven
separate executions of the control reported 0.001 seconds in both revisions;
peak RSS was 12,112–12,380 KiB before and 12,380 KiB after.

### Implementation review closure

| Finding | Class-wide closure |
| --- | --- |
| P1: the extracted iterative leaf dispatcher omitted four admitted dynamic aggregate-array field types that the deleted inline struct path freed | Add `DynVecArray`, `DynMaskArray`, `DynFixedArray`, and `DynFixedStructArray` to the canonical flat-buffer free arm. One helper owner places all four in the same Move struct and requires four runtime frees, so another sibling omission fails the owner. |
| P2: the benchmark measured compiler build time but only captured the generated program's output | Measure the executable separately for every baseline/candidate case and report `cleanup_timing` beside `build_timing`; retain the independent output and destructor-count checks. |

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
| lookup order | For both unit-frontend and codegen actions, lookup is exact key in writable primary, then exact key in packaged fallback, then producer. A packaged hit materializes/decodes directly and is not promoted into the writable root. An ordinary exact miss produces once and publishes only to the writable root. A later rehydration mismatch revokes that key's authorization instead: before invalidation the writable root atomically gains the zero-byte marker `rejected/unit/<UnitKey.full_digest()>`; marker existence is the fail-closed state and its contents are never decoded. The writable action is then unlinked and the packaged action remains immutable. Every candidate primary or packaged hit checks the primary marker again after complete value validation; a rejection that races the read therefore wins, while a marker installed after that final check orders after the successful lookup. A concurrent publisher likewise checks after publication, so its bytes are either removed by that check or by rejection's marker-before-invalidation sequence and never regain authorization. Compiler/key-format identity already participates in the digest, so widening the key or installing a new compiler selects a different marker. | Existing key equality authorizes initial reuse only until complete rehydration verification rejects it. The marker path is primary-root state, uses no ambient input, and a malformed file/directory/symlink at that exact path also disables the key (performance loss only). Primary and packaged stores otherwise use the same codecs and lookup functions parameterized by read policy; there is no relaxed “trusted release” decoder. Marker publication is guarded in-process until persistence succeeds; at most 1,024 failed exact markers are retained, and the rejected-next failure disables all unit-cache reuse/publication for that process. All returned summaries and objects retain their current owners and lifetimes; the extra allocation is one optional owned fallback path, while reads allocate the same bounded manifest/value buffers as an ordinary hit. | Parameterized frontend/codegen owners cover primary hit, fallback hit, both hit (primary wins), both miss, fallback hit after primary corrupt, publish-after-ordinary-fallback-miss, and rehydration rejection from each provenance. A producer counter proves a valid fallback hit invokes no frontend or LLVM producer; rejection owners prove the marker survives another build, fallback is not served, no action is republished under the rejected key, and both read-before-reject and publish-before-reject interleavings converge fail-closed. The process-state owner pins 1,024 retained failures and the rejected-next global fail-closed transition. |
| slot diagnostics | If the exact action misses, `FirstDiff` consults the writable slot first and the packaged slot second. The existing fixed component precedence is unchanged. `--cache-stats` continues to print `hit`/`miss`; it does not expose store provenance. | Slot pointers remain observability-only and can never authorize a hit. A malformed/foreign packaged slot is ignored exactly like a malformed/foreign writable slot. | Owners pin primary-before-packaged selection and prove changing vendored source reports the existing `unit source`/dependency reason rather than a false hit. |
| corruption and I/O | Missing packaged root/file, permission denial, non-regular target, unknown format, key mismatch, foreign schema, or any action/index manifest larger than 64 MiB is a clean miss. Any cache CAS object may be at most 256 MiB. Each exact path is opened nonblocking before its followed metadata is inspected; the observed target must be a regular file whose length fits the applicable bound. Manifests are then read through a bound-plus-one reader. Objects are streamed once through a fixed 64 KiB buffer into a private output sibling while the existing `Hash128` is computed incrementally; only an exact-length, exact-digest stream is renamed over the requested output. Growth, shrinkage, excess bytes, special files, and every read/write/rename failure are clean misses. After an exact key match, malformed value bytes or a digest-bad blob print `alignc: packaged cache entry corrupt; rebuilding` at most once per process, leave the packaged bytes untouched, and fall through to production/publication. Writable corruption retains its existing unlink-and-rebuild behavior before fallback is tried. Producer, output-write, and link errors retain their current diagnostics and precedence. | The packaged tree is immutable even when filesystem permissions would allow writes. Exact-path reads may traverse installer-owned links, but never block opening a FIFO/device and never allocate in proportion to an unchecked length. Every decoded key and content digest must match; runtime performs no directory walk. The incremental hash is byte-identical to `Hash128::of` for every chunk boundary and adds no second CAS identity. A packaged failure cannot fail an otherwise valid build. | Mutation owners cover every decoder stage, exact accepted/rejected manifest and object bounds, metadata/read length disagreement, FIFO/device targets, symlinked exact regular files, object chunk boundaries, blob truncation/digest mismatch, primary/fallback double corruption, staging cleanup, and verify no packaged path changes. |
| compiler/cache identity | Unit keys remain `UNIT_KEY_FORMAT_VERSION=1` plus `align_interface::FORMAT_VERSION` and the running-`alignc` byte hash. Every codegen/prelink/backend key additionally gains `llvm_build_id: Hash128` immediately after the exact `llvm_version` string; implementation bumps `CACHE_KEY_FORMAT_VERSION` 3→4 and `MANIFEST_FORMAT_VERSION` 3→4 while `CACHE_SCHEMA_VERSION` stays 1. `llvm_build_id` is the loader-producer's nominal build identity, not a structural digest of all library bytes: `Hash128::of(tag || raw-id)`, where tag `0` is the ELF GNU build-id note bytes and tag `1` is the Mach-O `LC_UUID` 16-byte payload from the mapped dynamic image containing `LLVMGetVersion`. `Hash128` is encoded as existing little-endian `lo: u64`, then `hi: u64`. No pathname reopen, package-version, or release-version fallback exists. | `dladdr` supplies only the mapped image base for the linked symbol. Linux matches that base through `dl_iterate_phdr`, accepts at most 1,024 loader-owned program headers and 1 MiB total `PT_NOTE` bytes, and reads only mapped note ranges whose file size fits their mapped size. macOS matches the base against at most 4,096 dyld images and parses at most 4,096 loader-owned commands occupying at most 16 MiB after the mapped Mach header. Every count, sum, address, command size, note size, and duplicate/absent ID is checked before slice formation or allocation; accepted-limit and rejected-next owners pin each resource bound. The identity therefore remains bound to the code-producing mapped image when its on-disk pathname is replaced, performs no file I/O, changes no loader/global state, and is memoized once per process. Missing symbol/base/image, malformed metadata, unknown object/tag, or absent/duplicate build id disables codegen cache lookup/publication with one note while leaving frontend reuse and uncached production available. Version and build id are compared before target/profile/source components; `FirstDiff::LlvmVersion` keeps its ordinal and reports `llvm version/build`. | Independent semantic→byte and byte→semantic goldens pin all three v4 key/manifest layouts and the new field order. ELF note and Mach-O command owners cover both byte orders, every truncation, overflow, duplicate/missing ID, every exact resource limit/rejected next value, and changed-build-id/same-version; the current-process owner resolves the actual mapped LLVM image. Same LLVM version with a different build id must frontend-hit and codegen-miss. Release final-layout smoke requires both compiler and LLVM identities to match. |
| warmed corpus and configuration | An empty private root is warmed once from a generated `module align_release_cache_warm` project with a trivial `main` that merges the three checked-in `pkg/` trees and imports `pkg.db`, `pkg.db.sqlite`, `pkg.db.postgres`, `pkg.db.pool`, `pkg.web`, `pkg.web.types`, `pkg.web.cookie`, `pkg.web.cors`, `pkg.web.multipart`, and `pkg.jwt`. The command is ordinary non-ThinLTO `alignc build`, `--profile release`, `--target-cpu baseline`, default runtime LTO on, PGO off. Only `actions/unit`, `actions/codegen`, their `index` slots, and referenced `cas` blobs enter the bundle. | The generated entry is training machinery, not a shipped API or source package; its exact cache entries are retained but cannot match an ordinary consumer entry. A manifest produced from the loaded-unit outcome stream records every expected first-party unit plus that one entry. The release fails if a checked-in first-party `.align` file is unreachable, any other unit is bundled, a referenced blob is absent, or an unreferenced blob remains. Descriptor/static-input application fixtures are excluded because their identities belong to consuming projects. | Release workflow owner compares source inventory, loaded outcomes, manifest inventory, and CAS references in both directions on every target. |
| native link dependency closure | The warmed source corpus declares exactly `crypto`, `pq`, `sqlite3`, `ssl`, `z`, and `zstd`. A cache hit skips frontend/codegen work but never suppresses final native linking, so every installation route must retain linker inputs for that complete set. Debian records the exact package sequence `llvm-22, clang-22, libclang-rt-22-dev, libpq-dev, libsqlite3-dev, libssl-dev, zlib1g-dev, libzstd-dev`. Homebrew records `llvm@22`, `libpq`, `openssl@3`, and `zstd`, exports those keg-only library roots, and uses the macOS SDK's `libsqlite3` and `libz`. | The source declarations are authoritative; the platform table is an explicit release mapping rather than an inference from cache contents or a successful warm link on the runner. Adding/removing a corpus `link("...")` declaration without updating the table and both package routes fails the workflow owner. Dependency installation precedes every warm/layout smoke, and a hit does not alter link-error precedence. | `scripts/test-pr-workflow.sh` derives the sorted declaration set from the three source trees, pins the exact Debian dependency field and Homebrew dependency/export table, and rejects drift in either direction. Native release layout smoke supplies the resulting link step on Linux and macOS. |
| applicability | On the ordinary non-ThinLTO build/run/size path, frontend entries may serve every profile/CPU/runtime-LTO/PGO choice because those inputs remain absent from `UnitKey`. Shipped codegen entries serve only the default release/baseline/runtime-LTO-on/PGO-off tuple. Other ordinary backend tuples may take a packaged frontend hit but must codegen-miss. `--thin-lto` continues to require every MIR through `build_per_unit`, so it uses neither packaged frontend entries nor packaged codegen/prelink/backend entries; item 4 adds no reusable-MIR format. | Exact existing keys and verb routing enforce every case; no option is normalized toward the warmed tuple. Package private-body edits invalidate that unit's codegen; public/interface edits also invalidate importers according to the existing structural dependency closure. Exact revert may re-hit the packaged entry. | Cartesian owner crosses package unchanged/private edit/public edit with frontend/codegen and default/dev/fast/native/no-rt-lto/PGO modes on the ordinary path, then separately proves ThinLTO performs zero packaged lookups and preserves its current ordered prelink/backend report. |
| clearing and upgrade | `alignc cache clear` removes only `cas`, `actions`, and `index` below the resolved writable root, exactly as today. It never touches packaged entries or safety-owned `rejected`, so a rejected incomplete key cannot be re-enabled by an optimization-state clear. `ALIGNC_CACHE=off` is the single way to demand a genuinely cache-free build. Installing a new compiler selects its adjacent bundle and new compiler/key digests; stale writable entries and rejection markers remain unreachable until the cache root is manually removed. | User cache deletion remains bounded to its three resolved optimization subtrees and keeps the existing symlink-safe removal. Rejection markers belong to compiler safety rather than reusable artifacts. Package managers own removal of the packaged tree on uninstall/upgrade. | CLI owners prove clear → packaged hit for an ordinary key, clear → continued miss for a rejected key, off → producer, custom root → no packaged hit, and uninstall/missing bundle → ordinary writable behavior. |
| source/package contract | The cache does not make first-party packages available. Imports still resolve only from the entry tree, and users still audit/update vendored source. A source tree from another tag or with any byte edit cleanly misses the affected entries. No source tree, package manifest, lockfile, registry metadata, or download operation is added to the release surface by this item. | Compiler owns import discovery; release machinery owns only derived cache bytes. Generic body slices already present inside an interface summary remain governed by the existing cache codec and are not a usable or resolvable package source tree. | Existing package-resolution negatives plus a packaged-cache smoke where absent source still yields the normal `cannot find module` diagnostic before any applicable cache lookup. |
| performance/resource evidence | Correctness requires exact eligible-unit hits and zero producer calls, not a wall-time threshold. Release evidence records archive-size delta and alternating cache-off versus empty-writable-plus-packaged-hit build time for each native artifact, with identical source, options, diagnostics, objects, executable, and stdout. | Measurements run after correctness on the final archive and do not enter the PR gate. No fixed speedup or bundle-size number is a public promise. | `bench/prebuilt_cache/run.sh` (implementation owner) and release summary artifact. |

The additive library surface is exact:

```text
align_codegen_llvm::loaded_llvm_build_id() -> Option<Hash128>
CacheContext::codegen_is_enabled(&self) -> bool
Hash128Stream::for_len(len: usize) -> Hash128Stream
Hash128Stream::update(&mut self, bytes: &[u8]) -> bool
Hash128Stream::finish(self) -> Option<Hash128>
WyHashStream::for_len(seed: u64, len: usize) -> WyHashStream
WyHashStream::update(&mut self, bytes: &[u8]) -> bool
WyHashStream::finish(self) -> Option<u64>
```

`loaded_llvm_build_id` owns the once-per-process mapped-loader metadata parsing and
returns `None` on every identity failure above. `CacheContext::is_enabled`
continues to mean frontend/root availability;
`CacheContext::codegen_is_enabled` additionally requires the LLVM identity.
The existing public codegen-key builders retain their signatures and return
their existing `Result`; every production caller checks `codegen_is_enabled`
before key construction, while a direct caller under an unidentified LLVM gets
the exact error `cannot identify loaded LLVM build for codegen cache`. Lookup
then behaves as disabled and publication is a no-op, so the error never turns a
valid CLI build into a failure. Both functions borrow no caller data and return
no native handle or retained path. `Hash128Stream` owns two fixed-size wyhash
states and requires exactly the length supplied at construction; `update`
returns `false` before consuming bytes that exceed it, and `finish` returns
`None` until exactly that many bytes were consumed. It allocates nothing,
borrows each update only for the call, and returns the same digest as
`Hash128::of(concatenated_updates)`. Its two lanes are the identically shaped
`WyHashStream` contract with an explicit seed and `wyhash`-identical result.

There are no detail levels, verification states, user-visible unavailable
fields, native text inputs, retained native handles, connection-global operations,
or user-visible record ordinals in this contract. `dladdr`, `dl_iterate_phdr`,
and dyld lend process-owned mapped-image metadata only for the duration of their
calls; the identity row fixes its ownership, bounds, validation, and
pre-side-effect rules. The applicable products
are the environment matrix, lookup matrix, object-format parser matrix, and
build-configuration matrix enumerated above; their orders and first failures
are fixed there rather than delegated to an installer.

### Implementation closure

The implementation is one capability PR because the release producer and the
runtime fallback are dormant without each other: landing either alone creates
no stable consumer. Before coding, map these cells to the diff and owner tests:

| Cell | Required closure | Implementation and owner |
|---|---|---|
| C1 root formation | Resolve the real executable directory once; append `share/align/cache/<CACHE_SCHEMA_VERSION>` without canonicalizing or scanning; preserve every existing `ALIGNC_CACHE` branch exactly. | `CacheContext::from_env_when`, `packaged_cache_root`, and the copied-compiler integration owner cover default/unset, custom, and off formation; `CacheContext::at` remains isolated. |
| C2 frontend read | Parameterize unit-manifest lookup by writable versus immutable policy; preserve key-before-value validation, diagnostic `FileId` reattachment, stale-entry retry, construction, move-out, and return. A rehydration mismatch is proof that the key does not authorize any recomputed value: publish its primary-root rejection marker before unlinking writable state, leave packaged state immutable, retry uncached, and suppress fallback lookup and frontend publication under that key across later processes. Every primary or packaged hit linearizes only after complete value validation and a final marker check, so a rejection racing the read wins. Marker publication failure keeps the current build rejected and cannot authorize a write in that process. | `unit_cache::{reject,is_rejected,authorize_lookup,lookup_with_policy,publish}` plus `CacheContext::lookup_unit`; unit-cache codec/tamper owners and `copied_compiler_reads_adjacent_packaged_frontend_and_codegen_cache` cover replay and cross-process ownership. The rehydration-rejection owners prove the marker, absent action, repeated miss, publication suppression, and the read-before-reject interleaving; clear-cache coverage proves the marker persists. |
| C3 codegen read | Parameterize action/CAS lookup by policy; preserve digest verification, object materialization, primary corruption unlink, packaged unavailable-I/O clean miss, and no packaged mutation. Every action/index path uses the bounded regular-file reader. Every CAS path uses the bounded streaming materializer and publishes its private output sibling only after exact length and digest validation. | `try_hit`/`materialize_blob` take `ReadPolicy`; cache owners cover packaged hit, exact manifest/object limits and rejected-next values, FIFO/device and length-change misses, primary precedence/corruption fallback, unavailable packaged blobs, digest-bad packaged corruption immutability, staging cleanup, and zero-producer hit. `Hash128Stream` chunk-partition owners pin digest parity. |
| C4 publication | Every ordinary miss publishes only a complete entry to the primary root; fallback hits, rehydration rejections, failures, early exits, and Drop publish nothing. A recomputed result after rejection may proceed through the uncached retry but can never regain authorization from the rejected key. Publication checks before writing and again after its final attempted rename; a marker racing between them makes the publisher invalidate its bytes, while a rejection that starts after the final check installs the marker before performing the same invalidation. | `CacheContext::{publish_after_miss,codegen}` and the existing unit publisher retain `root()` as the only destination; packaged-corruption and copied-compiler owners prove no promotion. The publish-before-reject interleaving owner proves that the symmetric finalization leaves the marker authoritative and no action reachable after both operations finish. |
| C5 control paths | Cover build/run/size, cache-stats, package retry, malformed source, producer error, link error, `ALIGNC_CACHE=off`, custom roots, and `cache clear`; check/emit/explain paths remain unchanged. | The ordinary CLI keeps its one `build_package_to` path and gates backend reporting with `codegen_is_enabled`; existing retry/failure owners compose with the copied-compiler and `verify-prebuilt-cache-layout.sh` clear/custom/off/source-negative checks. ThinLTO and inspection verbs retain their prior stores/routes. |
| C6 artifact graph | Warm only after final PGO compiler/runtime production, copy the exact bundle into tar/Debian/Homebrew layouts, and verify every manifest/blob/source-inventory edge in both directions. Derive the complete source-level native-link declaration set and require an explicit package mapping before any route is accepted. | `build-prebuilt-cache.sh` and `prebuilt-cache-inventory.py` own the exact 20-unit source/outcome/action/index/CAS graph; `release.yml` invokes them after PGO phase 3, archives `share`, carries it into Debian and Homebrew layouts, and includes the complete Debian link dependencies. `scripts/test-pr-workflow.sh` owns declaration-to-Debian/Homebrew parity. |
| C7 identity parity | Whole-program/per-unit construction, generic interface serialization, runtime-LTO digest, target resolution, compiler provenance, and allocation behavior reuse existing keys/codecs; the one v4 codegen-family codec adds the mapped LLVM build id to ordinary/prelink/backend keys and nowhere else. | `loaded_llvm_build_id`, the three v4 bidirectional goldens, the loader-base binding owner, and the bounded ELF-note/Mach-command parser matrix close the new field without pathname I/O; existing key builders supply all other components. The checked-HIR function-payload owner closes the first-party `pkg.web` per-unit prerequisite exposed by the corpus. |
| C8 output parity | Primary hit, packaged hit, cold miss, source edit, and exact revert produce byte-identical diagnostics, object bytes, link inputs, executable, and stdout for the same key. | Existing edit/revert/cache matrices retain source invalidation coverage; `bench/prebuilt_cache/run.sh` compares disabled versus packaged-hit diagnostics, every emitted object, executable, and stdout before measuring. |
| C9 installed identity | Native tar, extracted Debian/Homebrew layouts, and a real local-formula Homebrew installation retain the final compiler bytes, resolve the intended adjacent bundle, identify the exact loaded LLVM build, and hit before publication; any post-warm executable mutation fails the release. | `verify-prebuilt-cache-layout.sh` hashes compiler/cache before and after real hits; the native release job tests staging, installs a generated local formula through Homebrew, checks the installed compiler hash, and reruns the full layout owner. Workflow/formula structural owners pin tar, Debian, `skip_clean`, and `libexec/share`. |
| C10 resource closure | Frontend actions/indexes, codegen actions/indexes, and CAS objects all reject non-regular, over-limit, short, growing, or otherwise unreadable exact targets without unbounded allocation or a blocking special-file read. Every private materialization is removed on miss, corruption, write failure, rename failure, and unwind; only a verified complete object moves into the caller path. | The shared bounded-open/read helpers cover both frontend and codegen stores; `Hash128Stream` covers every object chunk boundary. Parameterized unit/codegen owners exercise exact 64 MiB/256 MiB limits without allocating those payloads by using metadata/sparse files, plus FIFO/device, shrink/grow, and staging cleanup cases. |

The author-side matrix pass must point each cell to implementation and a
regression before the implementation review. A finding in root selection,
immutability, decoding, identity, or release inventory triggers a class-wide
audit across both frontend and codegen stores.

The second implementation review reopened the matrix on
`release-native-dependency-and-untrusted-read-bounds`: the original artifact
graph cell proved cache completeness but did not close the still-live native
link step for every packaged corpus declaration, while the read cells bounded
decoded fields but not the filesystem bytes allocated before decoding. C6 and
the new C10 are the replacement boundary; release mapping and byte acquisition
must now close independently before an installed hit is accepted.

The next full-diff review reopened the matrix again on
`rehydration-rejection-key-authorization`: C4 had incorrectly treated an
independently recomputed value as sufficient to republish after verification
had proved the key incomplete. The replacement boundary separates value
validity from key authorization. Recomputed bytes may finish the current
uncached retry, but no value can be stored under that rejected key; immutable
fallbacks can therefore cost repeated verification, never create a false hit.

The following full-diff review reopened the matrix on
`rejection-linearization-after-value-validation`: C2 checked the marker before
reading an action but did not linearize a validated primary or packaged hit
against a rejection that raced that read. Every candidate hit now performs the
same primary-marker check after complete decoding. C4 is the symmetric write
boundary: its post-publication check and `reject`'s marker-before-invalidation
order ensure either interleaving leaves the marker authoritative and no action
reachable after both operations finish.

## Item 5: foreground watch builds

### Boundary and public-contract ledger

Item 5 adds one explicit foreground mode:

```text
alignc build FILE.align --watch
```

It is not a detached daemon, socket protocol, project server, or hidden
background process. The invoking terminal owns one compiler process, its
in-process memo, and its lifetime. This is the same residency mechanism already
used by `align-repl`, applied to the ordinary `build` path. A later language
server may consume the input-observation surface below, but it does not acquire
an IPC contract from this item.

The public contract is:

| id | Surface | Exact contract | Ownership, allocation, and errors | Owner |
|---|---|---|---|---|
| W1 | CLI | `alignc build FILE.align --watch`. `--watch` is a valueless, idempotent flag accepted only by `build`; every existing `build` flag and environment setting keeps its current meaning. `run`, `size`, inspection verbs, `cache clear`, and `db` reject it before reading source. `build --help` describes it as `rebuild on compiler-observed file changes; other toolchain/library changes need another observed change or restart`. There is no configurable interval/debounce, daemon, socket, or background option. | Arguments, resolved target/profile/jobs/cache/linker choice, PGO mode, current working directory, and external search environment are fixed at startup. The running compiler and loaded LLVM image are process-fixed identities. Runtime/profile archives, linker/tool executables, system libraries, and capability/user `-lNAME` results are explicit trigger-excluded inputs: their paths, metadata, and bytes are not watched, so replacing one alone starts no revision. The next compiler-observed change performs an ordinary new build/link step and may consume the replacement under the fixed configuration; restart forces that step immediately. Invalid combinations exit 1 before watcher creation. No new environment variable. | CLI parser/help table and real-binary invalid-combination/non-impact owner. |
| W2 | Attempt protocol | Revision 1 starts immediately. Each attempt writes exactly `alignc: watch: revision N started` to stderr before reading an input, then preserves the ordinary build's diagnostic/cache-stat meaning and success-line meaning. After successful atomic publication, one fallible stdout owner writes `alignc: built executable: ENCODED_PATH`. Before either terminal marker, the transcript flushes stdout and stderr in that order. While holding the sole stderr lock, it then performs one unbuffered write of the complete <=128-byte line `alignc: watch: revision N ready\n` or `alignc: watch: revision N failed\n`; `EINTR` retries, and a short/failed write exits 1 without another marker. On a pipe this write is below `PIPE_BUF` and atomic. `N` starts at 1 and increments by one; exhaustion is a watcher error. Any prior channel write/flush failure emits W9's error when stderr remains available and exits without a terminal marker; stderr failure exits without recursion. Every watch-mode stdout/stderr write is fallible. | All filesystem paths use `WatchPath`: percent-encode each raw Unix path byte outside `[A-Za-z0-9._/-]` as uppercase `%HH` (including `%` as `%25`). Dynamic watcher-error messages use `WatchText`, which retains printable ASCII except `%` and percent-encodes every other UTF-8 byte. `WatchPath` and admitted `WatchText` values are reversible single-line encodings. `WatchText` accepts at most 16,384 input bytes; a longer message becomes the static replacement `message exceeds 16384-byte limit`. A private `WatchTranscript` routes in-process diagnostics, cache stats, success, and markers. Every spawned `cc`/linker/strip stdout and stderr is piped through W13, never inherited. Automation accepts only a complete terminal line and never infers readiness from quiet time. | Pipe-backed transcript owner covers initial success/failure, two rebuilds, both-channel write/partial-write/flush failures, atomic marker write/short-write/`EINTR`, success-line visibility, every-byte path round trips, accepted/rejected-next messages, dynamic newlines, arbitrary child bytes, and injected marker-shaped paths/messages. |
| W3 | Publication | Each attempt uses the existing applicable ordinary or ThinLTO per-unit pipeline and its current retry rule. Before a successful candidate may link/publish, `finalize_watch_inputs(inputs, Some(output))` first resolves the current W6 graph and streams every logical input again to form the same total W5 semantic state. It compares that current state plus its before/opened-leaf/after identities with every original W4 read-time state and node. Any content hash/length, missing/nonregular/unreadable state, or path identity disagreement sets `changed_during_attempt`; such a candidate emits exactly `alignc: watch: inputs changed during revision`, produces `failed`, schedules a comparison, and never links. Only a stable candidate compares the fixed cwd/output lookup with every original and final directory-entry node. Exact absolute lexical equality rejects even a missing slot; an existing output whose nofollow `(device,inode)` matches any existing node or opened leaf also rejects, deliberately including distinct hard links and case-/Unicode-folded names. The lowest platform-byte-sorted alias emits exactly `alignc: watch: output 'ENCODED_OUT' aliases observed input 'ENCODED_INPUT'`, produces `failed`, and runs no link or rename. Other candidates retain the existing link and same-directory atomic rename. | `ENCODED_OUT` is `WatchPath` over the absolute startup-cwd path for the existing `stem(FILE)` output. Final semantic reads are nonblocking, use the W8 streaming buffer, and replace the returned set's states with the final snapshot. Retaining original read-time identities until that revalidation means a node removed/retargeted after supplying bytes cannot disappear; hashing the final target means an in-place same-inode write cannot publish stale or torn observed bytes. Mutation after the final graph/hash/alias snapshot retains W12's last-writer rule. Each attempt owns and drops private object/publish stages; other link/publication errors remain ordinary failures and preserve last-good output. | Parameterized same-resource/state owner covers every W5 transition and direct slot, traversed node, parent alias, leaf symlink, folded lookup, hard link, output absent, in-place write, truncate/extend, and removal/retarget barriers before/open/after both original and final reads. Real-binary owner runs last-good output after later failure. |
| W4 | Observed build inputs | The ordinary route uses `align_driver::build_path_pipelined_observed`, which owns the entry read and one call to the same private pipeline implementation as `build_package_pipelined`. The ThinLTO route uses `align_driver::build_path_per_unit_observed`, which owns the same read/observation front half and returns the existing `PerUnitWalk` consumed by the current ThinLTO CLI branch. Before retaining or issuing a filesystem call for any input, both form its absolute lexical path and apply W8 path validation with `ObservationFailed`. Otherwise they return the complete ordered `BuildInputSet` even when entry read or frontend fails. One call assigns encounter ordinals in this exact producer order: entry source first; newly discovered user modules in the existing breadth-first importer and source-import order; each file-backed static source and checked-metadata path at its existing descriptor-validation read in unit/source order; then the ordinary route's `--pgo-use` file at its existing snapshot read. Inline static sources and static paths rejected before root/path validation add no target; their owning Align source is already watched and is the only input that can repair that error. Missing paths are not canonicalized. W1's external link-time resources are trigger-excluded, not observation targets, even when `LIBRARY_PATH` names a project directory. Under watch, all path fields passed to the diagnostic renderer use W2 `WatchPath`; non-path diagnostic meaning/order remains unchanged. | For each admissible read, the observer privately retains the bounded W6 graph immediately before open, the opened leaf's `(device,inode)` from `fstat`, and the graph immediately after open/read. The union is the read-time evidence of bytes actually consumed even if a link is removed/retargeted before W3. Its distinct key is `(logical input path, access path, node kind plus identity or raw target)`, so different identities at one path and both retry attempts survive deduplication. It retains at most 131,072 keys across one attempt/retry; the rejected next key is `ObservationFailed`. Each public record owns one bounded `PathBuf` and semantic state; regular state owns only `Hash128`/length. External resource mutation follows W1 and never authorizes a cache artifact. | Parameterized library owner runs entry/import/static/metadata present, missing, replacement during every original/final read barrier, invalid-path exclusion, length/NUL boundaries, invalid UTF-8, producer/duplicate/retry order, read-time identity, and arbitrary non-NUL path bytes through ordinary/ThinLTO; ordinary also covers PGO. Trigger owner replaces each external resource class, proves no revision occurs from that alone, then proves a source edit and a restart each use the replacement exactly as the matching one-shot build. |
| W5 | Input, finalization, and repair records | `#[derive(Clone, PartialEq, Eq)] pub struct BuildInput { path: PathBuf, state: BuildInputState }`; `impl BuildInput { pub fn path(&self) -> &Path; pub fn state(&self) -> BuildInputState }`; `#[derive(Clone, Copy, PartialEq, Eq)] pub enum BuildInputState { Missing, Regular { content_hash: Hash128, len: u64 }, NonRegular, Unreadable }`; `pub struct BuildInputSet`; `impl BuildInputSet { pub fn inputs(&self) -> &[BuildInput]; pub fn changed_during_attempt(&self) -> bool }`; `#[derive(PartialEq, Eq)] pub struct WatchRepairDependency`; `impl WatchRepairDependency { pub fn path(&self) -> &Path }`; `pub struct FinalizedWatchInputs`; `impl FinalizedWatchInputs { pub fn inputs(&self) -> &BuildInputSet; pub fn alias_index(&self) -> Option<usize>; pub fn repair_dependency(&self) -> Option<&WatchRepairDependency>; pub fn into_parts(self) -> (BuildInputSet, Option<WatchRepairDependency>) }`; `pub struct BuildInputTopologyError`; `impl Debug + Display + Error for BuildInputTopologyError`; `pub fn merge_observed_build_inputs(first: BuildInputSet, retry: BuildInputSet) -> Result<BuildInputSet, BuildInputTopologyError>`; `pub fn snapshot_watch_repair(path: &Path) -> Result<WatchRepairDependency, BuildInputTopologyError>`; `pub fn finalize_watch_inputs(inputs: BuildInputSet, output: Option<&Path>) -> Result<FinalizedWatchInputs, BuildInputTopologyError>`; `pub enum BuildSourceError { Missing, NonRegular, InvalidUtf8 { offset: u64 }, Io { message: String } }`; `pub enum ObservedBuildAttempt { ObservationFailed { error: BuildInputTopologyError }, SourceFailed { error: BuildSourceError, inputs: BuildInputSet }, Pipeline { build: PipelinedPackageBuild, inputs: BuildInputSet } }`; `pub enum ObservedPerUnitBuild { ObservationFailed { error: BuildInputTopologyError }, SourceFailed { error: BuildSourceError, inputs: BuildInputSet }, Walk { walk: PerUnitWalk, inputs: BuildInputSet } }`. These are the complete new exports; fields/constructors/mutation/evidence stay private. | `inputs()` always contains one row per logical path sorted by ascending raw platform bytes. Within one call, the highest W4 encounter ordinal supplies a duplicate path's state; unequal state or evidence sets `changed_during_attempt`. Retry ordinals are ordered after every first-attempt ordinal, so retry state wins overlaps; merge unions evidence, ORs prior flags, sets the flag for any unequal overlap, adds retry-only paths, and re-sorts the union by path. The first aliased row in that same order is `alias_index`. `finalize_watch_inputs(Some)` performs W3 validation. When stable and aliased, it returns matching `alias_index` and an opaque repair dependency containing the bounded fixed output path plus alias-time W6 graph; otherwise both are `None`. `output=None` returns neither. Both public path arguments must be absolute and pass W8 validation before allocation or graph lookup. `snapshot_watch_repair` rebuilds the same bounded graph for transition/audit comparison. `into_parts` is the sole consuming projection and frees all read-time evidence; both components remain owned. Repair equality covers path plus every graph node/identity/raw target. | Exact export/trait inventory; external construction/mutation negatives; producer/duplicate/retry ordering; merge; finalization stable/unstable/alias/None; absolute/relative/NUL repair/output paths; repair snapshot/equality/changed-during-install/removal; evidence consumption; topology-error Display; and state/error owners. |
| W6 | Watch-set transition | A private topology graph resolves each absolute lexical W5 path with filesystem lookup semantics. It records every lexical and target-side directory-entry node visited, not only the final path: missing; directory `(device,inode)`; symlink `(device,inode,raw target bytes)`; leaf regular target `(device,inode)`; or other. Every existing node's tuple comes from nofollow metadata for that entry, so W3 can compare output against traversed nodes and filesystem folding is reflected by kernel lookup. Relative/absolute targets resolve through nested symlinks until a leaf, missing suffix, loop, or W8 bound. W3 finalization compares the current graph and semantic re-read with W4 evidence, returns the final semantic snapshot, then consumes evidence. An alias rejection additionally retains the fixed output path as `repair_output`; its complete W6 graph participates in handles and every event/audit comparison until a revision finds no alias or publishes successfully. Before waiting, add handles for every existing input/repair graph node and deepest existing directory above each unresolved suffix, rebuild both graphs and semantic snapshot, then remove obsolete handles. After success the next logical set is exactly the finalized inputs with no repair output. After failure, merge current attempt with absent last-success paths as before and retain only that revision's optional repair output. Earlier failed sets never contribute; entry is always present. | The watcher owns current input/repair graph paths, raw targets, native identities, and handles; read-time evidence exists only through W5 finalization. A loop is stable topology for W5 `Unreadable`; traversal excess is W8 error. Nofollow metadata, add, topology/snapshot, or removal failure is W9 fail-closed. Changed semantic state, changed input topology, any repair-output topology/identity change, or `changed_during_attempt` schedules one revision. A regular input inode-only change with identical state re-arms without revision unless it is also the retained repair output. Add-before-remove plus post-add rebuild closes every registration gap. | Injected owner covers failure merge, no-spin, all mutation barriers, output-named nodes, missing/symlink/cycle repair, removal failure, and direct/folded/distinct-hard-link alias removal/replacement with native event deliberately dropped. |
| W7 | Event and verification model | Linux uses inotify and macOS uses the native file-event backend selected by the implementation dependency for low-latency wakes. An event for any W6 node, rename/replacement, or overflow wakes the loop through W9's shared pipe. The idle loop polls that read end with the current debounce or audit deadline, consumes at most 4,096 hint bytes per wake, and then checks atomic state; readable, hangup/error, and deadline are the only wait results. Events are collected until 50 ms of quiet or 250 ms from the first event, whichever comes first. Independently, after every completed no-change comparison the loop arms one fixed two-second audit deadline. Either an event drain or that deadline rebuilds W6 topology, installs newly reachable handles, repeats topology, and streams every W5 regular input to recompute semantic state. A topology or semantic change starts exactly one revision; otherwise the next two-second deadline starts after that comparison completes. The audit is the correctness owner for dropped/native-unreported events, writable-`mmap` changes, and remote changes once ordinary reads expose their bytes; native events are the latency path, not correctness authority. Events received while compiling or comparing remain pending and cause a following comparison after the current attempt/scan completes. | The callback retains no event or path payload. A normal event stores the atomic dirty bit with `Release`; overflow also stores an atomic uncertain bit with `Release`. A fatal asynchronous backend error maps to one fixed class and `Release` compare-exchanges its nonzero code into a preallocated `AtomicU8`: 1 `Disconnected`, 2 `Io`, 3 `PathLost`, 4 `WatchLost`, 5 `Capacity`, 6 `InvalidConfig`, or 7 `Other`. The first stored class wins and later fatal errors are discarded. Every event/error path then writes one byte to W9's nonblocking shared wake pipe, retrying `EINTR`; `EAGAIN` is successful coalescing because the atomics retain all control state. The owner keeps the read end open until the backend and all callback delivery are stopped, so no other write error is reachable. At each post-revision/idle wake, the loop `Acquire`-loads W9's graceful signal first, then the fatal-error slot, before dirty bits, debounce, or timer work. Event, signal, and timer wakes share one poll and one non-overlapping comparison; audits never accumulate or overlap, and hashing retains only the W8 fixed buffer. Pipe reads retry `EINTR`; `EAGAIN` returns to poll, and EOF/other error is a W9 watcher error. The loop `AcqRel`-swaps dirty and uncertain to false before, never after, a comparison so a concurrent callback cannot be erased. The fatal slot is never cleared before process exit. There is no user-configurable interval or event-only mode. | Deterministic event/timer owner covers quiet/max debounce, irrelevant/no-op events, pipe readability/error, bounded hint reads, coalescing with an already-full pipe, overflow, rename, dropped-event and no-event mutation, multi-level target-side missing creation, symlink topology, event-during-build/scan, audit non-overlap, every fatal class, two-fatal-error first-wins ordering, and idle signal/fatal simultaneity. Native smoke covers replacement on each release OS and writable `mmap` on Linux. |
| W8 | Resource ceiling | Before retaining, encoding, allocating a graph for, or using a path as a filesystem-call argument, every absolute lexical input, fixed output, raw symlink target, and expanded graph path is checked in this order over borrowed raw bytes: length, first embedded NUL, then absolute-path shape where W5 accepts a public path. 1,023 bytes are accepted and 1,024 produce `path too long (maximum 1023 bytes; got LEN; hash HEX)`; an admitted path's first NUL produces `path contains NUL byte at offset OFFSET`; a relative public output/repair path produces `path is not absolute`. No rejected path reaches lookup or another side effect. An installed set admits at most 16,384 distinct logical inputs and 65,536 distinct combined input-plus-repair W6 graph nodes, with at most 40 followed symlinks per logical input or repair output. One attempt/retry merge retains at most 131,072 distinct W4 evidence keys. Behind W9's fixed prefix, the remaining rejected-next suffixes are exactly `too many inputs (maximum 16384)`, `too many path components (maximum 65536)`, `too many read-time path components (maximum 131072)`, or `too many symlink traversals (maximum 40)`; each exits before retaining/installing the rejected value. | Finalization may own at most 196,608 evidence-plus-current nodes and consumes evidence before transition. Add-before-remove may retain at most 32,768 logical records and 131,072 combined registrations. The backend owns those registrations, one fatal `AtomicU8`, dirty/uncertain atomics, one fixed 4,096-byte hint-read buffer, and W9's shared pipe at the kernel's finite default capacity; it never requests a resize and pipe bytes carry no state. There is no second wake channel. Every watcher/control descriptor is close-on-exec before child spawn. Finalization/event/audit hashing uses one 64 KiB buffer. W13 owns two 4,096-byte read buffers, two checked post-reap byte counters of at most 1,048,576, and one reusable 12,318-byte framed-line buffer; no output total is retained. Opens are nonblocking and `fstat`-regular. `LEN` and `OFFSET` are checked decimal byte counts; `HEX` is lowercase 32-digit `Hash128`; `WatchPath` expands to 3,069 bytes; `WatchText` admits 16,384; all arithmetic is checked. Memo retains 768 MiB. | Exact/rejected-next length/NUL/absolute path, input, combined graph, merged evidence, combined finalization, symlink, encoding/message, disjoint transition/repair, identity, sparse/FIFO, shared-pipe/control-fd close-on-exec, child-buffer/post-reap, fatal-slot, and memo owners. |
| W9 | Failure and process exit | Source, frontend-cache, W3 alias/input-instability, codegen, PGO, link, and publication failures produce `failed` and keep watching. `ObservationFailed`, topology/resource/finalization failure, watcher initialization/transition failure, a fatal W7 backend class, W7 shared-wake read failure, and W2/W13 transcript/pump failure produce `alignc: watch: watcher error: MESSAGE` and exit 1. SIGINT/SIGTERM request graceful stop; the first signal wins over a simultaneous backend error. An active revision finishes its ordinary result unless transcript output fails, waits every child, drops every stage, then emits exactly `alignc: watch: stopped by SIGINT` or `alignc: watch: stopped by SIGTERM` and exits 130/143. Idle handling is immediate because the shared pipe is the sole blocking wake; later signals coalesce; SIGKILL is ordinary immediate termination. | A process-global compare-exchange admits exactly one private signal installation. A second attempt fails before side effects. On the still-single-threaded CLI path, setup blocks both signals, saves the prior mask, creates the shared wake-pipe descriptors nonblocking and close-on-exec atomically where supported or applies `O_NONBLOCK` then `FD_CLOEXEC` to read and write ends before handler registration, registers both handlers, publishes the owner, then restores the mask. W7 callbacks borrow the same write descriptor only while the native backend owner is alive; orderly shutdown stops and drops that backend before pipe closure. Any pipe/flag/registration/mask failure rolls back while signals remain blocked: unregister in reverse order, close write then read, clear owner/guard, restore mask last. Success keeps the owner until exit. Each handler selects the first signal and writes one byte, retrying `EINTR`; `EAGAIN` from a full pipe is successful coalescing. The retained read end makes every other handler write result unreachable. No handler allocates, locks, formats, closes, or touches compiler state. | Child-process barrier owner covers every setup operation/failure and pending signal, then execs a helper during every child phase and proves all watcher/control descriptor numbers are closed there; it also proves reaping and no staging residue. Shared-wake owner covers idle event/signal/timer multiplexing, readable/error/hangup, pipe-full signal precedence, backend-before-pipe shutdown, and no second wait primitive. Backend/pump owners cover static fatal classes and arbitrary descendants. SIGKILL remains the force-stop escape. |
| W10 | Cache and artifact identity | No persisted format, namespace, cache key, compiler fingerprint, interface, object, runtime, or link identity changes. An attempt consults the existing persistent caches and process memo in their settled order. Source edit/revert is content identity, never mtime identity. `ALIGNC_CACHE=off` disables persistent reuse but leaves the already-settled in-process memo active. ThinLTO and PGO keep their existing key spaces and validation order. | The only long-lived allocation is the memo and watch state. Cache rejection markers remain process/persistence authoritative. A watch event cannot authorize a cache key or artifact. W1's trigger-excluded external resources affect each actual codegen/link step exactly as in one-shot builds but remain outside compiler/cache identity; their mutation alone does not schedule that step. | Memo-on/off, cache-on/off, edit/revert, rejected-key, ThinLTO, PGO, and external-resource trigger owners reuse existing cache matrices plus one multi-revision driver owner. |
| W11 | Platform and installation | Supported release hosts are Linux and macOS on the architectures already built by CI. An unsupported native backend, signal-handler/pipe setup failure, or backend initialization failure is a watcher error; the universal W7 audit is not a fallback for an unavailable native backend. Release archives need no service file, launch agent, socket directory, or new executable. `alignc --version` and one-shot commands are byte-for-byte unchanged. | The native-watcher and signal-hook dependencies are linked into `alignc`; the timer uses the standard library. No runtime package or daemon ownership is added. | Linux x86_64/ARM64 and Apple Silicon CI build plus native replacement/signal smoke; release inventory remains unchanged. |
| W12 | Concurrent invocations | Revisions are serial within one watch process. Separate watch and one-shot compiler processes keep the existing independent-publication rule: each uses a private stage and atomic rename, and the last successful rename to the same executable path wins. There is no cross-process or output-path lock. W2 markers attest only that their own process completed that revision; they do not reserve the path against external writers. | Watcher handles and memo state are process-owned. The only process-global mutation is W9's binary-owned singleton signal disposition/pipe lifetime; failed overlap and partial setup restore there, while a successful watch exits without restoration. Separate OS processes have independent dispositions. An application that requires one publisher or a filesystem transaction across W3's final alias snapshot must own that orchestration outside `alignc`, as it does for concurrent one-shot builds today. | Barrier owner runs two processes publishing the same final path, proves every observed image is complete, and accepts either winner; W9's child owner closes same-process singleton state. |
| W13 | Child-output pump | Every watch-mode `cc` (including its linker) and macOS `strip` child is spawned with nonblocking piped stdout/stderr. Both parent read ends and both pre-dup child write ends are close-on-exec; command setup maps only the intended write ends onto child stdout/stderr, closes every other pipe end, and leaves those two standard descriptors open across exec. One parent-owned loop checks direct-child status before every poll, polls both read ends for at most 50 ms, services stdout then stderr when both are ready, and reads at most 4,096 bytes per stream per cycle. Poll/read `EINTR` retries and `EAGAIN` returns to poll. Each nonempty chunk becomes exactly one complete line `alignc: watch: child stdout: ENCODED_BYTES\n` on stdout or `alignc: watch: child stderr: ENCODED_BYTES\n` on stderr. `ENCODED_BYTES` percent-encodes every raw byte outside ASCII `0x20..=0x7e` and `%` as uppercase `%HH`; concatenating decoded chunks reconstructs each stream. Per-stream order is fixed; cross-stream order is stdout-before-stderr poll order and has no semantic meaning. | The pump retains two 4,096-byte read buffers and one reusable 12,318-byte buffer for the larger prefix, worst-case encoded payload, and newline; it never retains whole output. It records the first read/transcript error but continues servicing both pipes while the direct child runs. Immediately after reaping that child, it performs one `FIONREAD` query per pipe, stdout then stderr, for the exact queued byte count. Counts through 1,048,576 are accepted; a larger count records `child stdout read: buffered output exceeds 1048576 bytes` or the stderr sibling, and a negative count uses `invalid buffered-byte count`. The pump reads at most each accepted snapshot count in 4,096-byte chunks, so later descendant writes cannot extend the quota, then closes both read ends without waiting for EOF. Query/read failure or EOF before the snapshotted count records the matching `child stdout read: MESSAGE` or `child stderr read: MESSAGE`. The first recorded pump/transcript error wins over child status; child exit status otherwise retains ordinary build meaning. No child byte can become an unframed marker, path, diagnostic, or success line. | Injectable pump owner covers empty/arbitrary/all-byte/marker-shaped output, simultaneous streams, >buffer output, `EINTR`, `EAGAIN`, 50-ms no-output child exit, read/write/flush/query/early-EOF error, child failure, exact/over-limit/negative post-reap count, descriptor-table inheritance, descendant-retained and continuously-writing write ends, wait/snapshot/drain/close, and exact decoded parity with one-shot direct-child streams through the accepted bound. |

`build_path_pipelined_observed` is additive. The existing
`build_package_pipelined` remains shape- and behavior-identical and calls the
same private implementation with observation disabled. Observation never
changes a diagnostic, cache decision, read order, static-input lock, codegen
schedule, link argument, or published byte. The exact signature is:

```text
pub fn build_path_pipelined_observed(
    source_map: &mut SourceMap,
    path: &Path,
    cache: CacheContext,
    reuse: UnitReuse,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
) -> ObservedBuildAttempt
```

The ThinLTO sibling is likewise additive and shares the exact observer and
entry reader:

```text
pub fn build_path_per_unit_observed(
    source_map: &mut SourceMap,
    path: &Path,
) -> ObservedPerUnitBuild
```

`--thin-lto` selects this sibling, reports/returns frontend diagnostics exactly
as the existing `walk_or_report` path does, rejects an empty successful walk,
and creates the existing unique object stage. For two or more units it calls
`build_thin_lto`; for exactly one unit it preserves the current
`codegen_units_parallel` fallback because there is no cross-unit boundary and
`build_thin_lto` requires at least two units. Both arms retain their existing
cache-stat report, deterministic library union, link, and atomic publication.
ThinLTO cannot combine with PGO by the existing CLI contract, so this sibling
has no target/profile/cache/PGO argument: those affect only the post-walk
codegen and existing keys, not observed project inputs. It does not consult the
persistent frontend cache, matching the current all-MIR ThinLTO front half.

The wrapper resolves `path` against the startup cwd, opens it nonblocking,
proves it regular with `fstat`, reads it once, records the W5 content state,
validates UTF-8, then calls the shared pipeline. Import and
static-input producers record their outcome at the read that already owns those
bytes. `PgoMode::Use` records the snapshotted profile bytes used for all keys
and LLVM consumers. No second discovery walk, directory scan, path guessing,
or ambient project configuration is allowed. `SourceFailed` prints the same
`alignc: cannot read 'PATH': ...` prefix as the one-shot helper, followed by
`revision N failed`; `Pipeline` uses the existing diagnostic rendering and
retry rules.

On the ordinary stale-cache shape, the CLI renders the first attempt while its
`SourceMap` is alive, performs the sole reuse-forbidden observed call with a
fresh `SourceMap`, then passes both sets to `merge_observed_build_inputs`.
The merged set is the only value that may be finalized, used for publication,
or installed. A second `ObservationFailed` or merge error consumes/drops the
first evidence and exits through W9; a second ordinary failure merges and then
uses `finalize_watch_inputs(..., None)`. This preserves the existing diagnostic
echo and retry count without exporting constructors or moving retry policy into
the library.

`BuildInputTopologyError` is opaque and manually implements `Debug` and
`Display`; it never derives through an internal path or `Hash128`. Its complete
`Display` set is `path too long (maximum 1023 bytes; got LEN; hash HEX)`,
`path contains NUL byte at offset OFFSET`, `path is not absolute`,
`too many inputs (maximum 16384)`,
`too many read-time path components (maximum 131072)`,
`too many path components (maximum 65536)`,
`too many symlink traversals (maximum 40)`, or one of W9's bounded
phase/path/message forms without the outer watcher-error prefix. The path-too-
long variant computes `LEN` and lowercase 32-digit `HEX` through the caller's
borrowed bytes before cloning or issuing a filesystem call; it retains only
those fixed-width values. Other variants retain only admitted `WatchPath`,
admitted `WatchText`, and fixed counters. `finalize_watch_inputs` consumes its
input even on `Err`, so read-time evidence cannot escape or be reused.

Input classification is total and precedes parsing or any cache/native side
effect for that input:

```text
1  missing path                    BuildInputState::Missing + BuildSourceError::Missing for entry
2  opened target is not regular   BuildInputState::NonRegular + BuildSourceError::NonRegular for entry
3  open/stat/read failure         BuildInputState::Unreadable + BuildSourceError::Io for entry
4  complete regular bytes         BuildInputState::Regular(Hash128::of(bytes), len)
5  invalid UTF-8 regular bytes     the same Regular state + BuildSourceError::InvalidUtf8 for entry
```

Imported-source and static/metadata failures retain their existing diagnostic
types after recording the matching W5 state. A path that is missing and whose
parent is also missing is still `Missing`; the watcher separately locates its
nearest existing lexical or resolved-target ancestor through W6. A complete
read that observes a different state for the same logical path sets
`changed_during_attempt`; W5 retains the highest producer-ordinal state while
forcing a following revision instead of treating that state as stable.

### Validation and event order

One revision has this logical order:

```text
started marker
bounded path validation, entry read, and read-time identity observation
complete frontend/static-input observation
existing cache/rehydration/codegen validation and one retry
merge first/retry observations when retry occurred
finalize_watch_inputs(Some(output)): current topology/resource,
  final semantic re-read, read-time stability, and output-alias validation
existing link and atomic publication
fallible success-line write and stdout flush
ready or failed marker
graceful-stop check
add new watches
post-registration snapshots
remove obsolete watches
drain/coalesce queued events
state comparison
next revision or wait
```

An ordinary failure does not skip observation or the terminal marker. The one
stale-cache retry belongs to the same revision: its inputs are unioned with the
first attempt, it prints no second `started`, and it produces one terminal
marker. Retry observations are consuming-merged before either outcome. A failed
candidate calls `finalize_watch_inputs(inputs, None)` after
ordinary processing and before the terminal marker; this consumes its read-time
evidence without resolving an output or authorizing publication. If the two
attempts observe different semantic or topology state, W3 rejects publication
and W6 schedules the next revision. The existing diagnostic echo rule remains
first-attempt `All`, retry `ErrorsOnly`.

Validation precedence within an attempt remains item 3's order. Watch-only
failures occur outside it:

```text
1  invalid CLI / unsupported flag combination
2  signal-handler and shared wake-pipe installation
3  initial native-watcher construction
4  per-path length, first-NUL, absolute-public-path, logical-input, and
   read-time-evidence limits during observation
5  first/retry observation merge and its combined evidence limit, when retried
6  ordinary source/frontend/cache/codegen result
7  finalize_watch_inputs(None): evidence consumption for a failed candidate;
   or finalize_watch_inputs(Some): current topology/resource,
   final semantic state, read-time stability, then output alias
8  child-output pump read/write result, child status, link, and publication
9  successful-build stdout line write
10 transcript stdout/stderr flush, terminal-marker write
11 live-input/combined-input-repair-topology limit
12 watch/repair-handle addition
13 post-registration input/repair topology and semantic snapshot
14 obsolete-handle removal
15 graceful signal, fatal backend slot, then event/timer delivery
```

For multi-invalid watch transitions, the lowest numbered failure wins. An
overflow is not an error and cannot outrank a build failure; it requests the
post-attempt state comparison. A watcher error after `ready` does not retract
the published executable, but the process exits nonzero because it cannot
promise another revision.

A pending graceful stop is control flow, not a watcher/build error. It is
checked immediately after an active revision's terminal marker and between
each W6 transition operation. The active revision's ordinary result therefore
precedes the stop line, while an idle signal has no invented revision. Because
the handler suppresses default termination, a synchronous linker/tool child is
always waited by its existing owner before the parent drops stages and exits;
a terminal-generated SIGINT may make that child fail sooner, while a signal
sent only to the parent waits for the child to finish.

Watcher-error messages identify the failing phase and path when one exists:
`initialize: MESSAGE`, `stdout write: MESSAGE`, `stdout flush: MESSAGE`,
`child stdout read: MESSAGE`, `child stderr read: MESSAGE`,
`wake read: MESSAGE`,
`add 'ENCODED_PATH': MESSAGE`, `snapshot 'ENCODED_PATH': MESSAGE`,
`remove 'ENCODED_PATH': MESSAGE`, the W8 exact resource messages,
`backend: disconnected`, `backend: io`, `backend: path lost`,
`backend: watch lost`, `backend: capacity`, `backend: invalid config`,
`backend: other`, or `revision counter exhausted`, each behind the single
`alignc: watch: watcher error: ` prefix. `ENCODED_PATH` is always W2
`WatchPath` and dynamic `MESSAGE` is `WatchText`; the underlying OS message is
informational and has no control-flow meaning. Asynchronous backend failures
use only the static class strings above; callback-owned dependency messages and
paths are never retained. Failure to write this line is the W2 unavailable-
stderr exit, not a recursive watcher error.

The native adapter's classification is exhaustive and ordered: callback-stream
termination or transport disconnect is `Disconnected`; an attached OS I/O
error is `Io`; refusal because the watched lookup no longer exists is
`PathLost`; explicit watch invalidation/removal is `WatchLost`; descriptor or
backend watch exhaustion is `Capacity`; an unsupported backend option/state is
`InvalidConfig`; and every remaining dependency variant is `Other`. Overflow
is the distinct nonfatal uncertain-bit case from W7. The adapter owner pins
every dependency variant to one class so a dependency update cannot silently
change or omit the mapping.

### Implementation closure matrix

This is one implementation capability. Input observation without the
foreground consumer is a dormant producer, while a watcher without exact
producer-owned inputs either misses changes or scans unrelated state. The
expected hand-written diff may exceed 1,000 lines because keeping these halves
together avoids a second path-discovery algorithm and one unreviewable gap
between read identity and event registration.

| Cell | Required closure | Planned implementation and owner |
|---|---|---|
| D1 CLI parity | Every existing build flag/default/error composes with W1; every other verb rejects `--watch`. Startup configuration and the external-resource trigger boundary are resolved/described once. | `main.rs` parser/help table and `watch_build` real-binary matrix, including exact help and no revision for a replaced external resource alone. |
| D2 input formation | Formation; length/NUL/absolute validation; private construction/read-only access; exact producer, duplicate, and retry order; move/return; merge; finalization; repair-token formation/consumption; success replacement; and failure merge cover every W5 state and ordinary/ThinLTO/PGO path. | One observer plus exported-surface negative/trait inventory; parameterized owners cover row/alias ordinals, merge state/evidence limits, finalization modes, repair snapshot/equality, every rejected path boundary, and Drop on every result. |
| D3 read/register/publication race | A semantic/topology change around original/final reads or registration cannot publish stale/torn bytes, replace a supplying resource, or wait on the wrong bytes. Rejected output aliases remain repair dependencies until their topology changes and a recovery revision revalidates them. | One semantic/graph authority serves observation, finalization, repair token, alias validation, and add-snapshot-remove; barriers cover writes/replacements/folding/hard links plus alias-time-to-install removal and dropped-event audit recovery. |
| D4 event lifecycle | Normal/identical/burst/max-debounce/full-pipe/overflow events, event during compile, every fatal class/first-error order, periodic audit, failed-set and repair-output merge, idle/active shared-pipe signal wake, signal/fatal precedence, and Drop wake/stop exactly once. | Injectable event/timer/error source, one shared pipe, dirty/uncertain/fatal/signal atomics, logical/repair merge owner, bounded-read poll owner, and backend-before-pipe cleanup; native adapters stay thin. |
| D5 build control paths | Initial/repeated success; ordinary/ThinLTO cardinalities; semantic/alias rejection and repair; retry/PGO/cache combinations; child output/success/failure; edit/revert/malformed input; and W1 external trigger behavior preserve ordinary link/publication semantics. | Existing owners plus `watch_build` revision, retry, finalization, repair recovery, W13 pump, last-good publication, external-trigger, and route/cardinality parity owners. |
| D6 concurrency and cleanup | Worker panic cleanup remains; callbacks/handlers do not block; atomics select overflow/fatal/signal; evidence, repair tokens, pump buffers/fds, workers, stages, handles, control fds, and direct children do not outlive owners. Signal setup is singleton and rollback-complete; every watcher/control fd is close-on-exec. Descendant-retained or continuously written child-output pipes cannot block cleanup. | Existing panic owner plus finalization/repair Drop, watcher atomics/Drop, every signal flag/setup barrier, backend-before-shared-pipe shutdown, exec descriptor-table negative, W13 post-reap snapshot/drain-close, two-publisher barrier, and bounded child owners. |
| D7 identity and authorization | Memo, persistent frontend, packaged fallback, object, ThinLTO, PGO, compiler/LLVM, target, and runtime identities are unchanged. Rejected frontend keys remain rejected across every revision and process. | Existing cache codec/rejection owners plus multi-revision exact-key test. |
| D8 resource bound | Exact/rejected-next length/NUL/absolute-path/input/combined-input-repair-graph/evidence/finalization/symlink/encoding/message/post-reap limits; shared wake pipe/fatal slot; non-overlapping scan; 64 KiB hash and W13 fixed buffers; nonblocking special-file rejection; checked counters; and memo refusal are reachable. | Parameterized limit/repair/encoding/pump/snapshot/overflow/fatal/FIFO/sparse/RSS/timer/identity owners. |
| D9 observable transcript | Marker count/order/atomicity, both-channel flush failures, reversible path/message/child-byte frames, instability/alias/resource/backend/wake/pump errors, in-process diagnostics/cache stats/stdout, decoded child streams, object/link order, behavior, and exits are exact for success/failure/recovery/watcher error. Linked executables are not byte-compared on macOS. | Pipe-backed golden transcript with all-byte/marker child injection and decoded bounded parity, short marker writes, mutation/repair, exact limits/classes, and one-shot object/link/program comparison. |
| D10 non-impact | One-shot build/run/size, `align-repl`, inspection verbs, release layout, and docs remain unchanged except for the additive flag/help text and its explicit external-resource trigger boundary. | Existing bounded gate, REPL owners, release workflow structural test, usage/help snapshot, and parameterized external-resource replacement owner. |

The author-side matrix-to-diff pass must bind every applicable cell to an
implementation site and a discriminating owner before the implementation
review. A review finding about a missed input, registration window, event
overflow, cache authorization, or cleanup reopens that entire class across D2,
D3, D4, D6, and D7 rather than patching one path.

### Design review closure

The first independent review of the item 5 ledger found four valid gaps. The
fix is class-wide and ledger-first:

| Finding | Closure |
|---|---|
| P1: the observed wrapper had no ThinLTO route | W4/W5 now define the ordinary pipelined and all-MIR per-unit observed siblings separately; D2/D5 require identical input observation and preserve the existing ThinLTO codegen/cache/link route. |
| P1: `Missing` content state could hide newly created ancestors or an intermediate-symlink replacement | W6/W7 now make lexical component topology an independently compared state, refresh/install it on every wake before content comparison, and own multi-level creation plus symlink replacement. W8 bounds the new dimension. |
| P2: default SIGINT/SIGTERM termination cannot run stage/child cleanup | W9 replaces default termination with one binary-owned async-signal-safe stop request and self-pipe. An active revision and its children finish/reap before conventional 130/143 exit; D4/D6 and acceptance cover every phase. SIGKILL is the explicit no-cleanup escape. |
| P3: 20 warmed units were all called first-party package units | The item 4 row now distinguishes 19 source package modules from the generated warm entry. |

Because the first two findings add a public observed route and change watcher
strategy, the revised design receives a fresh complete review before
implementation. A further P1/correctness finding reopens D2–D8 rather than
starting a third local patch round.

That fresh review found two more P1 gaps and two protocol/portability gaps, so
the local patch loop stopped and the matrix was reopened on
`symlink-resolution-and-event-loss`. The replacement boundary makes resolved
filesystem topology and periodic semantic verification independent correctness
owners; native delivery is only the low-latency accelerator:

| Finding | Redesigned closure |
|---|---|
| P1: a one-unit `--thin-lto` walk would call the two-or-more-unit backend | The exact ThinLTO route now branches on cardinality after observation, preserving `codegen_units_parallel` for one unit and `build_thin_lto` for two or more; D5 owns both. |
| P1: a dangling symlink could resolve outside the lexical input tree | W6 now follows bounded absolute and relative target chains, retains both lexical and target-side nodes, and watches the target-side deepest existing ancestor; D3/D8 own dangling, nested, cycle, and repair cells. |
| P2: a stderr `ready` marker could overtake block-buffered stdout | W2 requires successful stdout flush after publication and before `ready`; flush failure is an exact W9 protocol error, and D9 uses a pipe-backed owner. |
| P2: inotify may omit writable-`mmap` or remote-filesystem changes | W7 now performs a fixed two-second full semantic audit after each no-change comparison. Event and timer paths share the same non-overlapping comparison, and D4/D8 own dropped-event, no-event, and scan-lifetime cells. |

This is a boundary redesign rather than a second line-level correction. D2–D8
are reopened together: observation feeds resolved topology; resolved topology
and semantic bytes feed both event and audit wakes; cardinality selects the
existing ThinLTO backend; and one resource/cleanup model owns all paths. The
redesigned full diff requires a fresh independent review before implementation.

The review of that redesigned boundary exposed a separate same-resource axis
and one state-merge ambiguity, including another P1. The patch loop stopped
again and D2–D8 were reopened on `same-resource-and-failure-state`:

| Finding | Redesigned closure |
|---|---|
| P1: publication could replace a file-backed input that aliases the output | This revision put W6's resolved-slot authority after complete observation and before link/publication, covering lexical, parent-alias, and resolved-target matches. The later filesystem-identity review below supersedes its attempted distinct-hard-link allowance. |
| P1: `last_successful ∪ current_attempt` did not define an overlapping path's state | W5 assigns exact producer ordinals within an attempt/retry and W6 defines failure merge as all current records plus only absent last-success records. D2/D4 own overlap, absence, and no-spin behavior. |
| P2: W12 denied process-global mutation while W9 installed signal handlers | W9 now owns a process-global singleton with side-effect-free second-install rejection and exact partial-setup rollback; W12 names that sole mutation and limits concurrency claims to separate OS processes/output publication. D6 owns setup, overlap, rollback, and successful process-exit lifetime. |

The resulting capability still has one consumer boundary: the observed input
set, its resolved topology/slots, failed-state merge, event/audit loop, and
publication decision are mutually dependent. The newly redesigned full diff
receives another fresh independent review before implementation.

That review found two more P1 aliases plus four implementability/protocol gaps.
The matrix was reopened on `filesystem-slot-and-protocol-atomicity`; the
replacement boundary treats the entire input access path as publication safety
state and makes all long-lived transcript/setup operations explicitly fallible:

| Finding | Redesigned closure |
|---|---|
| P1: output could replace an intermediate symlink/directory used to reach an input | W3 compares output against every W6 traversed directory-entry node, not only lexical/final input slots; D3 owns an output-named intermediate symlink and directory. |
| P1: raw component bytes miss case-/Unicode-folded aliases | W3 uses native nofollow identity after filesystem lookup. It deliberately rejects distinct hard links too, avoiding a portable directory-entry-equivalence guess; D3/D8 own folded and hard-link cells on supported filesystems. |
| P2: arbitrary path bytes could inject a false protocol line | W2 defines reversible percent-encoding over raw Unix bytes and W4/W9 route every watch-mode path field through it; D8/D9 own capacity, round-trip, newline, non-UTF-8, and marker-shaped cases. |
| P2: a signal between handler registrations could retain default termination | W9 blocks both signals across pipe/handler setup, activation, and rollback, restoring the saved mask only after a complete owner or cleanup; D6 owns every barrier and pending delivery. |
| P2: the ordinary success-line write could fail before explicit flush | W2 moves both locked stdout write and flush under one fallible boundary with distinct watcher errors and no false `ready`; D9 owns full, partial, and flush failures. |
| P2: public `BuildInput` fields contradicted private construction/mutation | W5 makes both fields private, adds read-only accessors, fixes derives, and adds an external-construction negative in D2. |

D2–D9 are reopened together because public input encapsulation feeds topology,
every topology node feeds publication safety, path encoding spans all errors,
and signal/stdout setup owns whether the transcript can be promised. The full
redesigned diff receives a fresh independent review before implementation.

That review found one remaining publication race and two bounded-state gaps.
The matrix was reopened on `read-time-identity-and-async-errors`; the redesign
retains the identities that actually supplied bytes and makes every callback-
owned failure a fixed-size control value:

| Finding | Redesigned closure |
|---|---|
| P1: final alias resolution could forget a symlink/hard-link node removed or retargeted after its bytes were read | W4 retains bounded pre-open, opened-leaf, and post-read identities until W5 finalization. W3 compares that evidence with current topology, rejects any instability before link, and includes both sets in output-alias validation. D2/D3 own every removal/retarget barrier and evidence consumption. |
| P2: dirty/uncertain bits could not carry an asynchronous backend failure or deterministic first message | W7 maps fatal callbacks to seven static classes in one `AtomicU8`; compare-exchange preserves the first class and the one-slot channel is only a wake hint. W9 fixes signal precedence and the exact lines; D4/D6/D9 own every class, two-error ordering, and a full wake channel. |
| P2: retained/encoded paths had no platform-independent byte ceiling | W4/W8 admit 1,023 raw bytes and reject 1,024 before retaining, using, or encoding the path with an exact length/hash error. W8 separately bounds read-time, current, and combined nodes; D2/D8/D9 own accepted/rejected-next allocation and transcript behavior. |

D2–D9 are reopened together: observation now produces a linear read-identity
owner consumed by publication finalization, while event callbacks publish only
bounded atomic state consumed in the same signal/error/event order. This full
redesigned diff requires a fresh independent review before implementation.

The next review found a new P1 semantic-publication race plus two completeness
gaps, so line-level patching stopped and the matrix was reopened on
`semantic-stability-and-complete-input-union`:

| Finding | Redesigned closure |
|---|---|
| P1: same-inode content writes after observation could publish stale or torn bytes | W3/W5 finalization now streams every logical input into a new total semantic snapshot after codegen and before alias/link, with graph and opened-leaf identity around that read. Any semantic or topology difference rejects publication and schedules another revision. D3/D8/D9 own every in-place and topology barrier. |
| P2: private sets had no reviewed operation that could preserve both stale-cache attempts | W5 adds one exact consuming `merge_observed_build_inputs` export. It defines ordinal/state precedence, evidence union, the combined limit, and cleanup on `Err`; D2/D5 own success, second failure, and merge failure without exposing construction. |
| P2: project-supplied `-l` archives were absent from both observation and the restart boundary | This revision attempted to make every capability/user linked library restart-only even under `LIBRARY_PATH`. The following review showed that a later ordinary link could consume a replacement, so the trigger-excluded boundary below supersedes this wording. |

D1–D10 are reopened together because the retry merge is the sole owner of the
evidence later consumed by semantic finalization, the final snapshot authorizes
publication, and the help/non-impact contract must expose the only intentionally
unwatched input class added by user code. This full redesigned diff requires a
fresh independent review before implementation.

The review of that boundary found that its `restart-only` wording was stronger
than the ordinary per-revision linker mechanics. The matrix was reopened on
`external-link-trigger-semantics` rather than adding hidden pinning or a second
link-resolution path:

| Finding | Redesigned closure |
|---|---|
| P2: a later source edit could make the fresh linker consume a replaced archive despite the restart-only promise | W1/W4/W10 now define external link-time resources as trigger-excluded: replacement alone starts no revision, while the next observed change or restart runs the ordinary resolver and may consume it. D1/D5/D10 and acceptance cover each resource class, including replacement followed by a watched edit. |

This narrows the public promise to the exact existing linker semantics while
keeping every unobserved input visible. The changed W1 strategy receives a
fresh full review before implementation.

That review found three lifecycle gaps and reopened the matrix on
`subprocess-framing-and-alias-repair`:

| Finding | Redesigned closure |
|---|---|
| P2: inherited child stderr could inject a false terminal marker | W2/W13 pipe both child streams through bounded raw-byte framing and flush them before one atomic terminal-marker write. D5/D8/D9 own arbitrary bytes, both streams, pump errors, descendants, and decoded one-shot parity. |
| P2: removing a distinct-hard-linked rejected output did not change any installed input | W5 returns the alias-time output graph as an opaque repair dependency; W6 installs and compares it until removal/replacement triggers recovery or success clears it. D2–D5 own alias-time-to-registration races and event-dropped audit repair. |
| P2: signal-pipe descriptors could leak through `cc`/linker/strip exec | W8/W9 require nonblocking plus close-on-exec on both signal-pipe ends before handler publication, extend that rule to every watcher/control descriptor, and roll back each flag failure. D6 and acceptance inspect the exec child descriptor table. |

Child framing, repair topology, and descriptor lifetime meet at the same
revision/child transition, so they remain one implementation boundary. The
changed W2/W5/W6 strategy receives a fresh full review before implementation.

That review found five determinism and liveness gaps and reopened the matrix
on `ordered-input-and-shared-wake-liveness`:

| Finding | Redesigned closure |
|---|---|
| P2: producer ordinals did not fix public row, duplicate, or retry order | W4 enumerates each producer in encounter order; W5 raw-byte-sorts rows and fixes latest state, retry overlap, evidence union, and `alias_index`. D2 owns the complete order. |
| P2: public path arguments had no embedded-NUL contract | W5/W8 reject length, then first NUL, then relative shape before allocation, graph lookup, or filesystem side effect. D2/D8 own every boundary and precedence combination. |
| P2: the complete topology-error inventory omitted the logical-input limit | W8 and the complete `BuildInputTopologyError` list both name `too many inputs (maximum 16384)`; validation order and the rejected-next owner include it. |
| P2: the signal pipe and event channel gave the idle loop no single wait | W7/W9 replace the channel with one nonblocking shared wake pipe whose bytes are hints and whose atomics retain event/error/signal state. D4/D6 own poll, coalescing, precedence, and backend-before-pipe cleanup. |
| P2: post-reap draining could follow a continuously writing descendant forever | W13 snapshots each queued byte count once, enforces a 1,048,576-byte bound, drains only that quota, and closes. D6/D8/D9 own accepted/rejected counts, continuous writers, and decoded parity. |

These fixes define public ordering plus the two blocking wait boundaries, so
they remain one implementation strategy and receive a fresh full review.

### Acceptance and measurement

No build-completion latency is a public promise, so no benchmark is a
correctness gate. W7's two-second value schedules the next audit only; scan and
revision duration are not bounded by it. Acceptance proves the mechanism
directly:

- one process performs at least three revisions while its PID and memo stats
  remain continuous;
- an entry edit rebuilds only the content identities the existing pipeline
  reports as changed, and exact revert obtains producer-owned memo/cache hits;
- each unaffected imported unit's frontend and object outcome is measured from
  the real pipeline result, never inferred from corpus size;
- one-shot and watch revision object bytes, link-input order, decoded
  diagnostic/child streams, and program output are identical for the same
  inputs;
- one-unit and multi-unit `--thin-lto` watch revisions take their existing
  respective backend paths and match one-shot results without panic;
- changes to an imported source, file-backed static source, checked metadata,
  and `--pgo-use` snapshot each trigger one revision;
- creation behind relative and absolute dangling symlinks, nested-symlink
  replacement, and symlink-cycle repair each trigger one revision;
- direct output/input slot identity, a parent-directory alias, and an input
  symlink resolving to the output all fail before link/publication;
- an output-named intermediate directory/symlink, case- or Unicode-equivalent
  lookup, and a distinct hard-link name all conservatively fail before link;
- removing or replacing any rejected direct/folded/distinct-hard-link output
  after alias-time and before/during repair registration triggers exactly one
  recovery revision; dropping its native event is repaired by the next audit,
  while an unchanged alias waits without spinning;
- removing or retargeting an input symlink or hard link at each pre-open,
  opened-leaf, post-read, and pre-finalization barrier cannot erase the
  read-time identity: instability or aliasing fails before link, consumes the
  evidence, and a stable following scan may rebuild;
- rewriting, truncating, extending, or restoring a regular input in place at
  each original-read/final-read barrier is rehashed before publication; a
  differing snapshot emits `inputs changed during revision`, publishes
  nothing, and schedules a stable rebuild;
- after a failed edit, the current state wins every overlapping last-success
  path, last-success contributes only paths not reached by the failed attempt,
  and an unchanged failed state waits instead of spinning revisions;
- a stable content change with its native event deliberately dropped, plus a
  Linux writable-`mmap` change, is discovered by the next completed periodic
  semantic audit without overlapping scans;
- an unrelated event and an identical-content rewrite trigger none after state
  comparison;
- a pipe reader observes the ordinary success line before the matching `ready`,
  while injected stdout full/partial write and flush failures emit no false
  `ready`, and stderr write/flush failure exits 1 without recursion;
- arbitrary/all-byte and exact marker-shaped `cc`/linker/strip stdout/stderr is
  reconstructible only inside W13 frames and precedes the terminal marker;
  simultaneous >4,096-byte streams cannot deadlock, a descendant retaining a
  writer cannot delay direct-child cleanup, a continuously writing descendant
  is cut off at the post-reap snapshot, exact/over-limit queued counts are
  deterministic, and pump failures emit no marker;
- every raw path byte round-trips through `WatchPath`, and newline,
  non-UTF-8, quote, percent, and marker-shaped paths/messages remain one encoded
  line through `WatchPath`/`WatchText`; 16,384 message bytes are accepted and
  the next byte selects the exact static replacement;
- an absolute lexical or expanded graph path of 1,023 raw bytes is accepted;
  1,024 bytes fail before retention/I/O with the exact length and lowercase
  `Hash128`; every first embedded-NUL offset and relative public output/repair
  path fails in W8 order before allocation/I/O; 16,384/16,385 logical inputs
  and 131,072/131,073 read-time nodes prove both ceilings and exact errors;
- each asynchronous backend class produces its exact static line; two errors
  retain the first class even when the shared pipe is already full, while an
  idle simultaneously pending graceful signal wakes the same poll, wins, and
  performs backend-before-pipe cleanup; EOF/error on the pipe fails closed;
- a stale-cache retry consuming-merges first and retry semantic/evidence sets;
  entry/import/static/metadata/PGO producers follow W4 encounter order, public
  rows remain raw-byte-sorted, retry state wins duplicates, retry-only paths
  join that sorted union, any difference stays unstable, merge overflow drops
  both, and neither first-attempt evidence nor diagnostics are lost;
- `build --help` names the external-resource trigger boundary; replacing a
  runtime/profile archive, linker/tool executable, system-library fixture, or
  project archive selected through fixed `LIBRARY_PATH` starts no revision by
  itself, while the next source edit and a restart each consume the replacement
  exactly as the matching one-shot build;
- external code can read but cannot construct or mutate `BuildInput` fields;
- a failed second signal installation changes nothing, each partial setup
  or nonblock/close-on-exec failure unregisters/closes/clears/restores in W9
  order, signals delivered at every setup barrier are handled only after
  activation, an exec helper sees no watcher/control descriptor, and a later
  clean child can install successfully; and
- idle and active SIGINT/SIGTERM return 130/143 only after every owned child,
  stage, watcher handle, native callback owner, and shared wake pipe has been
  cleaned up.

A local `bench/watch_build` harness may report initial, edit, and revert wall
times plus the producer-owned stage counts. Those numbers guide later work but
do not gate this item or justify function-level incremental compilation.

### Documents and prerequisites

Prerequisites are items 1, 3, and 4 plus the already-shipped `align-repl`
residency consumer. Implementation updates this section's status,
`docs/impl/01-pipeline.md`, the CLI help/README command inventory, and
`docs/impl/16-test-policy.md`. It changes no language syntax or semantics, so
`draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, and
`docs/open-questions.md` do not change. Function-level invalidation remains
item 6 and is not smuggled into this boundary.

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
