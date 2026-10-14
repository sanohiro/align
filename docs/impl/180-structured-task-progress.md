# Structured task progress during caller work

AM-R08 in align-media selects complete SHA-256 verification concurrent with
caller-owned GPU preparation. The provider assessment reproduced wait-only
dispatch and a captured-string replacement use-after-free at aca32efd. This
capability closes capture access/retention before enabling earlier dispatch.
It does not resume K1, general aggregate-provenance experiments, exclusive or
owned task captures/results, or consumer GPU/adoption work.

## Public contract ledger

| Surface | Contract |
| --- | --- |
| `task_group { ... }` | The existing structured scope owns task environments/result/error slots. It joins all registered tasks before ordinary scope exit, return or `?` releases captured storage. Hard process abort/termination has no unwind promise. Ordinary locals retain their existing ownership. |
| `spawn(fn { ... }) -> Task<R>` | Preserve the literal zero-argument lambda, existing Copy capture admission, primitive Copy `R`, and `Result<R, Error>` task form. Snapshot captures once in source order and publish the fully initialized task for worker execution before returning its handle. Worker eligibility begins at spawn; no immediate-start or bounded-latency promise when the finite pool is occupied. The caller never runs the task synchronously inside spawn. A single task uses the worker path too. |
| Captured views | Copies of view descriptors share the original backing. Tasks may read it; caller/task code may not write, release, replace, invalidate or exclusively borrow overlapping captured backing until the covering join completes. Pending protection belongs to the group, independent of Task-handle liveness. Copies of primitive values remain independent. Task-local newly owned storage remains writable. GPU/native resource and region capabilities remain non-Send. No implicit byte copy or ownership transfer is introduced. |
| `wait()` / `wait()?` | Join every task registered since the preceding wait and read completion with an acquire edge. Preserve lowest-spawn-index error selection, ordinary-error all-task completion, empty-wait behavior, generation-specific success proofs, and repeated primitive `get()`. Completing a wait ends captured-backing protection even when its Result is Err; result-slot authorization still requires the existing success proof. New spawns establish new pending captures. |
| Nested progress | Wait retains caller participation in claiming its own group's tasks, so nested groups and mixed task_group/par_map workloads do not require a free helper to finish their own pending work. No guarantee covers arbitrary user-created cross-task blocking cycles. Queued helpers after join must not touch freed environments/slots. |
| Allocation and lifetime | Retain explicit group-region ownership of environments/slots and bounded process-pool ownership. Runtime bookkeeping may allocate per task/group; no zero-allocation or throughput promise. Registration while tasks run cannot relocate worker-observed task records or mutate shared arena metadata. Group end frees only after joining all live tasks. |
| ABI / identity | Keep existing TgBegin/TgAlloc/TgRegister/TgWait/TgEnd symbols and C signatures; the opaque runtime layout may change. Compiler/runtime implementation hashes invalidate affected objects. The mandatory mutable-access record described below is producer-checked, replay-validated and serialized in interface format 18; an unavailable record never means no writes. No new source API, CLI flag, environment variable, package or digest format. |
| Ownership of the change | Sema owns capture lifetime/access refusal and wait coverage; MIR owns cleanup order; LLVM remains pure lowering; runtime owns publication, claiming and completion. `std.crypto` keeps the existing EVP SHA-256 contract and full input verification. |
| Documentation | Synchronize draft.md §11, language-spec.md, design-notes.md, open-questions.md's structured-concurrency entry, guide/10-closures-and-parallelism.md and its Japanese mirror, plan11, and HIR/runtime ledgers. Synchronize the interface-format ledger and independent codec goldens. |

## Access record and pending storage authority

`MutableAccessSummary = Option<Vec<MutableAccessRoot>>` records may-access effects
on every reachable invocation path, including paths that later diverge or return
an error. `MutableAccessRoot` has `Contained(u32)` and `Storage(u32)` variants;
the index is the logical parameter position, including lifted captures after
explicit parameters. `Some([])` proves no access to input backing; `None` is
unavailable and conservatively reaches both kinds for every argument/environment.
The record does not authorize exclusive captures or change general mutable-view
admission. Explicit `borrow mut`/`out` access conflicts even for an empty callee
write set. Direct, indirect, imported, generic, pipeline and error callbacks use
the same action authority, once all eager arguments complete, even when the
invocation itself cannot return. An argument that diverges prevents invocation.

The format-18 function record inserts the optional access list immediately after
`parallel_transfer_params`. The presence tag is u8 (0 unavailable, 1 present),
followed when present by a u32-LE count and pairs of u8 kind (0 Contained,
1 Storage) and u32-LE parameter index. Canonical order is kind then index;
duplicates, unknown tags, out-of-range indices and records on generic templates
are rejected. Concrete producers certify/replay the record; generic templates
carry None and concrete instantiations recompute it. Codec encode/decode,
function identity and checked-HIR replay include the record. Independent byte
vectors and malformed controls own the exact field placement and rejection.

