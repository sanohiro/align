# Owned captures in structured tasks

Status: implemented provider capability; consumer integration and performance qualification remain separate.

The 2026-11-10 align-llm R41 witness needs to transfer an already admitted
file and an independent read window to a worker. Reopening its pathname does
not preserve admitted file identity. Plan228 supplies owned results but
deliberately leaves captures Copy-only. This is the deferred ownership
capability, not a scheduler or file-read performance defect.

All three request registers were inspected. Media AM-R08 retains immutable
captures and caller-owned GPU resources. Voice V008 deliberately excludes
transferring its process-global signal lease. Neither asks to widen native
resource sendability. Media/voice are unavailable as local checkouts; their
GitHub registers were read at blobs `4098dbadd4e297b82617f07954031d8f62073ba1`
and `a8504384d0464a92a0dab802fee79ea07db3df35`. Consumer code, pins and adoption
remain outside this work.

## Public contract

| Surface | Contract |
| --- | --- |
| Syntax | Existing `spawn(fn { ... }) -> Task<R>` consumes its literal task once. No new syntax, implicit clone, path reopen, task API or ambient option. Ordinary stored closures and repeatedly invoked pipeline callbacks retain Copy-only capture. |
| Capture ownership | Copy captures retain Plan180. A complete owned local of a worker-transferable type moves into the task at spawn, in first-use capture order. The worker receives it as an ordinary owning by-value parameter. The caller cannot read, borrow, move or capture the old owner again. A borrowed parameter cannot be moved into a task. |
| Transfer domain | Plan228's complete self-contained data graph, plus the existing `file` owner, at both capture and result boundaries. Owned capture roots are already-formed ordinary Move values with that complete graph, including direct Option/Result; bare owned fixed-array roots retain their existing capture exclusion. Task result roots retain Plan228's restrictions except for file. Share the graph walk, not the distinct root predicates. Existing type formation remains authoritative. All reachable fields, inactive variants and zero-length elements participate. Independent Copy data and owned string/buffer/scalar-array/AoS-array storage compose through existing records, tuples, sums and tagged carriers. |
| File | Transfer the same owned `file` descriptor wrapper, with no reopen, duplication or new allocation. `file` has no borrowed variant or implicit cursor. Existing positional read/write errors and short-read semantics remain. Files may return directly or in an already-admitted record through the existing owned-result lifecycle, allowing bounded window reuse without losing admitted identity. |
| Exclusions | Owned captures containing borrowed views, raw pointers, callables, boxes, task/region/native/resource owners other than `file`, malformed graphs or inline cycles reject. Arena-owned storage cannot cross an owning parameter boundary; preserve that rejection for task transfer. No exclusive-borrow capture or shared mutable capture is introduced. Signal subscriptions, GPU resources and HTTP owners remain non-Send. |
| Access | A moved owner grants the worker ordinary owned access, including local mutable rebinding and synchronous helpers. Shared Copy views remain read-only and protected through the covering join. Capturing a moved owner and an alias into its backing must reject in either capture order. Existing aliases cannot be used by the caller after transfer. |
| Return/error | `Result<R, Error>` keeps the one successful-wait boundary and lowest-spawn-index error. A file-bearing result follows Plan228's consuming get and untaken cleanup. Returning views of moved captures rejects. No new error code or cancellation mechanism. |
| Drop | Exactly one owner exists after publication. The lifted worker function drops unreturned captures on success, error, early return and nested-group exit. All registered tasks execute and join even if a handle or wait result is ignored. Group cleanup drops untaken owned results; taken results belong to the caller. Hard process termination keeps its no-unwind policy. |
| Allocation/ABI | Explicit input/output allocations remain visible. Reuse the task-group environment/result bookkeeping and runtime registration ABI. No new native symbol or public layout. Source slots are cleared after complete capture evaluation and before publication; task environment headers are one-time transfer staging, not independently dropped owners. |
| Validation/identity | Source checking, checked-HIR replay and MIR preflight must agree. HIR admits a Move-bearing literal only directly under Spawn. MIR SpawnTask carries its lifted target and capture operands directly; ordinary MIR Closure remains Copy-only, so no reusable owning Fn value exists. Authenticate exact capture operands, complete transfer types, actual callback return/fallibility and cleanup ABI before LLVM. Existing canonical capture/result type graphs identify generated helpers; implementation identity invalidates changed artifacts. |
| Consumers/performance | Provider acceptance proves exact bytes, file identity, cleanup, bounded window reuse and task progress. No startup speed claim is made. Native full-answer comparison and measured whole-startup/first-response benefit belong to align-llm after provider delivery. |

