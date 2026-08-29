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
| 5 | Foreground watch builds | Implemented — `alignc build FILE --watch` keeps one foreground compiler resident, observes the exact files consumed by ordinary and ThinLTO builds, revalidates their semantic/topology state, captures child output, and atomically preserves the last-good executable without a daemon or socket |
| 6 | Function-level incremental compilation | Design ledger below; implementation pending |

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
| W1 | CLI | `alignc build FILE.align --watch`. `--watch` is a valueless, idempotent flag accepted only by `build`; every existing `build` flag and environment setting keeps its current meaning. `run`, `size`, inspection verbs, `cache clear`, and `db` reject it before reading source. `build --help` describes it as `rebuild on compiler-observed file changes; other toolchain/library changes need another observed change or restart`. There is no configurable interval/debounce, daemon, socket, or background option. | Arguments, resolved target/profile/jobs/cache/linker choice, PGO mode, current working directory, and external search environment are fixed at startup. The running compiler and loaded LLVM image are process-fixed identities. Runtime/profile archives, linker/tool executables, system libraries, and capability/user `-lNAME` results are explicit trigger-excluded inputs: their paths, metadata, and bytes are not watched, so replacing one alone starts no revision. The next compiler-observed change performs an ordinary new build/link step and may consume the replacement under the fixed configuration; restart forces that step immediately. Configured `cc`/linker/strip executables are trusted code-generation inputs: successful direct-child exit must mean every tool-owned descendant has finished writing and has released every writable descriptor or pathname access to the output. A tool that returns while such a writer survives violates the supported-tool contract; W13 process cleanup and isolation do not detect or make that configuration safe. A workspace-built compiler's baked `crates/align_runtime/src` freshness tree is a separate trigger-excluded pre-link input: changing it alone starts no revision, while the next observed change or restart runs the existing recursive digest check and may produce the ordinary stale-runtime failure. An installed compiler's absent/unreadable tree retains the existing no-op rule. Invalid combinations exit 1 before watcher creation. No new environment variable. | CLI parser/help table and real-binary invalid-combination/non-impact owner, including archive replacement, workspace freshness-tree edit/restore, and direct plus internally spawning production `cc`/linker/strip routes that join every output writer before successful return. |
| W2 | Attempt protocol | Watch mode owns exactly one ordered protocol stream: process stderr and writes nothing to process stdout. The only unframed records are terminal lines. Every other logical record is emitted as one or more exact lines `alignc: watch: record KIND STATE: ENCODED_BYTES\n`, where `KIND` is `started`, `diagnostic`, `cache`, `success`, `notice`, `watcher-error`, `stop`, `child-stdout`, or `child-stderr`; `STATE` is `more` for every nonfinal chunk and `end` for the final chunk. Raw payload chunks are at most 4,096 bytes; an empty record has one `end` frame with an empty payload. `ENCODED_BYTES` retains ASCII `0x20..=0x7e` except `%` and percent-encodes every other byte plus `%` as uppercase `%HH`. Concatenating decoded chunks through `end` reconstructs that logical record exactly. After the final startup snapshot selects continuation, revision 1 starts immediately with a `started` record whose decoded payload is `alignc: watch: revision N started`; a startup stop/error emits no `started` or revision marker. After successful atomic publication, a `success` record's decoded payload is `alignc: built executable: ENCODED_PATH`. Before either terminal marker, the owner completes the preceding frame and flushes the stream. While holding its sole lock, it then performs one unbuffered same-descriptor write of the complete <=128-byte line `alignc: watch: revision N ready\n` or `alignc: watch: revision N failed\n`; `EINTR` retries, and a short/failed write exits 1 without another marker. On a pipe the terminal write is below `PIPE_BUF` and atomic. Framing makes marker-shaped payload bytes inert; same-descriptor order makes every preceding complete record observable before the marker. `N` starts at 1 and increments by one; exhaustion is a watcher error. Any prior protocol write/flush failure emits W9's framed error only if a later write remains possible, then exits without a terminal marker; another failure exits without recursion. | `WatchPath` percent-encodes each raw Unix path byte outside `[A-Za-z0-9._/-]` as uppercase `%HH` (including `%` as `%25`). `WatchText` retains printable ASCII except `%` and percent-encodes every other UTF-8 byte; it accepts at most 16,384 input bytes, after which the value is the static `message exceeds 16384-byte limit`. These inner encodings remain reversible after the outer record is decoded. A private single-writer `WatchTranscript` owns one 4,096-byte input chunk and one reusable 12,331-byte maximum frame, routes every in-process/child/control record, and alone writes terminal lines. Every spawned `cc`/linker/strip stdout and stderr is piped through W13, never inherited. Automation consumes only this stderr stream, treats every record line as data, accepts only an unframed complete terminal line, and never infers readiness from quiet time. | One-pipe transcript owner covers every startup outcome, kind/state, empty/exact/multi-chunk records, write/partial-write/flush failures at each frame, same-stream ordering, no process stdout, atomic marker write/short-write/`EINTR`, decoded ordinary parity under equivalent pipe-backed one-shot stdout/stderr, every-byte path/message/diagnostic/cache/child round trip, and exact marker-shaped payloads at every chunk boundary. |
| W3 | Publication | Each attempt uses the existing applicable ordinary or ThinLTO per-unit pipeline and its current retry rule. Before a successful candidate may link/publish, `finalize_watch_inputs(inputs, Some(output))` first resolves the current W6 graph and streams every logical input again to form the same total W5 semantic state. It compares that current state plus its before/opened-leaf/after identities with every original W4 read-time state and node. Any content hash/length, missing/nonregular/unreadable state, or path identity disagreement sets `changed_during_attempt`; such a candidate emits exactly `alignc: watch: inputs changed during revision`, produces `failed`, schedules a comparison, and never links. Only a stable candidate compares the fixed cwd/output lookup with every original and final directory-entry node. Exact absolute lexical equality rejects even a missing slot; an existing output whose nofollow `(device,inode)` matches any existing node or opened leaf also rejects, deliberately including distinct hard links and case-/Unicode-folded names. When both the output and a compared graph node are absent, W14 proves filesystem-folded slot equivalence through their retained common parent before link. The lowest platform-byte-sorted logical input aliased by any exact, identity, or W14 comparison emits exactly `alignc: watch: output 'ENCODED_OUT' aliases observed input 'ENCODED_INPUT'`, produces `failed`, and runs no link or rename. Other candidates pass the final output path to the matching W13 `_with_output` function. That function exclusively precreates one randomized same-directory regular tool stage, reserves its private name with a regular inode, closes the creation descriptor before spawn, and exposes only that pathname to `cc`/linker/strip. Supported tools may write through or replace the reserved inode and must satisfy W1's writer-joined successful-return handoff; every production route has both owners. After a successful final tool return and child-lease release, W13 reopens the pathname read/write with `O_NOFOLLOW|O_NONBLOCK`, requires the opened target to be a regular file, records its post-tool `(device,inode)`, and nofollow-verifies that the pathname still names that inode. Missing, open, symlink, nonregular, or post-open identity failure selects exact `tool stage ownership lost`, preserves an unclaimed entry, and publishes nothing. A regular replacement present at the supported tool's writer-joined handoff is tool-owned under W1/W12 and is therefore claimed. W13 then snapshots the reclaimed descriptor's identity/length/permission mode, accepts at most 8,589,934,592 bytes, rejects any larger value before hashing or publication-stage creation with exact `link output exceeds 8589934592-byte isolation limit`, hashes the source, and streams the accepted length through one 64-KiB buffer into a separately randomized publication stage, created only at that point and never named or opened by external code, before it resnapshots the tool stage's identity/length/permission mode and the publication stage's identity/length/permission mode/hash. Unequal source-before/source-after/destination length, permission mode, or hash, or unequal source identity, returns exact `link output changed during isolation`, performs identity-aware cleanup, produces `failed`, and publishes nothing. Equality nofollow-verifies the tool-stage pathname again before unlink; a matching entry is unlinked, a missing entry is already clean, while an unequal entry returns exact `tool stage ownership lost` and preserves the replacement without publication. W13 then nofollow-verifies the publication-stage pathname still names its retained descriptor identity before the existing atomic rename publishes only that isolated inode; missing or unequal identity returns exact `publication stage ownership lost` and preserves a foreign replacement. Under W1's handoff, no surviving descendant can mutate the source when isolation begins; a configured tool that leaves such a writer is outside W12's guarantees. Copy/open/mode/close/unlink failure remains an ordinary phase-specific link/publication failure and publishes nothing. | `ENCODED_OUT` is `WatchPath` over the absolute startup-cwd path for the existing `stem(FILE)` output. Final semantic reads are nonblocking, use the W8 streaming buffer, and replace the returned set's states with the final snapshot. Retaining original read-time identities until that revalidation means a node removed/retargeted after supplying bytes cannot disappear; hashing the final target means an in-place same-inode write cannot publish stale or torn observed bytes. Mutation after the final graph/hash/alias/probe snapshot retains W12's last-writer rule. Each attempt owns and drops private object and W14 probe stages, while W13 owns and drops its tool-output/publication stages; other link/publication errors remain ordinary failures and preserve last-good output. W13's isolation copy preserves the tool stage's executable mode and exact bytes but no pathname-derived external metadata; Align's executable link routes produce no required sidecar artifact. | Parameterized same-resource/state owner covers every W5 transition and direct slot, traversed node, parent alias, leaf symlink, existing and both-absent folded lookup, hard link, output absent, W14 intermediate-node/probe cleanup, in-place write, truncate/extend, and removal/retarget barriers before/open/after both original and final reads. Isolation barriers cover creation-descriptor release, nofollow/nonblocking current-inode reclamation, injected mutation after the first source snapshot and during copy, supported-tool writer joining plus direct-write and replacement routes, identity/length/mode/hash mismatch, tool-stage pre-unlink ownership loss, publication-stage pre-rename ownership loss, every stage operation failure, mode/byte parity, and final atomic publication. Real-binary owner runs last-good output after later failure. |
| W4 | Observed build inputs | The ordinary route uses `align_driver::build_path_pipelined_observed`, which owns the entry read and one call to the same private pipeline implementation as `build_package_pipelined`. The ThinLTO route uses `align_driver::build_path_per_unit_observed`, which owns the same read/observation front half and returns the existing `PerUnitWalk` consumed by the current ThinLTO CLI branch. Before retaining or issuing a filesystem call for any input, both form its absolute lexical path and apply W8 path validation with `ObservationFailed`. Otherwise they return the complete ordered `BuildInputSet` even when entry read or frontend fails. One call assigns encounter ordinals in this exact producer order: entry source first; the ordinary route's `--pgo-use` file at its existing pre-walk snapshot read; then newly discovered user modules in the existing breadth-first importer and source-import order; and each file-backed static source and checked-metadata path at its existing descriptor-validation read in unit/source order. Inline static sources and static paths rejected before root/path validation add no target; their owning Align source is already watched and is the only input that can repair that error. Missing paths are not canonicalized. W1's external link-time resources and workspace runtime-source freshness tree are trigger-excluded, not observation targets, even when `LIBRARY_PATH` or the baked workspace path names a project directory. Under watch, all path fields passed to the diagnostic renderer use W2 `WatchPath`; non-path diagnostic meaning/order remains unchanged before W2 record framing. | For each admissible read, the observer privately retains the bounded W6 graph immediately before open, the opened leaf's `(device,inode)` from `fstat`, and the graph immediately after open/read. The union is the read-time evidence of bytes actually consumed even if a link is removed/retargeted before W3. Its distinct key is `(logical input path, access path, node kind plus identity or raw target)`, so different identities at one path and both retry attempts survive deduplication. It retains at most 131,072 keys across one attempt/retry; the rejected next key is `ObservationFailed`. Each public record owns one bounded `PathBuf` and semantic state; regular state owns only `Hash128`/length. Trigger-excluded resource mutation follows W1 and never authorizes a cache artifact. | Parameterized library owner runs entry/import/static/metadata present, missing, replacement during every original/final read barrier, invalid-path exclusion, length/NUL boundaries, invalid UTF-8, producer/PGO-before-walk/duplicate/retry order, read-time identity, and arbitrary non-NUL path bytes through ordinary/ThinLTO; ordinary also covers PGO. Trigger owner replaces each external resource class or edits/restores the workspace runtime tree, proves no revision occurs from that alone, then proves a source edit and restart match the corresponding one-shot replacement or stale-runtime result. |
| W5 | Input, finalization, and repair records | `#[derive(Clone, PartialEq, Eq)] pub struct BuildInput { path: PathBuf, state: BuildInputState }`; `impl BuildInput { pub fn path(&self) -> &Path; pub fn state(&self) -> BuildInputState }`; `#[derive(Clone, Copy, PartialEq, Eq)] pub enum BuildInputState { Missing, Regular { content_hash: Hash128, len: u64 }, NonRegular, Unreadable }`; `pub struct BuildInputSet`; `impl BuildInputSet { pub fn inputs(&self) -> &[BuildInput]; pub fn changed_during_attempt(&self) -> bool }`; `pub struct FinalBuildInputSet`; `impl FinalBuildInputSet { pub fn inputs(&self) -> &[BuildInput]; pub fn changed_during_attempt(&self) -> bool }`; `#[derive(PartialEq, Eq)] pub struct WatchRepairDependency`; `impl WatchRepairDependency { pub fn path(&self) -> &Path }`; `pub struct FinalizedWatchInputs`; `impl FinalizedWatchInputs { pub fn inputs(&self) -> &FinalBuildInputSet; pub fn alias_index(&self) -> Option<usize>; pub fn repair_dependency(&self) -> Option<&WatchRepairDependency>; pub fn into_parts(self) -> (FinalBuildInputSet, Option<WatchRepairDependency>) }`; `pub struct BuildInputTopologyError`; `impl Debug + Display + Error for BuildInputTopologyError`; `pub fn merge_observed_build_inputs(first: BuildInputSet, retry: BuildInputSet) -> Result<BuildInputSet, BuildInputTopologyError>`; `pub fn snapshot_watch_repair(path: &Path) -> Result<WatchRepairDependency, BuildInputTopologyError>`; `pub fn finalize_watch_inputs(inputs: BuildInputSet, output: Option<&Path>) -> Result<FinalizedWatchInputs, BuildInputTopologyError>`; `pub enum BuildSourceError { Missing, NonRegular, InvalidUtf8 { offset: u64 }, Io { message: String } }`; `pub enum ObservedBuildAttempt { ObservationFailed { error: BuildInputTopologyError }, SourceFailed { error: BuildSourceError, inputs: BuildInputSet }, Pipeline { build: PipelinedPackageBuild, inputs: BuildInputSet } }`; `pub enum ObservedPerUnitBuild { ObservationFailed { error: BuildInputTopologyError }, SourceFailed { error: BuildSourceError, inputs: BuildInputSet }, Walk { walk: PerUnitWalk, inputs: BuildInputSet } }`. These are the complete new exports; fields/constructors/mutation/evidence stay private. | `inputs()` always contains one row per logical path sorted by ascending raw platform bytes. Within one call, the highest W4 encounter ordinal supplies a duplicate path's state; unequal state or evidence sets `changed_during_attempt`. Retry ordinals are ordered after every first-attempt ordinal, so retry state wins overlaps; merge unions evidence, ORs prior flags, sets the flag for any unequal overlap, adds retry-only paths, enforces W8's 32,768-row retry-merge limit before retaining the rejected next row, and re-sorts the union by path. The first aliased row in that same order is `alias_index`. `finalize_watch_inputs(Some)` performs W3 validation. When stable and aliased, it returns matching `alias_index` and an opaque repair dependency containing the bounded fixed output path plus alias-time W6 graph; otherwise both are `None`. `output=None` returns neither. Both public path arguments must be absolute and pass W8 validation before allocation or graph lookup. `snapshot_watch_repair` rebuilds the same bounded graph for transition/audit comparison. `into_parts` is the sole consuming projection and frees all read-time evidence; both components remain owned. `FinalBuildInputSet` is a distinct final-phase type with no conversion to `BuildInputSet`, so safe callers cannot pass consumed state to merge or finalization. Repair equality covers path plus every graph node/identity/raw target. | Exact export/trait inventory; external construction/mutation negatives; producer/duplicate/retry ordering; merge; finalization stable/unstable/alias/None; absolute/relative/NUL repair/output paths; repair snapshot/equality/changed-during-install/removal; evidence consumption; external compile-fail negatives for final-to-observed conversion/merge/refinalization; topology-error Display; and state/error owners. |
| W6 | Watch-set transition | A private topology graph resolves each absolute lexical W5 path with filesystem lookup semantics. It records every lexical and target-side directory-entry node visited, not only the final path: missing; directory `(device,inode)`; symlink `(device,inode,raw target bytes)`; leaf regular target `(device,inode)`; or other. Every existing node's tuple comes from nofollow metadata for that entry, so W3 can compare output against traversed nodes and filesystem folding is reflected by kernel lookup. Relative/absolute targets resolve through nested symlinks until a leaf, missing suffix, loop, or W8 bound. W3 finalization compares the current graph and semantic re-read with W4 evidence, returns the final semantic snapshot, then consumes evidence. An alias rejection additionally retains the fixed output path as `repair_output`; its complete W6 graph participates in handles and every event/audit comparison until a revision finds no alias or publishes successfully. Before waiting, W15 adds handles for every existing input/repair graph node and deepest existing directory above each unresolved suffix, rebuilds both graphs and semantic snapshot, and removes obsolete handles only after a stable pass. After success the next logical set is exactly the finalized inputs with no repair output. After failure, form the logical-path union from the current attempt plus absent last-success paths, using a checked union whose two at-most-32,768-row operands structurally cap retained watch state at 65,536 rows before graph construction, and retain only that revision's optional repair output. Before installing that failed baseline or waiting, W15 rebuilds the graph and W5 total semantic state for every union path; these refreshed current states, never the stored last-success states, become the comparison baseline. Earlier failed sets never contribute paths; entry is always present. | The watcher owns current input/repair graph paths, raw targets, native identities, and handles; read-time evidence exists only through W5 finalization. A loop is stable topology for W5 `Unreadable`; traversal excess is W8 error. Nofollow `ENOENT`, add `ENOENT`/`PathLost`, removal `ENOENT`/`PathLost`/`WatchLost`, and callback path/watch loss follow W15's nonfatal uncertain/replan contract. Other metadata, topology, snapshot, add, rollback, or removal failures are W9 fail-closed. Changed semantic state, changed input topology, any repair-output topology/identity change, or `changed_during_attempt` schedules one revision. A regular input inode-only change with identical state re-arms without revision unless it is also the retained repair output. W15's stable-baseline add-before-remove pass plus post-add rebuild closes ordinary registration gaps; its explicitly bounded tentative-generation compaction uses pre/post total snapshots and W7 audit as the correctness owner for the deliberate obsolete-handle gap. | Injected owner covers overlapping/disjoint exact-limit retry merges and exact-maximum failure retention, refresh of a changed last-success-only path after entry failure without repeated revisions, no-spin, all mutation barriers, output-named nodes, missing/symlink/cycle repair, every registration path-loss barrier/retry budget, repeated-generation compaction/bound, unexpected removal failure, and direct/folded/distinct-hard-link alias removal/replacement with native event deliberately dropped. |
| W7 | Event and verification model | Linux uses inotify and macOS uses the native file-event backend selected by the implementation dependency for low-latency wakes. An event for any W6 node, rename/replacement, or overflow wakes the loop through W9's shared pipe. The idle loop polls that read end with the current debounce, W15 transition-retry, or audit deadline, consumes at most 4,096 hint bytes per wake, and then checks atomic state; readable, hangup/error, and deadline are the only wait results. Events are collected until 50 ms of quiet or 250 ms from the first event, whichever comes first. Independently, after every completed no-change comparison the loop arms one fixed two-second audit deadline. Either an event drain or a due deadline rebuilds W6 topology, runs W15 registration, and reclassifies every logical input through W5's total classifier: missing lookup is `Missing`, existing nonregular target is `NonRegular`, open/stat/read failure is `Unreadable`, and a complete regular read streams its hash/length. Every prior-to-current state pair is compared, including recovery with unchanged topology; any semantic state change starts exactly one revision. A topology change also starts one revision except when a regular logical input has only an inode change, its `BuildInputState` is identical, and it is not the retained repair output; that W6 case re-arms without a revision. Every repair-output topology or identity change starts one revision. Otherwise the next two-second deadline starts after that comparison completes. The audit is the correctness owner for dropped/native-unreported events, permission/readability recovery, transient registration incompleteness, writable-`mmap` changes, and remote changes once ordinary reads expose their bytes; native events are the latency path, not correctness authority. Events received while compiling or comparing remain pending and cause a following comparison after the current attempt/scan completes. | Semantic classification failure is a W5 state, not a watcher error; only graph traversal other than expected loss, resource/backend-wide, unexpected handle, or control-pipe failure is fail-closed. The callback retains no event or path payload. A normal event stores the atomic dirty bit with `Release`; overflow, `PathLost`, and `WatchLost` store the atomic uncertain bit with `Release`. Only a backend-wide fatal asynchronous error maps to one fixed class: 1 `Disconnected`, 2 `Io`, 3 `Capacity`, 4 `InvalidConfig`, or 5 `Other`. W7 and W9 share one preallocated `AtomicU16` control word: bits 0..=2 encode signal 0 none, 1 SIGHUP, 2 SIGINT, 3 SIGQUIT, or 4 SIGTERM; bits 3..=5 encode fatal class 0..=5; bits 6..=15 remain zero. A backend callback uses a `Release` compare-exchange loop to set `class << 3` only while the fatal field is zero and preserves the signal field. The first fatal class wins and later fatal errors are discarded. Every event/error path then writes one byte to W9's nonblocking shared wake pipe, retrying `EINTR`; `EAGAIN` is successful coalescing because the atomics retain all control state. The owner keeps the read end open until the backend and all callback delivery are stopped, so no other write error is reachable. At each post-revision/idle wake and immediately before and after every W6/W15 transition operation, the loop performs one `Acquire` load of the shared control word. That single load is the post-return linearization point: a nonzero signal field wins over a simultaneously present fatal field, otherwise fatal wins over a synchronous unexpected operation error; expected path/watch loss has already become `uncertain` and is not an error candidate. Only after this control/error order does the loop inspect dirty bits, debounce, transition retry, or audit work. Event, signal, and timer wakes share one poll and one non-overlapping comparison; audits and transition retries never accumulate or overlap, and classification hashing retains only the W8 fixed buffer. Pipe reads retry `EINTR`; `EAGAIN` returns to poll, and EOF/other error enters the same final control snapshot as a synchronous watcher error. The loop `AcqRel`-swaps dirty and uncertain to false before, never after, a comparison so a concurrent callback cannot be erased. Neither control-word field is cleared before process exit. | Deterministic event/timer owner covers the complete 4x4 W5 state transition product, with explicit `Unreadable` permission/read recovery and read failure from each other state; W15 eight-attempt/50-ms churn and repeated tentative-generation compaction, quiet/max debounce, irrelevant/no-op events, pipe readability/error, bounded hint reads, coalescing, overflow/path/watch loss, rename, same-content regular inode replacement without a revision, repair-output identity replacement with a revision, dropped/no-event mutation, target-side missing creation, symlink topology, event-during-build/scan, comparison non-overlap, every fatal class/order, transition-error/control-state Cartesian order, and signal/fatal simultaneity. Native smoke covers replacement on each release OS and writable `mmap` on Linux. |
| W8 | Resource ceiling | Before retaining, encoding, allocating a graph for, or using a path as a filesystem-call argument, every absolute lexical input, fixed output, raw symlink target, and expanded graph path is checked in this order over borrowed raw bytes: length, first embedded NUL, then absolute-path shape where W5 accepts a public path. 1,023 bytes are accepted and 1,024 produce `path too long (maximum 1023 bytes; got LEN; hash HEX)`; an admitted path's first NUL produces `path 'ENCODED_PATH' contains NUL byte at offset OFFSET`; a relative public output/repair path produces `path 'ENCODED_PATH' is not absolute`. No rejected path reaches lookup or another side effect. One W4 producer call admits at most 16,384 distinct logical inputs; first-plus-retry merge admits 32,768; failure retention of that current set plus absent last-success paths is structurally bounded at 65,536 by its two admitted operands. The installed input-plus-repair W6 graph admits at most 65,536 distinct nodes, with at most 40 followed symlinks per logical input or repair output. One attempt/retry merge retains at most 131,072 distinct W4 evidence keys. Behind W9's fixed prefix, the remaining rejected-next suffixes are exactly `too many inputs (maximum 16384; next 'ENCODED_PATH')`, `too many merged inputs (maximum 32768; next 'ENCODED_PATH')`, `too many path components while resolving 'ENCODED_PATH' (maximum 65536)`, `too many read-time path components while reading 'ENCODED_PATH' (maximum 131072)`, or `too many symlink traversals for 'ENCODED_PATH' (maximum 40)`; each names the rejected logical path and exits before retaining/installing the rejected value. | Finalization may own at most 196,608 evidence-plus-current nodes and consumes evidence before transition. Add-before-remove may retain at most 131,072 logical records and 131,072 combined registrations. W15 permits eight immediate same-state registration replans, one 50-ms retry deadline, and at most one tentative generation. Before another generation is added, compaction reduces the retained union to at most the current 65,536 desired handles; the following at-most-65,536 additions therefore preserve the 131,072 simultaneous-registration bound across arbitrary churn. The backend owns those registrations, dirty/uncertain atomics, and one fixed 4,096-byte hint-read buffer, while W7/W9 share one control `AtomicU16` and W9 owns the shared pipe at the kernel's finite default capacity; it never requests a resize and pipe bytes carry no state. There is no second wake channel. Every watcher/control descriptor is close-on-exec before child spawn. Finalization/event/audit hashing uses one 64 KiB buffer. W2 owns one 4,096-byte record chunk plus one 12,331-byte maximum frame; W13 owns one process-global captured-child `AtomicBool`, one fixed armed guard record, two 4,096-byte read buffers, one fixed status record, two checked post-exit/pre-reap byte counters of at most 1,048,576, and at most one caller-allocated opaque panic payload at a time. W13 internal isolation accepts at most 8,589,934,592 tool-output bytes, owns at most two such simultaneously live output stages (17,179,869,184 bytes total), one 64-KiB copy/hash buffer, fixed source/destination identity/length/permission-mode records and `Hash128` snapshots; the rejected next byte is W13's exact isolation-limit error before second-stage creation. The pump neither copies nor grows that payload and retains no process list, output total, or cleanup timeout. Opens are nonblocking and `fstat`-regular. `LEN` and `OFFSET` are checked decimal byte counts; `HEX` is lowercase 32-digit `Hash128`; `ENCODED_PATH` is W2 `WatchPath`; `WatchPath` expands to 3,069 bytes; `WatchText` admits 16,384; all arithmetic is checked. Memo retains 768 MiB. | Exact/rejected-next length/NUL/absolute path and per-producer/retry-merge input, exact retained-watch maximum, combined graph, merged evidence, combined finalization, symlink, encoding/message/record-frame, disjoint transition/repair, registration churn/generation compaction, offending identity, sparse/FIFO, shared-pipe/control-fd close-on-exec, child-buffer/post-exit/pre-reap, output-isolation stage/buffer/identity/mode, control-word, and memo owners. |
| W9 | Failure and process exit | Source, frontend-cache, W3 alias/input-instability, codegen, PGO, link, and publication failures produce `failed` and keep watching. `ObservationFailed`, topology/resource/finalization failure, watcher initialization/transition failure, a fatal W7 backend class, W7 shared-wake read failure, W14 probe failure, and W2/W13 transcript/pump failure produce `alignc: watch: watcher error: MESSAGE` and exit 1. SIGHUP/SIGINT/SIGQUIT/SIGTERM request graceful stop; the first signal wins over a simultaneous backend error. An active revision finishes its ordinary result unless W2/W13 transcript/pump or W14 probe infrastructure fails, or W13 observes a graceful stop while a captured child is live. In the stop case W13 forwards the same signal, performs bounded grace plus mandatory group cleanup, and returns directly to W9 selection without `failed`; otherwise the revision waits every child, drops every stage, then emits exactly `alignc: watch: stopped by SIGHUP`, `alignc: watch: stopped by SIGINT`, `alignc: watch: stopped by SIGQUIT`, or `alignc: watch: stopped by SIGTERM` and exits 129/130/131/143. After mandatory probe/child cleanup, an infrastructure failure instead takes the post-return signal/fatal/error snapshot and emits only the selected stop or watcher error; an unavailable transcript emits neither. Idle handling is immediate because the shared pipe is the sole blocking wake; later signals coalesce; SIGKILL is ordinary immediate termination. | A process-global compare-exchange admits exactly one private signal installation. A second attempt fails before side effects. On the still-single-threaded CLI path, setup blocks SIGHUP, SIGINT, SIGQUIT, and SIGTERM in that order, saves the prior mask, creates the shared wake-pipe descriptors nonblocking and close-on-exec atomically where supported or applies `O_NONBLOCK` then `FD_CLOEXEC` to read and write ends before handler registration, registers handlers in that same order, publishes the owner, then restores the mask. W7 callbacks borrow the same write descriptor only while the native backend owner is alive. Any pre-publication pipe/flag/registration/mask failure rolls back while signals remain blocked: unregister in reverse order, close write then read, clear owner/guard, restore mask last. Publication transfers the handlers, atomics, guard, and both pipe descriptors to process-lifetime storage with no `Drop` path and no descriptor reuse. Every post-publication terminal path explicitly waits children, drops stages/handles, stops and drops the native backend, writes and flushes its final W2 line when possible, then calls `std::process::exit`; only kernel process teardown removes the handlers and closes the still-valid pipe descriptors. In the shared W7/W9 `AtomicU16`, each handler uses a compare-exchange loop to set bits 0..=2 to 1 SIGHUP, 2 SIGINT, 3 SIGQUIT, or 4 SIGTERM only while that field is zero, preserves the fatal field, and writes one byte after its successful or already-set decision, retrying `EINTR`; `EAGAIN` from a full pipe is successful coalescing. The retained read end makes every other handler write result unreachable, including during cleanup. No handler allocates, locks, formats, closes, or touches compiler state. | Child-process barrier and forwarded-stop owner covers every setup operation/failure and each pending SIGHUP/SIGINT/SIGQUIT/SIGTERM, then execs a helper during every child phase and proves all watcher/control descriptor numbers are closed there; it also proves explicit pre-exit cleanup and no staging residue. Shared-wake owner covers startup, idle, event, signal, timer, and infrastructure-failure multiplexing, readable/error/hangup, pipe-full signal precedence, backend shutdown with a still-live pipe, a signal at every terminal cleanup barrier, kernel-only handler/fd teardown, and no second wait primitive. Backend/pump/probe owners cover static fatal classes, mandatory cleanup, group-preserving descendants, direct detachment, W1's supported-tool writer-joined handoff, and W12's explicit detached-writer contract negative. SIGKILL and every other default-fatal process signal remain explicit no-cleanup process-failure exits; because W13 never exposes the publication stage to a child, they cannot mutate an already published candidate. |
| W10 | Cache and artifact identity | No persisted format, namespace, cache key, compiler fingerprint, interface, object, runtime, or link identity changes. An attempt consults the existing persistent caches and process memo in their settled order. Source edit/revert is content identity, never mtime identity. `ALIGNC_CACHE=off` disables persistent reuse but leaves the already-settled in-process memo active. ThinLTO and PGO keep their existing key spaces and validation order. | The only long-lived allocation is the memo and watch state. Cache rejection markers remain process/persistence authoritative. A watch event cannot authorize a cache key or artifact. W1's trigger-excluded external resources affect each actual codegen/link step exactly as in one-shot builds but remain outside compiler/cache identity; their mutation alone does not schedule that step. The workspace runtime-source tree likewise remains outside cache identity and runs its existing freshness digest only when a revision reaches pre-link validation. | Memo-on/off, cache-on/off, edit/revert, rejected-key, ThinLTO, PGO, external-resource, and workspace-runtime trigger owners reuse existing cache matrices plus one multi-revision driver owner. |
| W11 | Platform and installation | Supported release hosts are Linux and macOS on the architectures already built by CI. An unsupported native backend, signal-handler/pipe setup failure, or backend initialization failure is a watcher error; the universal W7 audit is not a fallback for an unavailable native backend. Release archives need no service file, launch agent, socket directory, or new executable. `alignc --version` and one-shot commands are byte-for-byte unchanged. | The native-watcher and signal-hook dependencies are linked into `alignc`; the timer uses the standard library. No runtime package or daemon ownership is added. | Linux x86_64/ARM64 and Apple Silicon CI build plus native replacement/signal smoke; release inventory remains unchanged. |
| W12 | Concurrent invocations | Revisions are serial within one watch process. Separate watch and one-shot compiler processes keep the existing independent-publication rule: each uses a private publication stage and atomic rename, each captured-output link additionally isolates its external tool stage, and the last successful rename to the same executable path wins. There is no cross-process or output-path lock. W2 markers attest only that their own process completed that revision; they do not reserve the path against external writers. | Watcher handles and memo state are process-owned. The only process-global mutations are W9's binary-owned singleton signal disposition/pipe lifetime and W13's captured-child lease. W9 failed overlap and partial setup restore there, while a successful watch exits without restoration; W13 overlap is side-effect-free and every pre-spawn or post-cleanup path releases its lease. Separate OS processes have independent dispositions and leases. An application that requires one publisher, protection from a same-uid process that scans or guesses private stage names, a tool that violates W1's successful-return handoff, or a filesystem transaction across W3's final alias/isolation snapshots must own that orchestration outside `alignc`, as it does for concurrent one-shot builds today. | Barrier owner runs two processes publishing the same final path, proves every observed image is complete, and accepts either winner; production-tool owners establish W1's writer-joined handoff for direct and internally spawning routes; W9's child owner and W13's lease/disposition owner close both same-process singleton states. |
| W13 | Child-output pump and public link seam | `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum LinkOutputStream { Stdout, Stderr }`; `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum LinkStopSignal { SigHup, SigInt, SigQuit, SigTerm }`; `pub trait LinkOutputSink { fn write(&mut self, stream: LinkOutputStream, bytes: &[u8]) -> std::io::Result<()>; fn stop_signal(&mut self) -> Option<LinkStopSignal> { None } }`; `pub fn link_objects_with_output(objs: &[&Path], exe: &Path, link_libs: &[String], profile: Profile, sink: &mut dyn LinkOutputSink) -> Result<(), String>`; `pub fn link_objects_instrumented_with_output(objs: &[&Path], exe: &Path, link_libs: &[String], profile: Profile, profile_rt: &Path, sink: &mut dyn LinkOutputSink) -> Result<(), String>`. These are W13's complete five-symbol export inventory. Existing `link_objects` and `link_objects_instrumented` retain inherited stdio and byte-for-byte behavior; watch uses only the matching `_with_output` function, including ordinary, PGO-use, PGO-instrument, ELF in-link strip, and macOS external `strip`. | Before pipe creation or spawn, each `_with_output` call acquires one process-wide captured-child lease; an overlapping call returns exact `child wait setup: another captured child is active` without side effects. It then queries `SIGCHLD` and accepts only `SIG_DFL` without `SA_NOCLDWAIT`; explicit `SIG_IGN`, `SA_NOCLDWAIT`, or another handler returns exact `child wait setup: incompatible SIGCHLD disposition`. The lease does not mutate signal state. Callers of this public seam must not change `SIGCHLD` disposition or reap children from unsafe/native code until return; the child pid is never exposed to safe callers. The `exe` argument is always the final output path. Each `_with_output` function itself exclusively reserves the private tool name with a regular inode, closes the creation descriptor before configured tools run, and passes only its private pathname to every configured tool. After final tool success and child cleanup it claims the current nofollow regular inode, verifies/copies it into a distinct unexposed publication inode, and atomically renames that isolated inode to `exe` only on complete success. It returns only after publication or identity-aware cleanup; callers receive no stage path, descriptor, token, or opportunity to publish the child-visible inode. Internal isolation assumes W1's supported-tool successful-return handoff; it detects mutation during its snapshots but cannot certify that an arbitrary surviving writer has become quiescent. Partial pipe/group/spawn setup drops the lease after closing every created descriptor. Immediately after successful spawn, one allocation-free private armed `CapturedChildGuard` owns all four parent pipe ends, pid/PGID, and lease; it closes both parent write copies before the first fallible pump operation; explicit completion performs the no-panic group-signal/direct-child-reap sequence and disarms it, while `Drop` performs that same sequence before any unexpected unwind can cross the link seam. Each `cc` or `strip` child is created as leader of one private same-effective-uid process group and spawned with separate blocking stdout/stderr pipe write ends. A process-group setup/spawn failure returns the existing phase-specific launch error before a child runs. All four original pipe ends are close-on-exec. Before spawn, the parent applies `O_NONBLOCK` only to its two read ends; read and write ends are distinct open-file descriptions, so command setup maps the still-blocking write ends onto child stdout/stderr, closes every other child end, and leaves only those two blocking standard descriptors open across exec. The parent closes its copies of both write ends immediately after spawn. One parent-owned loop verifies `getpgid(pid)` still equals the captured PGID and then calls `stop_signal` before each status observation and after each at-most-50-ms poll, then observes direct-child exit without reaping through `waitid` with fixed `P_PID`, `WEXITED`, `WNOHANG`, and `WNOWAIT` arguments before every poll, services stdout then stderr when both are ready, and reads at most 4,096 bytes per stream per cycle. The default control result is `None`. The first `SigHup`, `SigInt`, `SigQuit`, or `SigTerm` result closes both reads and sends matching SIGHUP, SIGINT, SIGQUIT, or SIGTERM to both the captured group and direct pid, retrying `EINTR` for each; an accepted send starts a fixed 250-ms monotonic grace, `ESRCH` skips directly to final pinned-group cleanup and reap, and another error records exact `child group cleanup: MESSAGE` and enters immediate SIGKILL cleanup. Later control results are not queried. Every path then performs mandatory group-SIGKILL/direct-reap cleanup. Absent an earlier retained error, the public function returns exact `child stopped by SIGHUP`, `child stopped by SIGINT`, `child stopped by SIGQUIT`, or `child stopped by SIGTERM`; the watch caller's already-stored W9 signal outranks both that sentinel and any cleanup error and emits only `stop`. Group-check/status/poll/read `EINTR` retries and `EAGAIN` returns to poll. A direct-child group mismatch records exact `child group changed`; another permanent group-check failure records exact `child group check: MESSAGE`, closes both reads, and enters cleanup. Each control query and nonempty read invokes the corresponding sink method inside `catch_unwind(AssertUnwindSafe(...))`. Both callbacks are synchronous and may block for an unbounded caller-controlled duration; the at-most-50-ms poll bounds only time spent in `poll`, not end-to-end stop detection. A nonempty read passes the matching stream and borrowed bytes; the sink cannot retain that borrow, and the trait need not implement `UnwindSafe`. A returned write `Err` records the first sink error. A panic from either method retains its first opaque payload, stops all later callbacks, closes both parent read ends, and enters mandatory group-signal/direct-child cleanup immediately; the process-global panic hook is neither replaced nor suppressed. Per-stream order is fixed; cross-stream order is stdout-before-stderr poll order and has no semantic meaning. The watch sink emits one W2 `child-stdout` or `child-stderr` `end` record per callback. | The pump retains two 4,096-byte read buffers, one fixed direct-child status record, and the watch sink borrows W2's common framed-record writer; none retains whole output. A returned sink error leaves both readable pipes open and drains/discards later chunks. A retained sink panic has already closed them and skips ordinary status/output completion. An unexpected permanent status failure after `EINTR`, including `ECHILD` after the admitted setup, records exact `child wait: MESSAGE`, closes both reads, and enters the same mandatory group-signal/direct-child cleanup path as a pump failure instead of returning. A permanent poll/read failure while the non-reaped direct child is live records the first error, closes both parent read ends immediately, sends SIGTERM to the private process group, and waits without reaping until a fixed 250-ms monotonic deadline. Every ordinary, child-failure, sink-error, sink-panic, status-failure, and pump-failure outcome keeps the direct group leader unreaped while sending SIGKILL to both the captured group and direct pid, retrying `EINTR` until each send is accepted or returns `ESRCH`. A non-`ESRCH` kill error is recorded once as exact `child group cleanup: MESSAGE` and retried every fixed 10 ms; it does not release the stage, lease, or caller. Only after accepted/`ESRCH` group and direct-pid SIGKILL does the parent reap the direct child through `EINTR` when it remains waitable, and it never uses the numeric PGID again. The unreaped leader pins that PID/PGID through the final group signal, eliminating post-reap reuse. Accepted/`ESRCH` direct-pid SIGKILL followed by reap is the no-further-user-execution boundary only for the direct child. An accepted group send proves delivery was queued, not that each group member has stopped, and W13 does not wait for or claim quiescence of descendants. Successful publication therefore requires W1's writer-joined handoff; group cleanup may leave only descendants that hold no writable descriptor or pathname access to the tool output. After completing its group/direct-pid signal sends and direct-child reap, W13 releases the child lease; on child success it then reopens the private pathname read/write with `O_NOFOLLOW|O_NONBLOCK`, establishes the current regular-file identity, and performs the isolation and publication sequence before returning. SIGKILL to the alignc process is the sole force-stop escape from an uncooperative or no-longer-signalable group. After ordinary direct-child exit is observed but before this cleanup, the pump performs one `FIONREAD` query per still-open pipe, stdout then stderr, for the exact queued byte count. Counts through 1,048,576 are accepted; a larger count records `child stdout read: buffered output exceeds 1048576 bytes` or the stderr sibling, and a negative count uses `invalid buffered-byte count`. It reads at most each accepted snapshot count in 4,096-byte chunks, so later descendant writes cannot extend the quota, then closes both read ends without waiting for EOF. Query/read failure closes both ends immediately. Without a panic, the first sink/group-check/status/poll/read/group-cleanup error wins over direct-child status or a stop sentinel; direct-child status otherwise retains ordinary build meaning. With a retained panic, cleanup errors cannot replace its payload: after final group/direct-pid SIGKILL/`ESRCH`, waitable-child reap, and lease release, `resume_unwind` resumes that same payload on the calling thread without invoking the panic hook again. No caller return can occur before the final group/direct-pid SIGKILL/`ESRCH`, direct-child reap when waitable, lease release, and either complete isolated publication or identity-aware stage cleanup. Exact export/trait inventory and an injectable pump/process owner cover both legacy inherited functions, both new functions, ordinary/instrumented/strip routes, external sink implementation, callback/control order/borrow/default-stop/blocking-delay behavior, blocking-child/nonblocking-parent flags, empty/arbitrary/all-byte/marker-shaped output, simultaneous streams, capacity backpressure, `EINTR`, `EAGAIN`, child/forwarded-stop/sink-return/sink-panic/group-check/status/poll/read/query/early-EOF/group-cleanup failure, unchanged panic-hook/resumed-payload behavior, accepted/rejected `SIGCHLD` states, overlap, partial-setup rollback, creation-descriptor release, current-inode reclamation, armed-guard Drop at every post-spawn barrier, post-exit limits, descriptor inheritance, direct group change and in-group descendants after every direct-child outcome, normal drain/close/pinned-group-and-pid-KILL/reap, forwarded-stop/grace/pinned-group-and-pid-KILL/reap, permanent-error TERM/grace/pinned-group-and-pid-KILL/reap, direct and internally spawning production-tool writer-joined handoff, explicit no-guarantee contract negative for a deliberately detached writer, and decoded one-shot parity under equivalent pipe-backed stdio. |