A captured function value also borrows its frame environment, including environments containing
only scalar captures. That environment is distinct from each captured value: a nonserialized
`ClosureEnvironment` projection carries its physical producer identity, and reconstructing the
same frame closure site is a write to that identity. Loop re-entry cannot overwrite an environment
still referenced by a task. Projecting a callback's captured value drops the environment marker,
so a returned view does not acquire a spurious dependency on the callable header. Named/noncapturing
function pointers have no frame environment. Existing capture admission is preserved.

Pending captures retain completed projected headers and non-storage dependencies
in `BorrowState`, independent of Task handles. Storage generations, not frozen
release-local names, identify overlap after ownership transfer. Pending headers
participate in reachable-content pruning, Current/Prior renaming and control
joins. Invalidation records a permanent conflict before a later wait can clear
the pending entry; diagnostic collectors follow the existing loop-probe sink
and dedup suppression. Synthetic exit joins clear protection before modeled
cleanup, but never before evaluating a return expression. Distinct fresh task
storage remains writable even when its elements contain shared captured views.

## Scheduler publication authority

Enqueue, registered-count increment, helper reservation and empty-queue helper
retirement share one generation mutex. Callbacks run outside generation and
pool locks. Completion is published after the final environment/result/error
access, including panic capture. Wait takes the sole caller's generation,
drains its queue and joins completed == registered; it never waits for queued
helper retirement. A late helper retains only an Arc of generation metadata,
finds no task, and cannot access the group or its arena. Each later generation
resets all indices, error/panic state and counters. Group end joins defensively.
A deterministic enqueue-versus-retirement owner closes the helper transition.

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Captured lifetime, replacement, move and Drop | Reuse completed borrow/storage-generation facts; retain pending roots across discarded/reassigned Task handles and owner transfers. Reject release/replacement/reallocation before join. `task_group` driver owner: original string replacement reproduction, byte views, borrowed carriers, array backing, helpers and legal post-wait twins. |
| Shared read / exclusive write | One task-access authority covers indexed stores, field/element stores, byte and vector writes, mutable/out helpers, buffer refill/reallocation, opaque cursor observation invalidation and transitive callable access. Captured view writes and pending frame-environment overwrite reject; newly created task-local storage succeeds. Validated-text observations resolve to their underlying bytes before access overlap. Cross direct/indirect/imported/generic helpers, nested carriers and aliases. Handcrafted-HIR replay must reject the same illegal programs. |
| Control flow and scope | Pending capture state joins by union, follows storage-generation renaming and participates in loop convergence. Only an executed covering wait clears its group; an inner wait never clears outer captures. Cross if/match/else/?/map_err, loop joins/back edges, early returns and malformed input. Source owners pair pending rejection with completed-join success and exact cleanup counters. |
| Publication and exactly-once execution | Each task descriptor is initialized before release publication and claimed once by a helper or the waiter. Registration may continue while earlier tasks execute. Runtime owner uses deterministic native handshakes, single/many tasks, reordered completion, repeated wait/new spawn and dropped handles. No sleep-based correctness proof. |
| Errors, panic and memory reclamation | All ordinary failures join; lowest registration index wins independent of completion order. Preserve worker panic observation and completion accounting. Joined/late no-op helpers cannot dereference released regions. Native owner pins task counts, errors, completion and cleanup under an external watchdog. |
| Saturated / nested scheduler | Reuse process pool and caller-assisted task claiming. Existing task_group-to-task_group and task_group-to-par_map watchdog owners plus single-worker and repeated-generation controls remain mandatory. Avoid native user-resource transport. |
| Whole/per-unit/interface/cache | Run accepted/rejected source families in both modes, generic instantiation and function-value forwarding. Access facts cross interfaces; pin exact field order/width/root ordering, unavailable/malformed refusal, independent encode/decode bytes and interface/cache invalidation. |
| Consumer applicability | Read all three request registers and inspect current sources. Media AM-R08 uses immutable path/digest views and scalar counts. align-llm Request41 also needs exclusive buffer capture/owned results and remains open. Current tracked align-llm bbf5c8c9 and voice 53016fa3 contain no task_group/spawn calls; voice hashes serially in assets/util. No consumer adoption or performance result is inferred. |
| Measurement | After correctness, retain a bounded local CPU-work/caller-work overlap comparison with identical complete digests. Timing is evidence, not a correctness gate or a media speedup promise. Real GPU traces, exact images, VRAM and fresh-process measurements remain media-owned. |

