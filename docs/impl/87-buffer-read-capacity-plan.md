# Observable buffer read-window capacity (Request 35)

## Capability boundary and public-contract ledger

One useful end-to-end capability exposes the existing buffer descriptor's usable
read window. A caller can test a best-effort reservation before native input,
without inferring it from a zero-length read. This implements Request 35's
capacity-accessor trigger; fallible construction and consumer adoption remain
separate work. Plan 65's settled allocation policy is unchanged. K1 is deferred.
The expected diff is below 1,000 handwritten lines, with no benchmark claim.

| Surface | Exact public record | Owner and acceptance |
| --- | --- | --- |
| `b.capacity() -> i64` | Exactly zero arguments on `buffer`, Pure and nonconsuming. Returns the descriptor's current usable read-window capacity, independently of initialized `b.len()`. This is the bound capacity-limited native fills may use, not hidden allocator spare capacity or a physical-memory/residency guarantee. No `cap` alias, argument, default, error, allocation, side effect or retained view. | Sema/checked HIR, existing MIR `BufferCapacity`, existing A29 native `i64(ptr)` query. Driver constructor/growth/read/EOF owners and native descriptor-versus-allocator-spare witness. |
| Capacity transitions | `buffer(n)` starts with length zero: successful best-effort reservation publishes capacity `n`; invalid/unreservable input publishes zero. `buffer.filled(n, v)` has initialized length `n` and capacity at least `n`. Existing `put_*`, `append` and `append_filled` retain old capacity or raise it to the new initialized length, whichever is greater. A shorter read or EOF changes length without shrinking the capacity. `read_line` deliberately grows beyond the old window; successful terminated/unterminated lines raise capacity to max(old capacity, stripped body length), while error/EOF retains capacity and clears length. Existing decoded/returned buffer producers publish capacity equal to initialized length, independently of allocator spare. Spare allocator bytes beyond the published window do not enlarge bounded native fills. | Existing runtime descriptor authority; focused empty/negative/unreservable/filled/growth/short-read/EOF, growing line/error/EOF and returned-buffer tests. No new allocation failure injection or process-global mutation. |
| Receiver/ownership | Exactly the stable receiver places already admitted by `bytes`/`len`: bound local, validated nested field rooted in a local or shared/exclusive parameter, and checked borrowed Option/Result/user-sum payload binding. Temporary and indexed handle receivers keep their existing rejection. Evaluate the receiver exactly once; a terminating receiver performs no query. Borrow the owner only during the call; the Copy i64 can outlive it. No move, source nulling, new Drop, replacement, return restriction or owner lifetime extension. | Plan 37 stable-place authority and existing borrowed projection validation. Whole/per-unit imported generic, nested field and borrowed payload owners; malformed root/path/type and use-after-move negatives. |
| Allocation/error limits | No recoverable allocator Result is added. The ordinary constructor retains best-effort reserve; filled construction and growth retain terminal invalid-size/OOM policy. A positive capacity does not promise future growth success, available host memory, page residency or immunity to the OOM killer. Querying cannot turn an advisory reserve into a guaranteed allocation API. | Settled scalar/capacity decision and plan 65; language and core string documents state the same boundary. |
| Artifact/interface | No new type, serialized field, wire format, CLI/build input, ambient configuration or type fingerprint. New HIR uses compiler Git namespace identity; existing MIR structural hashing and imported generic source reparsing cover artifacts. Prerequisites are shipped M9 buffer and plan 37 receiver rules. | Whole/per-unit imported helpers, existing interface identity. |

## Compiler and native inventory

| Boundary | Exact record and closure |
| --- | --- |
| HIR | `BufferCapacity { buffer: Box<Expr> }`, exact stable `Ty::Buffer` receiver and signed 64-bit result. Explicitly sweep every `BufferLen` sibling for purity, traversal/depth, escape/result region, storage, move, replay, finalization and checked producer validation. HIR count 347 -> 348. It returns a Static scalar while traversing its nonconsumed receiver. |
| MIR | Reuse `BufferCapacity(Operand)`, exact Buffer READ operand and i64 result; reject forged type/SSA/projection before LLVM. Lower once and stop after a terminating receiver. Existing internal HTTP/UDP users stay admitted. Capacity remains opaque to stack byte-storage optimization: do not substitute initialized length for capacity or widen the native read window. |
| Runtime ABI | Reuse `BufferCapacity` / `align_rt_buffer_capacity(*mut Buffer) -> i64`, A29, IndirectStorage / ArgMem Unstated, params [], escapes [0], Release None, no fresh/divergence. Existing query returns descriptor `cap`, null -> zero, and does not inspect source/artifacts. The unsafe pointer must be null or a live aligned Buffer shell under the existing exclusive mutation/shared read discipline. No native symbol, ABI shape, key count or export-count change. |