| W14 | Absent-slot collation proof | After W3's stable final semantic/graph snapshot and existing lexical/identity alias checks, but before link, every absent W6 input graph node is considered, including the first missing intermediate component as well as a missing logical leaf. A node whose resolved immediate parent and the missing output's resolved parent have the same `(device,inode)`, whose output lookup is one component, and whose raw component differs from the output leaf receives a collation probe. W3 opens and identity-checks the output parent as part of its final graph snapshot; through that retained common-parent descriptor, W14 exclusively creates `concat(OUTPUT_LEAF, SUFFIX)`, then nofollow-looks up `concat(INPUT_COMPONENT, SUFFIX)`. The same inode proves that the two absent public slots are one filesystem-folded entry and returns W3's existing alias rejection naming the owning logical input. `ENOENT` proves the names distinct for that snapshot. Components below a first missing parent have no existing parent slot and cannot denote the output entry; exact raw path/node equality remains W3's earlier check. | `SUFFIX` is exact ASCII `.aw-` plus 32 lowercase hex digits from 128 bits of OS randomness. Each pair gets at most 16 exclusive-create attempts. The retained parent supplies `NAME_MAX`; `LIMIT` is `min(NAME_MAX, 1023)`. Before allocating a probe value or performing probe-entry I/O, both suffixed components must be at most `LIMIT` bytes; otherwise exact `collation probe 'ENCODED_OUT'/'ENCODED_INPUT': suffixed component too long (maximum LIMIT; got LEN)` fails closed, where `LEN` is the larger rejected length. An unavailable/invalid `NAME_MAX`, randomness, create, nofollow lookup, identity, cleanup, and retry failure use exact `collation probe 'ENCODED_OUT'/'ENCODED_INPUT': MESSAGE`. `ENCODED_INPUT` is the logical input owning the absent node. One retained close-on-exec directory descriptor, one close-on-exec regular-file descriptor, and two fixed 1,024-byte component buffers are live at a time. | The probe touches neither public slot and reserves `.aw-` plus 32-lowercase-hex component names for cooperative Align processes while active. Immediately before cleanup unlink, it nofollow-compares the current name with the retained opened inode. Missing or unequal identity closes the retained fd, preserves the current name, and uses `probe ownership lost` as `MESSAGE` in W9's exact cleanup payload; equal identity permits unlink, followed by required `fstat` link count zero before close. Cleanup failure is a watcher error and link never starts. Concurrent Align processes never replace a claimed name and collision retries never unlink a name this process did not create; post-check replacement by a non-Align process is outside the explicit pathname-only concurrency contract. Injectable directory operations own parent open/identity, case-sensitive, ASCII-case-folded, Unicode-normalized, both-absent leaf and intermediate nodes, parent aliases, exact/raw-equal, deeper unresolved suffixes, 16-collision, randomness/`NAME_MAX`/create/lookup/identity/pre-unlink-replacement/unlink/fstat/close failure, cooperative collision, exact/rejected-next `LIMIT`/`LEN`, public-slot nonmutation, foreign-entry preservation, and no-residue cells; native macOS smoke proves the host's actual folded lookup. |

