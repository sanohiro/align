# Owned data results from structured tasks

Align-llm Request152 needs contiguous output bands without packing bytes into
scalar task results. The user explicitly selected this request after the initial
assessment. This is the future owned-result producer already reserved by the
structured-task contract, not a change to purity or exclusive capture rules.
Plans180/207 and all three consumer registers were inspected. LLM Request41
shares the result lifecycle but retains its separate exclusive-capture and
current prefetch qualification criteria. Media AM-R08/09 retain shared captures,
worker eligibility and repeated Copy result reads. Voice has no task consumer.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| API | Existing `spawn(fn { ... }) -> Task<R>`, successful `wait()`/`wait()?`, and `Task<R>.get() -> R`. No new syntax, options, defaults or ambient configuration. Worker eligibility and caller-assisted joins retain Plan180. |
| Result roots | Existing int/float/bool/char/unit and ordinary records, plus `string`, `buffer`, ordinary dynamic scalar arrays, AoS record arrays, tuples and user sums. Every root must already have the compiler's ordinary payload representation. Root fixed arrays, vectors/masks, RNG, Option, arbitrary Result, views and native/resource handles remain excluded. The existing outer `Result<R, Error>` uses exactly builtin Error and remains the single task-error boundary. |
| Complete result graph | Every reachable field, tuple element, variant and inactive/zero-length element must be self-contained data: existing independent Copy data or owned string/buffer/scalar-array/AoS-array backing, recursively composed through already-formed records, tuples, sums, Option/Result and fixed arrays. Borrowed str/slices/SoA, raw pointers, callables, boxes/regions/tasks, native handles/resources and unknown/malformed types reject. Dynamic ownership edges may form recursive heap graphs; inline cycles reject. This admits no new collection/type formation. |
| Copy get | For Copy R, copy the result without consuming the handle. Repeated reads remain valid after a successful wait for the originating group/generation. |
| Move get | For Move R, get consumes the Move Task handle and transfers exactly one result owner. The source slot's initialized ownership flag is cleared as part of the MIR take, even when the result is unused, immediately borrowed, wrapped or returned. Repeated get/use of the consumed handle rejects through ordinary MoveCheck. No clone or input/output byte copy is introduced. |
| Independence | A result contains no borrowed authority. It may outlive the group, worker frame, source captures and surrounding arena. Worker-local arena-backed results cannot escape into a task result: ordinary function return/lifetime validation still applies. Shared captures remain protected against caller/task mutation through join. Owned/exclusive captures remain excluded. |
| Failure and cleanup | Preserve lowest-spawn-index Error selection and join every task before returning Err. An errored task never initializes R. At group end, after joining, drop every initialized, untaken owned result exactly once, in spawn order, before freeing the group region. This covers ignored handles, failed/ignored waits, replacement, multiple generations, ordinary exits, return and `?`. A taken result is cleaned by its new owner. Hard process termination retains the existing no-unwind policy. |
| Allocation | Worker output allocations remain explicit ordinary language operations. Task bookkeeping/result slots retain explicit task-group region ownership. An owned slot contains R plus one initialized flag; registration retains one cleanup record in group bookkeeping. No zero-allocation or zero-bookkeeping-cost promise. Copy result storage/read behavior remains unchanged. |
| MIR authority | SpawnTask authenticates exact result/closure/fallibility/capture metadata. A dedicated `TaskTake` rvalue represents owned result extraction; BoxGet may read only Copy task results. The take result gets independent owned provenance. Preflight rejects wrong payload, non-Task source, forbidden result graphs, forged allocating producers and illegal Copy/Move reader substitutions before LLVM emission. Existing checked-HIR successful-wait and Move replay remain authoritative. |
| Native ABI | Extend existing `align_rt_tg_register(tg, tramp, thunk, env, slot, err_slot, cleanup)` to seven pointers. `cleanup` is null for Copy R or a compiler-generated `void(ptr slot)` thunk for owned R. Register the cleanup before publication. The caller alone mutates the group cleanup list. Worker initializes result then flag before acquire-observed completion; cleanup/get happen only after join. No new runtime symbol. Runtime ABI A81/effect escapes and ledgers/goldens change together. |
| Generated layout | Copy slots remain exact R. Owned slots are target-layout `{ R, i1 initialized }`, allocated with at least max(natural slot alignment, declared R alignment); R is field zero. A typed trampoline stores R and then true only on success. TaskTake loads R and clears the flag; malformed duplicate extraction aborts before loading an uninitialized result. Cleanup checks/clears the flag before invoking the existing canonical iterative Drop plan on R. No independent destructor semantics. |
| Artifact identity | Add a canonical generated task-cleanup identity keyed by the complete canonical result graph; distinct from the existing fallibility/result trampoline identity. Its canonical bytes are generated-ID version 2, tag 5, followed by the existing canonical result-type bytes; independently checked codec goldens pin both directions. Source/interface replay carries ordinary result types, with no new interface record. Compiler/runtime implementation identities invalidate affected artifacts. Whole/per-unit/imported generic definitions and cold/hit codegen must agree. |
| Performance acceptance | A provider-owned contiguous band-fill witness must be byte-identical to sequential execution, eliminate packing/index-array materialization, and show measured multicore speedup for a stated CPU workload. Report all samples, compiler/host/workers and allocation/merge scope. This is a local measurement, not a universal speedup promise or CI correctness gate. Consumer pin/helper migration and product `test-vision-prepare` remain consumer-owned. |
| Documentation | Synchronize draft §11, language-spec, design-notes, Settled task-result text, concurrency guide EN/JA, Plans180/207 links, HIR19 and runtime ABI20. Record capability status once in HANDOFF at completion. |

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Formation / complete graph | One self-contained task-result classifier shared by source, checked HIR and MIR/LLVM preflight. `task_owned_results` covers each root and nested owned/Copy component, recursive heap graphs, fixed/empty/inactive forms, and forbidden borrowed/native/callable/inline-cycle twins. Existing `producer_plain_copy_type_graph` and task Copy owners stay authoritative for prior results. |
| Construction / transfer / source nulling | Typed trampoline initializes owned slot only on Ok; MIR TaskTake transfers ownership and clears the slot flag. Native whole/per-unit owners cover fresh/called/branch/loop results, moved handles, get in eager siblings/borrowing calls, unused get and double-get rejection. Copy get remains repeated. |
| Replacement / Drop / return | Existing ordinary Drop owns extracted values; runtime group cleanup owns all untaken flags across generations. Native allocation/requested-live owners cover discarded handles/results, taken/untaken mixtures, output replacement, group-tail/early return, surrounding arena, nested groups and lifetime after captured backing replacement. Errors after worker-local allocation, failed/ignored wait and `?` must leave zero retained bytes. |
| Control and authority | Reuse task-group successful-wait/origin/generation proofs. Cross if/match/else/?/map_err/loop joins, early exits and malformed source. Pre-wait, ignored fallible wait, stale/inner-group wait and repeated owned get reject; successful Copy twins continue passing. Reject worker capture mutation, returning captured views or arena-backed owners, overlap through two captured aliases and escaped Task handles. |
| Runtime publication / cleanup | Runtime owner executes initialized/errored/taken/untaken cleanup records across waits and implicit end, verifies spawn-order destruction and join-before-cleanup with a caller-only pending final generation. Cleanup-list growth while tasks run cannot invalidate worker storage; late helpers retain only generation metadata. Existing saturated/nested mixed par_map owners retain scheduler progress. |
| Checked HIR / MIR / ABI | Corrupt result domain, callback result/fallibility, Task source/payload and Copy/Move read operation. Owner rejects every forged case without LLVM panic. Generated cleanup and runtime A81 signatures/effects agree with declarations and independent codec golden. Typed drop uses canonical Drop-plan authority for nested/recursive owners. |
| Imports / generics / cache | Imported concrete and generic output producers inside literal task closures run whole/per-unit, cold/hit frontend/codegen and ordinary release build. Existing lambda-in-generic-body exclusion remains. No consumer source/test/pin changes. |
| Consumer / performance | A reduced provider-only band kernel reads shared immutable input, produces contiguous owned output, joins and assembles bands in logical input order. Check every byte against a sequential oracle and preserve SIMD-capable inner loops; benchmark warmed sequential/parallel arms in fresh processes without resource probes. No integerized pointer, hidden task or packed scalar workaround. |

