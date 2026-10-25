# Plan243: bounded HTTP peer receive observation

Request AM-R23 requires a non-consuming connection observation before response.
Exact surface: ctx.peer_eof() -> Result<bool, Error>.
This observes incoming TCP EOF, not peer liveness or ability to receive a response.
The name deliberately states the narrow observation. No new type or error model.

- Impure shared borrow of an existing http_request_ctx; usable through borrowed
  helpers, non-consuming on Ok and Err; request views remain live. The receiver
  must be a bound Local or local-rooted Field place, matching check_http_ctx_method
  and independent http_request_ctx_place. Temporary receivers are rejected.
- No arguments or ambient configuration. Zero recv calls on admission failure;
  exactly one recv after admission, for one stack byte with
  MSG_PEEK | MSG_DONTWAIT. No wait, poll, allocation, mode/timeout change, loop,
  EINTR retry or ownership transfer. This is a finite operation count, not a
  hard scheduler wall-clock promise. Interrupted call returns existing Code(EINTR).
- Native pointer admission precedes socket use. Null/misaligned context/output
  or overlapping/overflowing context-shell/output address ranges returns Invalid
  without dereference
  or output writes. After pointer admission, initialize output to zero, then
  reject fd<0 with Invalid. Do not scan request metadata; that is unrelated to
  this bounded operation. Live OS failures map via existing io_error_to_status.
  WouldBlock becomes Ok(false), positive count Ok(false), zero Ok(true), other
  errors Err. Never relabel nonblocking WouldBlock as Timeout.
- True means peer has closed its sending direction after queued kernel bytes;
  both orderly full close and SHUT_WR can produce it. Peer may still receive.
  False means EOF not observed, including queued data and no event; does not prove
  live peer or promise a future write succeeds. Reset may return Error.Code;
  a receive probe may consume a pending socket error despite preserving bytes.
  No cached state; repeated calls are new observations and can differ.
- Read no application buffer bytes and consume no kernel receive bytes. Already
  co-read residual request bytes still follow existing close-after-response policy;
  kernel-queued pipeline data stays queued and can hide later EOF until drained.
  A true result can coexist with buffered residual bytes. No keepalive eligibility,
  parking, response framing, ctx content or ownership change.
- Normal responding, streaming/upgrading or Drop remains legal after observation,
  including Err; existing operations retain cleanup and write-error responsibility.
  Cancellation, GPU release and publication policy remain media-owned.
- ABI: i32 align_rt_http_ctx_peer_eof(ptr ctx, ptr out_u8). Status0 publishes exact
  0/1 at out. Admission errors leave output untouched; later errors leave zero.
  Result<bool,Error> uses existing scalar-result lowering (as process.member.finished),
  with dedicated native scratch initialized by the runtime after admission. The
  Result payload is loaded only on success; Err reconstruction never reads it. HostState native effects conservatively retain
  ordinary HTTP pointer attributes; no owner release, retained pointer or fresh
  pointer. No unsafe direct caller may fabricate a valid owned handle. The
  pointer contract requires a live allocated/aligned ctx and disjoint writable
  one-byte output; detectable malformed inputs above return Invalid.

Closure owners: source method arity/types/effects and use-after-consume; borrowed
helper and imported whole/per-unit; independent checked-HIR and malformed MIR
exact receiver/result/out ABI; injected native zero/data/WouldBlock/Interrupted/
reset outcomes with one-call counts; native pointer admission matrix crosses null/
misaligned context, null output (u8 alignment is 1), shell/output overlap and checked
address-range overflow, requiring Invalid, no recv and untouched output; valid
pointer admission followed by fd<0 requires Invalid, no recv and initialized zero;
valid disjoint pointers are the positive control. Allocation counter owner spans
admitted zero/data/WouldBlock/EINTR/reset outcomes and confirms no runtime allocation; real live peer, FIN, SHUT_WR plus
successful response, queued bytes preservation, co-read residual behavior, repeat
observation, keepalive next request, Drop and server-outlives/context-outlives
cleanup. Reuse existing response/accept owners for unaffected framing. No benchmark
claim beyond zero runtime allocations and zero-or-one bounded-size syscall according to admission.

