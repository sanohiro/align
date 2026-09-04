# pkg — ws

> English is authoritative. A synchronized Japanese mirror lives at `ja/ws.md`.
>
> **Status:** designed; implementation pending. No surface in this document is shipped until the implementation
> capability activates it together with its owner tests and runtime rows.

## Authoritative public-contract ledger

This ledger is the authority for the first `pkg.ws` capability. Later prose and implementation may
make a field more explicit but must not widen it. V1 is an RFC 6455 HTTP/1.1 server integrated into
the existing `pkg.web` route table. It is not a standalone listener, client, raw frame API,
extension framework, compression layer, or background task system. `pkg.web` owns routing,
middleware, request views, the accept loop, and `SO_REUSEPORT`; `std.http` owns the protocol-neutral
Upgrade transport; `pkg.ws` alone owns the WebSocket handshake, SHA-1 accept calculation, frame
grammar, masking, message assembly, automatic control replies, and close-code policy.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, order, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| `http_headers.count(name: str) -> i64` | Bound `http_headers` receiver, then `name`, evaluate once. `name` uses the existing header-name rule: nonempty ASCII token, no NUL; an invalid name aborts before scanning. The native row hard-aborts on a null or misaligned context and rejects negative/address-space-unrepresentable length or a null positive-length name before reference/slice formation; it then hard-aborts invalid token bytes before table scanning. A dangling nonnull pointer is outside the detectable ABI contract. | Pure, allocation-free, RFC 9110 ASCII-case-insensitive count of physical field rows. Repeated rows count separately; comma members do not. The result is `0..=HTTP_MAX_HEADERS` and preserves no cursor; no malformed-input sentinel exists. | Receiver and result are Copy. The scan borrows the request table for the call and returns no view. | `std.http` type/method/HIR/MIR own the operation; runtime key `HttpHeadersCount`, existing A37 `i64 @SYM(ptr, ptr, i64)`. The method and key enter interface/object/link identity; request bytes do not. | Shipped `http_headers` view and fixed header cap; zero/one/repeated/case/name-invalid, whole/per-unit, malformed-HIR, native hard-abort, and ABI owners. |
| `http_headers.tokens_valid(name: str) -> bool` | Same receiver/name evaluation and native hard-abort validation as `count`. Every physical row with that name is interpreted as one RFC 9110 comma-separated token list after optional whitespace trimming. | Pure and allocation-free. Returns `true` when the field is absent or every selected row contains one or more nonempty ASCII `token` members with no empty/trailing member; otherwise `false`. Quoted strings are not tokens. It scans every selected row and cannot be used to accept a valid first row while ignoring a malformed duplicate; malformed ABI input never maps to `false`. | Copy result; no returned view, mutation, retained cursor, or allocation. | `std.http` owns the shared token grammar; runtime key `HttpHeadersTokensValid`, existing A20 `i32 @SYM(ptr, ptr, i64)`. | Absent, one/many rows, OWS, comma boundaries, empty/quoted/non-ASCII/control members, split positions, native hard-abort, and differential RFC-token oracle. |
| `http_headers.contains_token(name: str, token: str) -> bool` | Receiver, `name`, then `token`, once left-to-right. Both strings must be nonempty ASCII tokens with no NUL; an invalid argument aborts before scanning. The native row validates context, then the complete name view/token, then the complete token view/token, hard-aborting at the first malformed item: null/alignment/length/range defects before safe view formation, then invalid token bytes before table scanning. Malformed input never maps to `false`. | Pure and allocation-free. Searches every comma member of every case-insensitively named field row and compares the member to `token` ASCII-case-insensitively. It does not certify the remaining members; a protocol first calls `tokens_valid` when their validity matters. | Copy result; borrows only for the call. | `std.http`; runtime key `HttpHeadersContainsToken`, existing A120 `i32 @SYM(ptr, ptr, i64, ptr, i64)`. | Repeated-row/member/case/OWS/collision/malformed-neighbor matrix plus native validation-order/hard-abort, whole/per-unit, and ABI goldens. |
| `http_request_ctx.upgrade_ready() -> bool`; trailing `pkg.web.types.Ctx.upgrade_ready: bool` | Bound live request context. No arguments or defaults. The copied field is populated exactly once before middleware and handler dispatch. The native row hard-aborts on a null or misaligned context before forming a reference; a dangling nonnull pointer is outside the detectable ABI contract. | Pure and allocation-free. True iff the parsed request is HTTP/1.1 and the parser retained no residual bytes after the complete request; false for HTTP/1.0 or any bytes co-read after the request. It performs no I/O and does not spend the context; malformed ABI input never maps to `false`. | Copy result/field. No view, mutation, allocation, or retained borrow. | `std.http` method plus runtime key `HttpCtxUpgradeReady`, existing A03 `i32 @SYM(ptr)`; `pkg.web.types.Ctx` gains the exact trailing field. Both enter interface/object/link identity. | HTTP/1.0/1.1 x empty/nonempty parser residual, middleware/prepare visibility, native hard-abort, no-I/O, whole/per-unit, malformed-HIR, and ABI owners. |
| `http_upgrade` | A global `std.http` Move handle constructed only by successful `ctx.respond_upgrade`. It is non-Copy, non-comparable, non-printable, and not a collection element. A raw handle may be a same-frame local or a by-value/`borrow`/`borrow mut` parameter. The only tagged carrier is one unnested `Result<http_upgrade, E>` same-frame local, where the complete storage graph of `E` contains no `http_upgrade`; it may be formed directly by the constructor or by ordinary `map_err`, and consumed by `?`, `else`, or `match`, but may not be a parameter, field, capture, or return. All Option, reversed/nested Result, user struct/sum/tuple, array/slice/box, global/constant, `out`, extern, capture, task, parallel, and function-return placements reject. | A live handle owns one upgraded byte stream. Read/write/deadline/shutdown are Impure and serialized by `borrow mut`. A failed read/write closes and poisons it; every later operation returns the same stored builtin `Error` without I/O. A spent handle returns `Error.Invalid` from read/write/deadline without buffer mutation, clock access, state change, or I/O; shutdown alone is idempotent Ok. Drop is close-only and never writes protocol bytes. On macOS/iOS, the accepted socket can enter a request context only after checked `SO_NOSIGPIPE` succeeds; failure closes it once and makes `srv.accept` return the mapped OS error before request read or context publication. | Owns the accepted fd and a small runtime state allocation, including optional monotonic start-plus-budget deadline state. Moving nulls the source. `shutdown` closes the fd and leaves a spent handle for ordinary Drop; free is null-safe. The request context remains separately owned and spent so its views stay valid during the pump. | New append-only canonical type-record-v3 leaves `Ty::HttpUpgrade=71` and `Scalar::HttpUpgrade=47`, after the shipped `CodecEncoder` 70/46 leaves; compiler owns carrier/effect/Drop rules, runtime owns fd, deadline, and sticky error. Exact root bytes are `[3, 0, 0, 0, 0, 71]`, and the field-level scalar encoding is `[47]`; a Result root uses existing tag 5 followed by this scalar and its error scalar. Interface type spelling, checked HIR/MIR, object, and runtime rows enter ordinary identities. | Shipped Move/borrow machinery and HTTP server; exact bidirectional root/scalar/Result semantic-byte plus unknown 72/48, truncated, and trailing-byte rejection goldens, positive carrier grammar, move/drop/replacement/control joins, forbidden placement/capture/return, live/spent/poisoned operation sweep, checked socket-option failure, and whole/per-unit owners. |
| `ctx.respond_upgrade(rb: response_builder) -> Result<http_upgrade, Error>` | Bound live `http_request_ctx`, then `rb`, once. Consumes `rb` only. Before any write it validates in exact order: HTTP/1.1; parser residual absent; status exactly 101; body absent; every stored header in insertion order, first requiring a nonempty ASCII RFC token name, then a value containing only HTAB, SP, visible ASCII, or obs-text bytes; exactly one `Upgrade` row containing one or more valid tokens; exactly one valid `Connection` token-list row containing `Upgrade` ASCII-case-insensitively; no `Content-Length`; then no `Transfer-Encoding`. It does not interpret the Upgrade protocol token. After semantic validation, checked addition computes `H = len("HTTP/1.1 101 Switching Protocols\r\n") + sum(len(name) + len(": ") + len(value) + len("\r\n")) + len("\r\n")`; an unrepresentable total is `Error.Invalid` while `ctx` is unspent. | Impure. The first validation or size failure leaves `ctx` unspent. It then allocates and fills one exact `H`-byte head—fixed status line, every stored header byte-exact in insertion order, final empty line—and allocates the handle shell before lifting the fd, marks `ctx` spent, and writes the complete head. Success publishes the Move handle. A write failure closes the fd, leaves `ctx` spent, publishes no handle, and returns the mapped builtin error. It never parks the connection. Allocation failure follows the locked hard-abort OOM policy and occurs before fd transfer or a wire byte. | The source-valid call frees the builder on every recoverable result. Let `B` be the producer-requested live heap of the moved-in builder shell, header vector, and header strings on entry, and `U = size_of::<HttpUpgrade>()`; the exact operation high-water excluding allocator-private metadata is `B + H + U`. Serialization makes exactly one `H`-byte allocation with no growth, relocation, or second serialized copy; it and the exact `U`-byte handle allocation coexist with all `B` bytes. Compile-time shell/layout assertions and an allocation high-water probe own the formula. The head is freed after the write attempt; the builder is freed before publication. Success transfers the fd to the preallocated handle; failure after transfer closes it. `ctx` retains only the parsed request buffer and remains alive for the pump's request views. | `std.http` HIR `HttpRespondUpgrade`, MIR operation, LLVM call, and runtime key `HttpRespondUpgrade` using existing A24 `i32 @SYM(ptr, ptr, ptr)`. No new ABI shape; A124 remains next unused. | Exact 1.0/1.1 and residual state, status/body/full header syntax/order/framing products, checked `H`, byte-exact serialization, `B + H + U` allocation/high-water/OOM, validation-before-write, success/failure ownership, spent-context, request-view lifetime, raw socket response, and malformed-HIR/runtime-pointer owners. |
| `u.read_exact(out: mut buffer, count: i64) -> Result<(), Error>` | Bound `http_upgrade`, mutable bare-local `buffer`, then `count`, once. `count` must be `0..=out.capacity()`; invalid count or zero-capacity with positive count aborts before handle-state inspection or I/O. Handle state is then selected before clearing `out.len`: spent returns `Error.Invalid` without changing the buffer; poisoned replays its sticky error without changing the buffer. A live call then clears `out.len` before transport work. | Impure. On a live handle, reads exactly `count` bytes, retrying interrupted syscalls. Success publishes exactly that many bytes; zero succeeds without I/O. EOF before completion is `Error.NotFound`; timeout and OS failures keep their builtin mapping. Any failure publishes zero bytes, closes/poisons the handle, and becomes its sticky error. It never reads beyond `count`. | Borrows both owners mutably for the call; no allocation or growth. Partial bytes remain unpublished and are discarded on failure. Spent/poisoned rejection mutates neither owner. | Compiler owns bound places/result; runtime key `HttpUpgradeReadExact`, existing A20. | Invalid args x live/spent/poisoned, zero/exact/next capacity, every partial split, EOF/timeout/EINTR/error, no-overread coalesced frames, buffer-generation invalidation, sticky error, and whole/per-unit owners. |
| `u.write(data: slice<u8>) -> Result<(), Error>` | Bound receiver then data once. Any positive length requires a valid borrowed range; NUL and arbitrary bytes are data. Arguments validate before state. Spent returns `Error.Invalid`; poisoned replays its sticky error, both without I/O. Empty on a live handle is valid and performs no syscall. | Impure, SIGPIPE-safe, and write-all. Success writes the complete slice in order. A partial-then-error closes/poisons and returns the mapped error; later operations replay it without I/O. | Data is borrowed synchronously and never retained or copied. No allocation or state change on spent/poisoned rejection. | `std.http`; runtime key `HttpUpgradeWrite`, existing A20. | Invalid range x live/spent/poisoned, empty/nonempty, short writes/EINTR/EPIPE/timeout, exact bytes, no-copy counter, sticky error, and platform parity. |
| `u.deadline(timeout_ns: i64) -> Result<(), Error>` | Bound receiver then timeout once. The value must be `1..=86400000000000`; invalid aborts before handle-state inspection or clock/state work. Spent then returns `Error.Invalid`; poisoned replays its sticky error, both without clock access or mutation. | Impure. On a live handle, captures one monotonic start plus budget and replaces any prior deadline. Every later read/write recomputes the same budget's remaining duration before each syscall, rounds positive native waits up so they do not expire early, and returns `Error.Timeout` without another syscall after exhaustion. A native timeout wakeup rechecks the clock and retries when budget remains. It never resets the budget per call, frame, or partial transfer. A fresh live handle cannot fail this operation. | Mutably borrows the handle; retains only fixed-size clock/budget state and allocates nothing. `borrow mut` excludes overlap with read/write/shutdown. | `std.http`; runtime key `HttpUpgradeDeadline`, existing A04 `i32 @SYM(ptr, i64)`. | invalid bound x live/spent/poisoned, replacement, cumulative partial read/write and multi-frame exhaustion, early native wakeup, ceil/recheck boundaries, no-call-after-expiry, and overlap owners. |
| `u.shutdown() -> Result<(), Error>` | Bound live or already-spent receiver once. | Impure and terminal. A live handle invokes native `shutdown(SHUT_RDWR)` once, treats `ENOTCONN` as already shut down, closes without retry while ignoring the cleanup-close status, and becomes spent. Any other shutdown failure is mapped and returned after that close. A previously successful shutdown is idempotent `Ok` without a syscall. A poisoned handle returns its sticky error without a syscall after ensuring its fd was already closed. No WebSocket frame is written. | Retains the small spent handle until Drop; shutdowns and closes the fd at most once. Drop of a still-live handle performs only the same no-retry cleanup close, with no native shutdown and no surfaced error. | `std.http`; runtime key `HttpUpgradeShutdown`, existing A03. `HttpUpgradeFree` uses A62. | live/spent/poisoned, peer-already-closed, shutdown errno, repeated shutdown, Drop order, shutdown/close-once and no-frame counters. |
| `pkg.web.types.UpgradeAccepted { response: response_builder, selected: string }` | Exact field/source order. `selected` is protocol-defined opaque metadata; `""` is the allocation-free absent value. | Move. It is legal only as the payload of `UpgradeDecision.Accept`. | Owns one response builder and one string. Moving transfers both; ordinary Drop frees both if unpublished. Dispatch transfers `selected` to pump only after upgrade success and drops it before any failure fallback otherwise. | `pkg.web.types` nominal interface definition and complete reachable graph. | formation/order/move/drop/interface/whole-per-unit owners. |
| `pkg.web.types.UpgradeDecision { Accept(UpgradeAccepted), Reject(response_builder), Failed(Error) }` | Closed discriminator/source order exactly `0..=2`. Produced by one prepare callback. | Move. Accept attempts `respond_upgrade`. Success invokes pump; on Err, dispatch logs the ordinary handler method/path/error diagnostic once and tries the fixed 500 through the same ctx, which writes only when validation left ctx unspent and otherwise fails silently after a committed-write failure. Reject uses ordinary `ctx.respond` with no handler-error log. Failed logs once and writes the fixed 500. | Exactly one active payload owner; branch move/drop and early-exit cleanup are exhaustive. The Accept error drops selected before constructing the fallback builder. | `pkg.web.types` ordinary sum identity. | every tag/payload/control/cleanup/fallback/log path, malformed checked HIR, and interface mutation owners. |
| `pkg.web.types.UpgradeHandler { validate: fn(slice<str>) -> bool, prepare: fn(Ctx, slice<str>) -> UpgradeDecision, pump: fn(Ctx, http_upgrade, string) -> Result<(), Error> }` | Exact field/source order. All callbacks are noncapturing/copyable function values; `validate` must be inferred Pure and receives the exact opaque values. | Copy. During `serve`, `validate` runs once per Upgrade row after common method/pattern/prefix and handler-storage checks but before segment and pair checks. False aborts before bind with exact diagnosis `pkg.web: route N (METHOD PATTERN) has invalid upgrade values`. `prepare` runs once after middleware and before an Upgrade write. `pump` runs once only after a successful 101, owning the transport and selected string. Pump `Ok` emits no log; pump `Err` uses the same exact method/path/error diagnostic as Stream and cannot send another HTTP response. | Three 16-byte function values; no record allocation/Drop. Validation may perform only its source-visible Pure work. The pump owns and must consume or Drop the handle before return. | `pkg.web.types`; function signatures, effect facts, and complete nominal graph enter interface identity. | direct/imported function values, validator true/false and Pure/Impure mismatch, exact startup precedence/diagnostic, pump Ok/Err logging, whole/per-unit dispatch, and no-capturing-environment owners. |
| `pkg.web.upgrade(method: str, pattern: str, values: slice<str>, validate: fn(slice<str>) -> bool, prepare: fn(Ctx, slice<str>) -> UpgradeDecision, pump: fn(Ctx, http_upgrade, string) -> Result<(), Error>) -> Route` | Arguments once left-to-right. No defaults. It performs no protocol validation itself; ordinary route validation owns method/pattern and invokes the supplied Pure validator for the opaque `values` retained in the Copy route. | Pure constructor. Produces one ordinary route with `Handler.Upgrade`, empty `stream_type`, and exact values/callbacks. It participates in the same radix priority, 405 `Allow`, prefix grouping, and middleware chain. HEAD fallback applies only to `Respond`, never Stream/Upgrade. | Returned Route is Copy and borrows method/pattern/values for the `serve` lifetime. No allocation. | `pkg.web` source and `pkg.web.types.Route`, whose exact new trailing field is `upgrade_values: slice<str>`. Router cache/source identity includes it. | mixed Respond/Stream/Upgrade routing, group/group_with copying, 404/405/HEAD/middleware, validator startup order, and zero hot-path allocation owners. |
| `pkg.ws.Message { Text(string), Binary(array<u8>), Close(Close) }` | Closed discriminator/source order exactly `0..=2`. Receive never exposes continuation, Ping, Pong, or raw control frames. | Move. Text is one complete valid UTF-8 message; Binary one complete byte message; Close the validated peer close. | Each nonempty payload owns ordinary heap storage; canonical empty string/array allocations follow existing representations. No payload borrows the connection or scratch. | `pkg.ws` nominal sum and reachable definitions enter interface/cache identity. | variant/order/interface, empty/nonempty allocation, move/drop/control, and whole/per-unit owners. |
| `pkg.ws.Close { code: Option<i64>, reason: string }` | Exact field/source order. `None` means an empty close payload. Received `Some(code)` admits `1000,1001,1002,1003,1007,1008,1009,1010,1011,1012,1013,1014`, registered `3000,3003,3008`, or private `4000..=4999`; all other values are invalid. Reason is the remaining valid UTF-8, possibly empty. | Move because reason is owned. A one-byte payload, invalid code, or invalid UTF-8 is never returned. | Owns only the reason string. | `pkg.ws` nominal interface definition. | empty/code-only/code+reason, every allowed/forbidden code boundary, invalid length/UTF-8, field/order and cleanup owners. |
| `pkg.ws.route(pattern: str, protocols: slice<str>, pump: fn(pkg.web.types.Ctx, http_upgrade, string) -> Result<(), Error>) -> pkg.web.types.Route` | Arguments once left-to-right. No defaults. Construction retains the opaque protocol list without inspecting it. The package-supplied Pure startup validator requires every server protocol to be a nonempty RFC token and rejects duplicate byte-exact names. Empty list means select none and ignore valid client offers. A nonempty list requires a client offer match; the first server-list entry offered by the client wins. Pattern validation remains `pkg.web`'s startup owner. | Pure and allocation-free. Returns a GET Upgrade route using the package validator and handshake prepare function. Invalid protocol configuration aborts only when `pkg.web.serve` performs route validation, before bind or tree construction. Middleware runs first and may answer before handshake. The selected protocol reaches the pump as an owned string; `""` means none. | Route retains pattern/protocol views for `serve`; no constructor allocation. The handshake clones only a nonempty selected protocol. | Ordinary `pkg.ws` source over `pkg.web.upgrade`; SHA-1 helper stays private and absent from the public interface. Root/internal source hashes and `pkg.web` dependency identity drive caches. | valid/invalid/duplicate protocol tables, constructor purity, exact bind-before validation/diagnosis, empty/required/first-server selection, mixed web routing/middleware, whole/per-unit, and vendorable subtree owners. |
| `pkg.ws.receive(borrow mut connection: http_upgrade, max_message_bytes: i64) -> Result<Message, Error>` | Receiver then bound once. `max_message_bytes` must be `0..=536870912`; invalid aborts before allocation/I/O. Zero admits only zero-byte Text/Binary messages. Every call also has one fixed `1048576`-byte source-work allowance. Each masked frame charges its exact 6/8/14 header bytes and every control frame charges its payload bytes; data payload bytes use only the caller message bound. Before each exact read of a charged unit, checked subtraction must succeed; exact exhaustion is allowed, while the rejected next unit is not read. | Impure. Reads exactly one application-visible event: a complete Text/Binary message or peer Close. It assembles continuation fragments, permits interleaved control frames, automatically replies to every Ping with an identical Pong and continues, and consumes/ignores every Pong. Client frames must be masked; server input with RSV set, reserved opcode, nonminimal length, invalid control shape, invalid continuation sequence, message-cap or source-work excess, invalid text UTF-8, or invalid Close is failed with the exact close policy below. Protocol/text/either-limit failure best-effort writes 1002/1007/1009 respectively, then shuts down and returns `Error.Invalid`; a close-write failure wins with its transport error. A valid peer Close other than code 1010 is echoed byte-exact; client-only 1010 is acknowledged with an empty Close frame. The original validated code/reason is returned after shutdown. A reply failure returns the transport error instead. Abrupt EOF is `Error.NotFound`; other transport errors pass through. | After valid bound checking, each call owns one fixed-capacity 32 KiB `buffer` allocation and one initially empty heap-mode `array_builder<u8>` accumulator. Its initialized length never exceeds `max_message_bytes`; first/nonempty growth uses the shipped `max(4, needed).next_power_of_two()` capacity rule, so one retained payload capacity never exceeds 512 MiB. Reallocation counts the old and new payloads simultaneously; Text conversion counts the complete staging array and exact returned-string allocation simultaneously. On 64-bit targets, the exact producer-requested live-heap ceiling attributable to one call, excluding allocator-private metadata, is `1073774720` bytes: a fixed 128-byte combined shell budget, 32768 buffer bytes, and at most two 536870912-byte payloads. Compile-time shell-size assertions and a live-byte resource probe enforce that bound. Masking removal appends each decoded payload byte once. Control-only paths and zero-length messages allocate no accumulator payload. Binary success transfers the builder allocation directly into the returned array. Text success builds the byte array, validates its complete view, clones it once into the returned string, then frees the staging array. Close allocates only a nonempty returned reason string. Every result is independent of scratch/connection; every error frees unpublished storage and leaves the handle spent/poisoned. | `pkg.ws` ordinary source owns RFC state, source-work accounting, and the fixed resource probe; `std.http` owns exact reads/writes. No WebSocket parser/runtime key, sidecar, registry, or HIR/MIR operation exists. | opcode/FIN/RSV/mask/7-16-64-bit length/fragment/control-interleave Cartesian product; exact/rejected-next message and source-work caps including zero-length continuation and Ping/Pong floods; UTF-8 splits; 1010 empty acknowledgment plus all other close codes; automatic reply/failure precedence; exact steady/reallocation/Text-clone live bytes, allocation/growth/copy/cleanup; and official-vector differential owners. |
| `pkg.ws.send_text(borrow mut connection: http_upgrade, text: str) -> Result<(), Error>` | Receiver then text once. UTF-8 is guaranteed by `str`; empty and NUL are valid. Checked length must fit the RFC 63-bit length. | Impure. Writes one unmasked FIN Text frame using the minimal 7/16/64-bit length encoding, then the borrowed payload. A header failure prevents payload write; any later failure poisons/closes. | One fixed 10-byte stack/header value; payload is never copied or retained. | `pkg.ws` source over `http_upgrade.write`; no native row. | 0/125/126/65535/65536/i64 bounds, exact wire bytes, no-copy, partial failure, and browser/client interop owners. |
| `pkg.ws.send_binary(borrow mut connection: http_upgrade, data: slice<u8>) -> Result<(), Error>` | Receiver then data once. Empty/NUL/arbitrary bytes valid; checked length as above. | Same as Text with opcode 2. | Same no-payload-copy contract. | `pkg.ws` source. | Same boundary/wire/error/copy matrix plus binary payload identity. |
| `pkg.ws.close(connection: http_upgrade, code: i64, reason: str, timeout_ns: i64) -> Result<(), Error>` | Arguments evaluate once left-to-right; the bound handle transfers only after code/reason/timeout validation. Code must be one of server-sendable `1000,1001,1002,1003,1007,1008,1009,1011,1012,1013,1014`, registered `3000,3003,3008`, or private `4000..=4999`. Reason is UTF-8 and `0..=123` bytes. Timeout must be `1..=86400000000000`. Invalid input aborts before transfer/I/O. | Impure. Installs one cumulative monotonic deadline, writes one unmasked FIN Close containing big-endian u16 code plus reason, then reads masked frames until the peer Close arrives or that same budget/transport/protocol fails. Data and Pong after the local Close are discarded; Ping still receives its identical Pong without resetting the budget. A valid peer Close completes the handshake regardless of its code/reason, then the server closes TCP and returns Ok. Timeout or any failure closes and returns that builtin error. Codes 1004/1005/1006/1010/1015, unassigned 1016..2999 and 3001..3999, and values outside u16 are rejected. | Fixed frame scratch plus the bounded 125-byte control payload; reason borrowed/no-copy. The consumed handle is cleaned exactly once on every path. No peer data allocation or result publication. | `pkg.ws` source over `http_upgrade.deadline/read_exact/write/shutdown`, pinned to the referenced IANA snapshot. Widening for a newly assigned code is an explicit contract change. | every allowed/forbidden range edge, 123/124 reason bytes, deadline bounds/quantization/exhaustion without reset, peer Close/Ping/Pong/data/protocol/EOF, simultaneous Close, wire endian, every failure, source nulling, and close-once owners. |