| W15 | Registration churn | A W6 transition distinguishes one stable baseline generation from at most one retained tentative generation. From a stable baseline it keeps every old handle until one complete add/post-add-snapshot pass is stable. Desired additions are raw-path-sorted and each successful new handle is recorded in insertion order. If the prior pass retained a tentative old-plus-new union, the next transition performs no addition until a fresh graph/semantic snapshot matches installed state. A difference schedules one revision while retaining that same union. Equality forms desired registration keys from raw path plus W6 native identity, retains at most the newest installed matching handle for each key, removes every nonmatching or older duplicate in raw-path/generation order, marks that at-most-65,536 remainder as the new baseline, and only then may add a next generation. Expected removal loss is idempotent; another compaction failure follows W9. Compaction deliberately trades native-event coverage of obsolete paths for the universal W7 audit and pre/post total snapshots, but keeps every currently desired handle already present. The complete post-transition snapshot runs after compaction even when no addition is needed. Synchronous add `ENOENT`/`PathLost` is expected topology churn: remove only that pass's new handles in reverse order, treating removal `PathLost`/`WatchLost` as idempotent success, set `uncertain`, and rebuild graph plus total semantic state. A difference from the installed logical/repair state schedules one revision immediately; an unchanged rebuild retries the pass at most eight times. After the eighth unchanged path-loss race, old handles remain installed, obsolete removal does not start, and the shared poll arms one fixed 50-ms transition-retry deadline. The next timer/event comparison starts a fresh eight-attempt budget. There is no spin and the two-second audit remains armed after a completed no-change comparison. | A post-add snapshot that observes change retains that bounded old-plus-new superset as the sole tentative generation and schedules the revision; only a stable post-add snapshot permits ordinary obsolete removal. Because each desired generation has at most 65,536 registrations, compaction reduces a retained union to at most 65,536 before at most 65,536 additions, so repeated changed snapshots never exceed W8's 131,072 simultaneous-registration ceiling. Nofollow `ENOENT` forms a W6 `Missing` node, while removal `ENOENT`/`PathLost`/`WatchLost` means already absent. Other metadata/add/remove errors, rollback failure other than those idempotent classes, capacity/resource exhaustion, and backend-wide failure remain W9 errors. W7 callback `PathLost` and `WatchLost` likewise set `uncertain` and wake the shared pipe instead of setting the control word's fatal field. | One injectable registration owner covers disappearance/reappearance before each add, after every successful prefix, during rollback, at post-add snapshot, and before each obsolete removal; unchanged eight-attempt exhaustion, 50-ms retry, callback path/watch loss, alias repair removal, event-dropped audit recovery, stable eventual add-before-remove, three-or-more consecutive changed post-add snapshots with pre-add compaction and fixed peak count, compaction-race audit recovery, partial-handle Drop, unexpected rollback/remove failure, resource failure, signal/fatal precedence, and exact no-exit/no-spin behavior. |

Unless a line is explicitly a W2 terminal marker, every exact output string in
W1–W15 names decoded record payload. W3 instability/alias text uses `notice`,
W9 watcher failures use `watcher-error`, and graceful-stop text uses `stop`.

After W3 completes input and output-alias validation, the selected W13
`_with_output` function exclusively precreates the randomized tool stage,
reserves its private pathname with a regular inode, closes the creation
descriptor, and gives external tools only that pathname. LLVM 22 `lld`
deliberately replaces its output inode, so neither retaining the descriptor nor
requiring the initial identity is compatible with that supported route. After
W13 releases its child lease, that same function opens the path read/write with
`O_NOFOLLOW|O_NONBLOCK`, requires a regular file, records the post-tool
identity, and nofollow-verifies the name again. A regular inode at the trusted
tool's W1 writer-joined handoff is the tool result whether the tool wrote
through or replaced the reservation; same-uid scanning or guessing remains
W12's explicit external-writer boundary. Only then does W13 create the distinct
publication stage through the same same-directory exclusive allocator.
External tools receive neither the publication name nor its descriptor.
Isolation order after tool-path reclamation is retained-source
`fstat`, identity/length/permission-mode snapshot, 8-GiB length check, first
streaming source hash, exclusive publication-stage creation, exact-length copy,
permission-mode application, second source
`fstat`/hash, destination `fstat`/hash, and identity/length/mode/hash equality
decision. Permission mode in W13 isolation is exactly `st_mode & 0o777`; no
set-id or other inode metadata is copied. W13 then nofollow-compares the
tool-stage name with its still-open descriptor: a matching entry is unlinked,
a missing entry needs no cleanup,
and an unequal entry is preserved and returns exact `tool stage ownership lost`
without publication. It next nofollow-compares the publication-stage name with
its still-open descriptor, closes, then atomically renames. Missing or unequal
pre-rename identity preserves the foreign entry and returns exact `publication
stage ownership lost`. `EINTR` retries and a
premature source EOF or extra byte is `link output changed during isolation`.
The first operational error is retained; cleanup keeps each descriptor open
while it nofollow-compares the pathname, removes only a matching identity,
treats a missing path as already cleaned, preserves an unequal current entry,
and then closes the descriptor. A reportable ownership or cleanup error
replaces the result only when it is the first error. An unexpected unwind runs the same
identity-aware stage guards without publishing or deleting a replacement. A
same-uid process that mutates a publication name after the final identity check
or discovers it by scanning/guessing is the explicit W12 external-writer case,
not a child-containment guarantee.

The complete additive export inventory is the union of W5's
input/finalization/repair surface and W13's link-output surface. W5's
"complete new exports" statement is scoped to its record surface; its
field/constructor privacy rule does not apply to W13's externally implementable
sink. `LinkOutputSink::write` is synchronous on the calling driver thread, the
borrowed byte slice is valid only for that call, and the trait requires neither
`Send` nor `Sync`. W13's five symbols are its complete new exports and complete
the item 5 inventory beyond W5. A sink refusal records exact
`child stdout output: MESSAGE`
or `child stderr output: MESSAGE`, continues draining both pipes, and is
returned after the final pinned-group signal, direct-child reap when waitable,
and lease release. The watch sink retains its first W2
transcript error before returning the refusal sentinel, so W2's more specific
`transcript write: MESSAGE` or `transcript flush: MESSAGE` wins over that
derived output error. Concatenating successful callbacks separately by stream
reconstructs the direct child's bytes through the accepted post-exit/pre-reap bound;
the watch framing means no child byte can become an unframed marker or another
record kind. A permanent live-child poll/read error selects W13's bounded
TERM/grace/KILL/direct-child-reap path; an unexpected `ECHILD` status failure is
exact `child wait: MESSAGE` and enters that same cleanup path. A
failed post-exit/pre-reap queued-byte query, read, or EOF
before the snapshotted count records exact `child stdout read: MESSAGE` or the
stderr sibling. The pump owner retains the 50-ms no-output child-exit case,
output larger than one buffer, exact/over-limit/negative counts, the child
descriptor table, and the complete wait/snapshot/drain/close sequence.

W13's captured-child lease owns one process-global `AtomicBool`; it serializes
only the two `_with_output` functions and is released on every pre-spawn
failure. The inherited-stdio functions do not acquire it. The `SIGCHLD`
disposition is checked after lease acquisition and before the first pipe
allocation. Safe callers cannot obtain the private pid, while mutation of the
process disposition or native reaping during the call is an explicit unsafe
caller violation rather than an `ECHILD` success path. Even then, an observed
`ECHILD` enters mandatory group-signal/direct-child cleanup and cannot release
the stage or lease early.
`catch_unwind` applies only around the externally implemented sink callback;
the pump itself has no panic-to-error conversion. The armed guard's explicit
and `Drop` cleanup paths are no-panic, retain the original payload through
child/group cleanup, and call
`resume_unwind` only after releasing the lease. The existing process panic hook
runs for the original panic exactly as Rust normally specifies and is never
temporarily replaced; resumed unwind does not run it again.

The private watch sink's `stop_signal` performs only an Acquire load of the
shared control word's first-signal field and maps
SIGHUP/SIGINT/SIGQUIT/SIGTERM to
`SigHup`/`SigInt`/`SigQuit`/`SigTerm`; it never locks, formats, allocates, or
writes a record. `None` is the only result for ordinary external sinks unless
they explicitly opt into the same stop contract. The 50-ms value bounds only
one pump poll; synchronous `write` and `stop_signal` callbacks may block and
there is no end-to-end graceful-stop latency bound. A pending signal is
observed at the next control checkpoint after the callback returns; callers
that require prompt graceful stop must keep both methods prompt, while SIGKILL
remains the force-stop escape. Forwarded stop is not
a watcher error or `failed` attempt: mandatory child cleanup returns its exact
internal sentinel, then W9's already-stored signal wins the post-return
snapshot and emits the sole `stop` record.

The matching group signal retries `EINTR`. An accepted send starts the 250-ms
grace; `ESRCH` skips directly to the pinned-leader final group signal and reap,
while another error records exact `child group cleanup: MESSAGE` and enters
immediate SIGKILL cleanup. For an external sink without W9's pending signal,
that first cleanup error outranks the stop sentinel; under watch, W9's
already-stored signal outranks both. Every stop path sends final SIGKILL to
both the captured group and direct pid while the unreaped leader pins both
identities, and reaps that child when waitable before the stage, lease, or
caller can proceed.

W2's statement that revision 1 starts immediately is conditional on the final
startup snapshot selecting continuation. Before that linearization point there
is no revision to mark, so a selected stop or initialization error follows the
startup rule below and emits no `started` record.

W7 has no user-configurable audit interval or event-only mode. Its semantic
`stat` failure means `fstat` of an already opened candidate leaf; W6 nofollow
`ENOENT` follows W15, while other metadata and graph traversal failures remain
fail-closed topology errors. The
W7 owner retains already-full-pipe coalescing, multi-level target-side missing
creation, two-fatal-error first-wins ordering, and idle signal/fatal
simultaneity in addition to the complete state product.

W9's watcher-transition failure category excludes every W15 expected
`ENOENT`/`PathLost`/`WatchLost` case. Those cases produce no diagnostic or exit;
only an unexpected operation class, exhausted resource, or backend-wide fatal
field in the shared control word can select `watcher-error`.

W8's rejected logical-path identity is the bounded owner key of the operation:
the observed input for input/evidence limits and traversal, or the fixed repair
output for repair-graph traversal. It is never an unbounded intermediate path.
W14's pre-unlink identity check reuses the retained parent/probe descriptors and
allocates no additional path.

After CLI validation creates the private W2 transcript, W9 publishes the
signal owner and restores the startup mask. The CLI then takes one Acquire load
of the control word before native-watcher construction. It then takes W7's
single-load signal/fatal snapshot plus any synchronous error after every
initialization operation and once more immediately before the first `started`
record. A
selected signal stops and drops any initialized backend, emits only the exact
W9 stop line, and exits 129/130/131/143 without a `started`, revision, or `ready`/
`failed` marker. A fatal or synchronous initialization error likewise emits no
`started`. The final pre-`started` snapshot is the startup-to-active
linearization point; a signal stored later belongs to the active first revision
and follows W9's ordinary active-stop rule.

In W14, a nofollow lookup that finds a distinct inode at the alternate random
name is a collision: the process cleans its own probe and retries with fresh
randomness. Only the same inode proves folding; `ENOENT` proves distinct names;
sixteen collisions select the bounded two-path error. Supported Linux/macOS
component comparison is congruent under appending the same ASCII suffix;
injectable casefold/normalization owners and native macOS smoke pin that
platform premise.

W14 visits W5 input rows in their public raw-byte order and each row's absent
graph nodes in W4/W6 encounter order. The first probe/resource failure stops
finalization; otherwise the first row with any folded node supplies W5's
`alias_index`, preserving the lowest-row alias rule independently of native
directory enumeration.

Within one W14 pair the deterministic order is parent `NAME_MAX`, checked
decimal `LIMIT`/`LEN`, OS randomness, exclusive create, alternate nofollow
lookup, then owned-probe cleanup. A cleanup error wins the tentative
same-inode/`ENOENT`/distinct-inode result; only successful cleanup may return an
alias, distinct-name result, or collision retry.

Every W14 path before cleanup, including collision, randomness, `NAME_MAX`,
create, and lookup failure, either created nothing or removes its own probe.
`unlink` retries `EINTR`; another unlink or link-count failure closes the
retained fd, emits exact `collation cleanup 'ENCODED_COMPONENT' for
'ENCODED_OUT'/'ENCODED_INPUT': MESSAGE`, and may leave only that random private
component. The diagnostic therefore identifies the recoverable residue; no
cleanup path touches a public input/output slot or a collision name.

The `.aw-` plus 32-lowercase-hex component space is reserved for active Align
W14 probes in a probed output parent. Cooperative Align processes exclusively
create their own random component and never remove or replace another process's
component. Immediately before unlink, cleanup nofollow-looks up that component
through the retained parent and compares its identity with the retained probe
fd. Missing or unequal identity means ownership was lost: cleanup closes its
fd, does not unlink the current component, and emits exact `collation cleanup
'ENCODED_COMPONENT' for 'ENCODED_OUT'/'ENCODED_INPUT': probe ownership lost`.
A non-Align process that removes or replaces a reserved component after this
identity check violates the explicit probe-namespace concurrency contract; no
safe pathname-only Unix primitive can atomically compare identity and unlink.
The barrier owner covers replacement before the check and concurrent Align
collisions; neither path removes the foreign component.