## Implementation closure matrix

| Axis | Implementation and exact owner plan |
| --- | --- |
| Formation/validation/malformed input | Add method dispatch on one checked/resolved receiver. Zero arity, wrong receiver, temporary/moved receiver and wrong expected result reject without panic or duplicate diagnostics. `m9_io::buffer_capacity_formation_and_move_diagnostics`; add valid producer and forged result/receiver/place cases beside the existing BufferLen checked-HIR and MIR projection owners. |
| Constructor/reservation/growth/native capacity | Existing runtime descriptor query, no runtime allocation-policy change. `align_runtime::buffer_capacity_tracks_read_window_not_allocator_spare` proves zero/reserved/filled, the three append growth routes, returned-vector buffers and distinct length/spare capacity. `buffer_capacity_tracks_growing_lines_and_retains_window_on_eof_or_error` proves line success/error/EOF. Existing huge-capacity owner is extended to query the zero window directly. No huge physical-memory commitment is used as a test. |
| Read/short read/EOF | Private driver input fixture and the real reader exercise capacity before read, after a full read, shorter read and EOF. Buffered line growth and returned-buffer transitions have exact native getter owners. New `m9_io::buffer_capacity_in_imported_helpers_and_read_windows` runs whole/per-unit; capacity stays fixed while length changes. Query omission/len substitution must fail exact values. |
| Move-in/out/nulling/Drop/replacement/return | Query only borrows; owned buffer can still move, return, replace and drop normally. Scalar query result may outlive owner. Existing buffer/containing-owner cleanup authority reused; focused imported helper and use-after-move owner binds this operation. No new representation or cleanup path. |
| If/match/else/try/map_err/loop/early exits | Scalar result crosses branch/loop joins and helper return; surrounding read Results use existing control operators. Extend the existing plan-37 positive borrowed buffer source to invoke capacity through nested/Option/Result projections. Existing containing cleanup and malformed borrowed-place owners remain authoritative. No fallible query or result carrier is invented. |
| Borrowed projections/dependent lifetime | Nonconsuming shared/exclusive complete-owner and borrowed payload calls. Query cannot consume/return/capture the handle or add mutation authority. Extend `borrowed_handle_receiver_hir_rejects_forged_places` and `borrowed_handle_receivers_reject_forged_mir_projections` with Capacity cases. Bind every source rewrite separately, or use literal new source rather than an unasserted rewrite. |
| Generic/interface/whole/per-unit | Imported generic wrapper around a buffer-owning record calls capacity. Both modes return exact values, preserving existing source-template and nominal identity. No interface schema change or consumer build. |
| Allocation/provenance/optimizer | Existing native getter performs only descriptor load. Exact MIR contract authenticates original Buffer/projection owner, without a writable/consuming permission. Stack byte-storage refuses capacity observations rather than changing their meaning; sibling visitor inventory and malformed MIR owner cover result/operand types. Existing internal-native consumers remain covered by the bounded gate. |
| Explicit deferrals | Fallible constructor, recoverable growth, allocator-spare inspection, host/cgroup memory admission, physical commitment, indexed/temporary receiver widening, nonempty handle-record array construction, consumer fixtures/adoption and K1. |

## Author ledger-to-prose consistency pass

Arguments/results, zero arity, static result lifetime, no allocation and exact
read-window authority are fixed above. No text/wire input, persisted format,
global native state or new cache graph is introduced. Existing ordinary method
validation establishes the receiver before arity/place checks. Invalid receivers
return the existing Error sentinel instead of a typed capacity node. Native null
is zero, while invalid non-null pointers remain outside the unsafe ABI contract.
Capacity can be lower than hidden allocator spare bytes; exposing those bytes
would alter existing bounded-read semantics and is explicitly excluded.