## Decision and package boundary

The first capability is one composable server path:

```text
pkg.web route + middleware + prefork accept
  -> protocol-neutral HTTP/1.1 Upgrade
  -> pkg.ws RFC 6455 handshake
  -> one owned pump-local byte transport
  -> complete typed messages
```

There is no second listener, router, worker pool, request context, or concurrency model. A WebSocket
route and an ordinary REST or SSE route share one `slice<pkg.web.types.Route>` and one call to
`pkg.web.serve`. An open connection occupies its worker exactly like an existing stream route.
Applications size `workers` explicitly for their expected simultaneous long-lived connections plus
ordinary HTTP traffic; V1 adds no async task, connection registry, or hidden thread.

The protocol-neutral seam belongs below `pkg.ws`: HTTP Upgrade transfers the accepted byte stream,
while the selected protocol decides what those bytes mean. The seam cannot import `pkg.ws`, and
`pkg.ws` imports `pkg.web` in the ordinary dependency direction. This keeps `pkg.web` independently
vendorable and prevents a package cycle. The Upgrade handle has no socket-address, raw-fd, TLS,
reader/writer conversion, or HTTP parser escape hatch.

## Public use

Declarations and positional calls are shown separately:

```align
import pkg.web
import pkg.web.types
import pkg.ws

fn chat(
  c: pkg.web.types.Ctx,
  connection: http_upgrade,
  protocol: string,
) -> Result<(), Error> {
  mut ws := connection
  loop {
    message := pkg.ws.receive(ws, 1048576)?
    match message {
      Text(text) => pkg.ws.send_text(ws, text)?
      Binary(data) => pkg.ws.send_binary(ws, data[..])?
      Close(_) => { break }
    }
  }
  return Ok(())
}
```

