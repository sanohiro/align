# Independent Copy records as structured task results

Media AM-R09 needs both encoder start and finish instants from one worker.
Its two-i64 `Timing` witness currently fails the primitive-result admission.
This capability extends the existing task result slot to ordinary independent
Copy records. It does not transfer an owner or a borrowed result across workers.
LLM Request41 still needs exclusive/owned captures or owned results; voice has
no current task consumer. Adoption and GPU qualification remain consumer-owned.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| `spawn(fn { ... }) -> Task<R>` | Retain the zero-argument literal lambda and existing capture/effect rules. R is an existing primitive Int/Float/Bool/Char/Unit or an ordinary record whose complete graph satisfies Plan182/205's `independent_copy_type`. Every reachable field, inactive payload and zero-length array element participates. Records may contain existing admitted numeric/bool/char/unit/RNG leaves, numeric vectors/masks, records, tuples, fixed arrays and Option/Result/sum carriers of those leaves. Views, raw pointers, callables, region/task/native/heap owners, unknown IDs, malformed types and inline cycles reject. No field-count or byte-size cap is added. Root tuple/sum/tagged/array/vector type formation stays outside this record-result extension. |
| Fallible result | A lambda returning `Result<R, Error>` uses the exact builtin Error and the existing single `wait` error boundary. Errors join all tasks and select the lowest spawn index. An Err never initializes its R slot. `Result` representations produced by concrete generic instantiation must have the same classification. |
| `Task<R>.get() -> R` | Requires the existing successful wait proof for the active originating group and generation. Copy the complete R; retain the Move handle, permit repeated get, and do not expose the slot's address. The copied value is independent of worker locals, captures and the task group and may outlive group end. No new proof transport through calls, returns, imports, captures or reconstructed aggregates. |
| Storage and cleanup | Allocate the one result slot in the explicit task-group region using the target's exact R size and the maximum of natural ABI alignment and its source `align(N)`. Snapshot captures and publish work as before. The typed trampoline stores R only on success; get loads the same physical type. R has no owned or borrowed backing, source nulling, individual Drop, replacement cleanup or hidden clone/allocation. Group exit/return/`?` still joins before region and captured-owner cleanup; unused results need no per-result cleanup. |
| Owners and identity | Sema owns admission and result/wait authority; checked HIR revalidates the same graph; MIR preserves typed SpawnTask/BoxGet; LLVM lowers ordinary aggregate return transport and target layout; runtime keeps its existing opaque slots, acquire completion and Tg C ABI. Existing nominal type identity and reachable definitions, interface records and compiler implementation cache identity apply; no new format, symbol, argument, ambient input or package API. |
| Documentation | Update draft §11, language-spec, design-notes, Settled task result text, the English/Japanese concurrency guide and exact Spawn/TaskGet records in the HIR ledger. Plan180's primitive-result boundary is historical; link this extension rather than rewriting its completed proof. Runtime ABI signatures do not change. |

## Implementation closure matrix

