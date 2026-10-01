# Explicit File and writer synchronization (Request 31)

## Capability boundary and public-contract ledger

One end-to-end capability adds the same explicit synchronization operation to
existing positional File and buffered/unbuffered writer owners. It does not
change ownership or add a handle, constructor flag, cursor, option, implicit Drop
sync, directory sync or transaction. Implementing both entrypoints together
shares one native policy and error authority instead of a dormant producer chain.
The expected diff is below 1,000 hand-written lines. No latency/performance promise
or benchmark is introduced. K1 and external consumer adoption remain deferred.

| Surface | Exact public record | Owner and acceptance |
| --- | --- | --- |
| `f.sync() -> Result<(), Error>` | Exactly zero arguments on the existing File; Impure and nonconsuming. Existing bound-local receiver and single-threaded/aggregate restrictions remain. Calls the platform sync operation on the same owned descriptor once. There is no buffer to flush, path lookup, constructor-origin test, access-mode gate, fd duplication, acquisition, close or allocation. Success is unit; failure uses the fixed errno table. Read-only owners receive the same native request and error mapping. | Compiler method/checked HIR/MIR/A03 lowering; runtime descriptor sync. Driver File lifecycle/control/import owners and native read/write/read-only descriptor owners. |
| `w.sync() -> Result<(), Error>` | Exactly zero arguments on the existing writer; Impure and nonconsuming under the existing writer borrow convention. Preserve writer stable-local/field and unbuffered standard-stream receiver gates. First perform exactly the existing buffer flush, including its partial-write/EINTR handling and buffer-clearing-on-error behavior. If flush fails return that error without a sync syscall. Otherwise issue one platform sync on the same descriptor, even for an empty accumulator. No buffer growth/copy, ownership change, kind/origin inference or new allocation. Sync failure after a successful flush leaves the flushed buffer empty; the owner remains usable and Drop is unchanged. | Compiler writer sibling passes and runtime flush-before-sync authority. Native scripted operation owner proves error precedence/no sync after failed flush, empty/repeated calls and post-error state; driver buffered readback before Drop. |
| Native platform policy | Linux uses exactly `fsync(fd)`. macOS uses exactly `fcntl(fd, F_FULLFSYNC)`; no weaker fsync fallback, retry, fdatasync option or silent unsupported success. One native request returning success completes the operation; EINTR and every other failure use the fixed errno table, without restarting the sync. Null native owner returns Invalid before any dereference, flush or syscall. Pipes, sockets, terminals and unsupported filesystems receive native errors through that same table; no special writer type is added. Other unsupported build targets have no newly claimed guarantee. | Native operation seam records selected request/call count/error mapping. Real Linux/macOS File and writer integration owners; valid shell with invalid fd and null-owner controls. |
| Meaning of success | Linux requests file data and associated metadata synchronization and waits for the kernel/device completion report. macOS requests file synchronization plus a device-cache flush through F_FULLFSYNC. This is the native OS guarantee, conditional on filesystem/device support and truthful completion. It does not promise survival against lying/failing hardware, arbitrary power loss, a newly created/renamed parent entry, other handles' buffered bytes, subsequent/concurrent writes, multi-file atomicity or remote-filesystem durability. No parent-directory synchronization is performed. A tmpfs success has no persistent backing store and supplies no crash-durability claim. | Native-call/selection proof is independent of functional fresh-open byte readback. Power failure is not simulated or inferred from readback. Actual filesystem type is recorded for local acceptance. |
| Ownership/lifetime/allocation | Borrow the existing owner only during the call; return only unit/Error, retaining no owner/path/view. No new runtime object, heap buffer or lifetime. Writer drains its existing accumulator without growing it. Neither operation consumes, nulls, closes, replaces or removes anything. Existing move/return/replacement/Drop cleanup stays exactly once and Drop never syncs implicitly. | Existing File/writer ownership authority and focused driver use-after-sync/move/early-error owners. |
| Artifact/interface | New HIR/MIR variants use ordinary compiler-build identity and structural MIR hashing. Imported generic templates reparse from source. No new type graph, schema field/version, reflection, format, ambient setting, CLI/build input or default. Prerequisites are the shipped M9 writer and M12 File. | Whole-program and per-unit imported helper tests; exact existing owner types remain nominal. |

## Native and compiler inventory