W9's active-revision ordinary-completion rule excludes W2/W13 transcript/pump
and W14 probe infrastructure failures. W14 always finishes probe cleanup and
W13 always finishes its mandatory child cleanup before terminal selection. If
either operation succeeds, a pending signal remains deferred until after that
revision's ordinary marker. If it fails, the post-return Acquire snapshot
selects signal, then the fatal backend slot, then the synchronous probe/pump
error; a selected signal emits `stop` without inventing `failed`, while a
selected infrastructure error emits `watcher-error` without `ready`/`failed`.
An unavailable W2 transcript can emit neither and retains its exit-1 rule.

A retained external W13 sink panic is not a W9 synchronous error and never
reaches terminal selection: W13 completes cleanup and resumes the original
payload. The private watch sink has a no-panic owner over every callback/error
result, so ordinary watch execution never exposes the process panic hook on its
protocol stream. An external sink's unchanged hook output belongs to that
library caller, not to a W2 watch transcript.

During W13's permanent-error grace, direct-child exit is observed without
reaping, so its pid/process-group id cannot be reused. At the 250-ms deadline
the parent sends the same final SIGKILL to the captured group and direct pid as
every other outcome while that leader remains unreaped. Accepted/`ESRCH`
direct-pid SIGKILL followed by reap closes further user execution only for the
direct child; the parent then never consults the numeric PGID again. W13 does
not prove descendant quiescence. Successful publication instead depends on
W1's supported-tool handoff: any surviving in-group or detached descendant has
already released every writable descriptor and pathname access to the tool
output before isolation begins.

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
`path 'ENCODED_PATH' contains NUL byte at offset OFFSET`,
`path 'ENCODED_PATH' is not absolute`,
`too many inputs (maximum 16384; next 'ENCODED_PATH')`,
`too many merged inputs (maximum 32768; next 'ENCODED_PATH')`,
`too many read-time path components while reading 'ENCODED_PATH' (maximum 131072)`,
`too many path components while resolving 'ENCODED_PATH' (maximum 65536)`,
`too many symlink traversals for 'ENCODED_PATH' (maximum 40)`, or one of W9's bounded
phase/path/message forms without the outer watcher-error prefix. The path-too-
long variant computes `LEN` and lowercase 32-digit `HEX` through the caller's
borrowed bytes before cloning or issuing a filesystem call; it retains only
those fixed-width values. Every other path-specific variant retains the
admitted logical input or public output/repair `WatchPath`; a global rejected-
next limit retains the admitted path whose insertion would cross the bound.
Other variants retain only admitted `WatchText` and fixed counters.
`finalize_watch_inputs` consumes its input even on `Err`, so read-time evidence
cannot escape or be reused.

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
fallible success-line write on the protocol stream and flush
ready or failed marker
control checkpoint: graceful signal, then fatal backend slot
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
2  private transcript construction, then signal-handler and shared wake-pipe
   installation
3  post-signal-owner-publication atomic control-word snapshot
4  initial native-watcher construction, with one atomic control-word snapshot
   plus the synchronous error after each operation
5  final pre-started atomic control-word snapshot; this linearizes startup
   versus the active first revision
6  per-path length, first-NUL, absolute-public-path, per-producer logical-input, and
   read-time-evidence limits during observation
7  first/retry observation merge and its 32,768-row/evidence limits, when retried
8  ordinary source/frontend/cache/codegen result
9  finalize_watch_inputs(None): evidence consumption for a failed candidate;
   or finalize_watch_inputs(Some): current topology/resource,
   final semantic state, read-time stability, existing output alias, then W14
   absent-slot collation proof and cleanup
10 W13 captured-child lease and `SIGCHLD` disposition, pipe/group/spawn setup,
   child-output stop/write return/panic or group-check/status/poll/read result,
   forwarded stop/grace when selected, pinned-group SIGKILL, child reap when
   waitable, lease release, resumed sink panic or link; then W3 tool-output
   identity/hash/length snapshot, isolated copy, resnapshot/destination hash,
   tool-stage unlink, and publication-stage rename
11 successful-build protocol-line write
12 single protocol-stream flush, terminal-marker write
13 one atomic control-word snapshot: graceful signal field, then fatal field
14 65,536-row retained-watch merge, then combined-input-repair-topology limit
15 W15 tentative-generation state comparison and compaction when present,
   then watch/repair-handle addition with at most eight immediate expected-loss
   replans; unchanged exhaustion arms the 50-ms retry without an error