## Implementation closure matrix

| Axis | Implementation boundary and owner checks |
| --- | --- |
| Formation | Share the complete transfer-graph classifier across source, HIR and MIR validation. `task_owned_captures` crosses string, buffer, arrays, records, tuples, sums, direct Option/Result, file and nested file records; rejects borrowed/native siblings, inactive invalid variants and bare owned fixed-array captures. One-shot lifting is an explicit per-lift mode; nested stored/pipeline lambdas must not inherit it. Reuse Plan228 graph-cycle owners. |
| Construction / move-in / source nulling | `check_spawn` explicitly selects one-shot lifting; ordinary `lift_lambda` keeps Copy-only admission. MoveCheck consumes captures and ends their owned backing generations as at an owning call, rejecting borrowed sources/use-after-move and caller aliases. MIR stages all captures before clearing sources and publishes only the complete environment. Owners cover both capture orders and multiple owned inputs. |
| Move-out / replacement / return | Existing function and task-result transfer owns returned captures. Direct and record file results, repeated generations, loop-carried replacement and returned buffers run with exact bytes. Repeated get, repeated spawn, use-after-move and borrowed returns reject. |
| Drop / ownership provenance | Worker parameter cleanup and Plan228 untaken slots close normal, error, ignored handle, ignored/failed wait, implicit join, early return, `?`, replacement and nested group paths. Count live allocations and open descriptors; arena-backed captures must fail rather than be freed by a worker. |
| Access / control | Distinguish moved self-contained capture authority from shared capture effects. Parameterized owners cross if/match/else/`?`/map_err, branch and loop joins, zero/one/multiple generations, ordinary mutable helpers and shared-view overlap in either capture order. Existing task access and wait-proof owners retain competing-spawn and stale/inner-wait controls. |
| Independent HIR / malformed MIR | Corrupt capture type, origin, consuming position and ordinary callable use. Checked-HIR closure validation admits owned graphs only under Spawn. MIR preflight authenticates the direct SpawnTask target/operands/signature and refuses Move-bearing ordinary closures without LLVM panic. Existing task result producer checks cover file result cleanup. |
| Imports / monomorphization / cache | Direct/imported owning helpers and generic record carriers run through whole/per-unit checks and native execution; cold/hit interfaces preserve ownership and generated helper identity. Existing generic-lambda exclusions remain. |
| File identity / I/O | Provider fixture opens one path, replaces/unlinks the name, transfers the admitted file, and reads the original bytes. Cross short read, EOF, read failure, subsequent caller failure and joined cleanup. Bounded repeated read windows return file plus buffer without reopening. |
| Regression | `task_owned_captures`, `task_owned_results`, `task_group`, `task_progress`, `task_callback_access`, ordinary `lambda`, affected sema/HIR/MIR classifier owners, bounded PR gate and workspace Clippy. No GPU/service changes or consumer qualification are part of these checks. |

The capture producer, single-call proof, worker cleanup and file-bearing result
form one useful capability. Splitting admission from transfer/cleanup would be
unsound, and file input without file return cannot support reusable admitted
bounded windows. If this exceeds 1,000 handwritten lines, retaining that one
lifecycle boundary avoids duplicated proof and dormant intermediate features.