One independently useful capability includes admission, publication, extraction
and cleanup. Splitting these would leave dormant ownership seams and repeat the
same safety proof. If the implementation exceeds 1,000 handwritten lines, this
complete lifecycle plus its control/ABI owners is the reason to keep one PR.

The author-side source-to-ledger pass must precede one fresh independent strategy
review. Resolve its boundary findings before implementation. The normal final
self-review, one fresh full-diff review and final-SHA preflight then apply.

## Author source-to-ledger pass

`check_spawn` already separates an outer builtin-Error Result and uses
`ty_to_scalar`; Buffer, String, DynArray, DynStructArray, Tuple and Enum have
that payload representation. The existing `task_result_type` deliberately
requires independent Copy data; its new graph walk must distinguish inline
cycles from recursion through dynamic ownership headers. Type formation remains
independently checked. `MoveCheck::TaskGet` already selects consuming access for
an owning result, but the MIR currently emits a nonconsuming BoxGet: admission
must wait for TaskTake and its ownership producer. Existing TaskGet escape,
region and borrow producers detach only classifier-approved independent data;
that same authority must include the newly admitted self-contained owners.

`source_move_statements` and `moved_drop_flag` currently recurse through TaskGet
to its handle. TaskTake must own slot extraction itself and publish fresh
result Drop authority; it cannot borrow a Task-local Drop flag or inherit a
surrounding arena's allocation mode. Existing get/wait replay remains necessary
even though the result becomes independent.