```align
import pkg.web
import pkg.ws

fn main() -> Result<(), Error> {
  protocols := ["chat.v1"]
  routes := [
    pkg.web.get("/health", health),
    pkg.ws.route("/chat", protocols[..], chat),
  ]
  return pkg.web.serve("127.0.0.1", 8080, routes[..], 4)
}
```

The pump parameter owns the Upgrade handle. The example rebinds it as mutable because every
receive/send operation takes an exclusive call-bounded borrow. A valid peer Close is already
answered under the 1010 exception below and the transport already shut down when `Message.Close` is
observed. Falling out or returning an
error before a Close simply drops the handle and closes the TCP connection without fabricating a
WebSocket close frame.

## Handshake contract

`pkg.ws.route` installs one package-owned prepare callback. The ordinary router admits only exact
GET to that row; another method follows the existing 404/405 selection and never invokes prepare.
After exact-GET route selection and a middleware Proceed, prepare validates the request in this
exact order, stopping at the first failure and performing no SHA-1, base64 encoding, response write,
or transport publication first:

1. defensively recheck the selected method is exact uppercase `GET`, then require
   `ctx.upgrade_ready` (HTTP/1.1 with no parser residual), then require the parsed request body is
   empty;
2. exactly one nonempty `Host` field exists;
3. every `Upgrade` row is a valid token list and one member equals `websocket`
   ASCII-case-insensitively;