16 stable post-registration input/repair topology and semantic snapshot
17 obsolete-handle removal, with already-lost handles as idempotent success
18 event/debounce/transition-retry/audit timer delivery
```

The step-13 control snapshot is repeated before and after every step 14–17
operation. W7's single Acquire load is the post-return linearization point and
selects its signal field, then its fatal field, then any synchronous failure;
that snapshot is final. A signal or fatal class stored after that load belongs
to the following control checkpoint.

For every other multi-invalid watch transition, the lowest numbered failure
wins. An overflow is not an error and cannot outrank a build failure; it
requests the post-attempt state comparison. A watcher error after `ready` does
not retract the published executable, but the process exits nonzero because it
cannot promise another revision.

A pending graceful stop is control flow, not a watcher/build error. Signal and
fatal state are checked immediately after an active revision's terminal marker
and before/after each W6 transition operation in W7 order. The active revision's
ordinary result therefore precedes the stop line unless W13 observes the signal
while a captured child is live; an idle signal has no invented revision. In that
child case the private sink reports the parent's first signal at the next
control checkpoint. One pump poll lasts at most 50 ms, but synchronous sink
callbacks are explicit unbounded backpressure and may delay that checkpoint.
W13 then forwards the signal to the private child group and direct pid,
escalates after 250 ms, sends final SIGKILL to both while the direct leader
still pins their identities, and returns without a
`failed` marker before the parent drops stages and emits `stop`.
Terminal-generated and parent-only SIGHUP/SIGINT/SIGQUIT/SIGTERM therefore have
the same bounded child-cleanup path.

Decoded `watcher-error` record payloads identify the failing phase and path
when one exists:
`initialize: MESSAGE`, `transcript write: MESSAGE`,
`transcript flush: MESSAGE`,
`child wait setup: MESSAGE`,
`child stdout read: MESSAGE`, `child stderr read: MESSAGE`,
`child output poll: MESSAGE`, `child wait: MESSAGE`,
`child group changed`, `child group check: MESSAGE`,
`child group cleanup: MESSAGE`,
`child stdout output: MESSAGE`, `child stderr output: MESSAGE`,
`wake read: MESSAGE`,
`add 'ENCODED_PATH': MESSAGE`, `snapshot 'ENCODED_PATH': MESSAGE`,
`remove 'ENCODED_PATH': MESSAGE`, the W8 exact resource messages,
`collation probe 'ENCODED_OUT'/'ENCODED_INPUT': MESSAGE`,
`collation cleanup 'ENCODED_COMPONENT' for 'ENCODED_OUT'/'ENCODED_INPUT': MESSAGE`,
`backend: disconnected`, `backend: io`, `backend: capacity`,
`backend: invalid config`,
`backend: other`, or `revision counter exhausted`, each behind the single
decoded `alignc: watch: watcher error: ` prefix. `ENCODED_PATH` is always W2
`WatchPath`; `ENCODED_OUT` and `ENCODED_INPUT` are its absolute-path form, and
`ENCODED_COMPONENT` applies the same byte codec to one raw component without a
separator. Dynamic `MESSAGE` is `WatchText`; the underlying OS message is
informational and has no control-flow meaning. Asynchronous backend failures
use only the static class strings above; callback-owned dependency messages and
paths are never retained. Failure to write this line is the W2 unavailable-
transcript exit, not a recursive watcher error.

The native adapter's classification is exhaustive and ordered: refusal because
one watched lookup no longer exists is nonfatal `PathLost`; explicit per-handle
invalidation/removal is nonfatal `WatchLost`; callback-stream termination or
transport disconnect is fatal `Disconnected`; an attached backend-wide OS I/O
error is fatal `Io`; descriptor or backend watch exhaustion is fatal
`Capacity`; an unsupported backend option/state is fatal `InvalidConfig`; and
every remaining dependency variant is fatal `Other`. Overflow is the third
nonfatal uncertain-bit case with path/watch loss. The adapter owner pins every
dependency variant to nonfatal uncertainty or one fatal class so an update
cannot silently change or omit the mapping.

### Implementation closure matrix

This is one implementation capability. Input observation without the
foreground consumer is a dormant producer, while a watcher without exact
producer-owned inputs either misses changes or scans unrelated state. The
expected hand-written diff may exceed 1,000 lines because keeping these halves
together avoids a second path-discovery algorithm and one unreviewable gap
between read identity and event registration.

| Cell | Required closure | Planned implementation and owner |
|---|---|---|
| D1 CLI parity | Every existing build flag/default/error composes with W1; every other verb rejects `--watch`. Startup configuration and both external-resource/runtime-freshness trigger boundaries are resolved/described once. | `main.rs` parser/help table and `watch_build` real-binary matrix, including exact help and no revision for an external replacement or workspace runtime-source edit alone. |
| D2 input formation | Formation; length/NUL/absolute validation with offending identity; private construction/read-only access; exact producer, duplicate, and retry order; move/return; merge; observation-to-final typestate transition; repair-token formation/consumption; success replacement; and failure merge cover every W5 state and ordinary/ThinLTO/PGO path. The complete additive surface is W5 plus W13. | One observer plus exact exported-surface negative/trait inventory; parameterized owners cover entry-then-PGO-then-walk producer order, row/alias ordinals, merge state/evidence limits, finalization modes, final-to-observed conversion/merge/refinalization compile-fail negatives, repair snapshot/equality, every rejected path boundary and identity, external link-sink implementation, and Drop on every result. |
| D3 read/register/publication race | A semantic/topology change around original/final reads or W15 registration cannot publish stale/torn bytes, replace a supplying resource, expose a child-writable publication inode, wait on the wrong bytes, or exit on expected path loss. Existing, exact-missing, and filesystem-folded both-missing output/input slots are rejected before link. Rejected output aliases remain repair dependencies until their topology changes and a recovery revision revalidates them. | One semantic/graph authority serves observation, finalization, repair token, existing alias validation, W14 common-parent collation proof, and W15 bounded add-snapshot-remove; barriers cover writes/replacements/folding/hard links, both-absent parent/leaf combinations, probe identity loss before unlink, concurrent Align collisions, every registration path-loss position, alias-time-to-install removal, tool-output mutation after the first isolation snapshot and during copy, W1 writer-joined handoff for direct and internally spawning production tools, permission-mode drift, identity-aware tool/publication-stage cleanup, and dropped-event audit recovery. |
| D4 event lifecycle | Startup-before-init, initialization, pre-`started`, normal/identical/burst/max-debounce/full-pipe/overflow/path-loss/watch-loss events, event during compile, W15 immediate/timed retry and tentative-generation compaction, the complete prior/current 4x4 W5 state product including readability recovery, every fatal class/first-error order, periodic audit, 16,384/32,768/65,536-row failed-set and repair-output merge bounds, refreshed failure baselines, identical-content inode replacement with and without repair-output role, idle/active four-signal shared-pipe wake, signal/fatal/synchronous-transition precedence, and terminal cleanup wake/stop exactly once. | Injectable event/timer/error source, one shared pipe, dirty/uncertain atomics plus the exact shared signal/fatal `AtomicU16` control word, startup and every before/after-transition control snapshot, W15 retry budget/deadline/generation state and repeated-churn peak bound, total semantic classifier, logical/repair merge owner including last-success-only mutation hidden by entry failure, bounded-read poll owner, backend shutdown, and process teardown; native adapters stay thin. |
| D5 build control paths | Initial/repeated success; ordinary/ThinLTO cardinalities; semantic/alias rejection and repair including W14's both-absent folded slot; retry/PGO/cache combinations; child output/success/failure/abort/detachment; edit/revert/malformed input; and both W1 trigger-excluded boundaries preserve ordinary pre-link/link/publication semantics. Both W13 `_with_output` entry points cover ordinary, PGO-use, PGO-instrument, ELF strip, and macOS strip without changing the legacy entry points. | Existing owners plus `watch_build` revision, retry, finalization, collation/repair recovery, exact five-symbol W13 inventory, external sink default/four-state forwarded stop and return/panic with resumed payload, accepted/rejected `SIGCHLD`, captured-child overlap, pump/pinned-process-group-and-pid cleanup, supported-tool direct-write/replacement and two-stage output isolation, last-good publication, external/resource-freshness trigger, and route/cardinality parity owners. |
| D6 concurrency and cleanup | Worker panic cleanup remains; watcher callbacks and signal handlers do not block, while W13 sink callbacks have explicit unbounded caller-controlled duration; atomics select overflow/fatal/signal; evidence, repair tokens, W14 random probes, W15 partial handle sets, pump buffers/fds/process groups, captured-child lease, workers, stages, handles, control fds, and direct children do not outlive owners. Signal setup is singleton and rollback-complete before publication; afterward the still-installed handlers and shared pipe remain process-lifetime with no close/reuse race. Every watcher/control/probe fd is close-on-exec. Descendant-retained, continuously written, or permanently unreadable child-output pipes cannot extend the fixed queued-byte quota or introduce an EOF wait; synchronous sink callbacks remain the explicit source of unbounded wall-clock delay before descriptor close, final group/direct-pid SIGKILL sends, and direct-child reap. W13 claims no descendant quiescence after an accepted group signal; W1 separately requires every supported tool to join output writers before successful return, and W12 gives a violating detached writer no artifact guarantee. | Existing panic owner plus finalization/repair Drop, W14 create/lookup/pre-unlink-identity/unlink/link-count/fd cleanup and cooperative-collision barrier, W15 reverse rollback/idempotent loss/tentative-generation compaction/unexpected failure, watcher atomics, every startup/signal barrier, backend shutdown while the process-lifetime pipe remains live, terminal `process::exit`, exec descriptor-table negative, W13 lease/disposition/partial-setup rollback, armed-child-guard Drop at every barrier, and unwind-safe sink cleanup/resumed unwind, blocking-write/nonblocking-read, every-outcome drain/pinned-group-and-pid-KILL/reap, forwarded stop at the next control checkpoint, one at-most-50-ms poll for prompt sinks, explicit blocking-callback delay, then matching signal/250-ms grace/pinned-group-and-pid-KILL/reap, permanent-error TERM/grace/pinned-group-and-pid-KILL/reap, direct-group-change detection, production-tool writer joining, explicit detached-writer contract negative, two-publisher barrier, and bounded child owners. |
| D7 identity and authorization | Memo, persistent frontend, packaged fallback, object, ThinLTO, PGO, compiler/LLVM, target, and runtime identities are unchanged. Rejected frontend keys remain rejected across every revision and process. | Existing cache codec/rejection owners plus multi-revision exact-key test. |
| D8 resource bound | Exact/rejected-next length/NUL/absolute-path/per-producer/retry-merge input, exact retained-watch-input maximum, combined-input-repair-graph/evidence/finalization/symlink/encoding/message/frame/post-exit limits and each bounded offending identity; W14 descriptor `NAME_MAX`, 1,023-byte effective component limit, 36-byte suffix, 16 attempts, one directory fd plus one probe fd and two fixed 1,024-byte buffers; W15 eight immediate attempts, 50-ms deadline, one tentative generation, and pre-next-generation compaction within existing handle maxima; W13 one lease bit, one armed guard record, one caller-owned panic payload, fixed status record, existing at-most-50-ms pump poll interval without an end-to-end stop bound across synchronous callbacks, and 250-ms abort grace; W13 exact/rejected-next 8-GiB tool output, at most 16 GiB across two output stages, one 64-KiB buffer, and fixed identity/length/mode/hash isolation snapshots; shared wake pipe/control word; non-overlapping scan; 64 KiB hash and fixed transcript/pump buffers; nonblocking special-file rejection; checked counters; and memo refusal are reachable. | Parameterized limit/merge/repair/encoding/frame/pump/snapshot/isolation/overflow/fatal/FIFO/sparse/RSS/timer/probe/registration-generation owners assert the encoded logical path or path pair, or fixed length/hash for the overlong case, on every refusal. |
| D9 observable transcript | The single stderr protocol stream fixes startup-without-`started`, marker/prerequisite order, marker atomicity, write/flush/wait-setup/sink/group-check/status/poll/read/group-cleanup failure, no-stdout behavior, reversible frames for every nonterminal record, instability/alias/collation/resource/backend/wake/pump errors with offending identity, non-diagnostic W15 expected path/watch loss, decoded in-process/child logical channels, object/link order, behavior, and exits for success/failure/recovery/watcher error. Active-child SIGHUP/SIGINT/SIGQUIT/SIGTERM forwards the same signal and emits only the terminal `stopped` line after cleanup, never `failed` or watcher error. Linked executables are not byte-compared on macOS. | One-pipe golden transcript and no-panic private-watch-sink owner with every startup checkpoint, kind/state/chunk boundary, all-byte and marker-shaped diagnostic/cache/message/path/child injection, decoded bounded parity, short marker writes, full/partial/flush/wait-setup/sink/group-check/status/poll/read/group-cleanup errors, terminal-generated and parent-only SIGHUP/SIGINT/SIGQUIT/SIGTERM during every child phase with prompt and blocking callbacks, pipe-backed one-shot child-stream parity, tool-output isolation, stage-path replacement, permission-mode drift, supported-tool writer joining, mutation/repair/collation/registration-compaction, exact limits/classes/identities, and one-shot object/link/program comparison. |
| D10 non-impact | One-shot build/run/size, both legacy inherited-stdio link entry points, `align-repl`, inspection verbs, release layout, and public input/output slots remain unchanged except for the additive flag/help text, W5 records, W13 `LinkStopSignal`/sink/link entry points and their explicit default-control/blocking-callback semantics, internal two-stage publication and `SIGCHLD` caller contracts, W14's reserved private probe namespace, and the external-resource/runtime-freshness trigger boundaries. | Existing bounded gate, W14 public-slot nonmutation plus cooperative-concurrency, clean-path no-residue, and cleanup-failure residue-identity owners, legacy/new link-route parity under equivalent pipe-backed stdio, external-sink default/opt-in four-signal stop and internal publication-isolation owners, REPL owners, release workflow structural test, usage/help snapshot, and parameterized external-resource/runtime-source mutation owner. |

The author-side matrix-to-diff pass must bind every applicable cell to an
implementation site and a discriminating owner before the implementation
review. A review finding about a missed input, registration window, event
overflow, cache authorization, or cleanup reopens that entire class across D2,
D3, D4, D6, and D7 rather than patching one path.

### Design review closure

This is chronological review evidence. When a later reopened matrix changes a
mechanism named by an earlier row, the later closure and W1–W15 ledger
supersede that mechanism; the earlier row remains the record of what triggered
the redesign.

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
| P2: W12 denied process-global mutation while W9 installed signal handlers | W9 now owns a process-global singleton with side-effect-free second-install rejection and exact partial-setup rollback; W12 named that then-sole mutation and limited concurrency claims to separate OS processes/output publication. The later W13 lease revision extends the inventory explicitly. D6 owns setup, overlap, rollback, and successful process-exit lifetime. |

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
| P2: dirty/uncertain bits could not carry an asynchronous backend failure or deterministic first message | W7 mapped callbacks to the then-seven static classes in one `AtomicU8`; compare-exchange preserves the first class and the one-slot channel is only a wake hint. W15 later reclassifies per-handle path/watch loss as uncertainty, leaving five fatal classes. W9 fixes signal precedence and the exact lines; D4/D6/D9 own every class, two-error ordering, and a full wake channel. |
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
| P2: the complete topology-error inventory omitted the logical-input limit | W8 and the complete `BuildInputTopologyError` list both name `too many inputs (maximum 16384; next 'ENCODED_PATH')`; validation order and the rejected-next owner include it. |
| P2: the signal pipe and event channel gave the idle loop no single wait | W7/W9 replace the channel with one nonblocking shared wake pipe whose bytes are hints and whose atomics retain event/error/signal state. D4/D6 own poll, coalescing, precedence, and backend-before-pipe cleanup. |
| P2: post-reap draining could follow a continuously writing descendant forever | W13 snapshots each queued byte count once, enforces a 1,048,576-byte bound, drains only that quota, and closes. D6/D8/D9 own accepted/rejected counts, continuous writers, and decoded parity. |

These fixes define public ordering plus the two blocking wait boundaries, so
they remain one implementation strategy and receive a fresh full review.

That review found three remaining stream and teardown gaps and reopened the
matrix on `single-transcript-and-process-lifetime-signal`:

| Finding | Redesigned closure |
|---|---|
| P2: a stderr readiness marker could be dequeued before prerequisite stdout | W2 moves every watch-mode record to one stderr protocol stream and leaves stdout unused. Same-descriptor order now places diagnostics, child frames, cache/success lines, and flush before the atomic marker; D9 owns one-pipe observation. |
| P2: nonblocking pre-dup child write ends exposed `EAGAIN` to tools | W13 leaves each write end blocking and applies `O_NONBLOCK` only to the distinct parent read open-file descriptions. D6/D9 own pipe-capacity backpressure and decoded one-shot parity. |
| P2: a live handler could write through a closed/reused wake descriptor | W9 permits rollback only before publication. Published handlers and pipe descriptors have process lifetime and no `Drop`; after explicit compiler/backend cleanup, `std::process::exit` gives them directly to kernel teardown without a reuse window. D4/D6 own signals at every terminal barrier. |

The transcript descriptor, child pipe flags, and signal descriptor lifetime are
one subprocess I/O strategy and receive a fresh full review.

That review found three final protocol/input-order gaps and reopened the matrix
on `framed-record-and-post-revision-precedence`:

| Finding | Redesigned closure |
|---|---|
| P2: ordinary in-process text could spoof an unframed terminal marker | W2 frames every nonterminal logical record with fixed kind/state, bounded raw chunks, and reversible byte encoding; terminal lines are the only unframed bytes. W8/D9 own the maximum frame and marker-shaped diagnostic/cache payloads at every chunk boundary. |
| P2: W7 and the validation table disagreed on fatal-versus-transition failure | W7 plus the numbered order use one Acquire snapshot before and after each transition: signal, fatal class, then operation error. D4/D9 own the Cartesian multi-invalid order and selected transcript. |
| P2: workspace runtime-source freshness reads were absent from the trigger inventory | W1/W4/W10 classify the baked tree separately as trigger-excluded. Mutation alone is quiet; the next observed edit or restart matches one-shot stale-runtime checking, and installed absent/unreadable trees remain no-op. D1/D5/D10 and acceptance own edit/restore. |

Record framing, terminal-result selection, and the complete pre-link input union
are one externally observable watch protocol and receive a fresh full review.

That review found three remaining public-seam and input-identity gaps and
reopened the matrix on
`link-seam-state-reclassification-and-error-identity`:

| Finding | Redesigned closure |
|---|---|
| P2: watch could reach linker and strip children only through inherited-stdio public functions | W13 adds the exact externally implementable sink plus ordinary and instrumented `_with_output` functions, covers every linker/strip route, and leaves both legacy functions unchanged. D2/D5/D10 own the complete export and route inventory. |
| P2: an unchanged regular inode could recover from `Unreadable` without a topology change | W7 reclassifies every logical input on both event and audit comparisons and compares the complete 4x4 W5 state product. D4 owns explicit permission/read recovery and dropped-event witnesses. |
| P2: path-specific topology errors omitted the path that must be repaired | W8 and `BuildInputTopologyError` retain and display the admitted encoded logical path for every path-specific or rejected-next refusal; only an overlong path uses its bounded length/hash identity. D2/D8/D9 own every exact payload. |

The exported link seam, total semantic state comparison, and bounded error
identity are implementation-defining contracts and receive a fresh full
review.

That review found a publication-safety P1 plus two startup/child liveness gaps
and reopened the matrix on `absent-slot-startup-and-pump-abort`:

| Finding | Redesigned closure |
|---|---|
| P1: two absent names that fold to one filesystem slot had no identity to compare | W14 adds a retained-parent, random-suffix collation proof before link. It detects ASCII-case and Unicode-normalized equivalence without touching either public slot, and W3/D3 keep the resulting alias as a repair dependency. |
| P2: a stop delivered after signal publication could wait through revision 1 | The startup-to-active boundary now has an Acquire snapshot before backend construction, after every initialization operation, and immediately before `started`. D4/D9 own every pending signal/error product and the no-invented-revision transcript. |
| P2: a permanent live-child read error could leave the child blocked on its full pipe | W13 gives each captured child a private process group and closes both reads plus performs bounded TERM/grace/KILL/direct-reap on a permanent poll/read failure. D5/D6/D8/D9 own normal sink drain separately from mandatory abort. |

The P1 changes the publication proof and the two P2 findings share its
pre-publication/early-exit lifetime boundary. W3/W14 collation, W9 startup, and
W13 process-group cleanup therefore remain one revised implementation boundary
and receive a fresh full review before implementation.

That review found one registration-liveness P1 and one child-quiescence gap,
so the patch loop stopped and the matrix was reopened on
`registration-churn-and-descendant-quiescence`:

| Finding | Redesigned closure |
|---|---|
| P1: a watched path disappearing during handle addition was treated as a fatal transition error | W15 makes synchronous add loss a bounded replan over a fresh W6 graph and total semantic state, keeps old coverage until a stable add-snapshot pass, and moves asynchronous per-handle loss into W7's nonfatal uncertainty path. Changed state schedules a revision; eight unchanged races defer one retry for 50 ms without spinning. D3/D4/D6/D8/D9 own every loss position, rollback, deadline, fatal-class distinction, and transcript. |
| P2: the direct child could exit while an in-group descendant retained a pipe or modified the stage | W13 observes direct exit without reaping, drains only one bounded queued-byte snapshot, closes both reads, then sends SIGKILL to the still-pinned private group before reaping the leader. The same order now covers success, child failure, and sink failure as well as the existing live-child pump-abort path. D5/D6/D8/D9 own every direct-child outcome, group-cleanup failure, and post-return stage/pipe quiescence. |

The P1 changes which native registration failures are recoverable, while the
P2 changes the successful child-lifetime boundary rather than only an error
path. W6/W7/W15 transition state and W13 process-group cleanup therefore form
one newly reviewed implementation strategy. This full redesigned diff requires
a fresh independent review before implementation.

That review found another child-lifetime P1 and a probe-ownership race, so the
matrix was reopened on `sigchld-and-probe-namespace-ownership`:

| Finding | Redesigned closure |
|---|---|
| P1: inherited `SIGCHLD` auto-reap state or an unproved group-kill failure could release the stage while a child remained live | W13 acquires one process-wide captured-child lease before allocation, rejects every disposition except `SIG_DFL` without `SA_NOCLDWAIT`, and makes unsafe disposition mutation/native reaping an explicit public caller violation. Every direct-child outcome retains the stage and lease through SIGKILL, waitable-child reap, and a 10-ms group-existence loop until `ESRCH`; neither `ECHILD` nor a kill/check error is an early return. D5/D6/D8/D9 own disposition, overlap, rollback, error precedence, and group-absence proof. |
| P2: pathname cleanup could unlink a replacement installed under the random W14 name | W14 reserves its high-entropy component namespace for cooperative Align probes, performs a nofollow retained-fd identity check immediately before unlink, and preserves an unequal or missing current entry with exact ownership-lost cleanup failure. Concurrent Align instances never replace a claimed name; mutation by a non-Align process after the final identity check is explicitly outside that pathname-only concurrency contract. D3/D6/D10 and the probe barrier owner cover replacement before the check plus concurrent collisions. |

The P1 changes process-global admission and the return boundary for every
captured tool, while the P2 changes the filesystem-concurrency contract used by
the publication proof. W13 child ownership and W14 probe ownership therefore
require one fresh full-diff review before implementation.

That review found an unwind P1 plus two phase/resource ownership gaps, so the
matrix was reopened on `unwind-phase-state-and-generation-bounds`:

| Finding | Redesigned closure |
|---|---|
| P1: an external sink panic could unwind past captured-child cleanup and permanently retain the lease | W13 arms a private captured-child guard immediately after spawn and catches only the sink callback with `AssertUnwindSafe`. It retains the original opaque payload, closes both reads without further callbacks, completes immediate KILL/reap/group-absence cleanup, disarms/releases the guard and lease, and then calls `resume_unwind`; any other unexpected unwind invokes the same no-panic guard `Drop`. It never replaces the process panic hook or turns the panic into an ordered diagnostic. D5/D6/D8 own panic before/after output, every post-spawn guard barrier, hook count, payload identity, child/stage absence, and immediate lease reuse. |
| P2: `into_parts` returned evidence-consumed state as the same type accepted by merge/finalize | W5 introduces the distinct `FinalBuildInputSet` phase returned by `FinalizedWatchInputs`; no constructor or conversion recreates `BuildInputSet`, and both merge and finalize continue to accept only the evidence-bearing observed type. D2 owns external compile-fail negatives for conversion, merge, and refinalization. |
| P2: consecutive changed post-add snapshots could accumulate unbounded handle generations | W15 permits only a stable baseline plus one tentative generation. Before another add, an unchanged total snapshot compacts the retained union to currently desired handles; a changed snapshot schedules a revision without adding. W8 proves at most 65,536 retained desired plus 65,536 new handles, and D4/D6/D8 own three-or-more changed generations, compaction races, and the fixed peak. |

All three findings concern a value that crossed its owning phase without a
closed transition: unwind skipped cleanup, finalization erased evidence without
changing type, and a retained handle generation became the next baseline.
W5/W13/W15 now make each transition explicit and bounded. The resulting full
diff requires a fresh independent review before implementation.

That review found two consistency and liveness gaps and reopened the matrix on
`foreground-child-stop-and-read-order`:

| Finding | Redesigned closure |
|---|---|
| P2: moving each captured tool into a private process group prevented a terminal-generated Ctrl-C from reaching that tool while W9 waited for it | W13 adds `LinkStopSignal` and a default-`None` `stop_signal` control method. The private watch sink maps W9's first signal without allocation or output; the pump observes it within the existing 50-ms poll bound, closes reads, forwards the matching signal, allows 250 ms, then performs mandatory KILL/reap/group-absence cleanup. W9's signal outranks the internal stop sentinel and cleanup error, so terminal-generated and parent-only SIGINT/SIGTERM emit only the terminal `stopped` line after quiescence. D5/D6/D8/D9/D10 own the public seam, callback/control ordering, error precedence, both delivery modes, and every child phase. |
| P2: W4 listed PGO observation after the importer even though the shared pipeline reads it before walking imports | W4 now fixes the actual temporal order as entry, ordinary `--pgo-use` snapshot, then breadth-first imports and static/metadata reads. D2 and the stale-retry owner assert that exact producer order independently of the public raw-byte sort. |

The first finding changes the public sink seam and foreground-stop strategy,
not merely one owner assertion. The full redesigned diff therefore requires
one fresh independent review before implementation.

That review found two child-lifetime P1s plus three resource/protocol gaps. The
matrix was reopened on `tool-output-isolation-and-pinned-group-cleanup`:

| Finding | Redesigned closure |
|---|---|
| P1: a descendant could leave the captured process group and later mutate the stage | W13 now declares ownership only of the direct child and descendants that preserve its assigned group, verifies the direct child's PGID at every pump barrier, and requires external callers to keep the output private unless their tools honor that contract. Watch does not rely on cooperation for publication: W3 gives tools one random stage, copies a stable identity/hash/length snapshot into a second random stage that external code never sees, unlinks the tool inode, and publishes only the isolated inode. A detached helper can outlive W13 by its own tool owner but cannot mutate the published candidate. D3/D5/D6/D8/D9/D10 own direct detachment, a detached writer at every copy barrier, isolation failure, and post-publication mutation attempts. |
| P1: SIGHUP or terminal SIGQUIT could take the default exit while a private-group tool survived | W9's complete graceful-control set is now SIGHUP/SIGINT/SIGQUIT/SIGTERM with conventional 129/130/131/143 exits, and W13 forwards the matching four-state `LinkStopSignal`. Every other default-fatal signal remains an explicit no-cleanup process failure, but W3's unexposed publication stage means such an exit cannot publish or leave a child able to mutate a public candidate. D4/D6/D9 own all four setup, idle, active-child, cleanup-barrier, terminal-generated, and parent-only cases. |
| P2: current-attempt plus last-success inputs could exceed the nominal installed limit | W8 distinguishes the 16,384-row producer limit and 32,768-row first/retry merge with exact rejected-next errors. W6's failure retention is a checked union of current and absent last-success rows, so its two admitted operands structurally cap it at 65,536 before graph construction without an unreachable extra refusal. W5/W6 and D2/D4/D8 own overlap, disjoint maxima, error precedence, Drop, and the resulting graph/registration ceilings. |
| P2: reaping the leader before probing its numeric PGID allowed unrelated reuse | W13 keeps the observed direct leader unreaped through final SIGKILL to both the captured group and direct pid. Accepted/`ESRCH` results prevent further user execution by the direct child and group-preserving owner set; only then does it reap the leader and it never consults either numeric identity again. D5/D6 own every outcome, kill error/retry, the pinned identities, and an immediate unrelated PID/PGID reuse attempt after reap. |
| P2: child-stream byte parity was impossible when legacy one-shot stdio was a TTY | W2/D9 scope byte parity to a one-shot invocation whose stdout and stderr are separate pipes, matching W13's non-TTY descriptor mode. Terminal-backed legacy behavior remains unchanged and is a non-impact check, not a byte-equality oracle. |

The P1 response changes the artifact-publication boundary and the child owner,
while the signal and PGID changes alter every captured-tool exit. W3/W9/W13
therefore form one redesigned containment strategy and require a fresh
full-diff review before implementation.

That review found a remaining child-lifetime P1 and one error-inventory gap, so
the matrix was reopened on
`mandatory-publication-isolation-and-topology-error-completeness`:

| Finding | Redesigned closure |
|---|---|
| P1: an accepted process-group SIGKILL did not prove descendants had stopped, yet an external caller could publish the same inode when tools preserved the group | W13 now claims a no-further-execution boundary only for the reaped direct child. Every `_with_output` caller, without exception for tool behavior or group membership, passes a private tool path and after successful return copies and verifies it into a distinct unexposed inode before making an executable path reachable; error and unwind paths drop it unpublished. Watch uses W3 as the sole implementation: group-preserving and detached descendants may continue briefly but can reach only the tool inode. D3/D5/D6/D8/D9/D10 own the external-caller negative, both descendant classes, mutation after accepted group signal, and isolated-byte/mode publication. |
| P2: the complete topology-error inventory omitted retry-merge overflow | The complete `BuildInputTopologyError::Display` set now includes exact `too many merged inputs (maximum 32768; next 'ENCODED_PATH')`, matching W5/W8 validation order and the reachable 32,769th-row refusal. D2/D8/D9 own the exact message, offending identity, pre-retention failure, and evidence Drop. |

The P1 removes tool cooperation as a publication precondition and changes the
public `_with_output` caller contract across every route. This latest W13/W3
boundary and the synchronized W5/W8 error inventory require one fresh
full-diff review before implementation.

That review found two ownership/liveness P1s and two consistency gaps, so the
matrix was reopened on
`stage-path-ownership-mode-and-callback-liveness`:

| Finding | Redesigned closure |
|---|---|
| P1: a surviving descendant could replace the child-visible tool-stage pathname and make cleanup unlink the replacement | W3 exclusively precreates the tool inode, reserves the private tool name and closes the creation descriptor before spawn; the later LLVM 22 closure supersedes this review's initial-inode-preservation assumption by claiming the current regular tool result only after W1's writer-joined handoff. It verifies pathname ownership after W13 and again immediately before unlink: matching is removed, missing is already clean at final cleanup, and unequal is preserved with exact `tool stage ownership lost` and no publication. Ordinary error cleanup and no-panic unwind guards apply the same identity-aware rule to both stages. The unavoidable same-uid mutation after the final comparison remains W12's explicit external-writer boundary. D3/D5/D6/D9 own supported-tool direct-write/replacement plus success/error/unwind replacement at every cleanup barrier and preservation of the foreign inode. |
| P1: a synchronous sink callback could block past the claimed 50-ms graceful-stop bound | W13 keeps synchronous non-`Send` callbacks and removes the false end-to-end latency promise. Fifty milliseconds bounds one pump poll only; a blocking `write` or `stop_signal` delays the next control checkpoint without a deadline, and SIGKILL remains the force-stop escape. Prompt sinks retain one-poll detection. D5/D6/D8/D9/D10 own prompt, blocked-then-returned, permanently blocked/force-stopped, sink-error, and signal-precedence cases. |
| P2: output isolation did not revalidate executable mode | W3 snapshots permission mode with source identity/length before copy, reapplies it to the destination, then requires equal source-before/source-after/destination length/mode/hash plus stable source and publication-stage identities. Mode-only drift selects exact `link output changed during isolation`; D3/D8/D9 own source and destination drift at both barriers and stable mode parity. |
| P2: current prose still claimed accepted group SIGKILL stopped in-group descendants | W13's live normative prose now limits the no-further-execution boundary to direct-pid SIGKILL plus reap and explicitly permits either descendant class to continue briefly against only the private tool inode. D6 and acceptance use that same boundary. |

These findings change stage Drop ownership, isolation validation, and the public
sink's graceful-stop contract. The latest W3/W13 boundary requires one fresh
full-diff review before implementation.

That review found one remaining public-ownership P1 plus control and status
consistency gaps, so the matrix was reopened on
`sealed-publication-and-atomic-control-arbitration`:

| Finding | Redesigned closure |
|---|---|
| P1: an external `_with_output` caller could open a descendant replacement after return and publish bytes not owned by the original tool result | The two `_with_output` functions now own the complete artifact transition. Their existing `exe` argument is the final output, while W13 internally reserves the private tool name, releases the descriptor during tool execution, performs child cleanup, then claims the current regular tool-result inode, verifies/copies into an unexposed publication inode, and atomically renames only the isolated inode before returning. No stage path, descriptor, or token crosses the public seam, so every caller gets the same post-tool claimed-identity proof and cannot publish child-visible bytes. W3 supplies the validated final path and classifies W13's result. D3/D5/D6/D8/D9/D10 own external calls, every tool route, replacement barriers, publication, error, and unwind. |
| P2: separate signal and fatal loads could select fatal when a signal arrived between them | W7/W9 replace the two atomics with one `AtomicU16`: bits 0..=2 retain the first signal and bits 3..=5 retain the first fatal class. Signal and backend compare-exchange loops preserve the sibling field; one Acquire load is each control checkpoint's linearization point, and its decoded signal field precedes its fatal field and synchronous error. D4/D6/D8/D9 own stores on both sides of the load, both CAS orders, first-within-field retention, full-pipe wake coalescing, and the exact selected transcript. |
| P2: item 4 was marked shipped while the release-distribution owner still called it unimplemented | `docs/impl/11-release-distribution.md` now records PR #893's shipped archive tree, immutable fallback, native package verification, and v4 LLVM identity in present tense. The item 4 ledger, release owner, and HANDOFF status agree. |

The P1 moves artifact ownership across the public W13 boundary, while the
control word changes signal/fatal arbitration at every watcher checkpoint.
This latest W7/W9/W13 strategy requires one fresh full-diff review before
implementation.

That review found two remaining P1s and one topology-rule inconsistency, so the
matrix was reopened on
`trusted-tool-handoff-and-refreshed-failure-baseline`:

| Finding | Redesigned closure |
|---|---|
| P1: a surviving output writer could replace the tool inode before W13's first isolation snapshot, making both snapshots agree on bytes the direct child did not hand off | W1 now classifies configured `cc`/linker/strip executables as trusted code-generation inputs and requires successful direct-child return to join every output writer and release every descendant's writable descriptor/path access. W3/W13 make that precondition explicit and no longer claim that pathname secrecy, process-group signaling, or two-stage copying certifies an arbitrary detached writer's quiescence. W12 assigns violating configurations no artifact guarantee. Production-route owners cover direct and internally spawning tools; the deliberate detached-writer fixture is a contract negative, not an isolation proof. |
| P1: a failed attempt could reinstall stale state for a last-success-only dependency and rebuild forever | W6 forms the bounded logical-path union as before, then W15 rebuilds every union graph and W5 total semantic state before installing the failed baseline or waiting. A dependency changed while entry failure prevents rediscovery therefore contributes its current classification once; D4 and acceptance own one revision followed by stable wait rather than a stale-state loop. |
| P2: W7 said every topology change rebuilds while W6 exempted identical-content regular inode replacement | W7 now carries W6's exact exception: an inode-only change with identical regular state and no repair-output role re-arms without a revision, while every repair-output identity/topology change rebuilds. D4 and acceptance own both sides. |

The first P1 narrows the external-tool trust boundary, and the second changes
failed-baseline installation. This latest W1/W3/W6/W7/W12/W13 strategy
requires one fresh full-diff review before implementation.

Linux x86_64 verification of the implementation found that LLVM 22 `lld`
replaces a precreated output inode rather than writing through it. Retaining an
open descriptor merely prevents the freed inode number from being immediately
reused and exposes that replacement deterministically. The matrix was reopened on
`tool-stage-descriptor-release-and-identity-reclamation`:

| Finding | Redesigned closure |
|---|---|
| P1: W3 required a retained open tool descriptor and preservation of its initial inode, while LLVM 22 `lld` replaces its output inode, making the required production linker fail closed on every watch link | W3/W13 keep the exclusively reserved randomized name but close the creation descriptor before any configured tool runs. The phase state contains only that private path, so no accidental descriptor retention is representable. After the final supported tool and child cleanup, W13 opens the name read/write with `O_NOFOLLOW|O_NONBLOCK`, requires a regular file, records that current identity, and nofollow-verifies the name before isolation. Under W1's writer-joined handoff and W12's exclusion of same-uid private-name discovery, a current regular replacement is the trusted tool's output and is claimed; missing, symlink, nonregular, or a replacement after reclamation selects exact `tool stage ownership lost` and preserves the unclaimed entry. Failure routes likewise claim and remove a current regular tool output before returning, while symlink/nonregular replacements remain preserved. D3/D5/D6/D9 own descriptor release, LLVM 22 `lld` replacement, direct-write parity, every reclamation failure, unclaimed-entry preservation, and the unchanged two-stage isolation proof. |

This changes the W3/W13 tool-stage ownership strategy and requires one fresh
full-diff review of the implementation candidate.

That review reported two liveness P1s and six observable, cleanup, and
complexity P2s. The sink-error P1 contradicted the pump's consume-before-error
branch, but the remaining findings exposed a common failed-state and
one-shot-parity gap, so the matrix was reopened on
`failure-state-liveness-and-observable-parity`:

| Finding | Closure |
|---|---|
| P1: semantic static-input rejection was recorded as `Unreadable`, so final classification of the unchanged regular file forced endless revisions; rejected metadata symlinks had the same mismatch | `observe_consumed_classification` treats an absent producer classification as a request for the W5 total filesystem classifier. Static semantic errors use that path, while missing, nonregular, and filesystem-I/O results remain explicit W5 states; metadata symlinks likewise use the followed state that finalization will compute while their nofollow graph evidence still records the rejected topology. Owners cover invalid UTF-8, embedded NUL, oversize, metadata-symlink rejection through a stable failed baseline, and retained `Unreadable` I/O classification. |
| Reported P1: a sink error stopped pipe consumption and could deadlock a verbose child | Rejected by inspection: `read_one` performs `Read::read` before testing `first_error`, so every later readable chunk is consumed and discarded. A refusing sink with 512 KiB of later child output owns the nonblocking completion and retained first error, making the existing W13 drain rule executable rather than relying on prose. |
| P2: watch diagnostics encoded an already-WatchPath `SourceMap` name a second time | W4 already requires watch path fields to enter the renderer as WatchPath. The renderer now emits that value directly before W2 applies WatchText and record framing. Owners cover spaces and `%` through one WatchPath decode after the W2 layers, and retry rendering uses the same path. |
| P2: a cache-reuse retry repeated first-attempt warnings | The reuse-forbidden retry selects `ErrorsOnly` for frontend, codegen-failure, and successful build diagnostics; the first attempt and stale-cache explanation remain `All`. The pure renderer owner covers warning suppression while retaining errors and paths. |
| P2: frontend cache outcomes lost their stage label and summary | Watch cache records now reproduce the one-shot frontend line and summary before ordinary codegen records. The same root-cause audit also restores ordinary miss spelling and ThinLTO prelink/backend labels and summaries. Real-binary owners cover frontend plus ordinary summaries and both ThinLTO stages. |
| P2: PGO instrumentation omitted the profile destination notice | The watch route emits the exact one-shot destination and merge/rebuild guidance as a `notice` record before linking, using fixed-startup `LLVM_PROFILE_FILE` or `default.profraw`. A real-binary owner requires the notice before `ready`. |
| P2: a first collation-probe descriptor metadata failure returned before cleanup | The exclusive-create descriptor remains owned; the failure path retries only to establish its identity, then runs the ordinary identity-aware cleanup and returns the original phase error. Cleanup failure retains its exact W14 precedence. |
| P2: retained last-success input union formation was quadratic | W6 seeds one deterministic `BTreeSet<PathBuf>` from current rows and admits each retained path once before the existing raw-byte row sort and graph limit. The duplicate-union owner covers current overlap and repeated retained paths. |

The P1 response changes failed-baseline classification and the parity audit
touches ordinary and ThinLTO reporting. This closure therefore requires one
fresh full-diff review before publication.

That revised implementation review found a new checked-metadata liveness P1
plus six boundary-totality gaps. The new P1 showed that the preceding matrix
classified semantic errors correctly but did not require one logical read to
have exactly one equivalent evidence owner. The matrix is reopened on
`single-observation-evidence-and-boundary-totality`:

| Finding | Redesigned closure |
|---|---|
| P1: one stable checked-metadata read produced unequal preclassification and byte-read evidence, setting `changed_during_attempt` forever | Checked-metadata lookup, nofollow kind rejection, actual regular-file open, bounded read, and opened-descriptor identity now form one `observe_consumed_classification` operation and one W4 evidence record. Missing/nonregular/I/O states remain total, while semantic parse rejection retains the regular bytes and opened identity already recorded by that same operation. No second observation of the logical path is representable. A present valid record and a stable malformed record both finalize with `changed_during_attempt == false`; edit/replacement remains changed. |
| P2: imported and static-input diagnostic paths did not all enter the renderer as WatchPath | The observed loader assigns an encoded WatchPath to every imported `SourceMap` file and uses the same encoding for import lookup/declaration diagnostics. The observed static-descriptor route renders structured `PathBuf` error variants through WatchPath while the ordinary route retains its existing display. Entry, import, and static path owners cover space, percent, and arbitrary non-NUL Unix bytes before W2 text framing. |
| P2: opened identity lookup omitted the W6 graph root | Identity membership covers `PathGraph.root` and every traversed node. Root-path and target-to-root owners prove a stable input cannot become a false changed attempt. |
| P2: `--watch=value` survived stripping and became a one-shot build | The watch parser rejects every `--watch=` spelling before positional dispatch; only exact valueless `--watch` is removed. A real-binary owner proves rejection before source or output work. |
| P2: `waitid`, post-exit queued-byte query, and pipe reads did not all retry `EINTR` | W13 routes status observation, `FIONREAD`, and read through one local retry-on-Interrupted authority. Deterministic injected operations prove interruption cannot select child-wait/read/early-EOF failure or skip queued bytes. |
| P2: W14 probe unlink did not retry `EINTR` | Owned-probe unlink uses one retry-on-Interrupted authority before link-count verification. A deterministic injected owner proves the exact interrupted-then-success path and preserves the existing permanent-error payload. |
| P2: first-probe identity failure substituted an unresolved prefix for `ENCODED_OUT` | Every W14 probe-phase error receives the complete fixed output path; intermediate missing-node identity remains only the logical-input context. The exact diagnostic owner distinguishes output, missing prefix, logical input, and private component fields. |
| P2: static source and checked-metadata opens could block after a regular-to-FIFO race | Static source, checked metadata, and the sibling publication-lock open route through one `O_NONBLOCK|O_CLOEXEC|O_NOFOLLOW` authority and require the opened descriptor to remain regular before a bounded read or lock. Stable FIFO owners cover all three routes; a direct nonblocking-open owner proves absence of a writer cannot stall the process. |
| P2: the controller still formed retained last-success paths with a nested current/prior scan | The controller now indexes the finalized failed paths once in a `BTreeSet<PathBuf>` and filters the at-most-32,768 prior rows through that key set. A direct owner covers overlap, prior-only retention, and exact-limit disjoint operands; `monitor_baseline` retains its separate deduplication owner. |

The P1 redesign collapses the checked-metadata read boundary rather than adding
another equivalence special case. Because it changes evidence ownership after a
P1 and the sibling audit changes process and cleanup totality, the revised
candidate requires one fresh full-diff review before publication.

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
  diagnostics, and program output are identical for the same inputs; child
  streams are byte-compared only when the one-shot child's stdout and stderr
  are likewise separate pipes, so both routes present non-TTY descriptors;
- external code implements `LinkOutputSink`; both `_with_output` functions
  preserve per-stream bytes and child result across ordinary, PGO-use,
  PGO-instrument, ELF in-link strip, and macOS external-strip routes, while the
  two legacy link functions retain inherited stdio and existing behavior; an
  implementation that omits `stop_signal` observes the default `None`, while
  injected `SigHup`, `SigInt`, `SigQuit`, and `SigTerm` return their exact stop
  sentinels only after final group/direct-pid SIGKILL/`ESRCH` and direct-child
  reap when waitable; every external caller passes only the final output path,
  while the `_with_output` function internally retains the tool inode, isolates
  it into a distinct unexposed inode, and returns only after atomic publication
  or identity-aware cleanup;
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
- on a case-sensitive parent, two absent unequal output/input leaf names pass
  W14 without touching either slot; on ASCII-case-folding and Unicode-
  normalizing parents the matching both-absent names select the existing alias
  error before link. Exact/raw-equal nodes are rejected earlier, while a
  multi-component missing input probes its first absent intermediate slot and
  no component below that nonexistent parent; every success, collision,
  descriptor `NAME_MAX`, exact/rejected-next `min(NAME_MAX, 1023)` suffixed
  length, randomness, and lookup result leaves no `.aw-` residue, two
  concurrent Align processes cannot remove each other's probe, and an
  injected cleanup failure names only its retained recoverable component;
  native events caused by the
  private create/unlink are an irrelevant wake whose comparison starts no
  revision. Replacement before the pre-unlink identity check preserves the
  foreign component and selects the exact `probe ownership lost` cleanup
  message; concurrent Align
  instances obey the reserved `.aw-` namespace, while post-check mutation by a
  non-Align process is the explicitly excluded concurrency case;
- removing or replacing any rejected direct/folded/distinct-hard-link output
  after alias-time and before/during repair registration triggers exactly one
  recovery revision; dropping its native event is repaired by the next audit,
  while an unchanged alias waits without spinning;
- disappearance or atomic replacement before each W15 add, after every added
  prefix, during reverse rollback, at the post-add snapshot, and before
  obsolete removal never exits on `ENOENT`/`PathLost`/`WatchLost`; changed
  topology schedules one revision, unchanged churn consumes exactly eight
  attempts then one 50-ms retry, old coverage remains, and eventual stability
  completes add-before-remove without leaked partial handles or spin;
- three or more consecutive changed post-add snapshots retain at most one
  tentative generation: a changed pre-add snapshot schedules without adding,
  while an equal snapshot compacts to at most 65,536 desired handles before
  the next at-most-65,536 additions. The injected peak remains exactly 131,072,
  and a mutation during the deliberate compaction gap is recovered by the
  mandatory post-transition snapshot or next audit, including zero-add passes;
- removing or retargeting an input symlink or hard link at each pre-open,
  opened-leaf, post-read, and pre-finalization barrier cannot erase the
  read-time identity: instability or aliasing fails before link, consumes the
  evidence, and a stable following scan may rebuild;
- rewriting, truncating, extending, or restoring a regular input in place at
  each original-read/final-read barrier is rehashed before publication; a
  differing snapshot emits `inputs changed during revision`, publishes
  nothing, and schedules a stable rebuild;
- the child sees only a randomized tool-output stage. After successful child
  cleanup and W1's writer-joined tool handoff, W13 copies it into a distinct
  unexposed publication stage and accepts
  only a stable source identity, a stable publication-stage identity, and
  matching source-before/source-after/destination length, permission mode, and
  hash. Mutation after the first source snapshot or during the copy either
  selects exact `link
  output changed during isolation` with no publication; mutation before that
  first snapshot is excluded by W1's successful-return handoff. Injected
  open/copy/mode/hash/close/unlink/rename failures,
  permission-mode drift at either snapshot, and tool-stage pre-unlink or
  publication-stage pre-rename replacement preserve the last-good output.
  Neither replacement is removed; the exact `tool stage ownership lost` or
  `publication stage ownership lost` is selected by its barrier, while a stable
  copy preserves exact executable bytes and mode.
  The injectable length/copy owner admits 8,589,934,592 bytes
  with bounded memory, while the next byte selects the exact limit error before
  creating the publication stage; a small real sparse fixture covers the OS
  path without making the gate stream 8 GiB;
- after a failed edit, the current state wins every overlapping last-success
  path, last-success contributes only paths not reached by the failed attempt,
  and W15 refreshes every union path before installing the failed baseline. If
  entry failure prevents rediscovery of a last-success-only dependency that
  changed during the attempt, one revision records its current state and then
  waits instead of repeatedly rebuilding against the stale successful state;
- a stable content change with its native event deliberately dropped, plus a
  Linux writable-`mmap` change, is discovered by the next completed periodic
  semantic audit without overlapping scans;
- every prior/current pair across `Missing`, `NonRegular`, `Unreadable`, and
  `Regular` is compared by both event and audit paths; in particular a
  permission/read failure followed by `Unreadable` to `Regular` recovery on
  the same inode triggers exactly one revision even when its native event is
  dropped;
- an unrelated event and a regular input's identical-content inode replacement
  trigger none after state comparison; the same identity change on the retained
  repair output triggers exactly one revision;
- one stderr-pipe reader decodes every `KIND`/`STATE` combination and observes
  each complete diagnostic, child stream, cache message, and ordinary success
  record before the matching unframed `ready`; watch-mode stdout is empty, and
  injected protocol full/partial write or flush failure emits no false `ready`
  and exits 1 without recursion;
- arbitrary/all-byte and exact marker-shaped `cc`/linker/strip stdout/stderr is
  reconstructible only inside W2 child records and precedes the terminal marker;
  simultaneous >4,096-byte streams cannot deadlock with a prompt sink, a
  descendant retaining a writer cannot extend the fixed post-exit/pre-reap
  byte quota or introduce an EOF wait, and exact/over-limit queued counts are
  deterministic. Child-visible writes remain blocking under pipe-capacity
  backpressure, parent reads remain nonblocking, a synchronous sink may delay
  wall-clock cleanup as declared, and pump failures emit no marker;
- explicit `SIG_IGN`, `SA_NOCLDWAIT`, another `SIGCHLD` handler, and a second
  captured-child call fail before pipe allocation/spawn with the exact wait-
  setup error and release the lease; an injected post-admission `ECHILD`
  closes both reads and enters group cleanup. A permanent group-check/poll/read failure
  after live status while `cc`, its linker, or `strip` is live
  closes both reads, terminates the private group, escalates after exactly
  250 ms when required, sends final SIGKILL to group and direct pid while the
  leader still pins both identities, reaps the direct child when waitable,
  releases the lease, and emits the first pump error;
  a sink failure instead drains/discards normally; descendants may retain their
  inherited stream descriptors briefly after return but cannot extend the
  bounded parent drain. On success, every supported production tool joins all
  output writers before its direct child returns; a deliberately detached
  writer violates W1 and has no artifact-integrity guarantee;
- ordinary success, child failure, and sink failure observe direct exit without
  reaping, drain only the fixed queued snapshot, close both reads, kill the
  pinned process group and direct pid, and reap only after both SIGKILL sends
  are accepted or return `ESRCH`; neither identity is used after reap. A direct
  child that changes group selects exact `child group changed`; injected
  group-preserving helpers may continue briefly after an accepted group signal
  only when they hold no writable descriptor or pathname access to the tool
  output. A production route that internally spawns a writer joins it before
  successful direct-child return.
  Group-kill failure selects exact `child group cleanup` ahead of child status
  and retains the stage/lease while retrying with the leader still unreaped;
- an external sink panic on either stream after zero or several callbacks runs
  the existing panic hook once, receives no later callback, closes both reads,
  immediately sends pinned-group/direct-pid SIGKILL, reaps the direct child,
  releases the lease, and resumes the identical payload on the calling thread
  without a second hook; the caller's resumed
  unwind then drops the private tool stage;
- an injected unexpected unwind at every post-spawn pump barrier runs the armed
  guard's no-panic cleanup before crossing the public seam; the direct child is
  reaped when waitable, the lease is absent, the caller's private stage then
  drops without publication, and a following captured call acquires the lease;
- every raw path byte round-trips through `WatchPath`, and newline,
  non-UTF-8, quote, percent, and marker-shaped paths/messages remain inner
  encoded values after record decoding; arbitrary diagnostic/cache bytes and
  exact terminal lines at empty, 4,095/4,096/4,097, and multi-chunk boundaries
  can appear only as framed payload; 16,384 message bytes are accepted and the
  next byte selects the exact static replacement;
- an absolute lexical or expanded graph path of 1,023 raw bytes is accepted;
  1,024 bytes fail before retention/I/O with the exact length and lowercase
  `Hash128`; every first embedded-NUL offset and relative public output/repair
  path fails in W8 order before allocation/I/O with its encoded path;
  16,384/16,385 per-producer inputs and 32,768/32,769 retry-merged inputs prove
  their exact rejected-next errors; two disjoint admitted 32,768-row operands
  form the exact 65,536-row retained-watch maximum before graph construction.
  Separately, 65,536/65,537 graph nodes, 131,072/131,073 read-time nodes, and
  40/41 symlink traversals prove each ceiling, exact error, and rejected
  logical-path identity;
- each asynchronous backend class produces its exact static line; two errors
  retain the first class even when the shared pipe is already full, while an
  idle simultaneously pending graceful signal wakes the same poll, wins, and
  performs backend cleanup while the process-lifetime pipe remains live. Both
  signal-first and fatal-first compare-exchange orders preserve both fields;
  stores immediately before the single Acquire snapshot participate, while
  stores immediately after it belong to the following checkpoint;
  EOF/error on the pipe fails closed; every transition failure paired with a
  pending signal/fatal class follows W7's single-load post-return snapshot
  exactly, including every store immediately before or after that atomic
  linearization point;
- asynchronous `PathLost` and `WatchLost` set uncertainty and wake the same
  pipe without a diagnostic or exit; `Disconnected`, backend-wide `Io`,
  `Capacity`, `InvalidConfig`, and `Other` remain the exhaustive fatal classes;
- a stale-cache retry consuming-merges first and retry semantic/evidence sets;
  entry then the ordinary PGO snapshot then import/static/metadata producers
  follow W4 encounter order, public rows remain raw-byte-sorted, retry state
  wins duplicates, retry-only paths join that sorted union, any difference
  stays unstable, merge overflow drops both, and neither first-attempt evidence
  nor diagnostics are lost;
- `build --help` names the external-resource trigger boundary; replacing a
  runtime/profile archive, linker/tool executable, system-library fixture, or
  project archive selected through fixed `LIBRARY_PATH` starts no revision by
  itself, while the next source edit and a restart each consume the replacement
  exactly as the matching one-shot build; editing only the baked workspace
  runtime-source tree likewise starts no revision, while the next source edit
  and restart each produce the same stale-runtime result as one-shot and a
  restore removes it; absent/unreadable installed trees retain the no-op;
- external code can read but cannot construct or mutate `BuildInput` fields;
  `FinalBuildInputSet` likewise has no external constructor or conversion to
  `BuildInputSet`, and compile-fail owners reject passing it to merge or
  finalization;
- a failed second signal installation changes nothing, each partial setup
  or nonblock/close-on-exec failure unregisters/closes/clears/restores in W9
  order, signals delivered at every setup barrier are handled only after
  activation, an exec helper sees no watcher/control descriptor, and a later
  clean child can install successfully;
- SIGHUP/SIGINT/SIGQUIT/SIGTERM pending immediately after signal publication,
  after every
  native-watcher initialization operation, or at the final pre-`started`
  checkpoint cleans initialized state and emits only `stopped`; no first
  revision or `started`/`ready`/`failed` record is invented, while a signal
  linearized after that final checkpoint belongs to revision 1; and
- idle and active SIGHUP/SIGINT/SIGQUIT/SIGTERM return 129/130/131/143 only
  after every owned child,
  stage, watcher handle, and native callback owner have been cleaned up; at
  every captured-child barrier, terminal-generated and parent-only signals are
  observed at the next control checkpoint, forwarded unchanged, allowed 250 ms,
  and escalated when required before pinned-group/direct-pid SIGKILL and direct
  reap. A prompt-return sink reaches that checkpoint after at most one 50-ms
  poll; an injected blocking sink proves the pending signal is handled only
  after callback return and carries no end-to-end latency promise. They
  emit only the matching `stopped` line, never `failed` or watcher error.
  Signals at every terminal barrier still target the unreused process-lifetime
  pipe, and the kernel closes that pipe with the process.

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

## Item 6: function-level incremental compilation

### Scope and settled boundary

This item refines the existing explicit `--thin-lto` path from one LLVM module
per source unit to one LLVM module per MIR function. It does **not** replace the
ordinary per-unit object path. The first implementation therefore inherits the
existing ThinLTO admission boundary exactly: only `build`/`run`/`size` under
`release` or `fast`, with instrument-PGO still rejected in combination with
`--thin-lto`. `dev`, `small`, `tiny`, `emit-obj`, `emit-llvm`, `explain-opt`,
ordinary builds, and PGO builds retain their current unit-granular codegen and
artifacts. No new CLI flag, environment variable, daemon, or ambient input is
introduced.

The granularity is a backend cache partition, not a new language visibility or
ABI boundary. One symbol producer classifies a stored MIR function as a true
root only when it is the direct C `main`, an explicit export, or has
`Function.exportable`; those definitions retain their existing canonical
symbols. Every other stored function is unit-local and receives the injective
raw LLVM symbol
`align_shard$v1$u$<unit-byte-length>$<lowercase-unit-hex>$f$<function-byte-length>$<lowercase-function-hex>`.
Decimal lengths have no leading zero except zero. This symbol uses hidden
external linkage only while ThinLTO and the native linker compose partitions;
the final executable gains neither a dynamic export nor a second source
visibility class. A wrapped `main` body is unit-local while its generated C
wrapper remains the true `main`. The unit component deliberately gives equal
consumer-side generic monomorphs in different units different symbols, retaining
the shipped internal-copy semantics without adopting the deferred
`linkonce_odr`/dedup design. ThinLTO remains the one mechanism that restores
cross-partition inlining and whole-program dead-code elimination, so output is
fully optimized rather than a collection of independently optimized functions.

The previous `--thin-lto` `N=1` rule narrows accordingly. Exactly one source
unit that forms exactly one function partition and no support partition still
takes the ordinary object path and stays byte-identical. A one-file program
with two or more partitions now has a real ThinLTO boundary and uses it. A
multi-unit program remains on ThinLTO even when type-only units leave only one
function partition. This is the only intentional change to the shipped `N=1`
contract.

### Public-contract ledger

The enum spellings used by the exact signatures below are:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThinFunctionLinkage {
    Root,
    UnitLocal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionThinLtoMode {
    WholeUnit,
    Partitioned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartitionKey {
    WholeUnit,
    Support,
    Function(ProgramCall),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThinPartitionSource {
    pub unit: String,
    pub partition: PartitionKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundImport {
    pub source: ThinPartitionSource,
    pub guid: u64,
    pub is_definition: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSourceDigest {
    pub source: ThinPartitionSource,
    pub prelink_digest: Hash128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FunctionThinLtoObservation {
    WholeUnit {
        source: ThinPartitionSource,
        codegen: CacheOutcome,
    },
    Partitioned {
        source: ThinPartitionSource,
        prelink_digest: Hash128,
        prelink: CacheOutcome,
        backend: CacheOutcome,
    },
}
```