Required agreement: draft.md, docs/language-spec.md, docs/design-notes.md,
Settled docs/open-questions.md, current M9 roadmap, checked-HIR ledger 19, plan
37's stable buffer row, and core-design/string.md plus its ja mirror. Runtime ABI
ledger 20 already owns this exact query; inventory counts remain unchanged.
HANDOFF records the completed capability once. Request 35's register answer must
separate the shipped accessor from the deferred fallible constructor and leave
consumer verification pending. Declarations above are separate from positional
source calls; focused owner programs syntax-check those calls.

## Independent plan-review closure

The fresh inspection-only review found one P2: describing every native read as
capacity-limited omitted `read_line`, whose existing contract grows the window.
The transition ledger and native owner explicitly cover stripped terminated and
unterminated line bodies, error/EOF capacity retention, and decoded/returned
`buffer_from_vec` capacity independent of allocator spare. This closes the
descriptor producer/transition class without changing allocation strategy.

## Author implementation closure

`check_buffer_capacity` reuses the once-checked receiver and stable Local/Field
formation gate. The explicit BufferLen sibling policies traverse the borrowed
receiver while producing a Static Copy scalar. Checked HIR authenticates exact
place/Buffer/i64; lowering uses the termination guard and existing BufferCapacity.
The native owner MIR contract qualifies exact Buffer READ/i64 before LLVM.
The runtime implementation, ABI row/effects/export counts and buffer allocation
policy are unchanged. Capacity observations remain excluded from stack-buffer
promotion by its existing nonescape whitelist.

| Matrix cells | Exact owner |
| --- | --- |
| Formation/arity/type/temporary/move/borrowed return/capture and one root diagnostic | `m9_io::buffer_capacity_formation_and_move_diagnostics` |
| Reservation/growth/read/short-read/EOF, scalar lifetime, move/return/replacement, control/import/generic and whole/per-unit | `m9_io::buffer_capacity_in_imported_helpers_and_read_windows` |
| Native null/zero/negative/unreservable/filled/put/append/append_filled and hidden spare/returned-vector distinction | `align_runtime::buffer_capacity_tracks_read_window_not_allocator_spare`, extended `buffer_huge_capacity_degrades_to_empty_window_not_abort` |
| Native terminated/unterminated growing lines, empty line, EOF/error retained capacity | `buffer_capacity_tracks_growing_lines_and_retains_window_on_eof_or_error` |
| Nested and user-sum borrowed projection; shared parallel Pure reading | `struct_handle_fields::borrowed_buffer_views_follow_optional_array_and_generic_sources`, `borrowed_buffer_views_support_parallel_shared_readers` |
| Borrowed-place malformed root/path/type and no independent cleanup | `borrowed_handle_receiver_hir_rejects_forged_places`, `borrowed_handle_receivers_reject_forged_mir_projections`, reused `borrowed_handle_io_failure_retires_the_containing_owner` |
| Malformed exact receiver/node/local/result, rejected before lowering/LLVM | `buffer_capacity_hir_rejects_forged_native_types`, `buffer_capacity_mir_gate_requires_exact_native_receiver` |
| Exhaustive HIR/replay/storage/validation inventory | `storage_generation_expr_variant_sweep`, checked-HIR coverage tripwire, `hir_body_validator_native` |

The borrowed user-sum fixture uses the canonical imported `views.Data.Present`
constructor, including the adjacent existing invalidation twin. Positive owners
query exact values in literal source, without operation-inserting rewrites.
No public policy, IR strategy or capability boundary changed during closure.

The native capacity owners pass on macOS Apple Silicon and Linux x86_64. Three
mutations are discriminated: returning initialized length, exposing hidden Vec
spare, and omitting the MIR native-owner type contract. Each fails its named
owner; restoration passes. Functional byte/window values are evidence of the
existing descriptor contract, not physical-memory commitment or an allocation
success promise. No new runtime implementation or allocation policy is shipped.

The dedicated huge-capacity owner and public driver qualify unreservable requests
on native macOS. Linux x86_64 Docker/Rosetta qualifies the two new descriptor/line
owners; its emulated allocator stops a huge duplicate request with SIGTRAP before
Rust observes reserve refusal, so no huge-allocation qualification is claimed
there. The existing dedicated refusal owner and public contract are preserved.