Primary references read: POSIX recv
https://pubs.opengroup.org/onlinepubs/9799919799/functions/recv.html
Linux recv MSG_DONTWAIT per-call
https://man7.org/linux/man-pages/man2/recv.2.html
Apple recv half-close and PEEK
https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/recv.2.html
Darwin installed SDK sys/socket.h confirms MSG_DONTWAIT=0x80; Linux uses its
platform libc constant. Use libc::MSG_DONTWAIT | libc::MSG_PEEK, not hand-copied flags.

Normative agreement: draft, digest, design notes, Settled, std HTTP EN/JA,
checked HIR and runtime ABI ledgers+golden. Native implementation and all IR
consumers belong in one useful capability; no standalone native-only seam.
Author completeness pass: exact input/output, every result/error, admission order,
allocation/ownership, absence of mode/global-state changes, artifacts and mirrors,
and acceptance owners are enumerated above. No text input, wire format, nominal
fingerprint, table or new public type is introduced; compiler/runtime identity
already invalidates cached bodies. New HIR/MIR variants must explicitly join all
analysis, cloning, interface/replay, effects, operand/slot, native ABI/export and
printing owners. Positive source examples will be compiled in the driver owner.
The entire native boundary remains necessary for one immediately usable operation.
A fresh independent strategy review accepted this contract and owner matrix.
The receiver, zero-or-one syscall count and native admission/output cases were
made explicit before implementation. Consumer cancellation and native transfer
policy remain outside this observation capability.

## Implementation closure matrix

| Invariant / path | Implementation | Discriminating owner |
|---|---|---|
| Bound local/field type and arity; Impure effect; shared borrow on both results | `check_http_ctx_method`, `EffectScan`, `MoveCheck`, independent HIR derivation | driver `peer_eof_types_effects_and_ownership`; HIR `http_peer_eof_hir_shape_and_effect` |
| Construction, move-in/out, source nulling, replacement and Drop | No new owner or move; bool/Error Result is existing Copy payload. Receiver is read once and never transferred. Existing context cleanup remains the owner. | source use-after-respond negative; native `http_peer_eof_co_read_residual_and_drop`; existing response cleanup owners |
| if/match/else/?/map_err, loop/branch joins and return | Existing Copy Result control flow; `lower_required!` precedes native action and success-only payload load. A receiver has no eager argument subtree. | source owner covers `map_err`, `?`, `else` and `if`; native driver covers generic return and loop re-observation; HIR/MIR used/discarded matrices |
| Generic monomorphization, interface replay, whole-program/per-unit calls and live request views | Explicit HIR depth, replay clone, finalization, Escape and Move inventories; no serialized type change | `peer_eof_imported_native_and_keepalive`, two imported generic instantiations and both executable modes |
| Malformed HIR/MIR and unused results | Exact bound receiver/Result and native READ/i32 status/bool scratch contract; no ordinary scratch Store | `http_peer_eof_hir_shape_and_effect` (five mutations, four entrypoints); `http_peer_eof_mir_shape` (six mutations, used/discarded) |
| Native admission order, disjoint output and checked ranges | `http_peer_eof_with` before any pointed-to access; output zero after admission then fd check | `http_peer_eof_native_admission_precedence` |
| Zero-or-one syscall, zero allocation, canonical bool, WouldBlock/EINTR/reset | FnOnce injection shares production admission and outcome mapping; public wrapper is one libc recv | `http_peer_eof_one_attempt_outcomes_and_allocation` with alloc-count; public native observations also measure allocation/free parity |
| Kernel bytes, half-close, full close, residual bytes, mode and ownership | Per-call PEEK/DONTWAIT; no context mutation or retained address | `http_peer_eof_live_half_close_and_queued_bytes`, `http_peer_eof_co_read_residual_and_drop` |
| Successful response and keepalive after observation | Existing serializer and park/drop owners; no new cleanup branch | driver `peer_eof_imported_native_and_keepalive`; existing `http_response_budget` and `http_server_accept_budget` owners |
| ABI types, effects, identity, exports and printing | RuntimeKey, A19 row, HostState effects, MIR print and explicit unsupported optimization inventory | codegen `runtime_abi` owners and `scripts/test-runtime-abi-exports.sh` |

No new heap/arena provenance, owner-bearing result, connection-global state,
replacement policy or serialization schema is introduced. Existing Result and
context owners close those unchanged axes; stream/Upgrade keep the same context
as response and require no separate observation-specific cleanup. The author
ledger-to-diff pass covers every admission, exact output, ownership, effect and
zero-or-one-call requirement above. This single native/IR/source boundary is
immediately usable and remains below 1,000 hand-written changed lines.