| Surface | Exact contract | Owner and acceptance |
| --- | --- | --- |
| CLI selection and defaults | Existing `--thin-lto` selects function partitions for `build`/`run`/`size`; no new flag or default changes. The existing `release`/`fast` restriction, PGO mutual exclusion, target/profile/runtime-LTO inputs, `-j`, cache root, diagnostics, and exit codes remain authoritative. Without `--thin-lto`, every command and byte-identity gate stays on the current path. | `align_driver` CLI owners compare flag-off objects/executables before and after, and exercise every accepted/rejected profile, verb, PGO, cache, and runtime-LTO state. |
| Partition record | The driver/codegen seam is `ThinPartition<'a> { unit: &'a str, view: PartitionCodegenView<'a>, impl_hash: Hash128, preserve_symbols: Vec<String> }`. The derived-`Debug` `PartitionCodegenView<'a>` is the sole MIR module-emitter and structural-fingerprint input: `Function { selected: &'a align_mir::Function, definition: ThinPeerDeclaration, peers: Vec<ThinPeerDeclaration>, structs: &'a [StructDef], enums: &'a [EnumDef], resources: &'a [ResourceDef], tagged_types: &'a [TaggedType], fn_types: &'a [FunctionTypeDef], tuples: &'a [TupleDef], externs: &'a [ProgramExtern], imported_fns: &'a [ImportedFn], callback_effects: &'a BTreeMap<ProgramCall, FnEffect> }` or `Support { thunks: Vec<SupportThunkRecord> }`. `ThinPeerDeclaration { logical: ProgramCall, abi: CanonicalFnAbi, symbol: String, linkage: ThinFunctionLinkage }`; `SupportThunkRecord { drop_thunk: String, representation_version: u32, drop_abi_fingerprint: [u8; 16], owner: SupportThunkOwner }`; and `SupportThunkOwner = Owned { hook: ThinPeerDeclaration } | Imported` also derive `Debug`. Support records are one per distinct validated Drop-thunk symbol, sorted by raw symbol bytes; compatible duplicate resource records collapse before both hashing and emission. The sole constructor is `function_partitions<'a>(units: &'a [PerUnitArtifact], exports: &[String]) -> Result<Vec<ThinPartition<'a>>, String>`: only `units` supplies the returned lifetime, while `exports` is consumed into owned symbol and preserve records. `partition_function_symbol(unit: &str, function: &Function, exports: &[String]) -> Result<(String, ThinFunctionLinkage), String>` is the sole definition/declaration/hash producer. `Root` uses the existing canonical symbol; `UnitLocal` uses the exact unit-qualified record above. Records borrow validated unit MIR and own only derived metadata. `preserve_symbols` is raw LLVM symbol identity, sorted and deduplicated. Neither MIR module emitter may read the original `Program` after view formation; target/profile/runtime-LTO and preserve inputs remain separately keyed as specified below. | Structural owners reject duplicate/missing logical identities and duplicate root symbols, prove stable ordering and hash identity, and permit the same monomorph `ProgramCall` in two consumer units only with different unit-qualified symbols. Resource owners independently mutate every support-record field, local-hook symbol/ABI, owned/imported classification, order, and compatible duplicate and prove hash/emission agreement. A compile-time signature owner proves the result survives release of the shorter `exports` borrow but not release of `units`. Every `Err` is a static-prefix internal-compiler diagnostic plus lowercase length-delimited offending identity; duplicate roots use `duplicate ThinLTO root symbol:<decimal-byte-length>:<lowercase-hex-bytes>`. No OS error text enters formation. |
| Build entry and completion | The additive driver entry is `build_function_thin_lto(units: &[PerUnitArtifact], cache: &CacheContext, target: &BuildTarget, profile: Profile, exports: &[String], rt_lto: bool, jobs: usize) -> Result<FunctionThinLtoBuild, String>`. `FunctionThinLtoMode = WholeUnit | Partitioned`; every field of `FunctionThinLtoBuild { mode: FunctionThinLtoMode, observations: Vec<FunctionThinLtoObservation>, prelink_bc: Vec<PathBuf>, objects: Vec<PathBuf>, profile: Profile, link_libs: Vec<String>, object_stage: ArtifactStage }` is private. The only observation accessors are `FunctionThinLtoBuild::mode(&self) -> FunctionThinLtoMode` and `FunctionThinLtoBuild::observations(&self) -> &[FunctionThinLtoObservation]`; a caller can neither construct nor mutate the stored topology. Canonical partition order is import-DAG unit order, support first within a unit, then functions by raw logical-name UTF-8 bytes. `Partitioned` returns exactly one `Partitioned` observation per partition in that order; each record binds its exact `ThinPartitionSource`, digest, prelink outcome, and backend outcome. `WholeUnit` returns exactly one `WholeUnit` observation whose source is `{ unit: units[0].unit, partition: PartitionKey::WholeUnit }`. For every observation, each nested `CacheOutcome.unit` equals `source.unit`; its stage is `Codegen`, `ThinLtoPrelink`, or `ThinLtoBackend` exactly as named by the containing variant. Cache-disabled partition observations remain present with both outcomes `hit = false` and `miss_reason = None`. The private constructor rejects any mode/variant/source/stage/unit mismatch. `link_libs` is the deterministic first-seen union of every import-DAG unit's MIR-owned list, formed during validated inventory; `profile` is copied from the build input. An empty inventory returns `ThinLTO partition inventory is empty` before stage creation. Only `units.len() == 1` with exactly one function partition and no support returns `WholeUnit`, using the unchanged ordinary object-cache/codegen route. Every other nonempty inventory, including a multi-unit/type-only graph with one function partition, returns `Partitioned`. After inspecting the two immutable accessors, a caller must consume the result through `FunctionThinLtoBuild::link_and_publish(self, exe: &Path) -> Result<(), String>` or `FunctionThinLtoBuild::link_and_publish_with_output(self, exe: &Path, sink: &mut dyn LinkOutputSink) -> Result<(), String>`. Both methods use only stored configuration, write, flush, and close the private response file inside `object_stage` before spawn, then run the existing ordinary or captured-watch link and atomic-publication owner respectively; neither exposes or accepts an object/bitcode path, response path/argument, profile, or library list. Consuming `self` makes completion single-use and drops every private path after success, returned error, or unwind. The build validates the complete borrowed inventory and response-path representation before it creates `object_stage`; no caller can create or derive the stage through this seam. | Library owners cover empty rejection, exact one-source/one-partition byte identity and ordinary cache observability, a multi-unit/type-only single-partition positive `Partitioned` route, exact typed observation identity/order in hit/miss/disabled modes, multi-partition formation, exact stored-profile and MIR-library-union use, digest-only inspection, `jobs == 0` normalization to one, deterministic error selection, result construction privacy, response close-before-spawn, both completion methods, success/error/unwind stage Drop, and compile-fail attempts to construct or mutate the result topology, supply a second link configuration, obtain a private artifact path, or link objects directly. Existing `build_thin_lto` remains available for unit-granular tests and non-CLI callers until the hard cutover PR updates them coherently; no dual CLI path ships. |
| Function partition | Exactly one stored `Program::fns` body is defined. Every referenced local MIR peer is emitted as an exact Align-ABI declaration. Definition and peer declarations consume the same `partition_function_symbol` result: roots keep canonical linkage; every non-root definition is unit-qualified hidden external. Imported public declarations keep their canonical producer symbol. The partition owns every private compiler helper, constant, relocation record, main wrapper, Drop helper, callback trampoline, and parallel/task thunk reached while lowering that selected body. It sees the complete canonical type tables, extern declarations, imported public declarations, callback-effect table, and resource declarations from the unit. | `align_codegen_llvm` partition-module owners compare declaration ABI against whole-unit codegen for every parameter/return/cleanup mode and cover direct calls, function addresses, closures, indirect calls, static relocations, callbacks, parallel kernels, Drop, direct/wrapped main, malformed MIR, same-monomorph/two-consumer units, and generic monomorphs called from multiple sibling partitions. |
| Support partition | A source unit gets one support partition iff its validated `SupportThunkRecord` sequence contains at least one `Owned` record. It defines each owned hidden external `void(ptr)` thunk once, declares the exact `Owned.hook` symbol/`CanonicalFnAbi`, emits each `Imported` thunk as a declaration only, and defines no Align function body, main wrapper, function-local helper, descriptor, static record, or canonical type table. Function partitions only declare resource thunks. Runtime-LTO bitcode is not merged into support. Codegen and fingerprinting consume the same deduplicated sequence and may not rediscover ownership or hook symbols from `Program::resources`/`Program::fns`. | Resource owners prove one prevailing thunk definition, consumer-only declarations, correct hook binding, no duplicate symbol, and exactly-once Drop for ordinary, generic, imported, returned, and early-exit resources. Mutating an owned hook target or ABI, changing owned/imported classification, or adding/removing a distinct thunk must miss support prelink; adding a compatible duplicate resource record must not. A unit without an owned thunk gets no empty support object. |
| Runtime LTO | `--rt-lto` keeps its current guarded definition/attribute/fallback contract. The baked bitcode is merged independently into each function partition that needs module construction, before that partition's prelink pipeline; definitions remain internal to that partition. This deliberately avoids a new cross-partition runtime ABI or a duplicate external definition against `libalign_runtime.a`. ThinLTO may inline/remove each local copy. | Existing runtime-LTO IR/executable owners plus a multi-function string-kernel owner cover guarded-attribute XOR, parse/layout fallback, no duplicate externals, deterministic bytes, and final code-size/runtime bounds below. |
| Optimization and invalidation | Every `Partitioned` build writes summary-bearing prelink bitcode for misses, recomputes one fresh global thin-link over all function/support partitions, and runs backend optimization only for backend misses. `WholeUnit` performs none of those phases. A body-only edit misses that function's prelink partition. An unchanged caller backend misses only when the fresh import/export decision or an imported source partition digest changes. A peer signature, canonical type/resource/function-type table, callback-effect fact, function add/remove, export root, target/profile/LLVM/compiler/runtime-LTO input, or dependency interface change invalidates every partition whose exact module input changes. No mtime participates. | The invalidation matrix below asserts structured outcomes and actual artifacts, never elapsed time. Exact revert re-hits the old CAS entries. |
| Cross-unit policy | Without `--thin-lto`, cross-unit calls remain opaque exactly as today. With `--thin-lto`, the one combined index contains every partition from every source unit, so the existing cross-unit import and promotion policy also applies between function partitions. True `pub`, direct/generated `main`, and explicit export roots form the preserve set; unit-qualified synthetic shard linkage does not. Equal consumer monomorphs remain separate unit-local definitions, not ODR candidates, and a call always resolves to the caller unit's copy. | Existing ThinLTO inlining, aggregate ABI, preserve-set, and private-body invalidation owners are rerun over partitioned units. A two-consumer generic owner proves distinct definitions/call targets and executable parity; neither private synthetic boundary may appear in the dynamic symbol table. |
| Persistent identity | `PrelinkKey` and `BackendKey` gain destination `partition: PartitionKey` immediately after `unit`. `BackendKey.inbound_imports` becomes `Vec<InboundImport>` and `import_source_digests` becomes `Vec<ImportSourceDigest>` using the exact records above, so every source edge and digest identifies both unit and source partition. A `ThinPartitionSource` wire record is its unit string followed by `PartitionKey`; inbound records then append `guid` and `is_definition`, while digest records append `prelink_digest`. Key format and manifest format both bump from 4 to 5. Full and slot digests include the destination partition; the slot is `(phase, version, compiler_build_id, unit, partition)`. Each complete backend-key sequence is sorted and deduplicated by canonical source-unit UTF-8 bytes, canonical partition bytes, then GUID/kind where present. Exactly one digest record exists per distinct imported source partition. | Cache codec golden vectors fix every destination/source tag, multi-function-same-unit edge, field order, width, malformed-input rejection, old-v4 miss, exact-revert hit, digest verification, and primary/packaged-cache behavior. An owner edits each of two imported sibling functions independently and proves only its source digest changes while the caller backend misses both times. CAS remains content-addressed and schema version 1. |
| LLVM module identity | The stable C-string identifier is the ASCII record `align-shard-v1$<unit-byte-length>$<lowercase-unit-hex>$s` for support or the same prefix plus `$f$<function-byte-length>$<lowercase-function-hex>` for a function. Decimal lengths have no leading zero except zero; hex is two lowercase digits per byte. Identity is injective and contains no ambient path, temporary path, hash collision, or embedded NUL. The human cache label is `<unit>::support` or `<unit>::<logical-function>` and is display-only. | Golden vectors independently check semantic-to-bytes and bytes-to-semantic identity, prefix/length ambiguity, non-ASCII path bytes admitted by the existing UTF-8 unit surface, embedded NUL refusal before cache or LLVM I/O, and stable IDs across staging roots and processes. |
| Cache observability | In `Partitioned` mode, `--cache-stats` retains the current per-record `prelink` then `backend` lines and per-phase summaries, now ordered by import-DAG unit, support first, then function logical-name bytes. Counts are partition counts. Existing miss reasons retain their meanings: selected/global/peer input change is `own code changed`/`implementation changed`; an import frontier change is `cross-unit imports changed`; a new partition is `no prior entry`. Cache disabled still prints one disabled line. `WholeUnit` renders the existing ordinary object-cache transcript byte-for-byte. | CLI owners assert the complete cold/edit/revert/hot transcripts, including a body edit whose unchanged sibling prelinks hit and whose import-dependent backends alone miss. Watch framing reuses these exact lines without a second renderer. |
| Link input and publication | Partition objects are staged under controlled ordinal ASCII names and reach the existing linker/publication owner only through one of the result's two consuming completion methods, in deterministic partition order. To avoid `ARG_MAX` becoming a valid-program limit, the linker consumes one private GNU/Clang response file. On the supported Unix ELF/Mach-O hosts, each native `OsStr` path is encoded from its raw bytes as `"` + bytes + `"` + LF: `"` becomes `\"`, `\` becomes `\\`, every other byte including spaces, tabs, and invalid UTF-8 is copied verbatim, while NUL/LF/CR is rejected with `cannot encode ThinLTO object path:<decimal-byte-length>:<lowercase-hex-bytes>` during inventory, before stage/cache/artifact I/O. The response-file path itself is passed as one native `OsString` argument formed by prefixing raw `@` to its raw path, never converted through UTF-8. The selected GCC/Clang driver must round-trip this common quoted grammar before adoption. The file is closed before spawn, never cached or published, and drops with the result-owned object stage after the synchronous link-and-publication method returns or unwinds. Final executable isolation and atomic publication are unchanged. | Linux GCC→lld/system, Linux Clang→lld/system, and macOS Clang→system-linker owners cover spaces, tabs, quotes, backslashes, invalid UTF-8, 0/1/many objects, cyclic calls, raw `@`/option-shaped path bytes, LF/CR rejection before side effects, ordinary and captured-watch completion, tool failure, and byte-identical final links. |
| Allocation and side effects | Partition inventory allocates `O(functions + declarations)` bounded records and borrowed indexes. The build entry owns the later `ArtifactStage`; LLVM contexts/modules, prelink files, cache materializations, objects, and the response file remain explicit per-partition/build allocations. The consuming completion method is the sole owner of response-file creation, linker spawn, and executable publication. No stage creation, source/artifact read, cache write, LLVM call, or linker spawn occurs until all unit inventories, stable identities, controlled names, and response-path representations validate. Cache writes remain best-effort; compiler output and link/publication failures remain hard errors. | Allocation/failpoint owners cover each construction, stage-create, lookup, publish, thin-link, backend, response-write, spawn, link, publication, and Drop barrier with no partial public artifact. |
| Compatibility and documents | Language syntax, type/ownership/error semantics, MIR semantics, interface/package/runtime ABI, ordinary cache entries, frontend-cache format, generated-program behavior, and `draft.md`/`docs/language-spec.md` do not change. The implementation updates this section, `docs/impl/01-pipeline.md`, `docs/impl/10-cache-first-optimization.md`, `docs/impl/16-test-policy.md`, the ThinLTO settlement/deferral text in `docs/open-questions.md`, and the shipped status in `HANDOFF.md`. `docs/impl/07-roadmap.md` remains the historical M15/ThinLTO unit-boundary record and receives only an explicit item-6 supersession note, never rewritten history. | One consistency owner checks the current CLI/help, plan, cache docs, test-policy owner map, open-question settlement, roadmap supersession note, and HANDOFF status agree. No language mirror is applicable. |