4. every `Connection` row is a valid token list and one member equals `Upgrade`
   ASCII-case-insensitively;
5. exactly one `Sec-WebSocket-Version` exists and its value is exact `13`;
6. exactly one `Sec-WebSocket-Key` exists and is canonical standard base64 for exactly 16 bytes:
   24 ASCII bytes, 22 standard alphabet bytes followed by `==`, with the decoded tail bits zero;
7. every `Sec-WebSocket-Protocol` row, when present, is a valid nonempty token list; then select the
   first server-list entry offered byte-exactly by the client, or reject when the server list is
   nonempty and none matches.

Failure inside prepare produces a normal empty-body 400 response. A WebSocket-version failure
additionally emits `Sec-WebSocket-Version: 13`; an HTTP-version or residual failure does not. No
invalid request invokes the pump. `Origin` is application policy and
is deliberately handled by ordinary middleware before this validation. Extensions are not
negotiated: `Sec-WebSocket-Extensions` is ignored and the 101 contains none, so all received RSV bits
remain invalid.

Success computes exactly:

```text
base64(SHA-1(key_text || "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))
```

and returns a header-only 101 builder with exactly `Upgrade: websocket`, `Connection: Upgrade`,
`Sec-WebSocket-Accept: <value>`, and, when selected, one `Sec-WebSocket-Protocol` row. SHA-1 is a
private package helper implemented for this fixed accept proof only. It is not added to
`std.crypto`, cannot hash application input through a public API, and makes no collision-resistance
promise. The RFC's canonical key `dGhlIHNhbXBsZSBub25jZQ==` must produce
`s3pPLMBiTxaQ9kYGzzhZRbK+xOo=` independently in source and wire tests.