The capture safety and scheduler publication path form one consumer capability:
an eager runtime without capture protection is unsound, while a dormant new
scheduler API would duplicate lifecycle proof. A prerequisite capture repair
may stand alone only if it independently fixes current wait-only use-after-free.
The full boundary may exceed 1,000 handwritten lines because it closes compiler
access facts, runtime progress and their shared cleanup/serialization owners;
keeping this proof together reduces dormant interfaces and integration risk.

Before implementation, perform the author ledger pass and one independent
inspection of this safety strategy and capability boundary. Preflight review
then inspects the final implementation once alongside its local owner gates.

The independent plan inspection selected the above access-record, stable-generation,
probe-diagnostic and scheduler-transition invariants before implementation. Its
findings did not require a different capability boundary or wider capture API.

## Closure evidence

The author ledger-to-diff pass binds the obligations above to these authorities
and owners; none requires reopening deferred capture admission:

- `MoveCheck::register_task_capture`, `record_mutable_access_roots`, and
  `BorrowState::{invalidate_matching,end_generations}` retain pending storage.
  `align_sema::tests::task_capture_lifetime_and_access_matrix` covers discarded
  handles, owner consumption/replacement, byte validation, direct/indirect and
  pipeline/error callbacks, primitive snapshots, fresh task storage, closure
  environment reuse, nested groups, branch/match joins and loop back edges.
  `mutable_access_checked_hir_rejects_forged_facts` authenticates replay.
- `MoveCheck::record_invocation_access` and `record_pipeline_access` collect
  action effects using completed operands; each pipeline stage consumes its
  own input prefix. The inference worklist propagates these facts to callers.
  `task_progress::imported_access_facts_reject_shared_writes` owns whole/per-unit
  imported and generic forwarding, read-only controls and post-wait writes.
- MIR `emit_exit_cleanup` and `lower_task_group_block` retain their existing
  join-before-release order. The `task_group` driver suite owns normal and
  early exits, fallible tasks and generation-specific result access;
  `staged_storage_generation_actions_follow_cleanup_boundaries` and
  `task_group_tail_moves_clear_the_inner_source` retain staging and cleanup
  counts. No new Drop representation or ownership transfer is introduced.
- `TgGeneration::{register,run,join}` owns publication, claiming and completion.
  Runtime `tg_single_task_progresses_before_wait_and_generations_reset`,
  `tg_helper_retirement_and_late_helpers_preserve_claims`,
  `tg_end_joins_pending_work`, `tg_wait_runs_all_tasks_pool_backed`, and
  `tg_wait_returns_first_errored_slot_by_index` own the lifecycle. Nested task
  and par-map owners run with both the host pool and one worker. The separate
  `par_map_cold_start` process keeps the small-map policy and task policy distinct.
  `task_progress::tasks_progress_before_wait_across_compilation_modes` uses a
  native condition-variable handshake, under the shared process-group watchdog.
- Interface `write_fn`/`read_fn`, `encode_interface_surface`, and checked-HIR
  replay carry the mandatory record. Codec owners
  `mutable_access_has_independent_bytes_and_malformed_controls` and
  `function_record_pins_mutable_access_field_placement`, the full format-18
  drop-state goldens, and
  `mutable_access_changes_interface_identity_and_survives_round_trip` pin its
  byte and identity effects. The unit-cache semantic/byte goldens independently
  pin frontend schema 18. Missing facts remain conservative.

## Local CPU measurement

After the focused builds finished, an unchanged temporary copy of media's
`validation/align-requests/task-start.align` reported task start at 2,270,762 ns
and caller pre-wait at 202,365,876 ns. The baseline at aca32efd reported
200,150,431 ns and 200,146,951 ns respectively. Compiler SHA-256 for the measured
candidate binary was
`bfb20ce88d3e10ec86c2be2ca0e1f2055aa71ed8d284ca59b40228cd0c891f8a`.
Both reproductions used the default release code profile, `--no-rt-lto
--jobs 1`, and `ALIGNC_CACHE=off`.

A CPU-only comparison verified a complete 64 MiB file (bytes 0 through 255
repeated) eight times on each of two paths. Each pass created its own EVP
SHA-256 stream, hashed the complete mapped file and compared all 64 hex digits
against Python hashlib's independently computed
`281e519df3077b557c6b03f5da83c4e8d397219259615dd7c3308f89cae8f2a6`.
Every row checked 1,073,741,824 bytes. The sequential form called both paths;
the concurrent form spawned one path, ran the other on the caller, then joined.
Three fresh-process rows per form, alternating sequential/concurrent, measured
467,411,176 / 465,772,860 / 461,454,115 ns sequential and
241,845,991 / 236,906,434 / 245,060,470 ns concurrent. Medians were 465.8 ms and
241.8 ms. Compiler/link/probe children had a 90-second deadline and an owned
process group; one exclusive temporary directory held and removed all inputs,
objects and executables. This is local CPU overlap evidence, not media GPU,
image, VRAM or end-to-end startup qualification.