`PartitionKey` is nominal compiler artifact identity, not a structural hash.
Its canonical bytes are one `u8` tag (`0 = WholeUnit`, `1 = Support`,
`2 = Function`) followed only for tag 2 by one `u32` little-endian byte length
and those UTF-8 logical `ProgramCall` bytes. A `ThinPartitionSource` is one
`u32` little-endian unit-byte length plus its UTF-8 bytes, then those
`PartitionKey` bytes. An inbound-import sequence is a `u32` little-endian count
followed by each source record, one `u64` little-endian GUID, and one `u8`
definition tag (`0` or `1`). An import-source-digest sequence is the same count
and source records followed by `Hash128.lo` then `Hash128.hi` as two
little-endian `u64`s. Unknown partition/bool tags, invalid UTF-8, truncation, a
declared length greater than the remaining bounded manifest, duplicate source
digest records, noncanonical order, or trailing bytes are clean misses. The
reader preallocates at most the existing 1,024-element cap and never from an
untrusted byte length. Semantic-to-byte and independent byte-to-semantic golden
vectors own the complete nested records, including two function partitions in
one source unit.

`impl_hash` is
`Hash128::of(format!("align-thin-partition-impl-v1\n{view:?}").as_bytes())`,
where `view` is the exact derived-`Debug`
`PartitionCodegenView` consumed by emission. The compiler build id already
namespaces this internal representation, as it does for the current complete
MIR structural hash. A function view covers the selected full `Function`, its
definition record, every referenced local peer ABI declaration (logical name,
canonical parameter/return/borrow/region/cleanup ABI,
`ThinFunctionLinkage`, and exact raw symbol), the complete canonical
struct/enum/resource/tagged/function-type/tuple tables, extern and imported
declarations, callback-effect facts, and the function role. It excludes every
unrelated local function body.

