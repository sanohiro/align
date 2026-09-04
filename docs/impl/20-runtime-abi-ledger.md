# Runtime native ABI ledger

## Status and authority

This is the exact native symbol/type/attribute appendix for L2b-a2-am-r and
L2b-a2-am-c1. It records the LLVM 22 declaration surface emitted by the current
backend for every validated runtime target and the additional externally
visible runtime definitions that occupy link identities. The keyed surface is
generated from a trivial valid program; the complete base and `alloc-count`
surfaces are independently compared with the Rust runtime exports.

With bounded canonical JSON, process capture, bounded HTTP response bodies,
owned JSON, exclusive filesystem publication, retained-root regular-file access, HTTP client
raw/SSE receive streaming, asymmetric signatures, `std.log`, `core.codec`, `pkg.frame`, `pkg.kv`,
`pkg.csv`, `pkg.ws`, and `pkg.template`, there
are 347 `RuntimeKey` variants and a one-to-one native-symbol record. Relative to Am-c1, F-B added
`ArrayBuilderNewIn` and `ArrayBuilderPushBytes`; the four
AEAD symbols that were previously selected from `AeadCipher × AeadDir` become
ordinary typed keys; they may no longer bypass the registry. Eighteen always-built
runtime records have no `RuntimeKey` and instead use the eighteen-variant
`UnkeyedRuntimeKey`: the two main-wrapper callees
`align_rt_report_error` and `align_rt_args_build`, plus the runtime-internal
`align_rt_arena_reset`, `align_rt_realloc`, and
`align_rt_http_serialize`, and eight package-internal PostgreSQL codec helpers:
`align_rt_f32_to_bits`, `align_rt_f32_from_bits`, `align_rt_f64_to_bits`,
`align_rt_f64_from_bits`, `align_rt_f32_text_len`, `align_rt_f64_text_len`,
`align_rt_f32_text_write`, and `align_rt_f64_text_write`, plus the four compiler-private
`core.test` child-control rows recorded below and the package-internal checked TCP timeout row
`align_rt_tcp_conn_set_io_timeout`. The base native registry therefore has 365 records. Request 12
adds the keyed bounded-builder stack initializer and consuming status/out-slot finish; both reuse existing ABI shapes
A51 and A19.
The explicit `alloc-count` runtime feature may expose seven
test/benchmark-only definitions: the allocation/free and finder counters plus
`align_rt_requested_live_reset`, `align_rt_requested_live_bytes`, and
`align_rt_requested_live_peak`. `par-map-probe` may expose four more:
`void @align_rt_test_par_map_force_caller(i32)`,
`i64 @align_rt_test_par_map_min_chunk()`,
`i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)`, and
`i64 @align_rt_test_par_map_workers()`. `task-group-probe` and
`crypto-asymmetric-probe` change internal Rust state only and add no unmangled native export.

The compiler-visible native registry is always exactly the 365 base records.
There is no target option, environment variable, Cargo feature, linked-runtime
inspection, or other ambient input that changes it. The eleven optional probe
records extend only the verification-time maximum runtime-export table to 376.
They never gain a `RuntimeKey`, callable/declaration policy, collision
reservation, or compatible-extern reuse. Their spellings remain ordinary
program/extern/export identities in a normal build. Probe-feature runtime
builds are test/benchmark fixtures and never link user artifacts. Thus runtime
feature selection changes neither accepted callable input nor MIR, interface,
artifact, or cache identity. Registry membership is never inferred from symbol
spelling.

Request 11 added six regular `RuntimeKey` rows for bounded process capture. Request 5 subsequently
added the two bounded-HTTP setters, and Request 9 then added `BuilderWriteUint`.
The raw HTTP receive-stream capability subsequently added six keyed rows for buffer-capacity
inspection, stream construction/access/read, and Drop. The SSE capability added four keyed rows for
the consuming transition, state getters, and event read. The asymmetric signature suite then added
six keyed rows. The `core.test` child-control extension then added four unkeyed rows, and `std.log`
added six keyed rows, `core.codec` then added eight, `pkg.frame` added two, `pkg.kv` added one
source-reachable unkeyed row, `pkg.csv` added one keyed row, `pkg.ws` added eleven keyed rows, and
`pkg.template` added five keyed rows. Their runtime
definitions and registry entries activated atomically at their respective capability boundaries: the current exact counts
are 347 keyed records, 365 base records, and 376 records in the maximum optional-probe export table.
No new probe category was introduced. The implemented `pkg.kv` row reuses an existing ABI shape. Its two
independently useful prerequisites first hardened the shared TCP timeout substrate and existing
TCP-derived writers without changing a symbol, key, shape, attribute, or count.

## core.test child-control extension

The `core.test` design added four compiler-private unkeyed rows while leaving the then-current keyed
count at 314. The registry keys, LLVM declarations, Rust exports, collision reservation, runtime
ABI fingerprint, and test-mode selectors activated atomically. Only a generated
test harness may select them; they receive no language-callable `RuntimeKey` or compatible user
extern reuse.

The first-capability test-callgraph validator rejects every catalog-reachable
`ExprKind::ProcessCommand` before runtime selection or artifact allocation. It therefore adds no
containment descriptor, supervisor-status codec, or fifth child-control ABI row; ordinary
production `process.command` continues to select its shipped runtime entries unchanged.

| Unkeyed key | Exact symbol and LLVM declaration | Exact Rust ABI |
|---|---|---|
| `TestLaunchRecvV1` | A110: `i32 @align_rt_test_launch_recv_v1(i32, ptr)` | `extern "C" fn(i32, *mut u32) -> i32` |
| `TestFdCloexecV1` | A111: `i32 @align_rt_test_fd_cloexec_v1(i32)` | `extern "C" fn(i32) -> i32` |
| `TestAckV1` | A112: `i32 @align_rt_test_ack_v1(i32, i32)` | `extern "C" fn(i32, u32) -> i32` |
| `TestReportV1` | A113: `i32 @align_rt_test_report_v1(i32, i8, i8, i32, i32)` | `extern "C" fn(i32, u8, u8, i32, u32) -> i32` |

These declarations occupy A110 through A113. The implemented `std.log` design occupies A114 through
A117, `core.codec` occupies A118 through A120, `pkg.frame` occupies A121/A122 below, and `pkg.csv`
occupies A123. A124 is the next unreserved design shape. All
four declarations carry the existing
generated `nounwind` function attribute and no curated
parameter attribute. `TestLaunchRecvV1` requires a non-null four-byte-aligned output, stores zero
before I/O, performs one blocking datagram receive with a fixed 17-byte capacity and EINTR retry,
requires the exact 16-byte `ALTESTL` v1 envelope with zero reserved bytes, and stores the decoded
little-endian ordinal only on success. The generated harness, not the runtime, validates that ordinal
against its linked catalog. `TestFdCloexecV1` adds `FD_CLOEXEC` to fd 3 without changing any other
descriptor flag.

`TestAckV1` accepts every `u32` ordinal and emits the exact 16-byte `ALTESTA` v1 envelope.
`TestReportV1` accepts only outcome 0 with tag 255/code zero, or outcome 1 with tag 0..=4 and code
zero unless tag 4; it emits the exact 20-byte `ALTEST\0` v1 envelope. Each encoder uses one
stack-resident fixed array and one datagram send, retries EINTR, and maps a short send to `EIO`.
Every row returns zero on success or a positive raw OS code, with `EINVAL` for an invalid ABI
argument and `EPROTO` for malformed launch bytes. They allocate nothing, retain no pointer or
descriptor, never close fd 3, and change no process-global state. The harness owns fd 3 until
successful `process.exec` closes it through close-on-exec or process termination closes it after the
harness returns. The independent driver codecs and the runtime codecs both pin the three semantic
goldens in `core-design/test.md`; malformed-input, EINTR, short-send, export-parity, whole/per-unit,
and reserved-child-exit owners land with the rows.

## `pkg.kv` TCP capability and prerequisites (implemented 2026-09-02)

The two independently useful prerequisites are implemented in the shipped shared timeout and
TCP-writer substrates without changing an ABI identity. The checked package row and its consumer
are also implemented. For every usable address and positive `timeout_ns`,
`align_rt_tcp_connect` records a monotonic start and positive `Duration` budget immediately before
the first `F_GETFL`, then checks `F_GETFL` and `F_SETFL(flags | O_NONBLOCK)` before `connect`.
Either failure records its fixed errno-mapped status, closes that candidate, and continues to the
next address without calling `connect`. After checked installation, exactly one immediate
`connect` is issued: zero succeeds, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` enter the wait, and every
other errno is mapped immediately. Either immediate terminal result wins even if the budget is
simultaneously exhausted. The in-progress path continues against the same start/budget pair; it
never forms an absolute `start + budget`, so `Instant::checked_add` overflow cannot turn a huge
positive timeout into an unbounded wait. Each iteration subtracts `start.elapsed()` from the
budget. A positive remainder is rounded up to the next millisecond and saturated at `i32::MAX` for
one `poll`, so the complete positive i64 range remains bounded through repeated chunks; an
exhausted remainder returns `AL_TIMEOUT` before another poll. It does not issue a final
zero-timeout `poll` call.
EINTR recomputes the remainder, any other poll error is mapped immediately, and a zero result
causes another monotonic recheck and re-poll only when time remains or returns `AL_TIMEOUT` without
another poll when the budget is exhausted. A positive readiness/error event wins over a
simultaneously exhausted budget and is resolved through `SO_ERROR`. Every immediate or polled
success then checks `F_GETFL` and
`F_SETFL(flags & !O_NONBLOCK)`. Restoration failure closes that candidate, records the failure, and
continues, so no connection is published before checked blocking-mode restoration. The existing
nonpositive raw-ABI blocking path stays unchanged: public HTTP callers reject negative values before
this ABI, and raw `tcp.connect` supplies zero.

A nonzero `getaddrinfo` result returns before address iteration. `EAI_NONAME`/`EAI_NODATA` maps to
`AL_INVALID`; every other symbolic EAI value maps to
`AL_CODE.saturating_add(eai.saturating_abs())`. The already-cleared connection output remains null,
no socket is attempted, no address-list owner escapes, and transient host/service storage drops
before return. Direct symbolic EAI owners pin both mapping categories, null output, zero socket
calls, and cleanup.

After successful resolution, resolver order is observable. Unsupported families, null addresses,
and zero address lengths are skipped without changing the last failure. The first successful usable
address wins. No usable address returns `AL_INVALID`; if every attempted candidate fails, the
runtime returns the last status from socket creation, nonblocking `F_GETFL`, nonblocking `F_SETFL`,
an immediate connect errno, poll error/timeout, `getsockopt(SO_ERROR)` failure, nonzero `SO_ERROR`,
blocking-restore `F_GETFL`, or blocking-restore `F_SETFL`. Direct mixed-address owners place a
skipped entry and a later success after every failure class; all-failure variants pin the last
attempted status and candidate close count. DNS and the sum across addresses have no end-to-end
deadline.

The same prerequisite makes the shared positive-nanosecond socket-timeout conversion exact for
`std.net`, `std.http`, and the checked package row:
`ceil(timeout_ns / 1000)` microseconds, split into normalized
`timeval { tv_sec, tv_usec: 0..999999 }`; exact microseconds remain exact and zero retains the
existing clear/no-timeout meaning.

The command-capture consumer of the same poll-millisecond conversion also replaces its absolute
`Instant::checked_add` deadline with a monotonic start and positive `Duration` budget. Its complete
positive-i64 range therefore cannot degrade to an unbounded run; every positive remainder rounds up
and saturates exactly as above, while its existing post-syscall timeout-wins checkpoint order stays
unchanged. Direct owners cover exact/next and maximum-positive ns, us, ms, chunk, and deadline
boundaries; `F_GETFL`/`F_SETFL` install and restore failures on immediate and polled success; early
zero-result recheck versus exhausted/no-call poll, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` versus other
immediate errno, EINTR remainder recomputation, readiness at the deadline, every resolver skip and
last-status failure class, candidate close/continuation, a blocking-mode probe on every published
connection, HTTP plain/TLS/pool rearm, and command pipe-drain/post-EOF reap.