`ctx.respond_upgrade` repeats the HTTP-version, residual, complete response-header syntax, and
101/framing safety checks at the ownership boundary. Package validation decides the normal 400
response; the lower check prevents malformed checked HIR or another Upgrade protocol from
publishing an invalid transport.

## Frame and message state machine

V1 accepts only the RFC base framing with no extensions:

| Frame state | Rule |
|---|---|
| Header | FIN, RSV1..3, opcode, mask, and 7-bit length are read exactly. Every client frame is masked; every server frame is unmasked. RSV is zero. |
| Length | 0..125 is inline; marker 126 carries a big-endian u16 value at least 126; marker 127 carries a big-endian u64 with its high bit zero and value at least 65536. Nonminimal or non-i64-representable values are protocol errors before payload allocation/read. |
| Data start | Opcode 1 starts Text; 2 starts Binary. A new data opcode while fragmented is active is a protocol error. |
| Continuation | Opcode 0 is legal only while fragmented state is active and preserves the original Text/Binary kind. FIN completes it. A continuation without an active message is a protocol error. |
| Control | Close/Ping/Pong are FIN-only and length at most 125. They may occur between fragments. Reserved 3..7 and 11..15 are protocol errors. |
| Ping | Payload is unmasked and immediately returned in an unmasked Pong before parsing continues. A Pong-write error closes and wins over any later frame result. Ping is not exposed as a Message. |
| Pong | Valid payload is consumed and ignored before parsing continues. V1 exposes no application heartbeat API, timer, or background receive. |
| Close | Empty payload maps to `Close { code: None, reason: "" }`. Length one, an inadmissible wire code, or invalid UTF-8 reason fails. A valid payload other than client-only code 1010 is echoed exactly; 1010 receives an empty Close acknowledgment. The original code/reason is returned and the transport is closed before `Message.Close` is published. |
| Text | UTF-8 validation covers the complete reassembled message, not individual fragments. Invalid final text sends 1007 and returns `Error.Invalid`. |
| Limit | The cumulative decoded data payload is checked before every payload read/copy. Exact `max_message_bytes` succeeds; the next byte sends 1009 without reading that frame payload. Separately, each masked header charges exactly 6/8/14 bytes and each control payload charges its byte length against the fixed 1048576-byte source-work allowance. Exact exhaustion succeeds; a next charged unit sends 1009 without reading that unit. Data payloads do not consume source-work, and control payloads do not consume the data-message cap. |