A support view covers the complete sorted `SupportThunkRecord` sequence and
nothing else. An owned record therefore fingerprints the exact emitted thunk
symbol, representation version, Drop-ABI fingerprint, and the local hook's
logical identity, `CanonicalFnAbi`, linkage, and exact symbol. An imported
record fingerprints the exact thunk declaration symbol and Drop ABI. A change
to any such field, to owned/imported classification, or to the distinct thunk
set changes the support hash; compatible duplicate resource metadata that
emits no second declaration does not. The enclosing prelink key separately
carries the unit and `PartitionKey`; the unit-qualified symbols are nevertheless
present in the view so codegen and hash cannot classify them differently. Both
views exclude source spans in normal unlocated MIR, `link_libs`, the
temporary/staging path, cache root, job count, and wall clock. There is no
second support inventory, ownership test, hook lookup, field list, or linkage
list downstream of view construction.

### Validation and error precedence

All inputs are compiler-produced checked MIR, so a partition-formation failure
is an internal compiler error, not a new source diagnostic. The deterministic
pre-side-effect order is:

1. validate the complete unit list and partition-independent per-unit MIR
   envelope/table invariants;
2. collect raw local function identities, classify every root versus unit-local
   definition through the sole symbol producer, and reject duplicate logical
   identities, duplicate canonical root symbols, a missing selected body, more
   than one C `main`, or a body/declaration ABI mismatch; equal unit-local
   symbols in different units are impossible because the unit bytes differ;
3. validate every resource record, classify each distinct Drop-thunk symbol as
   `Owned` or `Imported`, derive an owned hook declaration through the same
   ABI/symbol producer as a function peer, reject incompatible duplicate
   records, and form the one sorted `SupportThunkRecord` sequence consumed by
   both support hashing and emission;
4. scan every selected body and generated-record input for referenced local
   peers, form their ABI/symbol records, then form the preserve set, partition
   structural hash, stable module identity and its bijective
   `ThinPartitionSource` lookup, controlled ordinal output name, future
   randomized stage basename, and native response-path representation; reject
   an unknown target, duplicate module identity, symbol/registry collision,
   embedded NUL, LF/CR, or representation overflow before creating a directory;
5. only after every unit succeeds, create the result-owned private stage and
   perform cache lookup/materialization;
6. produce prelink misses and recompute thin-link; resolve every returned source
   and destination module identity through the validated lookup, reject an
   unknown/duplicate edge or missing source digest before any backend lookup,
   then produce backend misses;
7. after any cache-stat rendering, consume the result through exactly one
   completion method, which writes and closes the response file, links, and
   atomically publishes in that order.

Multiple invalid units report the first import-DAG unit. Within one unit,
step-2 function identity/ABI failures in logical-name byte order precede the
step-3 support-inventory failure; a multi-invalid owner pins that product. Once
side effects begin, canonical partition order is authoritative: the lowest
support-first producer error, then thin-link/edge-resolution error, then the
lowest backend partition, then completion-method
response-file/link/publication error. Cache corruption is evicted and rebuilt,
never selected over a real producer failure. A cache write failure remains a
note and cannot replace a successful artifact.

Thin-link edge-resolution failures use only stable module bytes and numeric
fields: `ThinLTO edge source unknown:<length>:<hex>`, `ThinLTO edge destination
unknown:<length>:<hex>`, `ThinLTO edge duplicated:<source-length>:<source-hex>:<destination-length>:<destination-hex>:<guid-decimal>:<0|1>`, or `ThinLTO source digest
missing:<length>:<hex>`. Lowercase hex covers the complete offending module-id
bytes, decimal fields have no leading zero except zero, and this resolution
never includes an OS error.

### Invalidation matrix

| Edit/input | Prelink result | Backend result |
| --- | --- | --- |
| Exact repeat | every partition hit | every partition hit after a fresh thin-link |
| Private body-only edit with unchanged ABI/effect/table facts | edited function miss; support and sibling functions hit | edited function misses; only callers/importers whose fresh edge or imported source digest changes also miss |
| Caller imports two function partitions from one source unit; either callee changes independently | only the edited callee prelink misses | caller misses against the edited source `PartitionKey`/digest; the sibling source record and unrelated backends stay unchanged |
| Exact revert | prior edited partition CAS hit; siblings remain hits | prior backend frontier CAS entries re-hit |
| Private function signature change | every partition in that source unit whose peer declaration changes misses; consumers remain governed by interface visibility | fresh decisions; affected callers and changed definitions miss |
| Resource Drop thunk, ABI, owned/imported classification, or local hook symbol/ABI change | support misses from its changed `SupportThunkRecord`; function partitions miss only when their exact resource table or peer declarations change | support misses; only importers whose fresh edge or support digest changes also miss |
| Public signature/effect/generic-template change | producer partitions and dependency-interface-keyed consumer partitions miss | fresh decisions across the changed interface frontier |
| Function add/remove or canonical type/resource table change | every exact shard module input changed by the inventory/table misses; support presence may change | fresh decisions; removed entries become unreachable CAS blobs |
| Runtime-LTO toggle/bitcode, target, profile, LLVM build, or compiler build change | every affected partition misses in a disjoint key | every affected partition misses |
| `-j`, cache root, staging path, response-file path, or source mtime only | no key change | no key change; output remains byte-identical |
| Corrupt/truncated/wrong-digest manifest or blob | only that partition cleanly misses after eviction | only that partition cleanly misses after eviction |

### Implementation closure matrix

| Axis | Required closure | Owner |
| --- | --- | --- |
| Formation and validation | Every MIR function appears in exactly one function partition; every owned resource thunk appears in exactly one support partition; imported/resource/runtime declarations have no accidental definition. Complete type and callable preflight still runs before lowering. Function identity/ABI errors precede support-inventory errors before side effects; producer failures use support-first partition order afterward. Malformed ids, tables, descriptors, duplicate names, and unknown thin-link module ids diagnose instead of panic. | partition inventory, function-plus-support multi-invalid precedence, unknown/duplicate thin-link edge, codegen malformed-MIR, resource, callback, and ABI owners |
| Symbol provenance and duplicate definitions | Direct C/generated `main`, explicit exports, and `Function.exportable` roots retain the existing canonical symbols. Every other stored definition and each same-unit declaration uses the exact unit-qualified synthetic symbol; imported public declarations remain canonical. The combined index has one prevailing definition per external symbol. Equal consumer monomorphs in different units remain distinct non-ODR copies, and no origin spelling heuristic is introduced into MIR. | root/unit-local classification sweep; two-consumer same-monomorph direct/address-taken/callback owners; duplicate-root rejection; symbol semantic↔byte goldens; ThinLTO prevailing-index assertion; final dynamic-symbol negatives |
| Construction and move-in/out | Parameter modes, cleanup pointers, slot alignment, aggregate return ABI, moves, source nulling, and replacement retain the whole-unit lowering. Peer declarations are mechanically derived from the same ABI producer as definitions. | parameter/return ABI parity sweep; ownership and replacement suites |
| Direct/control paths | Direct and recursive calls, `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, breaks, early return, and malformed unreachable paths resolve across partitions and retain cleanup order. Mutual recursion may remain as hidden cross-object calls when not imported. | control-flow/cleanup executable corpus plus symbol/IR checks |
| Address-taken/generated paths | `FnAddr`, closure/lifted targets, indirect/raw calls, tasks, parallel kernels, SQLite callbacks, static function relocations, and generated descriptors bind the exact peer definition even when ThinLTO declines import. Private generated helpers stay in their consuming function partition. | function-value, task, parallel, DB callback, static relocation, and descriptor owners |
| Types, generics, and interfaces | Complete reachable canonical type tables accompany each partition; each consumer-side monomorph gets one function partition in that consumer unit and a unit-qualified symbol, so equal logical monomorphs across units neither collide nor merge. Public interfaces, MIR semantics, and their hashes do not encode partitioning or the backend-only symbol. | canonical graph, one/two-consumer generic cross-unit, sibling-call, interface/MIR byte-identity, and deep-type owners |
| Drop, resources, and runtime | Function-local shared Drop helpers preserve exactly-once child order; support-owned resource thunks bind local hooks and imported declarations correctly; runtime-LTO remains internal per function partition; ordinary runtime archive capability selection is unchanged. | recursive-Drop matrix, resource lifecycle, runtime ABI, rt-lto, failpoint, and allocation-count owners |
| Partition fingerprint symmetry | The sole `PartitionCodegenView` constructor closes formation, hashing, and emission. Function views contain exactly the selected body, definition/peer records, and shared tables consumed by function emission; support views contain exactly the deduplicated thunk records consumed by support emission. No emitter re-reads `Program` to rediscover a symbol, ABI, ownership class, declaration, or table. | total-field mutation sweep for both view variants; owned/imported and duplicate-thunk resource mutations; hash-change iff emitted prelink changes; exact-revert CAS hit; source guard against post-view `Program` access |
| Ordinary/PGO/diagnostic lanes | Flag-off builds, PGO builds, the `dev`/`small`/`tiny` profiles, `emit-obj`, `emit-llvm`, and `explain-opt` never call partition formation and retain current output. The `size` verb **does** use function partitions when its accepted `--thin-lto` plus `release`/`fast` route is selected, exactly like `build` and `run`. Only a one-source/one-function/no-support inventory selects `WholeUnit`; a multi-unit/type-only graph with one partition remains `Partitioned`. `--thin-lto` validation remains before any stage/cache/LLVM work. | negative route/source-level guards, positive `size --thin-lto` and multi-unit/type-only route owners, and before/after byte fixtures |
| Public result topology | Immutable accessors expose `WholeUnit` as one typed observation binding its `WholeUnit` source to one ordinary outcome. `Partitioned` exposes exactly `P` typed observations, each binding one partition source to its digest and both phase outcomes in canonical partition order, including cache-disabled misses. Nested stage/unit fields must agree with the containing variant/source. No public record contains a stage-derived path. | zero/error, whole-unit, support/function, same-unit sibling identity, hit/miss/mixed, disabled-cache, mismatch-construction, mutation-attempt, and compile-fail privacy owners |
| Cache and deterministic build | Every destination and source `PartitionKey`, edge, digest, and manifest field has one codec; exact repeat/revert, two sibling imports from one unit, corruption, packaged fallback, cache-off/on, independent roots, `-j1`/parallel, and stable module ids produce correct byte-identical artifacts. Fresh thin-link is never cached or skipped, and every returned module id resolves through the validated inventory before backend lookup. | nested cache codec/golden, sibling-import stale-reuse mutation, invalidation, ThinLTO SV, and subprocess owners |
| Link and publication | All partition objects appear once in deterministic response order; hidden shard boundaries link on ELF and Mach-O without becoming dynamic exports. Public results expose only typed source/digest/outcome observations, never a stage-derived object, bitcode, response-file, or directory path. Link errors, signals, and unwind remove private files and preserve the last good/watch publication contract. | linker response, public API compile-fail/privacy, symbol hygiene, build/run/size, watch, and publication owners |
| Resource/performance promise | On the fixed `pkg.db` edit corpus, a private leaf-body edit must reduce median codegen-plus-thin-link wall time to at most 75% of the pre-item-6 unit-ThinLTO baseline over seven alternating runs, with structured counts proving the intended hit frontier. Cold `--thin-lto` wall time and peak RSS must remain at most 125% of baseline. Final executable size and the existing runtime benchmark corpus must remain within 5% unless a separately reviewed optimization explains and owns an improvement. Cache-off/on and cold/hot executables remain byte-identical within the candidate. | local `bench/function_incremental` pathological/edit/revert/cold controls, existing ThinLTO compile-time gate, runtime corpus, `/usr/bin/time` RSS, and size owner |

The implementation is one capability PR even if it exceeds roughly 1,000
hand-written lines. Partition formation, synthetic linkage, summary identity,
cache keys, fresh thin-link decisions, object ordering, and linker input are one
closed correctness chain: a producer-only split would expose no useful stable
consumer, while a cache-first split could serve artifacts whose linkage owner
does not exist yet. Keeping the chain together avoids two temporary persisted
formats and duplicated ABI/symbol proof. The author-side matrix-to-diff pass
must point every applicable row above to code and an owner before review.

The deliberate first boundary is `--thin-lto`, not ordinary builds. Extending
the same partition manifest to the default unit-cache/frontend-rehydration path
is a later item-6 slice only after the explicit path meets the cold, edit, RSS,
size, and runtime bounds. It requires its own ledger amendment because a hot
ordinary build currently obtains its object key without materializing MIR.

### Design review closure

The first three independent adversarial review passes found six P2 contract
gaps. They share the `route-stage-ownership-and-native-path-encoding` axis, so
the matrix was reopened on that axis and each complete finding set was
corrected in one coherent pass. The fourth pass found one P1 symbol-soundness
gap plus two P2 capability/routing contradictions. Per the implementation gate,
the local patch loop stopped and the matrix reopened on the new
`partition-symbol-provenance-stage-capability-and-route-cardinality` axis. The
redesigned boundary gives every non-root definition a unit-qualified symbol,
seals every stage path, and makes source-unit cardinality an explicit route
input. The fifth pass found a new P1 source-partition cache identity gap plus
two P2 result/validation-product gaps. The local patch loop stopped again and
the matrix reopened on
`partition-cache-edge-identity-result-order-and-validation-product`: backend
edges and digests now carry complete source partition identity, public vectors
have one canonical topology, and validation precedence is a pinned two-phase
product. The sixth pass then found a new P1 support-fingerprint hole and one P2
public-observation ambiguity. Per the same gate, the local patch loop stopped
and the matrix reopened on
`function-support-structural-hash-and-public-partition-observation`. Function
and support emission now consume symmetric typed codegen views that are also
the complete structural-hash input, while one typed public observation binds
each partition identity to its digest and phase outcomes:

| Finding | Class-wide closure |
| --- | --- |
| The closure matrix grouped the accepted `size --thin-lto` verb with non-ThinLTO profiles | Separate verbs from profiles explicitly: `size --thin-lto` on `release`/`fast` is a positive partition consumer; `dev`/`small`/`tiny`, PGO, flag-off, and diagnostic lenses remain negative routes. Add a positive size owner rather than relying only on rejection tests. |
| The proposed build API accepted a caller-created staging path even though validation promised no stage before complete inventory | Remove the staging argument. `build_function_thin_lto` validates first, then constructs and privately owns `ArtifactStage` inside an externally non-constructible result whose Drop bounds every returned path. Extend failpoints across stage creation and result Drop. |
| Response-file quoting did not define invalid UTF-8, newline, escaping, or rejection precedence | Define the exact Unix raw-byte GNU/Clang quoted record, pass the `@file` argument as native bytes, preserve spaces/tabs/invalid UTF-8, reject NUL/LF/CR during inventory before stage creation, and require real GCC/Clang plus lld/system round-trip owners on Linux and macOS. |
| The partition constructor returned `ThinPartition<'_>` while borrowing both `units` and `exports`, leaving the output lifetime ambiguous | Name `'a` explicitly on `units` and `ThinPartition<'a>` while keeping `exports` independent and consumed into owned symbols; add a compile-time borrow boundary owner. |
| The result owned the response-file stage but exposed no operation capable of writing and consuming that file for linking | Keep object paths private and add single-use consuming completion methods for ordinary and captured-watch linking. Each method writes and closes the response file inside the owned stage, synchronously links and publishes, and then drops the stage on every exit. |
| Completion accepted a second profile and library list that could disagree with codegen and MIR capability identity | Store the build profile and deterministic MIR-derived library union privately in the result. Completion accepts only the publication path and optional watch sink, so no caller can substitute link configuration. |
| P1: equal consumer-side monomorphs in two units would become duplicate prevailing hidden externals | Do not change the accepted internal-copy monomorphization model to ODR. Use one injective unit-qualified hidden symbol for every non-root stored function, derive same-unit declarations through that sole producer, and keep only true roots canonical. Add two-consumer and prevailing-index owners. |
| Public `prelink_bc` paths revealed the private stage directory and therefore its object names | Make bitcode and object paths private. Expose ordered content digests for inspection, with internal artifact owners and public compile-fail capability checks. |
| The `WholeUnit` result condition omitted source-unit cardinality | Require exactly one source unit, one function partition, and no support partition. A multi-unit graph remains `Partitioned` even when type-only units leave one function partition. |
| P1: backend edges and imported digests named only the source unit, so two imported sibling partitions could collapse and permit stale reuse | Replace both records with exact `ThinPartitionSource { unit, partition }` identity, specify the nested wire format/order, resolve shim module ids through one validated bijection, and add independent sibling-callee edit owners. |
| Public result vectors had no exact mode/cardinality/order contract | Fix canonical partition order and define `WholeUnit` as one ordinary outcome/no digest and `Partitioned` as `P` digests plus prelink-then-backend `2P` outcomes in every cache mode. |
| Function validation ran before support formation while prose claimed support errors came first | Make function identity/ABI validation precede support inventory before side effects, retain support-first canonical partition order for producer failures, and pin the combined multi-invalid case. |
| P1: the support partition had no complete structural hash, so a stale thunk could call a changed cleanup target | Replace function-only fingerprint prose with one `PartitionCodegenView` seam. Its support variant owns the deduplicated emitted thunk records, including exact thunk ABI/ownership and local hook ABI/symbol; hashing and emission consume that same view and cannot rediscover fields independently. |
| Public cache outcomes and digests still lacked a typed partition identity | Replace parallel public vectors with one `FunctionThinLtoObservation` per source partition. Each partitioned record binds `ThinPartitionSource`, digest, prelink outcome, and backend outcome; private storage plus immutable accessors preserve constructor-enforced unit/stage/variant agreement. |

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