The direct runtime owners are `socket_timeout_timeval_quantization`,
`tcp_connect_timeout_budget_quantization`, `tcp_connect_transition_and_address_matrix`,
`tcp_connect_resolver_status_and_order_matrix`, `tcp_connect_positive_timeout_publishes_blocking_fd`,
`http_timeout_quantization_plain_tls_pool_rearm`, and `command_timeout_budget_quantization`. No
registry declaration, source-reachable symbol, ABI count, or runtime export changed at this boundary.

The implemented second independently useful prerequisite repairs the already-shipped
`TcpConnWriter` -> `IoWriterWrite` path rather than adding a second write ABI. The private runtime
`Writer` gains a socket sink kind and macOS/BSD readiness bit; only `align_rt_tcp_conn_writer` sets
the kind. A nonempty socket-kind write keeps the existing complete partial-write loop, EINTR retry,
and `EAGAIN`/`EWOULDBLOCK` timeout mapping, but Linux calls `send(MSG_NOSIGNAL)` and macOS/BSD
performs checked `SO_NOSIGPIPE` before the first send on that writer shell, caching only success.
The option failure sends nothing through that call and remains retryable; positive-length zero
progress deterministically returns `AL_CODE` (`core.Error.Code(0)`). File and standard-stream
writers retain the existing generic `write(2)` path. Connection-derived writers remain unbuffered
and non-owning, so `IoWriterFree` performs no write and does not close the socket.
`SO_NOSIGPIPE` is monotone and idempotent per socket: overlapping shells may each attempt it, each
sends only after its own successful result, a failed shell remains retryable, no shell Drop clears
it, and connection close discards it. No process-global signal state changes. Direct owners cover a
failed install with no send followed by retry, both success/failure orders for overlapping shells,
shell Drop without option clear, connection close, and closed-peer subprocess routes through the
direct slice overload, builder overload, `std.log`, and `io.copy`, plus file/standard-stream and
partial/EINTR/timeout/zero-progress parity. In particular, the existing keyed
`IoWriterWriteBuilder` identity keeps A19's
`i32 @align_rt_io_writer_write_builder(ptr, ptr)` declaration and
`unsafe extern "C" fn(*mut Writer, *mut Builder) -> i32` Rust ABI. At that prerequisite boundary it
remained in the then-shipped 330/347/355 keyed/base/maximum counts and delegated its borrowed
builder bytes to the hardened `IoWriterWrite` row. The existing `TcpConnWriter`, `IoWriterWrite`,
`IoWriterWriteBuilder`, and
`IoWriterFree` identities, LLVM declarations, Rust exports, attributes, registry entries,
fingerprints, and counts remain unchanged.

The direct runtime owners are `tcp_writer_complete_send_transition_matrix`,
`tcp_writer_macos_nosigpipe_state_matrix`, `tcp_writer_generic_fd_parity_and_socket_lifecycle`, and
`tcp_writer_closed_peer_routes_do_not_sigpipe`.

The implemented `pkg.kv` capability adds exactly one package-internal,
source-reachable unkeyed row. It closes the checked-configuration failure domain that the existing
public timeout setters cannot: those setters return Unit and discard `setsockopt` failure. The new
row is a general TCP-connection operation rather than a RESP parser or package-specific helper:

| Unkeyed key | Exact symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `TcpConnSetIoTimeout` | `align_rt_tcp_conn_set_io_timeout` | A04: `i32 @SYM(ptr, i64)` | `unsafe extern "C" fn(*mut TcpConn, i64) -> i32` |

`TcpConnSetIoTimeout` first rejects a null connection with `AL_INVALID`, then rejects
`timeout_ns` outside `1..=86400000000000` with `AL_INVALID`; either rejection occurs before reading
the fd or calling `setsockopt`, and a live connection remains usable after the range rejection.
Every non-null call has the unsafe precondition that the pointer names one live, unfreed `TcpConn`
held with exclusive logical access for the complete call, with no live reader/writer shell derived
from that connection and no other value retaining one at entry. A dangling or concurrently
aliased pointer or a live derived shell violates that precondition and is not detectable; no read,
write, other configuration, reader-or-writer construction, free, or Drop may overlap.

A target-connection retainer is classified by runtime provenance rather than numeric-fd equality:
its active recursive Drop graph reaches an initialized direct/buffered reader derived from that
connection, a derived writer, or a logger owning such a writer. The positive value graph uses
direct leaves, acyclic user-struct fields, nested active `Option`/`Result`, active user-sum paths
rooted in a logger/retaining struct/nested sum/tagged carrier, and elements of source-constructed
fixed arrays of retaining structs, including ordinary local, parameter, return, move, and by-value-
call placement. The fixed-array element is the retaining struct, so this composes existing struct-
field and fixed Move-struct-array rules rather than widening direct handle placement. Direct handle
collection/fixed-array/tuple/box elements and direct reader/writer
user-sum payloads reject formation. The admitted dynamic-array/slice shapes for retaining
structs/sums can name a type. A direct `DynStructArray` may additionally occupy a dynamic-array/
slice element, tuple element, or builtin `Option`/`Result` payload. Every admitted shape in this
paragraph except the tuple wrapper may occupy a user-struct field and then recurse through the
ordinary acyclic struct/tagged/sum carrier grammar. Every current
materializer, builder, decode, and
Move-slice producer rejects a live handle-retaining value. Inactive arms,
moved/null leaves, and carriers containing only other-
connection shells have target count zero; a compatible call requires zero even when a carrier can
otherwise reach multiple or mixed-provenance leaves.

For an admitted input the row uses the normalized ceil-to-microsecond `timeval` above, then installs
`SO_RCVTIMEO`. A failure returns its fixed errno-mapped status without attempting `SO_SNDTIMEO`;
otherwise it installs `SO_SNDTIMEO` and returns that status, or zero only after both succeed. Let
`R0` and `S0` be the receive/send option states at entry and `T` the requested state. Receive failure
leaves `{R0,S0}`, send failure leaves `{T,S0}`, and success leaves `{T,T}`. After either option
failure, a compatible caller must retire the still-owned connection, perform no read, write,
configuration, reader-or-writer construction, or retry on it, and invoke its ordinary free/Drop
path exactly once; the zero-derived-shell entry state leaves no shell cleanup to order against that
close. Success preserves usability and may construct derived shells afterward, but a later timeout
call may overwrite both options only after every such shell and retaining value has dropped. The row
itself allocates, retains, rolls back, closes, or consumes nothing. The null x
range product directly owns validation order and the no-fd/no-option side-effect rule.
Parameterized direct-runtime owners pre-arm both option states and pin live/exclusive plus
zero-derived-shell entry preconditions, the exact normalized `timeval`, option order, call counts,
returned status, the
`{R0,S0}`/`{T,S0}`/`{T,T}` post-state product, range-rejection retry versus option-failure retry
prohibition, zero overlapping/post-failure reader/writer-constructor calls, retirement, and later
free/Drop. One source-derived parameterized owner traverses the canonical recursive `DropPlan` and
matches every `DropPlan` node exhaustively, so a future cleanup-node variant requires
classification. Fixed arrays of retaining structs add no `DropPlan` node, so a separate owner pins
their `ty_is_move` and element-plan composition; source formation and no-live-producer negatives
own the admitted and excluded storage edges. Together they cross direct/buffered reader, direct
writer, logger-owned writer, struct/tagged/sum/fixed-struct-array placement,
active/inactive/moved-out state, target/other/mixed provenance, and zero/one/multiple target leaves.
They exclude every nonzero target count without invoking the unsafe row. For each positive carrier
class, the success cycle configures at zero, constructs and moves the leaf into that carrier, moves
it out where supported and drops it or recursively drops the smallest owning carrier, observes
zero, and reconfigures. The package
calls only on a fresh exclusively owned unpublished
connection with both entry options clear and before shell construction; its owner
closes after either option failure and proves that resolution is not reopened, no other address is
attempted, and no partially configured client is published.

The LLVM and Rust definitions use A04's default C calling convention and have no curated function,
return, or parameter attributes. The compiler recognizes the fixed physical symbol for exact ABI
compatibility, collision reservation, and source reachability. This adds no language builtin,
checked-HIR or MIR operation, call-spelling selector, or new ABI shape.

At package implementation, the one new key, symbol, definition, collision reservation, typed
registry row, runtime ABI fingerprint input, base/maximum export entry, and source-compatible extern
reuse activated atomically. It increased the unkeyed/base/maximum counts by one and the keyed count
by zero: the exact then-current keyed/base/maximum counts were 330/348/356. It reused an existing
shape, so A123 remained the next unused active shape until the later `pkg.csv` implementation below.
The current unkeyed count is eighteen, thirteen of which
are source-reachable. Both prerequisite hardenings and the new row are active with the `pkg.kv`
consumer.
Exact public consumption, poisoning, and owner matrix: `pkg-design/kv.md`.

## Implemented std.log extension (2026-08-31)

The logging implementation added four new shapes and two existing-shape keyed rows atomically:

| Runtime key | Exact symbol | ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `LogNew` | `align_rt_log_new` | A114: `ptr @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Writer, i32) -> *mut Logger` |
| `LogEnabled` | `align_rt_log_enabled` | A115: `i32 @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Logger, i32) -> i32` |
| `LogLine` | `align_rt_log_line` | A116: `i32 @SYM(ptr, i32, ptr, i64)` | `unsafe extern "C" fn(*mut Logger, i32, *const u8, i64) -> i32` |
| `LogLineBuilder` | `align_rt_log_line_builder` | A117: `i32 @SYM(ptr, i32, ptr)` | `unsafe extern "C" fn(*mut Logger, i32, *mut Builder) -> i32` |
| `LogFlush` | `align_rt_log_flush` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger) -> i32` |
| `LogFree` | `align_rt_log_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger)` |

All six keys, symbols, declarations, and definitions are active. At that capability boundary they
changed the inventories from 314/331/339 to 320/337/345 keyed/base/maximum-optional-probe records
and extended the implemented shape range through A117. No curated attribute, optional feature, or
target-dependent row was added. The public/runtime ownership and validation contract remains in
`std-design/log.md`.

## Implemented `core.codec` extension (2026-09-01)

The codec implementation activates eight keyed rows atomically with their checked-HIR records and
owner tests. They extend the inventories from 320/337/345 to 328/345/353
keyed/base/maximum-optional-probe records and the implemented shape range through A120.

| Runtime key | Exact symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `CodecOpenV1` | `align_rt_codec_open_v1` | A118: `i32 @SYM(ptr, i64)` | `unsafe extern "C" fn(*const u8, i64) -> i32` |
| `CodecEncoderNewV1` | `align_rt_codec_encoder_new_v1` | A119: `i32 @SYM(i64, ptr)` | `unsafe extern "C" fn(i64, *mut *mut CodecEncoder) -> i32` |
| `CodecEncoderPutI64V1` | `align_rt_codec_encoder_put_i64_v1` | A120: `i32 @SYM(ptr, ptr, i64, ptr, i64)` | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const i64, i64) -> i32` |
| `CodecEncoderPutF64V1` | `align_rt_codec_encoder_put_f64_v1` | A120 | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const f64, i64) -> i32` |
| `CodecEncoderPutBoolV1` | `align_rt_codec_encoder_put_bool_v1` | A120 | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const u8, i64) -> i32` |
| `CodecEncoderPutStrV1` | `align_rt_codec_encoder_put_str_v1` | A120 | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const AlignStr, i64) -> i32` |
| `CodecEncoderFinishV1` | `align_rt_codec_encoder_finish_v1` | existing A50: `ptr @SYM(ptr)` | `unsafe extern "C" fn(*mut CodecEncoder) -> *mut Buffer` |
| `CodecEncoderFreeV1` | `align_rt_codec_encoder_free_v1` | existing A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut CodecEncoder)` |

`CodecOpenV1` receives one compiler-formed valid byte view at any base alignment, performs the exact
owned/heap-allocation-free six-stage `ALNCOL01` validation in `core-design/codec.md`, rejects column
count 1025 before descriptor access, and returns zero or `AL_INVALID`; name uniqueness uses exactly
two `[u16; 1024]` stack arrays, ten stable merge passes, and at most 9,217 lexicographic comparisons.
It retains nothing and has no output slot. MIR forms the `{ input_ptr, input_len }` batch scalar from
the still-live input only after zero. Row and column counts lower to alignment-1 little-endian loads
from header bytes 16 and 24, with target-required swaps and u32-to-i64 zero extension for columns;
they are not hidden scalar fields. Remaining metadata and four typed projections reread descriptor
bytes with byte or alignment-1 little-endian operations and target-required swaps; element access
uses the same alignment-1 rule. `find` and all of these paths lower from the validated input without
another runtime row; no typed descriptor pointer or per-element opaque call is introduced.

`CodecEncoderNewV1` stores null to its nonnull aligned output before validating rows or allocating,
and publishes one allocator-provenanced shell only on zero. Each put requires a nonnull shell,
nonnegative signed lengths, null only for a zero-length name/value range, and the compiler-private
valid element/header range. It rejects a 1025th successful column before allocation or mutation and
completes name/row/kind/duplicate/final-size validation and all
fallible staging allocation before committing one column; `AL_INVALID` leaves the shell byte-for-
byte equivalent for future output. The str entry reads exact `{ ptr, i64 }` headers and copies no
cell before the complete prospective call is admitted. Successful columns are also kept in a
sorted name index, so duplicate admission binary-searches byte-exact names; fixed-index movement is
bounded by the 1024-column cap. OOM uses the hard-abort allocator path and
never returns a status.

Finish receives one live complete shell, allocates and fills the exact canonical final buffer,
consumes all staging, and returns the existing nonnull Buffer pointer; a null/private-invalid shell
hard-aborts because no source-valid call can form it. Free is null-safe and releases unfinished
staging exactly once. Every row is C calling convention and `nounwind`. Open and all encoder rows
carry no curated memory, parameter, or return attribute.

## Implemented `pkg.frame` extension (2026-09-01)

The implementation activates two new shapes after A120 in one atomic boundary. Both symbols, keys,
shapes, collision reservations, declarations, definitions, exports, fingerprint inputs, and totals
are active.

| Runtime key | Symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `FrameInnerJoinI64V1` | `align_rt_frame_inner_join_i64_v1` | A121: `i32 @SYM(ptr, i64, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const u8, i64, i64, *mut AlignStr) -> i32` |
| `FrameInnerJoinStrV1` | `align_rt_frame_inner_join_str_v1` | A122: `i32 @SYM(ptr, ptr, i64, ptr, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, *const u8, i64, *const u8, *const u8, i64, i64, *mut AlignStr) -> i32` |

A121 receives two unaligned little-endian i64 value ranges and their element counts. A122 receives
two validated string-column offset/data pairs and their row counts; each offset base addresses an
alignment-1 `(rows + 1) * 4` little-endian i32 range, and each data pointer addresses the bytes
through the validated final offset. The last pointer in both rows is a nonnull, correctly aligned,
writable `AlignStr` header used privately as `{ ptr, element_count }` for 16-byte `RowPair`
elements. It is not an Align source `str`. Empty output is `{ null, 0 }`; nonempty output comes from
the ordinary runtime allocator with the target ABI alignment of `{ i64, i64 }` and must be freed by
the existing dynamic-array owner.

Both rows are C calling convention and `nounwind`. They carry no other curated memory, return, or
parameter attribute. Runtime first
requires and zeroes the output header, then validates negative limit before either input, left view
before right view, right-table load-factor/capacity/byte arithmetic before allocation, and every
output bound before output allocation. For positive right length `R`, the exact logical index uses
`Q = R + ceil(R / 3)`, the smallest power-of-two `C >= max(8,Q)`, two `C`-entry i64 head/tail
tables, and one `R`-entry i64 next-link table. `Q`, `C`, and `16*C + 8*R` must fit i64 and the target
allocation-size domain; `R == 0` needs no index. A representability failure returns private `-2`
even when the semantic join result would be empty. Zero returns success; private `-1` means only
`JoinError.InvalidLimit`; private `-2` means only `JoinError.LimitExceeded`. A positive
`AL_INVALID` identifies a malformed compiler-
private ABI and hard-aborts in compiler-produced lowering. Every return frees transient scratch and
an error publishes no output.

The right input is always indexed in ascending ordinal order. The runtime counts stable matches
before one exact output allocation, probes again to fill left-major/right-ascending pairs, confirms
equality after every hash match, and retains no input pointer. Activation moved the
keyed/base/maximum-optional-probe totals from 328/345/353 to 330/347/355 and made A123 the next
unreserved shape at that time. The later `pkg.csv` implementation occupies A123, so A124 is now the
next unreserved design shape. Exact public semantics and owner matrix:
`pkg-design/frame.md`.

## `pkg.csv` extension (implemented 2026-09-03)

The implementation adds one keyed shape, `RuntimeKey`, declaration, definition, export, collision
identity, fingerprint input, and count atomically:

| Runtime key | Symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `CsvDecodeSoaV1` | `align_rt_csv_decode_soa_v1` | A123: `i32 @SYM(ptr, i64, ptr, i64, ptr, i32, i32, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const CsvField, i64, *mut Arena, i32, i32, i64, *mut AlignStr) -> i32` |

Arguments are input pointer/byte length, descriptor pointer/count, destination arena, checked
header tag, checked line-ending tag, inclusive row bound, and a nonnull writable output header.
The arena pointer has Rust `align_of::<Arena>()`; the descriptor and output pointers have their
target-native `repr(C)` alignments.
The row is C calling convention and `nounwind`, with no other curated function, return, or parameter
attribute. The runtime zeroes output before public validation. Status 0 is success, 1 is
only `pkg.csv.Error.Invalid`, 2 is only `LimitExceeded`, and `-1` is only malformed private ABI.
The runtime returns no other status; the canonical wrapper aborts on `-1` or any forged other i32.
OOM and an impossible second-pass mismatch also
abort. Recoverable error publishes no output and does not advance the arena.

Input length zero accepts either input pointer and performs no dereference or slice formation;
positive length requires a nonnull readable range of exactly that many bytes. The arena is nonnull,
aligned to `align_of::<Arena>()`, live, and exclusive. The output is nonnull, aligned to
`align_of::<AlignStr>()`, writable, and exclusive. The descriptor count is positive, converts
exactly to `usize`, and has no CSV-specific upper bound; its nonnull pointer is aligned to
`align_of::<CsvField>()` and denotes that many immutable records. Every
positive name length has a nonnull readable byte range. Counts, lengths, byte products, and address
additions fit the declared integer and target pointer-offset domains before any Rust reference,
slice, or typed load is formed.

Input bytes, descriptor records, and descriptor-name bytes remain immutable for the complete call.
The live arena control object and output header are disjoint from each other and those immutable
ranges. Input may occupy a prior live allocation of the same arena; the next arena allocation
cannot overlap it. The runtime returns `-1` for a mechanically detectable
negative/null/misaligned/overflowing representation before typed access. Once those guards pass,
exact-compatible unsafe source callers establish dereferenceability, lifetime, provenance,
immutability, and overlap; checked compiler-produced calls derive them before lowering. The guards
cannot prove an arbitrary nonnull address's backing range and do not make an otherwise invalid
unsafe call defined.

`CsvField` is target-native `#[repr(C)]` / non-packed LLVM
`{ name_ptr: ptr, name_len: i64, name_hash: u64, tag: i32, reserved: i32 }`. The global uses at least the exact
`repr(C)` alignment, size, and five field offsets. `name_hash` is the canonical
`align_hash::wyhash(name_bytes, WY_SEED)` with `WY_SEED = 0`; codegen and runtime call the same
shared implementation. `align_hash` owns the algorithm/seed convention; A123 owns descriptor
validation order and the authenticated stored hash. Runtime name validation recomputes and authenticates it before later header
projection reads the stored hash without rehashing the descriptor. Names are nonempty static source identifiers in
declaration order: first byte ASCII `_`/letter, remaining bytes ASCII `_`/letter/digit, and not one
of exact reserved tokens `fn`, `return`, `mut`, `pub`, `module`, `import`, `if`, `else`, `true`,
`false`, `arena`, `task_group`, `match`, `loop`, `break`, `template`, `unsafe`, `extern`, or `as`.
NUL, non-ASCII, invalid UTF-8, punctuation, and every other spelling return private `-1` during the
descriptor phase. `reserved` is zero. `tag` is `(signed << 16) | (kind << 8) | width`:
integer kind 0 with width 1/2/4/8 and sign bit 16; bool kind 1 width 1; float kind 2 width 4/8; str
kind 3 width 16; char kind 4 width 4. All other bits are zero. Before input access or arena effects,
the runtime validates output and arena, every count/range/pointer product, every descriptor field,
and each name's grammar while hashing its bytes once. It does not compare descriptor names with one
another. Record validation and checked descriptor emission prove declaration-order uniqueness for
compiler-produced calls; an exact-compatible unsafe caller must provide the same pairwise-unique
precondition. A violation is outside the unsafe contract, is not authenticated by the runtime, and
is not promised `-1`.