| Boundary | Exact record and closure |
| --- | --- |
| HIR | `FileSync { file: Box<Expr> }` / `WriterSync { writer: Box<Expr> }`, exact existing receiver type and Result(Unit, canonical Error). Enumerate every FileLen and WriterFlush sibling visitor, Impure/escape/storage/move/finalize/checked/depth/replay pass. Existing borrowing and formation diagnostics remain. HIR variant count increases by two. |
| MIR | `FileSync(Operand)` / `WriterSync(Operand)`, exact i32 native status. Evaluate receiver once, stop if it diverges, then use existing `lower_status_result` for unit/Error. Include all operand/result, optimization, resource, native-call and loop-opacity authorities. Exact receiver/result rejection must happen before LLVM for forged MIR. |
| ABI | `IoFileSync` / `IoWriterSync`; `align_rt_io_file_sync(*mut RwFile) -> i32` / `align_rt_io_writer_sync(*mut Writer) -> i32`. Both A03 `i32(ptr)`, no native out slot. Pointers must identify a live aligned shell of the exact family under the existing no-overlap ownership discipline; null is Invalid. Add Rust signature tripwires and complete golden inventories. |
| Effects/counts | HostState / ArgMem Unstated, params [], escapes [0], Release None, no fresh/divergence, matching WriterFlush. Runtime keys 453, base symbols 471, alloc 478, par probe 475, task probe 471, maximum 482. No new shape or LLVM attribute. |
| Runtime implementation | One cfg-selected native sync authority shared by both methods. Writer flush precedes it and gates it on success; File delegates without a buffer or access-mode restriction. A narrow compile-time native-operation seam permits deterministic no-fallback/error/order proof without process-global injection. No process/global-state mutation or restoration. |

## Implementation closure matrix and exact owner checklist

| Axis | Implementation and owner proof |
| --- | --- |
| Formation/malformed input | Both zero-argument methods, wrong arity/type, missing imports, temporary-receiver gate, Impure/Pure parallel rejection and receiver lifetime checks. Extend checked-HIR forged sibling owner and MIR exact receiver/i32 contract owner for both variants; no panic on invalid nodes. |
| Construction/acquisition/native input | No new object or acquisition. Native null pointer rejects before dereference; a live shell with fd -1 returns the fixed EBADF mapping. Platform selector and one-request identity are tested independently from byte visibility; injected EINTR/ENOTSUP/EIO return errors without retry/fallback. |
| Buffered state/error precedence | Buffered payload shorter than writer capacity is absent from a fresh read before sync and present afterward, before Drop. Flush failure clears the buffer per existing contract and makes zero native sync requests. Sync failure after successful flush leaves empty buffer and open usable owner. Empty/repeated writer sync still issues the request. No global stdout/umask/environment mutation. |
| Move-in/out/nulling/Drop/replacement/return | Representation unchanged. Driver uses owner after sync, returns/passes it to helpers, replaces it, and rejects use after move. Sync does not consume or close; explicit cleanup/Drop remains exactly once. Native ownership guards close descriptors even on assertion unwind. |
| If/match/else/try/map_err/loop/early exits | Driver parameterizes both methods over Result control, helper early return and repeated loop calls; errors preserve owner usability. Existing direct method lowering stops on terminated receiver paths. Negative tests keep handle join restrictions unchanged. |
| Generics/interface/whole/per-unit | Imported generic helper invokes File/writer sync and returns Result<Unit, Error>; run both compiler modes. No serialization/type fingerprint changes. |
| Provenance/allocations | Existing fd/shell families, no new owner or output. Existing buffer flush retains capacity and no native path is formed. Current owner/drop and constructor coverage is reused, not duplicated into every control combination. |
| Borrowed writer projections/dependent lifetime | Extend existing parameterized plan-37 owners to invoke WriterSync through nested borrowed writer fields and borrowed Option/Result payload bindings, on success and native-error paths. Containing owners still clean up exactly once; no borrowed payload is consumed, escaped or independently dropped. Keep connection-backed writer lifetime/capture/owner-retirement rejection. Extend `borrowed_handle_receiver_hir_rejects_forged_places` and `borrowed_handle_receivers_reject_forged_mir_projections` to WriterSync over malformed root/path/type cases. Existing projection/parent cleanup assertions must bind the new operation itself, not only WriterFlush. |
| Native platform acceptance | macOS APFS real F_FULLFSYNC succeeds for ordinary read/write and read-only File; Linux real fsync succeeds on the test filesystem. Record its type. Fresh-open readback proves visibility, not crash survival; scripted native policy proves selected-request identity and error semantics; the cfg-selected provider arm is inspected for the exact syscall. Nonpersistent tmpfs is explicitly excluded from a persistence claim. |
| Explicit deferrals | Directory synchronization/publication durability protocol, data-only option, rollback, implicit sync, hardware fault/power-cut test rig, filesystem confinement, concurrent-handle ordering, consumer code and K1. |

## Author ledger-to-prose consistency pass

All public arguments/results have fixed types, lifetime and allocation rules;
zero arity, null-owner precedence, flush-before-sync and no fallback are explicit.
There is no text/wire input, persisted format, runtime inspection, changing global
state or new cache graph. Each native error uses the existing canonical Error
mapping; no synthetic unsupported discriminator or second error model is added.
The method shape does not imply a parent-entry durability guarantee. Acceptance
separates operation identity, error ordering and fresh-open visibility. The native
seam, owned fixtures and descriptor guards close malformed and lifecycle cases
without ambient mutation. One shared capability closes producer-to-consumer ABI.