SpawnTask allocates zeroed group storage before registration. Its trampoline
stores only successful payloads, and TgGeneration completion follows callback
return. `TaskGroup` currently retains no per-result cleanup, and wait discards
the generation: cleanup records therefore belong to the group across waits,
not to a generation or the source Task local. Register appends cleanup before
publication; TgEnd joins before invoking it and before dropping the arena.
Copy get remains plain BoxGet. The existing iterative Drop emitter accepts an
explicit helper function and supplies one destructor semantics for owned data.

Existing generated task identities are canonical fallibility/result pairs;
a distinct cleanup tag avoids aliasing callable ABIs. Runtime A81 has only the
TgRegister consumer, so its seventh pointer, exact effect escapes and independent
ABI golden change as one contract. All native boundary, ephemeral expression
identity, cleanup and shared-capture obligations have owners in the matrix.
The consumer witness uses explicit named task handles and collects them in band
order; this does not add arrays of Task handles or transport wait proofs through
aggregate reconstruction, which remain outside the existing scoped task model.

The independent strategy review completed CLEAN before production edits. Task
callbacks returning Move data use the existing `DynamicBit` physical return
ABI (`{ value, cleanup }`). The trampoline authenticates that ABI and extracts
its value before filling the slot; the self-contained result graph excludes
caller-region ownership. The slot flag records successful initialization, not
the callback's path-selected cleanup bit (empty payloads still initialize R).

## Author matrix-to-diff closure

- Admission and complete graph: `task_result_type`, checked-HIR Task metadata
  and MIR tagged-program validation share one classifier. The type graph is
  collected first; a separate inline-edge walk prevents a header-reached memo
  from masking an inline cycle. `task_owned_complete_graph` covers both sibling
  orders, recursive headers, forbidden leaves and unsupported roots.
- Publication/ABI/identity: typed task trampolines use DynamicBit return transport
  for Move payloads and initialize target-aligned slots only on success.
  `task_owned_roots_execute` crosses direct/fallible and scalar/header/aggregate
  return shapes, including tuple, AoS and inactive owned sums. Generated-ID
  bidirectional goldens and the literal A81 declaration pin the new bytes/ABI.
  Both the cold-pool integration caller and the existing scheduler benchmark
  supply null cleanup for their unchanged Copy results.
- Ownership and cleanup: TaskTake clears the slot on every extraction, is
  effectful in loop facts, and gives independent owned producer authority.
  Sema/MIR cleanup facts detach from the caller arena. `task_owned_lifecycle_every_exit`
  proves zero requested-live bytes after unused get/handle, replacement, moved
  handle, wrapped get, eager sibling failure, ignored/failed waits, return/?,
  nested groups and multiple generations. Ordinary Borrow parameters still
  require stable places; immediate built-in observations of a fresh get are covered.
- Runtime ordering: `tg_owned_cleanup_survives_waits_and_follows_join_in_spawn_order`
  grows the caller cleanup list during execution, retains it through two waits,
  and leaves a final caller-only pending task. Cleanup-before-join therefore
  fails deterministically. Existing task progress/saturation owners are retained.
- Rejection/replay: source negative twins and
  `task_owned_hir_wait_move_and_payload_replay` close wait/Move/payload/fallibility;
  `task_owned_metadata_rejects_reader_and_callback_forgery` closes substituted
  readers, invalid operands/payloads, fallibility and allocating Task producers.
  Shared-capture access/callback owners retain their existing restrictions.
- Composition/performance: `task_owned_imported_generics_and_cache` executes
  whole/per-unit and both frontend-cache rounds. The provider band witness reads
  shared real input, returns eight ordered owned arrays and compares every byte;
  the local benchmark measures the complete output operation without probes.

Existing type-formation restrictions are not widened: recursive-header admission
is graph classification, not permission to construct an otherwise forbidden
recursive builder element. Existing Copy-record coverage owns its fixed-vector,
fixed-array and inactive Copy payload combinations; no duplicate fixtures are added.