| Axis | Implementation and owner closure |
| --- | --- |
| Formation and complete graph | One `task_result_type` authority composes the existing primitive/record root grammar with `independent_copy_type`; source and checked HIR consume it. Reuse `producer_plain_copy_type_graph` for full graph/cycle/invalid-leaf classification. New `task_copy_records` source and HIR owners pair valid nested records with borrowed/owned/raw/callable/zero-length/inactive-payload negative twins. |
| Construction, transport and reads | Preserve literal lifting and captured environment layout. Allocate/store/load the exact record LLVM type using target data and existing return transport. The trampoline calls are explicitly non-tail: the callback returns runtime status, not R; this also avoids LLVM22 ARM64 tail-call classification crashing on an empty aggregate R. Execute small, empty, mixed-width, padded/explicitly aligned and large nested record returns, infallible/fallible, capturing/noncapturing, repeated get and concurrent tasks with distinct expected fields. A malformed MIR owner rejects inadmissible R and mismatched result metadata before LLVM emission. |
| Lifetime, move, replacement and Drop | Only independent Copy R is admitted. Detach TaskGet result region, completed content/storage and borrowed-source provenance at the existing region/escape/borrow authorities; still evaluate its child and enforce Task origin/wait/liveness before the copy. Return R through group tails and early returns, including fixed-array fields, then replace captured backing before reading R; also replace R normally, discard result/handle, and create/drop worker-local owned scratch before returning R. Existing task handle moves/joins and group cleanup remain. Borrowed/owning result graphs reject; escaped Task and invalid pending capture access remain rejected by existing owners. No result source nulling or per-result Drop is introduced. |
| Control, successful wait and errors | Reuse existing `task_group`, task proof and `task_progress` owners for scheduler/capture control. New record owners cross if/match/loop results, `else`, `?`, `map_err`, repeated waits/new generations, nested group reads and early return/error. Negative owners cover pre-wait get, ignored/failed wait, stale generation and inner wait authorizing an outer task. Error-path evidence verifies first-error selection without reading an uninitialized R. |
| HIR and MIR authenticity | Update Task local-type, Spawn and TaskGet domains together. HIR mutations cover a valid record changed to a forbidden graph, mismatched closure return/task/get types and fallibility. Existing wait-proof replay still runs. MIR preflight rejects forbidden R and wrong Task result metadata before allocation or indirect calls; the directly preceding same-block FnAddr/Closure producer must agree on zero explicit parameters, captures, actual lifted result/fallibility and cleanup ABI; Task loads must agree with their payload. Valid generated thunk/result signatures retain canonical metadata. |
| Imports, monomorphization and cache | Driver owner checks/runs whole and per-unit modules, imported concrete/generic record producers and cold/hit frontend caches. Generic helpers may produce a concrete record inside the spawned lambda; the existing prohibition on lambdas inside generic function bodies remains. All needed record definitions and generated callable identities survive ordinary interface/partition transport; no new interface field or serialization schema is needed. |
| Scope and verification | `task_copy_records`, existing `task_group` and `task_progress`, focused HIR/classifier and malformed-codegen owners close this boundary. Run workspace build, bounded gate and Clippy for publication. No performance/resource improvement is promised, so no benchmark is a correctness gate. The exact consumer witness must check in both modes. |

The author ledger pass maps every changed public promise to the matrix above.
One independent adversarial plan review must confirm the result-ownership and
ABI boundary before implementation. This is one useful consumer capability;
separate admission, slot and consumer PRs would leave dormant unsafe seams.


The independent plan review confirmed the result domain and aggregate return
strategy. Its provenance finding is incorporated above: admission alone is
insufficient because the existing TaskGet Struct path inherited the group's
region and storage roots. Result-slot alignment also includes explicit source
alignment. Both obligations are part of the one implementation boundary.


## Closure evidence

`task_result_type` composes the existing independent graph with the primitive or
ordinary-record root grammar in sema, checked HIR and LLVM preflight. TaskGet's
region, storage-escape and borrowed-source producers detach only admitted result
data. The Task child still passes the existing origin, generation, access and
success proof. `task_request_is_valid` binds the slot type and literal closure
producer to the real callable return/fallibility and captures before LLVM.
`type_align` supplies declared alignment; the trampoline and BoxGet share the
ordinary target-derived aggregate type and return transport.

`task_records_execute_and_replay` executes the two-field consumer shape, zero-size
and large mixed records, fixed arrays/tuples/tagged fields, aligned roots,
captured text, worker-local owned scratch, repeated/nested/generation reads,
group-tail and early returned results, first-error propagation, imported generic
producers and whole/per-unit/cold/hit compilation. The zero-size case exposed an
LLVM22 ARM64 tail-call classification crash; non-tail trampoline calls retain the
status/result ABI boundary and this execution owner detects its recurrence.
`task_records_reject_authority_and_invalid_waits` covers complete forbidden
result graphs and unsuccessful/stale/inner wait authority in both modes.

`hir_body_validator_task_record_results` owns formation and expression corruption,
including constructor-free Task locals whose record graph is independently
changed to an excluded leaf or cycle. `task_record_result_metadata_and_alignment`
pins explicit allocation alignment and six malformed callable/slot/load cases.
The existing independent graph, task_group, task_progress and non-Send malformed
MIR owners retain neighboring type, capture, scheduler and cleanup guarantees.
The unchanged media reproducer passes whole/per-unit checking. No consumer
adoption or performance improvement is claimed.