Validation order is output, arena, output zeroing; header tag, line-ending tag, row bound;
positive representable descriptor count, table size/alignment/non-null guards, then each declaration-
order record's positive name length, name range/source-identifier bytes and one hash, matching
`name_hash`, tag, and zero reserved field; input representation; complete UTF-8 validation;
CSV/header/data/layout; and finally a
nonempty allocation/fill. Thus negative `max_rows` returns 1 before malformed descriptor inspection
whenever output and arena are valid. Invalid UTF-8 returns `-1` before BOM/CSV parsing and arena
allocation. Malformed private input returns `-1`; descriptor and input are never inspected before
their preceding phase.

For `F` descriptor records containing `B` total name bytes, the pre-input phase visits exactly `F`
records and hashes exactly `B` bytes. Absent-header decode performs no later name lookup. Present-
header decode has `H <= 1024` physical names and performs one bounded fixed-table lookup per
descriptor; confirmed-collision candidate comparisons are at most `F * H`, with at most `B * H`
descriptor-name bytes compared. Test counters pin these bounds for wide and common-prefix schemas,
so the uncapped descriptor domain has no quadratic descriptor-to-descriptor path.

Activation is one atomic package/HIR/MIR/runtime capability: it adds the key, exact declaration
golden, definition/export, collision reservation, capability collection, fingerprints, and all
owners together. It changes current keyed/base/either-four-row-probe/maximum totals from
330/348/352/356 to 331/349/353/357 and makes A124 the next unreserved active shape. A source extern
cannot activate the row or select checked `CsvDecode`; exact compatible source-
extern reuse follows the ordinary registry rule. No partial producer may land. Exact semantics,
validation order, allocation contract, and closure matrix: `pkg-design/csv.md`.

## `pkg.ws` extension (implemented 2026-09-04)

The implemented `pkg.ws` capability activates eleven keyed identities, all on existing ABI shapes:

| Runtime key | Exact symbol | Existing ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `HttpRespondUpgrade` | `align_rt_http_respond_upgrade` | A24: `i32 @SYM(ptr, ptr, ptr)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *mut ResponseBuilder, *mut *mut HttpUpgrade) -> i32` |
| `HttpUpgradeReadExact` | `align_rt_http_upgrade_read_exact` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpUpgrade, *mut Buffer, i64) -> i32` |
| `HttpUpgradeWrite` | `align_rt_http_upgrade_write` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpUpgrade, *const u8, i64) -> i32` |
| `HttpUpgradeDeadline` | `align_rt_http_upgrade_deadline` | A04: `i32 @SYM(ptr, i64)` | `unsafe extern "C" fn(*mut HttpUpgrade, i64) -> i32` |
| `HttpUpgradeShutdown` | `align_rt_http_upgrade_shutdown` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut HttpUpgrade) -> i32` |
| `HttpUpgradeFree` | `align_rt_http_upgrade_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut HttpUpgrade)` |
| `HttpHeadersCount` | `align_rt_http_headers_count` | A37: `i64 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64) -> i64` |
| `HttpHeadersTokensValid` | `align_rt_http_headers_tokens_valid` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64) -> i32` |
| `HttpHeadersContainsToken` | `align_rt_http_headers_contains_token` | A120: `i32 @SYM(ptr, ptr, i64, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64, *const u8, i64) -> i32` |
| `HttpHeadersContainsTokenExact` | `align_rt_http_headers_contains_token_exact` | A120: `i32 @SYM(ptr, ptr, i64, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64, *const u8, i64) -> i32` |
| `HttpCtxUpgradeReady` | `align_rt_http_ctx_upgrade_ready` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut HttpRequestCtx) -> i32` |

These rows activate together in `RuntimeKey`, declarations, definitions, exports, collision
identity, fingerprints, and count assertions. At that capability boundary the inventory became 342
keyed records, 360 base records, 367 with the seven-row `alloc-count` probe, 364 with the four-row
`par-map-probe`, and 371 with both; A124 remained the next
unreserved active shape.

The same capability hardens the existing HTTP accepted-socket setup without adding a key or shape.
On macOS/iOS, after the existing best-effort `TCP_NODELAY` and `SO_KEEPALIVE`, `SO_NOSIGPIPE`
installation is checked before reading a request or publishing a request context. Failure captures
that errno, closes the accepted fd once without retry while ignoring the close result, and makes
`srv.accept` return the original mapped OS error; no context or writable connection escapes. Linux
retains `send(MSG_NOSIGNAL)`. A socket-option failpoint owns the failure ordering and close count.

`HttpRespondUpgrade` first validates and zeroes its writable aligned output. Invalid output returns
`AL_INVALID` without inspecting or consuming inputs. It then requires and takes a nonnull aligned
builder before ctx validation, so every later result consumes it. After the exact semantic checks,
checked addition computes `H = len("HTTP/1.1 101 Switching Protocols\r\n") +
sum(len(name) + len(": ") + len(value) + len("\r\n")) + len("\r\n")`; an unrepresentable total
returns `AL_INVALID` with ctx unspent. The runtime allocates and fills exactly one `H`-byte head
without growth or a second serialized copy, then allocates the fixed handle shell, all before fd
transfer or wire I/O. With entry builder heap `B` and `U = size_of::<HttpUpgrade>()`, the exact
producer-requested operation high-water excluding allocator-private metadata is `B + H + U`;
compile-time layout assertions and an allocation probe own it. OOM hard-aborts
before transfer. It publishes the handle only after the validated HTTP/1.1, residual-free 101 head
with complete RFC header syntax writes completely and the fd moves from the request context. The
readiness getter returns true only for HTTP/1.1 with no parser residual. Read validates
arguments/state before clearing a live buffer and publishes length only after exact success;
write borrows bytes and is SIGPIPE-safe write-all. Deadline retains one monotonic start-plus-budget
in the opaque handle; every later read/write recomputes the same remaining budget before each
syscall, rounds positive waits up, rechecks an early native timeout wakeup, and makes no call after
exhaustion. Shutdown invokes native `SHUT_RDWR` once, treats ENOTCONN as success, then performs one
no-retry cleanup close; other shutdown errors are returned after close. Free performs close only.
Each operation closes at most once. Caller-invalid precedes handle state; spent read/write/deadline
return `AL_INVALID` without mutation/clock/I/O, shutdown is idempotent, and poisoned operations replay
the stored status without I/O.

The feature-gated requested-live probe resets one explicit measurement window and tracks only the
allocation families attributed to it. `buffer` charges its fixed 64-byte shell budget plus reserved
payload, `array_builder` charges its 64-byte shell budget and C-owned growth (including old+new
overlap at realloc), and a frozen builder transfers that shell charge to its payload through Text
conversion. Ordinary string clone allocation then records the simultaneous staging/result peak.
The owner exercises Binary and Text at a bounded concrete size and separately pins the exact
64-bit maximum equation `128 + 32768 + 2 * 536870912 = 1073774720`; production builds compile every
probe hook and export out.

Header query pointers borrow the live request context for the call and retain nothing. Count and
token validation check context, then name; both membership operations check context, complete name
view/token, then complete searched-token view/token. Header names are case-insensitive;
`HttpHeadersContainsToken` compares members ASCII-case-insensitively and
`HttpHeadersContainsTokenExact` compares them byte-exactly. A null or misaligned context hard-aborts before Rust reference
formation; negative or address-space-unrepresentable length and null positive-length range reject
before slice formation; invalid token bytes hard-abort after safe view formation but before table
scanning. `HttpCtxUpgradeReady` applies the same context rule.
Dangling nonnull pointers remain outside the detectable ABI contract. No malformed query maps to
an ordinary zero/false result. All other pointer/length/count/capacity/address products are rejected
before Rust reference or slice formation as specified by their status-returning rows.

All eleven exports use the Rust C calling convention and must not unwind across it. Their generated
LLVM declarations preserve the reused A03/A04/A20/A24/A37/A62/A120 shapes' current empty curated
function-attribute sets: this capability adds no `nounwind`, memory, return, or parameter attribute
and does not mutate shared shape fingerprints. Exact public semantics, status mapping, validation
order, ownership, allocation, cache identity, and closure matrix: `pkg-design/ws.md`.

## `pkg.template` extension (implemented 2026-09-04)

The `pkg.template` capability activates five keyed identities on existing ABI shapes:

| Runtime key | Exact symbol | Existing ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `TemplateHtmlNew` | `align_rt_template_html_new_v1` | A47: `ptr @SYM()` | `extern "C" fn() -> *mut TemplateHtmlBuilder` |
| `TemplateHtmlWrite` | `align_rt_template_html_write_v1` | A73: `void @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder, *const u8, i64)` |
| `TemplateHtmlRaw` | `align_rt_template_html_raw_v1` | A73: `void @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder, *const u8, i64)` |
| `TemplateHtmlToString` | `align_rt_template_html_into_string_v1` | A83: `{ ptr, i64 } @SYM(ptr)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder) -> AlignStr` |
| `TemplateHtmlFree` | `align_rt_template_html_free_v1` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder)` |

All five rows activate together in `RuntimeKey`, declaration and export inventories, compatible-
extern collision identity, fingerprints, and count assertions. The inventory is therefore 347
keyed, 365 base, 372 with `alloc-count`, 369 with `par-map-probe`, and 376 with both optional probe
sets. No probe category or ABI shape is added, and A124 remains the next unreserved shape.

The exact 32-byte shell, validation order, escaping table reuse, zero-copy finish, ownership,
allocation, cache identity, and closure matrix are owned by `pkg-design/template.md`.

## Planned `std.xml` extension (designed 2026-09-05; inactive until implementation)

The accepted XML capability plans eight keyed identities, all on existing ABI shapes:

| Runtime key | Exact symbol | Existing ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `XmlParse` | `align_rt_xml_parse` | A08: `i32 @SYM(ptr, i64, ptr)` | `unsafe extern "C" fn(*mut u8, i64, *mut *mut XmlReader) -> i32` |
| `XmlNext` | `align_rt_xml_next` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader) -> i32` |
| `XmlName` | `align_rt_xml_name` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32` |
| `XmlAttributeCount` | `align_rt_xml_attribute_count` | A29: `i64 @SYM(ptr)` | `unsafe extern "C" fn(*const XmlReader) -> i64` |
| `XmlAttributeName` | `align_rt_xml_attribute_name` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32` |
| `XmlAttributeValue` | `align_rt_xml_attribute_value` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32` |
| `XmlText` | `align_rt_xml_text` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32` |
| `XmlFree` | `align_rt_xml_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader)` |

The Rust definitions use C calling convention and may not unwind across it. Generated declarations
retain the reused A03/A08/A19/A20/A29/A62 shapes' empty curated function, return, memory, and
parameter attribute sets. These are `unsafe` boundaries. Before dereference, the runtime may inspect
pointer integers and lengths to reject null where forbidden, misalignment, negative length,
noncanonical empty, address-range overflow, and supplied-range alias. A caller may rely on those
exact shape rejections. Every nonnull pointer that passes its shape checks and could then be accessed
must have provenance, lifetime, dereferenceability, accessibility, and the declared shared or
exclusive access for its exact range against access not represented by the call. Parse input is
canonical `{null,0}` with no allocation or one positive-length allocator-compatible owned range.
Getters/count share a shape-valid live shell allocation, while next and nonnull free hold it
exclusively. When an operation follows the shell's stored input pointer, its live allocation and exact
readable range are also caller preconditions unless the shell was published unchanged by this
runtime. Violating a post-shape-check pointer/access precondition is outside the ABI contract and is
not promised a safe abort.

Within those preconditions, the runtime checks representable raw ranges and every detectable
input/output/shell alias before mutation, Rust reference creation, or slice creation. A mechanically
rejected parse returns positive `AL_INVALID`, leaves output untouched, and accepts no input
ownership. In particular, nonnull zero-length input is rejected and never freed. After that
preflight it stores null and accepts no allocation for canonical empty or the positive-length owned
input: status zero publishes the sole shell, while `-1` releases accepted input responsibility (a
no-op for canonical empty) and means public `Error.Invalid`. An
output-bearing getter likewise leaves output untouched on mechanical preflight failure, then zeros
its `{ptr,i64}` output; any later wrong-state/index/private-state failure leaves canonical zero for
borrowed and owned results alike. Success fills a borrowed name view or publishes one completed
owned value/text allocation. `XmlNext` returns only `0=None`, `1=Start`, `2=End`, or `3=Text`.
Count is only `0..=256`; every impossible getter result aborts. Free is null-safe; a nonnull argument
must be a genuine exclusively held shell, and detectable malformed fields abort before following an
invalid stored input pointer. The runtime does not authenticate an arbitrary dangling address.

Design acceptance reserves no new shape: A124 remains the next unreserved shape and active
keyed/base/probe totals do not change. Implementation must activate all eight keys, symbols,
declarations, definitions, exports, collision identities, fingerprint rows, count assertions, and
their type/Drop consumers atomically. The grammar, status mapping, ownership, validation order,
allocation contract, and closure matrix are authoritative in `std-design/xml.md`.

## HTTP client raw receive-stream substrate (implemented)

The first HTTP receive-stream capability adds exactly six keyed records and no new ABI shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `BufferCapacity` | `align_rt_buffer_capacity` | A29: `i64 @SYM(ptr)` |
| `HttpClientRequestStream` | `align_rt_http_client_request_stream` | A24: `i32 @SYM(ptr, ptr, ptr)` |
| `HttpReadStreamFree` | `align_rt_http_read_stream_free` | A62: `void @SYM(ptr)` |
| `HttpReadStreamHeader` | `align_rt_http_read_stream_header` | A22: `i32 @SYM(ptr, ptr, i64, ptr)` |
| `HttpReadStreamRead` | `align_rt_http_read_stream_read` | A24: `i32 @SYM(ptr, ptr, ptr)` |
| `HttpReadStreamStatus` | `align_rt_http_read_stream_status` | A29: `i64 @SYM(ptr)` |

`HttpClientRequestStream` consumes the request and publishes a dependent stream handle only on
success. Header lookup publishes a borrowed view into the retained head block. Read writes an exact
byte count to its out slot and mutates only the caller buffer; a source-visible zero-capacity buffer
is rejected before this ABI is called. `HttpReadStreamFree` is null-safe and closes rather than
draining an incomplete response.

## HTTP client SSE receive extension (implemented)

The second HTTP receive-stream capability adds exactly four keyed records and no new ABI shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `HttpReadStreamSse` | `align_rt_http_read_stream_sse` | A50: `ptr @SYM(ptr)` |
| `HttpSseStreamLastEventId` | `align_rt_http_sse_stream_last_event_id` | A83: `{ ptr, i64 } @SYM(ptr)` |
| `HttpSseStreamNext` | `align_rt_http_sse_stream_next` | A24: `i32 @SYM(ptr, ptr, ptr)` |
| `HttpSseStreamRetryMs` | `align_rt_http_sse_stream_retry_ms` | A29: `i64 @SYM(ptr)` |

The transition preserves the one runtime pointer while MIR nulls the consumed raw source. `Next`
writes the fixed 64-byte native envelope only through its output pointer and returns the existing
signed HTTP status discriminator. The ID getter returns a stream-bound view, retry uses `-1` as the
`None` sentinel, and the raw/SSE types share the one null-safe free row.

## Asymmetric signature delta (implemented 2026-08-30)

The post-pkg.db implementation adds six keyed records atomically. They are included in the exact
current counts and the shipped A00–A109 table below.

| Runtime key | Exact symbol | ABI row and exact declaration |
|---|---|---|
| `CryptoPrivateKeyFromPem` | `align_rt_crypto_private_key_from_pem` | A106: `i32 @SYM(i32, ptr, i64, ptr)` |
| `CryptoPublicKeyFromPem` | `align_rt_crypto_public_key_from_pem` | A106: `i32 @SYM(i32, ptr, i64, ptr)` |
| `CryptoPublicKeyFromJwk` | `align_rt_crypto_public_key_from_jwk` | A107: `i32 @SYM(i32, ptr, i64, ptr, i64, ptr)` |
| `CryptoSign` | `align_rt_crypto_sign` | A108: `i32 @SYM(i32, ptr, ptr, i64, ptr)` |
| `CryptoVerify` | `align_rt_crypto_verify` | A109: `i32 @SYM(i32, ptr, ptr, i64, ptr, i64, ptr)` |
| `CryptoKeyFree` | `align_rt_crypto_key_free` | A62: `void @SYM(ptr)` |

`algorithm` is the closed `i32` ABI form of `0=RS256`, `1=ES256`, `2=Ed25519`; the runtime validates
it before narrowing. The JWK row passes Ed25519's absent second component as null/zero. Constructor
and sign output slots must be non-null/aligned and are null-initialized; verify's final `i32` slot must
be non-null/aligned and is zero-initialized. An invalid slot returns `AL_INVALID` without writing.
Every input length is nonnegative and `usize`-representable before slice formation; zero length may
carry null and uses an internal empty sentinel, while positive length requires non-null. Ed25519's
absent second JWK pair is exactly null/zero. Every handle repeats the closed key-kind byte and each
operation checks algorithm, public/private class, and kind before EVP. Its private shell fields own
one ordinary library context, its explicitly loaded built-in default provider, and the PKEY.
Private PEM is canonical PKCS#8 v1 `PrivateKeyInfo` version zero decoded only through
`d2i_PKCS8_PRIV_KEY_INFO` and `EVP_PKCS82PKEY_ex`; one exact `SensitiveDer` and every private
re-encoding scratch cleanse before free on all paths. Each fallible OpenSSL call clears and drains
its thread-local error queue. Provider checks and verify exhaust a disjoint native-return ×
`Empty`/`InputOnly`/`CodeBearing` queue table: documented zero plus Empty/InputOnly is
`AL_INVALID`/false, CodeBearing dominates a zero, and negative/unsupported/unexpected returns are
`AL_CODE`. Decoder/import empty/unknown/resource/internal/fetch entries are `AL_CODE`. Every
decode/import/signature/digest fetch uses exact `provider=default`; the key and operation provider
pointers must equal the owned provider before publication/action, and Ed25519 construction performs
wrapper-owned canonical/on-curve/non-small-order point validation. Final free order is PKEY,
`OPENSSL_thread_stop_ex`, provider unload, library-context free, then shell free. Status and cleanup
follow the exact crypto design ledger: zero success; `AL_INVALID` for direct or closed-queue
constructor/key rejection and malformed internal ABI; `AL_CODE` for opaque provider/allocation/
empty-or-unknown queue failure; post-view signature mismatch publishes
false; and free is null-safe and one-time. These rows are part of the main keyed inventory; they
increase the keyed/base/max counts by six and extend the shipped ABI range to A00–A109.

## Request 9 owned JSON extension

The implemented Request 9 design in `docs/impl/24-owned-json-plan.md` adds exactly
one keyed base record while reusing an existing LLVM function shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `BuilderWriteUint` | `align_rt_builder_write_uint` | A66: `void @SYM(ptr, i64)`; Rust receives `(*mut Builder, u64)`; no curated attributes |

The implementation changed the exact counts from 293 to 294 `RuntimeKey`
variants, 306 to 307 base records, and 314 to 315 maximum optional-probe exports.
The key, registry row, LLVM declaration/selection, unmangled Rust export,
key↔symbol bijection, and base/export parity must land atomically. The wrapper
checks the existing sticky limit and calls the existing internal
`builder_push_u64`; signed widths keep `BuilderWriteInt`, while every unsigned
width is zero-extended and selects `BuilderWriteUint`. The ABI still carries the
same 64 bits as LLVM `i64`; only Rust's decimal interpretation is unsigned.

Request 9 also extends the compiler/runtime interpretation of the existing
`JsonField.tag` kind byte passed through A103: kind `8`, width `16`, null
`sub` is an owned `string`; kind `9`, width `16`, null `sub` is an owned
`array<string>`. A direct `Option<string>` uses kind `8` with its validated
nonnegative record-relative `opt_tag`; required forms use `-1`. Kinds `0..=7`
retain their shipped meanings.

Those rows are emitted only from a target-local `OwnedJsonDescV1` whose
`OwnedJsonInterfaceEnvelopeV1` target/ABI identity has already validated before
descriptor offsets are read; whole-program/private/monomorph producers construct
the same envelope locally.
Owned decode calls A103 with the existing final arena argument null; the new
kind itself selects free-standing allocation. The `JsonField` C layout and A103
signature do not change. Owned encode does not pass these kinds through A80:
direct and optional strings use A73's existing JSON-string writer, and
`array<string>` uses A74 with the shipped borrowed-element tag
`(3 << 8) | 16` because encode only reads each `{ptr,len}`. A80 retains its
current descriptor domain. `BuilderWriteUint` and kinds `8`/`9` are shipped;
the exact counts and A66 row above are current.

## Request 14 exclusive filesystem publication (implemented)

Request 14 adds exactly two keyed records and no new ABI shape. The implementation
activated these rows atomically with the HIR/MIR/runtime support, taking
the registry to 296 keyed / 309 base / 317 maximum optional-probe records:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `IoWriterCreateExclusive` | `align_rt_io_writer_create_exclusive` | A08: `i32 @SYM(ptr, i64, ptr)` |
| `FsRenameNoReplace` | `align_rt_fs_rename_no_replace` | A09: `i32 @SYM(ptr, i64, ptr, i64)` |

The constructor row keeps the existing `writer` output-slot convention: the
runtime checks a null `out_writer` first, clears it before later validation, and
never publishes a writer on a recoverable failure. Both path operands are
borrowed views; the native runtime constructs ephemeral NUL-terminated copies,
constructing rename's source before its destination. The native implementation
must use Linux `renameat2(..., RENAME_NOREPLACE)` or macOS
`renameatx_np(..., RENAME_EXCL)` and must not fall back to ordinary replacing
rename, existence checks, `link` plus remove, or a subprocess. The full path,
allocation, platform, pair-cleanup, and owner matrix is in
`docs/impl/27-fs-exclusive-publication-plan.md` and
`docs/impl/std-design/fs.md`.

## Request 18 retained-root regular-file access (implemented)

Request 18 adds exactly two keyed records and no new ABI shape. The implementation activated the
rows atomically with HIR/MIR lowering, checked-HIR validation, native exports, and retained-directory
traversal, taking the registry to 298 keyed / 311 base / 319 maximum optional-probe records:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `IoReaderOpenBeneath` | `align_rt_io_reader_open_beneath` | A12: `i32 @SYM(ptr, i64, ptr, i64, ptr)` |
| `IoWriterCreateExclusiveBeneath` | `align_rt_io_writer_create_exclusive_beneath` | A12: `i32 @SYM(ptr, i64, ptr, i64, ptr)` |

Both constructors keep the existing reader/writer output-slot convention: validate the slot first,
clear it, validate/copy/parse the complete root before inspecting relative, then
validate/copy/parse the complete relative before a filesystem call and traverse from retained
directory descriptors. The runtime publishes a handle only after the final regular-file open or
exclusive create succeeds. The exact declaration golden, key/symbol bijection, exports,
whole/per-unit declarations, and rt-LTO inventory update in the same capability. The public
contract and closure matrix are in
`docs/impl/29-fs-retained-root-plan.md` and `docs/impl/std-design/fs.md`.

## Request 13 recursive owned JSON replacement (design accepted)

Request 13 added no runtime key, symbol, LLVM function shape, C signature, or
optional probe. At that capability boundary its implementation preserved the
then-current 294/307/315 counts while atomically replacing V1 descriptor
production with `OwnedJsonGraphDescV2` and retaining A103 decode and A80 encode:

| Existing ABI | V2 use |
|---|---|
| A103 `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64, ptr)` / `align_rt_json_decode` | root `JsonField` table, null arena, recursive kind-4/5/7 edges, and owned leaf kinds 8/9 |
| A80 `void @SYM(ptr, ptr, ptr, i64)` / `align_rt_json_encode_object` | one root builder/base/table call; kind 8 reads owned string as the existing `{ptr,len}` string layout and kind 9 reads `array<string>`; nested tables use the same validated graph |

Kinds 8/9 remain illegal for arbitrary borrowed/AoS/union tables. They enter A80
only from a validated V2 owned graph. Options retain `opt_tag`; nested records
and record arrays retain `JsonField.sub`; no C layout changes. The implementation
must prove A103/A80 table identity and registry counts unchanged, and delete the
flat V1 encode-part path rather than keep two runtime producers. Exact graph
bytes, allocation/drop tags, and owner matrix are in
`docs/impl/25-recursive-owned-json-plan.md`. Until that implementation lands, the
preceding Request 9 rows are the shipped ABI domain.

The key-to-symbol mapping is `key -> "align_rt_" + snake_case(key)` except:

```text
Print       -> align_rt_print_i64
CliCommand  -> align_rt_cli_command_new
HttpRequest -> align_rt_http_request_new
```

The four added keys map regularly:

```text
CryptoAesGcmOpen              -> align_rt_crypto_aes_gcm_open
CryptoAesGcmSeal              -> align_rt_crypto_aes_gcm_seal
CryptoChacha20Poly1305Open    -> align_rt_crypto_chacha20_poly1305_open
CryptoChacha20Poly1305Seal    -> align_rt_crypto_chacha20_poly1305_seal
```

Every symbol occurs exactly once below. `@SYM` is replaced with that row's
symbol. Braces after a function type are exact function attributes; return and
parameter attributes remain inline. An absent brace means no curated
attribute. The four rt-LTO guarded symbols `align_rt_str_eq`,
`align_rt_str_starts_with`, `align_rt_str_ends_with`, and
`align_rt_str_eq_ignore_case` use the shown declaration attributes when
rt-LTO is off. When rt-LTO is on, their curated declaration attributes are
withheld before their visible bodies are linked; LLVM then derives attributes
from those bodies. `align_rt_str_cmp` is not guarded and always keeps A01.

| ABI | Exact LLVM declaration | Symbols |
|---|---|---|
| A00 | `i32 @SYM(ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_utf8_valid` |
| A01 | `i32 @SYM(ptr readonly captures(none), i64, ptr readonly captures(none), i64) {nofree nosync willreturn memory(argmem: read)}` | `align_rt_str_eq`, `align_rt_str_starts_with`, `align_rt_str_ends_with`, `align_rt_str_cmp`, `align_rt_str_eq_ignore_case` |
| A02 | `i32 @SYM(ptr readonly captures(none), i64, ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_str_contains` |
| A03 | `i32 @SYM(ptr)` | `align_rt_io_writer_flush`, `align_rt_http_stream_finish` |
| A04 | `i32 @SYM(ptr, i64)` | `align_rt_json_doc_kind`, `align_rt_fs_exists`, `align_rt_fs_remove`, `align_rt_child_kill`, `align_rt_tcp_conn_set_io_timeout` |
| A05 | `i32 @SYM(ptr, i64, i32, ptr)` | `align_rt_json_decode_array`, `align_rt_json_decode_scalar` |
| A06 | `i32 @SYM(ptr, i64, i64, i64, ptr)` | `align_rt_tcp_connect` |
| A07 | `i32 @SYM(ptr, i64, i64, ptr)` | `align_rt_json_doc_key`, `align_rt_tcp_listen`, `align_rt_udp_bind`, `align_rt_compress_gzip_compress`, `align_rt_compress_zstd_compress`, `align_rt_http_serve`, `align_rt_http_serve_shared` |
| A08 | `i32 @SYM(ptr, i64, ptr)` | `align_rt_json_doc_as_str`, `align_rt_json_doc_as_i64`, `align_rt_json_doc_as_f64`, `align_rt_json_doc_as_bool`, `align_rt_fs_read_file`, `align_rt_fs_write_file_builder`, `align_rt_fs_read_dir`, `align_rt_dns_resolve`, `align_rt_io_reader_open`, `align_rt_bytes_as_str`, `align_rt_io_writer_create`, `align_rt_io_file_create`, `align_rt_io_file_open`, `align_rt_base64_decode`, `align_rt_base64url_decode`, `align_rt_hex_decode`, `align_rt_percent_decode`, `align_rt_form_decode`, `align_rt_compress_gzip_decompress`, `align_rt_compress_zstd_decompress`, `align_rt_http_parse`, `align_rt_regex_compile`, `align_rt_regex_captures_group`, `align_rt_env_get` |
| A09 | `i32 @SYM(ptr, i64, ptr, i64)` | `align_rt_fs_write_file`, `align_rt_process_exec`, `align_rt_crypto_ct_equal`, `align_rt_env_set` |
| A10 | `i32 @SYM(ptr, i64, ptr, i64, i64, i64, i64, i64, ptr)` | `align_rt_crypto_argon2id` |
| A12 | `i32 @SYM(ptr, i64, ptr, i64, ptr)` | `align_rt_process_spawn`, `align_rt_io_reader_open_beneath`, `align_rt_io_writer_create_exclusive_beneath` |
| A13 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_crypto_hkdf_sha256` |
| A15 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_aes_gcm_seal`, `align_rt_crypto_aes_gcm_open`, `align_rt_crypto_chacha20_poly1305_seal`, `align_rt_crypto_chacha20_poly1305_open` |
| A16 | `i32 @SYM(ptr, i64, ptr, i64, ptr, ptr, ptr, i64, i64)` | `align_rt_json_decode_soa` |
| A17 | `i32 @SYM(ptr, i64, ptr, ptr)` | `align_rt_json_doc_parse`, `align_rt_fs_read_file_view`, `align_rt_fs_read_bytes_view` |
| A103 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_json_decode` (final nullable arena) |
| A104 | `i32 @SYM(ptr, i64, ptr, i64, i64, ptr, ptr, i64, i64, ptr)` | `align_rt_json_decode_struct_array` (final nullable arena) |
| A105 | `i32 @SYM(ptr, i64, ptr, ptr, ptr)` | `align_rt_json_decode_union` (final nullable arena) |
| A106 | `i32 @SYM(i32, ptr, i64, ptr)` | `align_rt_crypto_private_key_from_pem`, `align_rt_crypto_public_key_from_pem` |
| A107 | `i32 @SYM(i32, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_public_key_from_jwk` |
| A108 | `i32 @SYM(i32, ptr, ptr, i64, ptr)` | `align_rt_crypto_sign` |
| A109 | `i32 @SYM(i32, ptr, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_verify` |
| A18 | `i32 @SYM(ptr, i64, ptr, ptr, i64, ptr, i64, ptr, i64, i64)` | `align_rt_json_scan_next` |
| A19 | `i32 @SYM(ptr, ptr)` | `align_rt_builder_finish_bounded_stack`, `align_rt_tcp_accept`, `align_rt_command_run`, `align_rt_io_writer_write_builder`, `align_rt_http_accept`, `align_rt_http_respond`, `align_rt_http_stream_reject` |
| A20 | `i32 @SYM(ptr, ptr, i64)` | `align_rt_io_writer_write`, `align_rt_cli_get_bool`, `align_rt_regex_is_match`, `align_rt_http_stream_send`, `align_rt_http_stream_send_event` |
| A21 | `i32 @SYM(ptr, ptr, i64, i64, ptr)` | `align_rt_http_get_many`, `align_rt_regex_find` |
| A22 | `i32 @SYM(ptr, ptr, i64, ptr)` | `align_rt_cli_parse`, `align_rt_http_resp_header`, `align_rt_http_client_get`, `align_rt_regex_find_all`, `align_rt_regex_split`, `align_rt_regex_captures`, `align_rt_http_ctx_header`, `align_rt_http_read_stream_header` |
| A23 | `i32 @SYM(ptr, ptr, i64, ptr, i64, ptr)` | `align_rt_http_client_post` |
| A24 | `i32 @SYM(ptr, ptr, ptr)` | `align_rt_http_client_request`, `align_rt_http_client_request_stream`, `align_rt_http_read_stream_read`, `align_rt_http_respond_stream` |
| A25 | `i64 @SYM()` | `align_rt_time_now`, `align_rt_time_instant`, `align_rt_process_cpu_count` |
| A26 | `i64 @SYM(ptr readonly captures(none), i64) {nofree nosync willreturn memory(argmem: read)}` | `align_rt_hash64` |
| A27 | `i64 @SYM(ptr readonly captures(none), i64, ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_str_find`, `align_rt_str_rfind` |
| A28 | `i64 @SYM(ptr readonly captures(none), ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_str_finder_find` |
| A29 | `i64 @SYM(ptr)` | `align_rt_child_wait`, `align_rt_run_output_code`, `align_rt_io_file_len`, `align_rt_buffer_len`, `align_rt_buffer_capacity`, `align_rt_rng_next`, `align_rt_http_resp_status`, `align_rt_http_read_stream_status`, `align_rt_regex_group_count` |
| A30 | `i64 @SYM(ptr, i64)` | `align_rt_json_doc_len` |
| A31 | `i64 @SYM(ptr, i64, i64)` | `align_rt_rng_range` |
| A32 | `i64 @SYM(ptr, i64, i64, i64, i64, ptr, ptr, i64)` | `align_rt_group_sum_str`, `align_rt_group_min_str`, `align_rt_group_max_str`, `align_rt_group_count_str` |
| A33 | `i64 @SYM(ptr, i64, i64, i64, ptr, i64, ptr, i64)` | `align_rt_group_multi_str` |
| A34 | `i64 @SYM(ptr, i64, i64, i64, ptr, ptr, i64)` | `align_rt_dict_encode_str` |
| A35 | `i64 @SYM(ptr, i64, ptr, ptr, i64)` | `align_rt_group_count_i64` |
| A36 | `i64 @SYM(ptr, ptr)` | `align_rt_udp_recv_from`, `align_rt_io_reader_read`, `align_rt_io_reader_read_line`, `align_rt_io_copy` |
| A37 | `i64 @SYM(ptr, ptr, i64)` | `align_rt_io_file_pread`, `align_rt_cli_get_i64`, `align_rt_regex_group_index` |
| A38 | `i64 @SYM(ptr, ptr, i64, i64)` | `align_rt_io_file_pwrite` |
| A39 | `i64 @SYM(ptr, ptr, i64, i64, i64, i64, ptr)` | `align_rt_par_map_reduce` |
| A40 | `i64 @SYM(ptr, ptr, i64, ptr, i64, i64)` | `align_rt_udp_send_to` |
| A41 | `i64 @SYM(ptr, ptr, i64, ptr, ptr, i64)` | `align_rt_group_sum_i64`, `align_rt_group_min_i64`, `align_rt_group_max_i64`, `align_rt_group_sum_str_cols`, `align_rt_group_min_str_cols`, `align_rt_group_max_str_cols`, `align_rt_group_count_str_cols` |
| A42 | `noalias ptr @SYM() {nofree nounwind}` | `align_rt_arena_begin`, `align_rt_tg_begin` |
| A43 | `noalias ptr @SYM(i64) {nofree nounwind}` | `align_rt_alloc`, `align_rt_array_builder_new` |
| A44 | `noalias ptr @SYM(ptr, i64) {nofree nounwind}` | `align_rt_str_finder_new`, `align_rt_builder_new` |
| A45 | `noalias ptr @SYM(ptr, i64, i64) {nounwind}` | `align_rt_arena_alloc`, `align_rt_array_builder_new_in`, `align_rt_tg_alloc` |
| A46 | `noalias ptr @SYM(ptr, ptr, i64, i64, i64, i64, ptr)` | `align_rt_par_map` |
| A47 | `ptr @SYM()` | `align_rt_io_reader_stdin`, `align_rt_http_client_new` |
| A48 | `ptr @SYM(i32, i32)` | `align_rt_io_writer_std` |
| A49 | `ptr @SYM(i64)` | `align_rt_buffer_new`, `align_rt_http_response_new` |
| A50 | `ptr @SYM(ptr)` | `align_rt_tg_wait`, `align_rt_tcp_conn_reader`, `align_rt_tcp_conn_writer`, `align_rt_io_reader_buffered` |
| A51 | `ptr @SYM(ptr, i64)` | `align_rt_array_builder_init_stack`, `align_rt_builder_init_bounded_stack`, `align_rt_cli_command_new` |
| A52 | `ptr @SYM(ptr, i64, ptr, i64)` | `align_rt_command_new`, `align_rt_http_request_new` |
| A53 | `ptr @SYM(ptr, ptr, i64)` | `align_rt_builder_init_stack` |
| A54 | `void @SYM() {noreturn}` | `align_rt_div_fail`, `align_rt_alloc_size_fail`, `align_rt_process_abort` |
| A55 | `void @SYM(double)` | `align_rt_print_f64` |
| A56 | `void @SYM(float)` | `align_rt_print_f32` |
| A57 | `void @SYM(i32)` | `align_rt_print_bool`, `align_rt_print_char` |
| A58 | `void @SYM(i64)` | `align_rt_print_i64`, `align_rt_time_sleep` |
| A59 | `void @SYM(i64) {noreturn}` | `align_rt_process_exit` |
| A60 | `void @SYM(i64, i64) {noreturn}` | `align_rt_bounds_fail`, `align_rt_len_mismatch_fail`, `align_rt_utf8_boundary_fail` |
| A61 | `void @SYM(i64, i64, i64) {noreturn}` | `align_rt_range_fail` |
| A62 | `void @SYM(ptr)` | `align_rt_arena_end`, `align_rt_tg_end`, `align_rt_free`, `align_rt_str_finder_free`, `align_rt_builder_pop_comma`, `align_rt_tcp_conn_free`, `align_rt_tcp_listener_free`, `align_rt_udp_socket_free`, `align_rt_child_free`, `align_rt_command_env_clear`, `align_rt_command_free`, `align_rt_run_output_free`, `align_rt_io_reader_free`, `align_rt_io_writer_free`, `align_rt_io_file_free`, `align_rt_buffer_free`, `align_rt_array_builder_free`, `align_rt_array_builder_free_stack`, `align_rt_array_builder_free_strings`, `align_rt_array_builder_free_strings_stack`, `align_rt_crypto_random`, `align_rt_crypto_key_free`, `align_rt_rng_seed_os`, `align_rt_cli_command_free`, `align_rt_cli_parsed_free`, `align_rt_http_request_free`, `align_rt_http_read_stream_free`, `align_rt_http_resp_free`, `align_rt_http_client_free`, `align_rt_http_server_free`, `align_rt_regex_captures_free`, `align_rt_regex_free`, `align_rt_http_ctx_free`, `align_rt_http_response_free`, `align_rt_http_stream_free`, `align_rt_builder_free`, `align_rt_builder_free_stack` |
| A63 | `void @SYM(ptr, double)` | `align_rt_builder_write_f64` |
| A64 | `void @SYM(ptr, float)` | `align_rt_builder_write_f32` |
| A65 | `void @SYM(ptr, i32)` | `align_rt_builder_write_bool`, `align_rt_builder_write_char` |
| A66 | `void @SYM(ptr, i64)` | `align_rt_print_str`, `align_rt_builder_write_int`, `align_rt_tcp_read_timeout`, `align_rt_tcp_write_timeout`, `align_rt_command_timeout`, `align_rt_free_string_array`, `align_rt_array_builder_push`, `align_rt_rng_seed_with`, `align_rt_http_timeout`, `align_rt_http_client_timeout`, `align_rt_free_response_array` |
| A67 | `void @SYM(ptr, i64, i64, i32)` | `align_rt_buffer_put` |
| A68 | `void @SYM(ptr, i64, i64, i64, ptr)` | `align_rt_gather_i64` |
| A69 | `void @SYM(ptr, i64, i64, ptr)` | `align_rt_json_doc_at` |
| A70 | `void @SYM(ptr, i64, ptr, i64, ptr)` | `align_rt_json_doc_get`, `align_rt_dict_lookup` |
| A71 | `void @SYM(ptr, i64, ptr, ptr)` | `align_rt_json_doc_elems` |
| A72 | `void @SYM(ptr, ptr)` | `align_rt_array_builder_push_bytes`, `align_rt_buffer_bytes` |
| A73 | `void @SYM(ptr, ptr, i64)` | `align_rt_builder_write`, `align_rt_builder_write_json_str`, `align_rt_command_cwd`, `align_rt_buffer_append`, `align_rt_array_builder_push_str`, `align_rt_array_builder_append`, `align_rt_cli_flag_bool`, `align_rt_http_body`, `align_rt_http_rb_body` |
| A74 | `void @SYM(ptr, ptr, i64, i32)` | `align_rt_json_encode_scalar_array` |
| A75 | `void @SYM(ptr, ptr, i64, i64)` | `align_rt_rng_shuffle`, `align_rt_cli_flag_i64` |
| A76 | `void @SYM(ptr, ptr, i64, i64, ptr, i64)` | `align_rt_builder_write_str_int_str` |
| A77 | `void @SYM(ptr, ptr, i64, ptr, i64)` | `align_rt_command_env`, `align_rt_cli_flag_str`, `align_rt_http_header`, `align_rt_http_rb_header` |
| A78 | `void @SYM(ptr, ptr, i64, ptr, i64, i64)` | `align_rt_json_encode_struct_array` |
| A79 | `void @SYM(ptr, ptr, ptr)` | `align_rt_json_encode_union` |
| A80 | `void @SYM(ptr, ptr, ptr, i64)` | `align_rt_json_encode_object` |
| A81 | `void @SYM(ptr, ptr, ptr, ptr, ptr, ptr)` | `align_rt_tg_register` |
| A82 | `{ i64, i64 } @SYM(ptr readonly captures(none), i64) {nofree nosync willreturn memory(argmem: read)}` | `align_rt_hash128` |
| A83 | `{ ptr, i64 } @SYM(ptr)` | `align_rt_run_output_stdout`, `align_rt_run_output_stderr`, `align_rt_array_builder_build`, `align_rt_array_builder_build_stack`, `align_rt_builder_finish`, `align_rt_builder_finish_stack`, `align_rt_cli_usage`, `align_rt_http_resp_body`, `align_rt_http_ctx_method`, `align_rt_http_ctx_path`, `align_rt_http_ctx_body`, `align_rt_builder_into_string`, `align_rt_builder_into_string_stack` |
| A84 | `{ ptr, i64 } @SYM(ptr, i64)` | `align_rt_str_clone`, `align_rt_base64_encode`, `align_rt_base64url_encode`, `align_rt_hex_encode`, `align_rt_percent_encode`, `align_rt_form_encode`, `align_rt_html_escape`, `align_rt_crypto_sha256`, `align_rt_crypto_sha512`, `align_rt_str_trim`, `align_rt_str_trim_start`, `align_rt_str_trim_end`, `align_rt_path_base`, `align_rt_path_dir`, `align_rt_path_ext`, `align_rt_path_normalize` |
| A85 | `{ ptr, i64 } @SYM(ptr, i64, i64, i64)` | `align_rt_chunks` |
| A86 | `{ ptr, i64 } @SYM(ptr, i64, ptr, i64)` | `align_rt_crypto_hmac_sha256`, `align_rt_path_join` |
| A87 | `{ ptr, i64 } @SYM(ptr, ptr, i64)` | `align_rt_cli_get_str` |
| A88 | `{ ptr, i64 } @SYM(ptr, ptr, i64, i64, i64)` | `align_rt_rng_sample` |
| A89 | `{ ptr, i64 } @SYM(ptr, ptr, i64, i64, i64, i64, ptr, ptr)` | `align_rt_par_map_filter` |
| A90 | `{ ptr, i64 } @SYM(ptr, ptr, i64, ptr, i64, i32)` | `align_rt_regex_replace` |

Request 11 keyed delta:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `CommandMaxCapture` | `align_rt_command_max_capture` | A66: `void @SYM(ptr, i64)`; no curated attributes |
| `CommandRunBytes` | `align_rt_command_run_bytes` | A19: `i32 @SYM(ptr, ptr)`; no curated attributes |
| `RunBytesCode` | `align_rt_run_bytes_code` | A29: `i64 @SYM(ptr)`; no curated attributes |
| `RunBytesStdout` | `align_rt_run_bytes_stdout` | A83: `{ ptr, i64 } @SYM(ptr)`; no curated attributes |
| `RunBytesStderr` | `align_rt_run_bytes_stderr` | A83: `{ ptr, i64 } @SYM(ptr)`; no curated attributes |
| `RunBytesFree` | `align_rt_run_bytes_free` | A62: `void @SYM(ptr)`; no curated attributes |

All six use the regular `align_rt_` plus snake-case key mapping and occupy collision-reserved native
identities as soon as the capability activates. At that capability boundary,
`runtime_abi_registry_is_complete_and_unique` owned the then-current 294/307 counts,
key/symbol bijection, and reverse lookup; the exact extern-type matrix owns every
parameter/return/attribute cell; the checked-in declaration golden owns spelling and row order; and
the base/feature runtime-export parity owners require all six definitions in every normal runtime
while rejecting any missing, duplicate, near-spelled, or wrong-signature record. The capability must
not add a direct declaration outside this registry.

Request 5 bounded-HTTP shipped delta:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `HttpMaxResponseBodyBytes` | `align_rt_http_max_response_body_bytes` | A66: `void @SYM(ptr, i64)`; no curated attributes |
| `HttpClientMaxResponseBodyBytes` | `align_rt_http_client_max_response_body_bytes` | A66: `void @SYM(ptr, i64)`; no curated attributes |

Both use ordinary keyed-native identity and are mandatory base exports. The implementation updated
registry counts, bijection, declaration golden, and base runtime-export parity in the same change.
The HTTP-private negative result sentinel is not an ABI
symbol: client-response MIR maps it to reserved `Error.Code(-1)` before the common positive status
decoder, so it cannot collide with a saturating encoded errno.

Unkeyed native records:

| Owner | Exact LLVM declaration | Runtime export presence |
|---|---|---|
| main error wrapper | `i32 @align_rt_report_error(i32)` | every Unit/Result main wrapper; no attributes |
| argv wrapper | `{ ptr, i64 } @align_rt_args_build(i32, ptr)` | only argv main; no attributes |
| arena implementation | `void @align_rt_arena_reset(ptr)` | always linked; runtime-internal, no curated declaration attributes |
| allocator implementation | `ptr @align_rt_realloc(ptr, i64)` | always linked; runtime-internal, no curated declaration attributes |
| HTTP implementation | `i32 @align_rt_http_serialize(ptr, ptr)` | always linked; runtime-internal, no curated declaration attributes |
| PostgreSQL codec | `i32 @align_rt_f32_to_bits(float) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `float @align_rt_f32_from_bits(i32) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f64_to_bits(double) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `double @align_rt_f64_from_bits(i64) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f32_text_len(float) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f64_text_len(double) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f32_text_write(float, ptr, i64) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f64_text_write(double, ptr, i64) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| pkg.kv TCP configuration | `i32 @align_rt_tcp_conn_set_io_timeout(ptr, i64)` | always linked; package-internal compatible extern; no curated declaration attributes |
| allocation probe | `i64 @align_rt_alloc_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| allocation probe | `i64 @align_rt_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| requested-live probe | `void @align_rt_requested_live_reset()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| requested-live probe | `i64 @align_rt_requested_live_bytes()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| requested-live probe | `i64 @align_rt_requested_live_peak()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_new_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| parallel probe | `void @align_rt_test_par_map_force_caller(i32)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_workers()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |

## Machine gates

Am-c1 replaces the current ABI/declaration, dedicated-consumer, AEAD-selection,
and attribute authorities with one typed `RuntimeAbi` row per identity:
`{ key, symbol, return type, ordered parameter types, return attrs, parameter
attrs, function attrs, rt_lto_policy }`. Declaration and call lookup consume
that row. `key` is `RuntimeAbiId`, either `Keyed(RuntimeKey)` or
`Unkeyed(UnkeyedRuntimeKey)`. The eighteen base unkeyed records use the same
typed-row machinery; only `ReportError` and `ArgsBuild` have a dedicated Align main-wrapper
declaration policy and yield typed wrapper handles when that wrapper requires
them. The other sixteen yield no unconditional compiler handle. An exact
compatible source extern may declare or reuse every non-test-control unkeyed row except
`ArgsBuild` through the ordinary extern path. The four compiler-private
`core.test` rows reject source reuse by policy. The remaining thirteen unkeyed rows
are source-reachable. `ArgsBuild`
returns the native `{ptr, i64}` argv view, which no source-valid extern return
can express: `str`/slice view returns are rejected, `raw` is not a valid
`layout(C)` field, and the closest source-valid `layout(C) { u64, i64 }` return
lowers as `{i64, i64}` rather than `{ptr, i64}`. It is therefore wrapper-only,
the only non-test-control base unkeyed row whose ABI has no compatible
source-extern form. The legacy mixed string map remains as a handle-only alias seam
for unchanged `Rvalue::Call(String)` resolution through c1: it is populated in
post-c1 class order from stored definitions, non-shadowed imports, externs,
alphabetical `RuntimeKey::ALL` aliases, then existing generated aliases. The
keyed aliases are mutually unique, so their relative normalization preserves
every final binding. The alias seam cannot define a symbol, type, attribute,
membership, or reuse rule. C3 deletes it.

Registry and extern-compatibility preflight create no LLVM value. C1 fixes LLVM
construction order as stored definitions in vector order, non-shadowed imports
in vector order, externs in vector order, keyed native rows in alphabetical
`RuntimeKey::ALL` order, then generated helpers in their existing order. This
intentionally changes only the relative keyed-native declaration order from the
hand-written pre-c1 source; keyed physical symbols are mutually unique and all
program/import/extern claimants still precede them. Textual/raw LLVM order
changes once; object bytes may change or remain equal and have no equality
promise. Compiler build id changes cache identity for one miss then an
unchanged-input hit. Every symbol spelling and final legacy alias binding
remains fixed. A source extern is compatible exactly when its
source-derived LLVM function type equals the fixed row; source externs carry no
curated native attributes. The selected native row supplies every curated
attribute and rt-LTO policy to the one reused handle. A stored/imported program
function with the same physical spelling is not native reuse and retains the
current LLVM uniquification result until c3 encodes program symbols.
Main-wrapper emission reuses a type-compatible `ReportError` extern or adds its
row, and always adds the wrapper-only `ArgsBuild` row when argv marshalling is
required. Both remain in current wrapper order. Attributes are applied through
typed row handles, never by a symbol prefix scan that could select a program claimant. Thus a same-spelled program
claimant keeps its current physical name/uniquification but loses accidental
native attributes, while the actual possibly-suffixed native declaration gains
the row attributes.

The eleven probe rows have
`verification_presence = AllocCount | ParMapProbe`: their exact signatures are
checked against the corresponding runtime fixture, but their names do not
participate in compiler collision validation and no compatible-extern reuse
path is synthesized for them. The compiler uses the fixed base table before
LLVM construction and receives no runtime-feature input.

Tests compare:

- all 347 keys, mapped symbols, LLVM declaration types, and default attributes
  against this table through the checked-in
  `crates/align_codegen_llvm/tests/golden/runtime_abi_declarations.txt`;
- the 365 base native symbols against default-feature `align_runtime` exports,
  plus every actual Rust definition's normalized native return and ordered
  parameter types against the declaration golden, failing on either direction's
  difference through `scripts/test-runtime-abi-exports.sh`;
- the 372 `alloc-count` and 369 `par-map-probe` native symbols against
  `align_runtime` built with each feature separately, including the eleven exact
  probe signatures above;
- the 376 maximum native symbols against `align_runtime` built with
  `alloc-count,par-map-probe,task-group-probe`, while proving
  `task-group-probe` adds no unmangled export;
- rt-LTO off/on attributes for every guarded symbol, with missing,
  declaration-only, wrong-type, internal, private, available-externally, and
  non-C-calling-convention artifact negatives;
- all 365 identities through the one `RuntimeAbiId`-keyed row iterator and all
  365 exact registry function types through the production compatibility
  predicate, one return mutation per row, and one mutation of every parameter
  ordinal; source-valid compatible reuse for a keyed builtin and the thirteen
  source-reachable unkeyed rows; exact `ArgsBuild` `str` rejection plus the
  source-valid `layout(C) { u64, i64 }` aggregate mismatch; and
  compatible reuse representatives for each of the five checked-in attribute
  classes (`#0`–`#4`), with the native row supplying its curated attributes;
- one mutation of each registry attribute class, symbol, and key through the
  checked-in golden and uniqueness owners;
- ordinary extern and program-definition positives for all eleven probe
  spellings while the normal runtime export set excludes them; and
- trivial whole-program and per-unit-shaped emitted IR with identical
  alphabetical runtime declarations, whose exact rows are owned by the
  checked-in declaration golden above.

For rt-LTO, codegen first requires all four guarded logical symbols in this
deterministic order: present, exact registry function type, body present,
external linkage, and C calling convention. Any failed check loudly falls back
without merging and reapplies the curated attributes to every guarded
declaration. The complete baked definitions then begin with their logical
symbols. Before linking, codegen renames each one to
the captured physical LLVM name of its typed declaration. This is normally the
same spelling; when a preceding program/import claimant forced LLVM
uniquification, it is the suffixed native name. The merge therefore fills only
the typed native handle and never the same-spelled program claimant. After
linking, every captured typed handle must still be an external C-convention
definition with a body before attributes are removed and linkage becomes
internal; a missing body or changed linkage/convention is a compiler error,
never a silent static-runtime fallback after partial mutation.

## D14 generated SQLite scalar-callback ABI

D14's callback entrypoints are program-generated helpers, not runtime-native declarations. They do
not add a `RuntimeKey`, `UnkeyedRuntimeKey`, runtime export, compatible-extern reuse row, or registry
count. The generated-family identity includes callback kind/version, target identity, exact Align
signature, effect, return provenance/cleanup, and the canonical C signature. Each semantic callback
has one private/internal Align target reference, one program-lifetime 32-byte v1 descriptor, and one
C-callable generated definition whose physical name is reserved by the ordinary generated-symbol
collision preflight.

The exact external callback ABIs are:

| Kind | Exact LLVM function type | Required attributes and body boundary |
|---|---|---|
| ScalarFunction v1 | `void @GEN(ptr, i32, ptr)` | C calling convention, `nounwind`; hard-abort null context/database handle, validate argc/argv and all 0..127 SQLite values in type/byte-count/final-pointer order with immediate errcode after null Text, normalize empty views to a stable non-null sentinel, build fixed stack scratch, call the exact Align target once, consume its dynamic cleanup result, call one result/error family, return void |

The function is not `readnone`, `readonly`, `willreturn`, or `nofree`: SQLite value/result routines
may allocate or mutate native state, scalar application code may be Impure, and the Align target
may hard-terminate. `nounwind` states only that no language or Rust unwind crosses C. Descriptor
validation and generated-body preflight occur before its address is installed in
SQLite. Whole-program, per-unit, and ThinLTO emission must agree on the descriptor bytes, target
symbol, physical generated name, C type, and attributes; a malformed checked program emits neither
descriptor nor callback definition.