## Author-side source pass

Before this capability, `lift_lambda` rejected `ty_capture_is_move` before publishing captured
trailing ByValue parameters. Spawn consumes a literal directly, unlike ordinary
Copy function values. HIR's closure checker already knows its Spawn context.
MoveCheck reads every Closure capture; the Spawn transfer must own
consumption, without making repeatedly called ordinary closures consuming.

`task_callback_access` maps callback writes to captured caller roots. Only the
proved self-contained moved inputs may be exempt from that shared-view refusal;
other captures and transitive callback requirements keep their checks.

Replace the existing intermediate frame closure for every Spawn with direct
target/capture operands in SpawnTask. Lowering stages all operands, clears source
ownership, and constructs the group environment before publication. The shared
typed environment thunk invokes the lifted function once; ordinary parameter
cleanup owns release. The helper identity uses the actual callback return type
(including its Result) and cleanup ABI, never the HIR Task's ok-only Fn tag.
Ordinary MIR Closure remains Copy-only. This removes a reusable owning Fn value
instead of adding a second single-use validation state machine.

Ordinary local moves preserve alias storage generations. Spawn must additionally
end transferred generations, as `finish_successful_call_arguments` does, so
caller aliases cannot race the worker or survive its Drop. Escape replay uses
`call_transfer_contains_arena_owned`; merely outliving the group is insufficient.

`RwFile` owns one fd and no cursor. Its existing null-safe destructor and
Plan228's canonical Drop support already provide the release primitive. A file
result still needs the same complete graph authentication as other owned data.
No raw-handle transport or runtime worker is added.

## Review and verification

The independent preimplementation strategy review required ordinary owning-call
alias invalidation, explicit arena-provenance rejection, separate input/result
root predicates, and direct task operands instead of an owning Copy Fn. All four
are reflected above. The draft, condensed spec, Settled text, design rationale,
HIR ledger and concurrency guide EN/JA agree; Plans180/228 retain their historical
scope with a superseding reference.

The author extracted the `must`/`exact`/`every`/`before`/`reject`/`required`
obligations from this ledger and checked them against the diff:

| Obligation | Implementation and discriminating owner |
| --- | --- |
| Complete transfer domain, borrowed/native/arena refusal | `task_owned_capture_type`, `task_transfer_graph`, `check_spawn_capture`; sema `task_owned_transfer_roots_keep_native_and_arena_boundaries`, driver `owned_capture_rejects_aliases_borrows_and_non_send_graphs`, existing MIR `task_owned_complete_graph` |
| Expire all inputs and aliases before publication, both orders | MoveCheck's Spawn completion and MIR Spawn lowering; driver rejection matrix and `owned_capture_roots_cleanup_and_imports` |
| Exactly one cleanup owner through every ordinary exit | Existing parameter Drop and owned task slots; driver data/file lifecycle owners assert zero live allocations after a positive peak and restored descriptor count |
| Checked-HIR and exact MIR callback ABI before LLVM | `closure_matches`, `task_request_is_valid`, `task_environment_identity`; HIR `task_owned_capture_hir_consumption_is_replayed` and codegen `task_owned_capture_mir_rejects_forged_transfer_and_copy_closure` |
| Byte identity, bounded reuse, imports and cache | `file_capture_identity_errors_and_cleanup` reads the original opened inode after pathname replacement, crosses short/EOF/error, and executes whole/per-unit plus cold/hit interfaces |
| Worker progress and unchanged shared captures | Existing `task_progress`, `task_callback_access`, `task_group`, `task_owned_results` and `lambda` owners |

The guide example passes both source-checking modes. Baseline rejects the R41
admitted-file witness at capture admission; the implemented native witness reads
successfully. No benchmark is required because this capability makes no startup
speed claim. Consumer full-answer and startup measurements remain pending.