The partial-message state stays call-local because `receive` consumes all Ping/Pong frames until one
data message or Close completes. No generic transport attachment, package sidecar, global registry,
or parser state survives a successful data return. An outgoing heartbeat API is deferred together
with an explicit timer/observation policy; sending Ping while discarding Pong would claim liveness
without evidence.

## Error and close precedence

Caller validation aborts before ownership transfer or I/O. Once reading begins, exact frame syntax
precedes length representability, then the cumulative message cap, payload transport, unmasking,
and final Text/Close UTF-8 validation. A malformed header chooses 1002 even if its declared payload
would exceed the caller cap. A valid header whose declared data bytes exceed the remaining cap
chooses 1009 without reading payload. A transport error while sending an automatic 1002/1007/1009,
Pong, or Close reply replaces the protocol result because it is the last observable operation needed
to fulfill the wire contract. Shutdown failure after a successful write likewise wins.

Automatic close is best-effort only until a transport operation reports failure; there is no second
error payload or log. A server-initiated `close` waits under its one cumulative deadline for the peer
Close, continues the required Ping replies without resetting that budget, discards application data
after entering Closing, then performs the server-side TCP close. After any terminal receive/send result the handle is either live and
synchronized, or closed/poisoned. No parser error leaves unread bytes on a reusable connection.

## Native ABI and fixed identities

The capability adds these keyed runtime rows, all reusing existing shapes:

| Key | Symbol | Shape | Native contract |
|---|---|---|---|
| `HttpRespondUpgrade` | `align_rt_http_respond_upgrade` | A24 | `unsafe extern "C" fn(*mut HttpRequestCtx, *mut ResponseBuilder, *mut *mut HttpUpgrade) -> i32` |
| `HttpUpgradeReadExact` | `align_rt_http_upgrade_read_exact` | A20 | `unsafe extern "C" fn(*mut HttpUpgrade, *mut Buffer, i64) -> i32` |
| `HttpUpgradeWrite` | `align_rt_http_upgrade_write` | A20 | `unsafe extern "C" fn(*mut HttpUpgrade, *const u8, i64) -> i32` |
| `HttpUpgradeDeadline` | `align_rt_http_upgrade_deadline` | A04 | `unsafe extern "C" fn(*mut HttpUpgrade, i64) -> i32` |
| `HttpUpgradeShutdown` | `align_rt_http_upgrade_shutdown` | A03 | `unsafe extern "C" fn(*mut HttpUpgrade) -> i32` |
| `HttpUpgradeFree` | `align_rt_http_upgrade_free` | A62 | `unsafe extern "C" fn(*mut HttpUpgrade)` |
| `HttpHeadersCount` | `align_rt_http_headers_count` | A37 | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64) -> i64` |
| `HttpHeadersTokensValid` | `align_rt_http_headers_tokens_valid` | A20 | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64) -> i32` |
| `HttpHeadersContainsToken` | `align_rt_http_headers_contains_token` | A120 | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64, *const u8, i64) -> i32` |
| `HttpCtxUpgradeReady` | `align_rt_http_ctx_upgrade_ready` | A03 | `unsafe extern "C" fn(*mut HttpRequestCtx) -> i32` |

No A124 shape is consumed. The keyed inventory grows by ten only when implementation activates the
complete capability. Header rows and the readiness row borrow the request context and retain
nothing. A null/misaligned readiness receiver returns false without dereference; source HIR cannot
form that case. All pointer/length pairs validate negative/overflow/null products before slice
formation. `HttpRespondUpgrade` first requires a writable aligned output slot and zeroes it; an
invalid slot returns `AL_INVALID` without inspecting or consuming either input. It then requires and
takes a nonnull aligned builder before validating ctx, so every later status consumes that builder;
a null/misaligned builder or ctx returns `AL_INVALID`, and semantic validation returns the ordinary
`Error.Invalid` status. Free is null-safe. Read validates the buffer/count and handle state before
clearing a live buffer, never grows past its fixed capacity, and publishes length only after complete
success. Every transport row selects caller-invalid before live/spent/poisoned state; spent maps to
`AL_INVALID`, poisoned maps to its stored status, and neither performs I/O. Upgrade takes the builder
on entry, but takes the request fd only after all validation and serialization succeed; the output
remains null until the response head writes fully. No unwind crosses any row.

## Implementation capability boundary and closure matrix

The implementation is one capability PR because the new `pkg.web` Handler variant is dormant
without its std transport consumer, while `pkg.ws` cannot be tested without both. Splitting the
producer/consumer chain would create an unusable public extension seam and duplicate ownership
proof. The expected hand-written change may exceed 1,000 lines across sema/HIR/MIR/codegen/runtime,
the `pkg.web` source, the new package, and owner tests; the single boundary has lower integration
risk because one socket-level oracle closes the transfer exactly once. Generated Japanese prose and
mechanical golden updates are excluded from that estimate.

| Closure axis | Required implementation cells | Exact owner evidence |
|---|---|---|
| Formation and type placement | Type name; exact 71/47 codec leaves; raw same-frame local/parameters; unnested same-frame Result Ok carrier from constructor/`map_err`; function-value pump signature; reject every other collection/aggregate/tag/capture/task/parallel/out/extern/global/return path and malformed future variant. | Exact bidirectional root/scalar/Result bytes and unknown/truncated/trailing rejection; parameterized sema + checked-HIR positive/negative placement sweep, variant tripwire, whole/per-unit pump compilation. |
| Construction and ownership | Builder validation, checked serialized-head size/allocation, handle-shell allocation before fd move-in, ctx source spending, output publication, handle move/local replacement, Result local/pump move-in, `?`/`else`/`match`/`map_err`, branch/loop joins, early exit, Drop. | Runtime allocation/fd counters plus MIR source-null/cleanup matrix and socket E2E; exact head high-water and OOM-before-transfer failpoint. |
| Header/readiness views | Count/token validation/contains across repeated rows and every split; source-invalid aborts; native null/alignment/length/range hard-abort before safe view formation and token-byte hard-abort before table scan; HTTP version x residual readiness; ctx lifetime; no allocation. | Runtime differential oracle, direct-ABI subprocess abort matrix, and web handshake raw-request vectors. |
| Web dispatch | Respond/Stream/Upgrade x static/param/wildcard x method/HEAD/405 x group/group_with x middleware Proceed/Respond/Failed. Validator true/false/effect; prepare Accept/Reject/Failed; upgrade validation/write failure; pump Ok/Err. | `apps_web_upgrade` owner with exact startup-order/diagnostic, route-table, and socket assertions; existing web suites remain unchanged. |
| RFC handshake | Every seven-step validation row, duplicate headers, token case/grammar, canonical key, version rejection header, server-order subprotocol selection, extensions ignored, SHA-1/base64 golden. | Independent raw HTTP oracle plus RFC 6455 accept vector; browser client interop. |
| Frame grammar | FIN/RSV/opcode/mask, 7/16/64-bit minimal lengths, control size/fragment, data/continuation state, arbitrary TCP splits/coalescing, mask positions, exact charged header/control work. | Independent frame encoder/oracle, exhaustive bounded mutation corpus, randomized fragmentation differential test. |
| Message/control | Text/Binary/fragmented UTF-8, Ping automatic Pong, Pong consume/ignore, Close empty/code/reason including 1010 empty acknowledgment, outgoing send, timed complete close handshake, every allowed/forbidden code. | Raw peer wire captures and typed result assertions. |
| Bounds and precedence | negative/zero/exact/next message cap, exact/rejected-next fixed source-work allowance including zero-length continuation and Ping/Pong floods, every length arithmetic edge, malformed-plus-oversized, cap-plus-EOF, protocol-close write/shutdown failure, caller-invalid x live/spent/poisoned transport operations, no overread. | Pairwise multi-invalid matrix, source-read/work/state/syscall/clock/buffer counters, exact boundary twins. |
| Allocation and cleanup | Exact `H`-byte Upgrade response head coexisting with builder storage and the preallocated handle shell; header stack storage, fixed scratch, partial-message storage across control policy, result ownership, no send payload copy, OOM abort, every unpublished failure, fd close once; exact `1073774720` producer-requested receive live-heap ceiling including simultaneous old/new growth and Text staging/result payloads plus the 128-byte shell budget. | Allocation/copy/fd counters and failpoints after each allocation/read/write/publication boundary; checked-`H`/head-allocation high-water owner; compile-time receive-shell assertions and a live-byte resource probe for steady, growth, Binary transfer, and Text clone peaks. |
| ABI and lowering | Ten exact keys/symbols/shapes and the reused shapes' exact empty curated function-attribute sets; pointer/null/alignment/length/capacity/output validation; header/readiness hard-abort policy; HIR/MIR operation records; LLVM calls; rt-LTO on/off. The platform accept prerequisite checks `SO_NOSIGPIPE`, closes on failure, and publishes no ctx. | Registry/export/compatibility goldens, direct runtime/subprocess matrix, socket-option failpoint, malformed HIR/MIR, optimized/unoptimized and whole/per-unit parity. |
| Cache and distribution | web types/root/router, ws root/private SHA-1, std type/operations, runtime key/body and explicit target inputs invalidate their exact interfaces/objects/links; request bytes and ambient registry state do not. | Edit/revert cache owners; vendorable `pkg/web` and `pkg/ws` source inventory; prebuilt add/remove checks only when shipped. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/ws.md`, `docs/impl/pkg-design/web.md` and its Japanese
mirror, `docs/impl/std-design/http.md`, `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`,
`docs/history.md`, `docs/open-questions.md`, `docs/impl/03-types.md`, `docs/impl/04-mir.md`,
`docs/impl/05-backend-llvm.md`, `docs/impl/07-roadmap.md`,
`docs/impl/17-library-boundary-prerequisites.md`, `docs/impl/19-hir-validation-ledger.md`,
`docs/impl/20-runtime-abi-ledger.md`, and `HANDOFF.md` must agree before implementation. Shipped
package inventories remain unchanged until source activation.