Required synchronized sources are draft.md, docs/language-spec.md,
docs/design-notes.md, the Settled section of docs/open-questions.md, current M9/M12
roadmap, std-design/fs.md and ja mirror, checked-HIR ledger 19 and runtime ABI
ledger 20, plus plan 37’s stable-writer-method ledger. Function/method declarations appear in text; owner programs syntax-check
actual positional calls. HANDOFF records the capability once, after implementation.

Native policy sources: [Linux fsync(2)](https://www.man7.org/linux/man-pages/man2/fsync.2.html)
fixes file/metadata synchronization and the separate parent-directory requirement;
[Apple fcntl(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html)
fixes F_FULLFSYNC's extra device-cache request and hardware limitations. The local
APFS read/write and read-only descriptor probe returned zero for F_FULLFSYNC.
No stronger physical persistence conclusion is drawn from this probe.

## Independent plan-review closure

One fresh inspection-only adversarial plan review found one P2: the new writer
operation's projection/dependent-owner proof was not explicitly bound to owners
for nested borrowed fields and borrowed optional/result payloads. The matrix now
binds those existing parameterized owners to WriterSync itself, including parent
cleanup, forbidden consume/escape, TCP dependent lifetime, and malformed checked
HIR/MIR root/path/type witnesses. Plan 37's stable-writer-method ledger is added
to the synchronized sources. This closes an acceptance-cell gap without changing
the proposed public/native strategy; no second full plan review is needed. No
other P1/P2 findings were reported.

## Implementation closure and native qualification

`check_file_method` and `check_writer_method`, explicit sibling visitors, checked
HIR and `native_owner_mir_contract` form the exact unit/Error and i32 boundary.
`lower_file_expr` and the shared writer lowering borrow once, honor terminated
receivers and use `lower_status_result`. Native `sync_fd_with_ops` selects one
request; `writer_sync_with_ops` gates it behind the existing `flush_buf`. Neither
changes owner layout, cleanup transport, interface format or allocation policy.

| Closure cells | Exact owner |
| --- | --- |
| Formation, arity/result, temporary/move/effect gates | `m12_file_io::sync_formation_effect_and_move_diagnostics`, existing `file_constructors_require_std_fs_import` |
| Control, returned generic owners, replacement, whole/per-unit | `m12_file_io::sync_file_and_buffered_writer_in_imported_helpers` |
| Connection-dependent return, retirement and capture | `m12_file_io::sync_preserves_connection_writer_lifetime` |
| Borrowed nested/optional/result projections and containing cleanup | `struct_handle_fields::borrowed_writer_sync_preserves_nested_and_optional_owners`, parameterized `borrowed_handle_io_failure_retires_the_containing_owner` |
| Borrowed consume/escape and malformed places | `borrowed_handle_projections_reject_consumption_and_escape`, `borrowed_handle_receiver_hir_rejects_forged_places`, `borrowed_handle_receivers_reject_forged_mir_projections` |
| Exact malformed HIR/MIR receiver/result and fail-closed entrypoints | `sync_methods_hir_reject_forged_native_types`, `sync_methods_mir_gate_requires_exact_native_receiver` |
| Native request count, errno/no retry/fallback, flush order, retained capacity | `align_runtime::file_sync_policy_and_writer_error_ordering` |
| Native read/write/read-only File, buffered readback and reuse before Drop | `file_sync_native_file_and_buffered_writer_readback` |
| Null/invalid descriptors and socket failure without close | `file_sync_invalid_and_unsupported_descriptors` |
| Variant/ABI inventory and exact native type/export parity | `storage_generation_expr_variant_sweep`, checked-HIR coverage tripwire, `runtime_keys_are_complete_unique_and_alphabetical`, runtime ABI type/effect/golden owners, `scripts/test-runtime-abi-exports.sh` |

The native owner passes on macOS APFS and Linux x86_64 Ubuntu 24.04 overlayfs.
These are native-operation and visibility qualifications, not ext4 or power-cut
persistence certification. The scripted flush/order owner fails when flush is
omitted and when the native request is omitted; the restored three native owners
pass. The cfg-selected native provider has exactly one F_FULLFSYNC arm on macOS
and one fsync arm on Linux. Physical crash/device-cache behavior is explicitly
outside the test's evidence; no stronger persistence conclusion is drawn.

## Preflight review closure

One independent full-diff inspection found two P2s and no production correctness
or soundness defect. The existing exact sync decision is moved from the historical
Open item into Settled. Each new sync insertion/rewrite now has separate source
pattern/result assertions; buffer-only negative cases are explicitly exempt.
The counted cleanup twins also assert the formed WriterSync count independently
of relocating their owned path. Omitting any of the three operation rewrites or the independent path rewrite
fails its named owner at the source assertion before that case is compiled. The focused borrowed consume/escape and success/error cleanup
owners close this class without changing public contract, IR shape or strategy.