The author-side pass must prove:

- every public record above has exact source order, discriminator, type, default, evaluation,
  error, effect, ownership, lifetime, allocation, owner, identity, prerequisite, and acceptance;
- method x header multiplicity/token grammar x protocol-list state x prepare decision x HTTP version
  x parser-residual state has one response/status/header/pump rule;
- FIN x RSV x opcode x mask x inline/16/64-bit length x fragmentation x interleaved control x
  zero/exact/next cap has one read/copy/result/close rule;
- every text input is UTF-8 by type, handshake token/key grammar is ASCII-exact, embedded NUL is
  rejected in header tokens but remains data in WebSocket Text/Binary payloads, and no native
  pointer is formed before its validation phase;
- every multi-invalid input follows the stated precedence before allocation, read, write, fd move,
  or result publication;
- every native scalar width, parameter order, pointer role, status, attribute, output initialization,
  and activation count is fixed in both directions without consuming A124;
- every header/readiness query hard-aborts detectable malformed native context/view shape before
  reference/slice formation and invalid token bytes before table scan, and no invalid input is
  conflated with an ordinary zero/false;
- the Upgrade response computes checked exact wire length, allocates one head and the handle shell
  before fd transfer, and counts their overlap with the still-live builder storage;
- the macOS/iOS accepted-fd path checks `SO_NOSIGPIPE`, closes once on failure, and publishes no
  request context or process-terminating write path;
- every raw transport operation crosses caller-invalid x live/spent/poisoned state with one return,
  mutation, clock, syscall, close, and Drop rule;
- every receive crosses exact/rejected-next message bytes and charged source-work bytes, and its
  steady, growth, Binary-transfer, and Text-clone layouts stay within the fixed live-heap ceiling;
- the generic Upgrade seam cannot expose a raw fd, smuggle the handle through an aggregate/capture,
  or let request views outlive the spent context retained by the pump;
- Ping/Pong processing keeps partial message state call-local and introduces no sidecar, registry,
  hidden heartbeat, or result variant;
- all examples parse with accepted syntax and declarations remain separate from positional calls;
  and
- no implementation cell consumes a WebSocket client, HTTP/2 extended CONNECT, TLS termination,
  permessage-deflate, arbitrary extensions, raw frames, background heartbeats, async scheduling,
  connection broadcast registry, or standalone listener.

## Review finding-to-fix ledger

| Finding class | Authoritative resolution and propagation |
|---|---|
| P1 canonical type identity | Reserve the actual post-`pkg.csv` append-only leaves 71/47, pin complete bidirectional semantic-byte vectors and unknown/truncated/trailing rejection, and propagate the same values through type, HIR, MIR, and cache ledgers. |
| P1 parser residual | Retain explicit residual state in `HttpRequestCtx`; expose the protocol-neutral HTTP/1.1-and-residual-free readiness bit to `pkg.web.Ctx`; reject false in prepare as normal 400 and repeat both lower checks before fd transfer. |
| P1 spent handle | Define caller-invalid first, then live/spent/poisoned behavior for every read/write/deadline/shutdown operation; spent returns `Error.Invalid` without buffer/state/clock/I/O mutation and shutdown remains idempotent. |
| P2 HTTP version path | Make readiness the first handshake condition after exact GET so HTTP/1.0 rejects before SHA-1 through the ordinary 400 path. |
| P2 response-header syntax | Validate every stored name as RFC token and every value as HTAB/SP/visible-ASCII/obs-text in insertion order before Upgrade-specific fields or any write. |
| P2 route purity | Add a Pure total validator callback to the generic Upgrade record; keep route construction Pure and perform protocol configuration rejection once during existing pre-bind route validation with an exact diagnostic. |
| Author carrier audit | Replace the ambiguous direct-Ok-only wording with one positive grammar for raw same-frame handles and an unnested same-frame Result Ok carrier, including `map_err` and consuming control forms, while forbidding storage and escape. |
| P1 Copy web context | Preserve the shipped Copy view record while `serve` retains the request owner, so the same value may reach prepare and pump; append only the readiness boolean in both language mirrors. |
| P2 client-only close code | Preserve received 1010 as returned data but acknowledge it with an empty server Close; byte-exact echo remains for every other accepted peer Close. |
| P2 frame-work bound | Charge exact masked-header and control-payload bytes against a fixed per-call 1048576 allowance, with exact/rejected-next no-read and 1009 owners. |
| P2 transient allocation bound | Count scratch, fixed shell budget, simultaneous realloc old/new payloads, and Text staging/result payloads under an exact `1073774720` requested-live-byte ceiling and resource probe. |
| P1 checked SIGPIPE suppression | After best-effort `TCP_NODELAY`/`SO_KEEPALIVE`, make macOS/iOS accepted-socket `SO_NOSIGPIPE` installation a checked prerequisite: capture its errno, close once without retry while ignoring close status, and return the original mapped OS error from `srv.accept` before request read, ctx publication, Upgrade, or any write. Linux retains `MSG_NOSIGNAL`. |
| P2 malformed query ABI | In ctx/name/token order, hard-abort detectable null/alignment/length/range defects before safe view formation and invalid token bytes before table scan, so native defects cannot alias ordinary zero/false results; cover each row in subprocess owners. |
| P2 reused ABI attributes | Preserve each reused shape's existing empty curated function-attribute set; Rust C exports must not unwind, but the generated declaration does not gain an LLVM `nounwind` promise or mutate shared shape fingerprints. |
| P2 Upgrade head storage | Compute checked exact wire length `H`, allocate/fill one exact `H`-byte head with no growth or second copy, allocate the handle shell before fd transfer, and instrument the peak where both coexist with all builder storage. |

## References

- [RFC 6455 — The WebSocket Protocol](https://www.rfc-editor.org/rfc/rfc6455.html)
- [RFC 9110 — HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html)
- [IANA WebSocket Protocol Registries](https://www.iana.org/assignments/websocket/) — snapshot
  retrieved 2026-09-03; registry last updated 2026-06-10. The close-code rows in the ledger are
  pinned to that snapshot and do not widen automatically.
