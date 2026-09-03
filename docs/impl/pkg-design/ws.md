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
| `http_headers.count(name: str) -> i64` | Bound `http_headers` receiver, then `name`, evaluate once. `name` uses the existing header-name rule: nonempty ASCII token, no NUL; an invalid name aborts before scanning. | Pure, allocation-free, RFC 9110 ASCII-case-insensitive count of physical field rows. Repeated rows count separately; comma members do not. The result is `0..=HTTP_MAX_HEADERS` and preserves no cursor. | Receiver and result are Copy. The scan borrows the request table for the call and returns no view. | `std.http` type/method/HIR/MIR own the operation; runtime key `HttpHeadersCount`, existing A37 `i64 @SYM(ptr, ptr, i64)`. The method and key enter interface/object/link identity; request bytes do not. | Shipped `http_headers` view and fixed header cap; zero/one/repeated/case/name-invalid, whole/per-unit, malformed-HIR, and ABI owners. |
| `http_headers.tokens_valid(name: str) -> bool` | Same receiver/name evaluation and name validation. Every physical row with that name is interpreted as one RFC 9110 comma-separated token list after optional whitespace trimming. | Pure and allocation-free. Returns `true` when the field is absent or every selected row contains one or more nonempty ASCII `token` members with no empty/trailing member; otherwise `false`. Quoted strings are not tokens. It scans every selected row and cannot be used to accept a valid first row while ignoring a malformed duplicate. | Copy result; no returned view, mutation, retained cursor, or allocation. | `std.http` owns the shared token grammar; runtime key `HttpHeadersTokensValid`, existing A20 `i32 @SYM(ptr, ptr, i64)`. | Absent, one/many rows, OWS, comma boundaries, empty/quoted/non-ASCII/control members, split positions, and differential RFC-token oracle. |
| `http_headers.contains_token(name: str, token: str) -> bool` | Receiver, `name`, then `token`, once left-to-right. Both strings must be nonempty ASCII tokens with no NUL; an invalid argument aborts before scanning. | Pure and allocation-free. Searches every comma member of every case-insensitively named field row and compares the member to `token` ASCII-case-insensitively. It does not certify the remaining members; a protocol first calls `tokens_valid` when their validity matters. | Copy result; borrows only for the call. | `std.http`; runtime key `HttpHeadersContainsToken`, existing A120 `i32 @SYM(ptr, ptr, i64, ptr, i64)`. | Repeated-row/member/case/OWS/collision/malformed-neighbor matrix plus whole/per-unit and ABI goldens. |
| `http_upgrade` | A global `std.http` Move handle constructed only by successful `ctx.respond_upgrade`. It is non-Copy, non-comparable, non-printable, and not a collection element. A local may pass it by value, `borrow`, or `borrow mut`; user structs/sums/tuples, Option/Result payloads other than the constructor's direct Ok slot, arrays/slices/boxes, globals/constants, `out`, externs, captures, tasks, and parallel values reject. A user function may not return it. | A live handle owns one upgraded byte stream. Read/write/deadline/shutdown are Impure and serialized by `borrow mut`. A failed read/write closes and poisons it; every later operation returns the same stored builtin `Error` without I/O. Drop is close-only and never writes protocol bytes. | Owns the accepted fd and a small runtime state allocation, including optional monotonic start-plus-budget deadline state. Moving nulls the source. `shutdown` closes the fd and leaves a spent handle for ordinary Drop; free is null-safe. The request context remains separately owned and spent so its views stay valid during the pump. | New `Ty::HttpUpgrade`/`Scalar::HttpUpgrade`; compiler owns carrier/effect/Drop rules, runtime owns fd, deadline, and sticky error. Interface type spelling, checked HIR/MIR, object, and runtime rows enter ordinary identities. | Shipped Move/borrow machinery and HTTP server; formation, direct constructor carrier, move/drop/replacement/control joins, forbidden placement/capture/return, deadline/sticky-error, and whole/per-unit owners. |
| `ctx.respond_upgrade(rb: response_builder) -> Result<http_upgrade, Error>` | Bound live `http_request_ctx`, then `rb`, once. Consumes `rb` only. Before any write it validates in exact order: HTTP/1.1; status exactly 101; body absent; every stored header under the existing insertion-order name/value guards; exactly one `Upgrade` row containing one or more valid tokens; exactly one valid `Connection` token-list row containing `Upgrade` ASCII-case-insensitively; no `Content-Length`; then no `Transfer-Encoding`. It does not interpret the Upgrade protocol token. | Impure. The first validation failure leaves `ctx` unspent and returns `Error.Invalid`. After complete validation it serializes one final 101 head, lifts the fd, marks `ctx` spent, and writes the complete head. Success publishes the Move handle. A write failure closes the fd, leaves `ctx` spent, publishes no handle, and returns the mapped builtin error. It never parks the connection. | The source-valid call frees the builder on every result. Success transfers the fd to one new handle; failure after transfer closes it. `ctx` retains only the parsed request buffer and remains alive for the pump's request views. | `std.http` HIR `HttpRespondUpgrade`, MIR operation, LLVM call, and runtime key `HttpRespondUpgrade` using existing A24 `i32 @SYM(ptr, ptr, ptr)`. No new ABI shape; A124 remains next unused. | Exact 1.0/1.1, status/body/header-order/framing products, validation-before-write, success/failure ownership, spent-context, request-view lifetime, raw socket response, and malformed-HIR/runtime-pointer owners. |
| `u.read_exact(out: mut buffer, count: i64) -> Result<(), Error>` | Bound live `http_upgrade`, mutable bare-local `buffer`, then `count`, once. `count` must be `0..=out.capacity()`; invalid count or zero-capacity with positive count aborts before state/I/O. It clears `out.len` before transport work. | Impure. Reads exactly `count` bytes, retrying interrupted syscalls. Success publishes exactly that many bytes; zero succeeds without I/O. EOF before completion is `Error.NotFound`; timeout and OS failures keep their builtin mapping. Any failure publishes zero bytes, closes/poisons the handle, and becomes its sticky error. It never reads beyond `count`. | Borrows both owners mutably for the call; no allocation or growth. Partial bytes remain unpublished and are discarded on failure. | Compiler owns bound places/result; runtime key `HttpUpgradeReadExact`, existing A20. | Zero/exact/next capacity, every partial split, EOF/timeout/EINTR/error, no-overread coalesced frames, buffer-generation invalidation, sticky error, and whole/per-unit owners. |
| `u.write(data: slice<u8>) -> Result<(), Error>` | Bound live receiver then data once. Empty is valid and performs no syscall. Any positive length requires a valid borrowed range; NUL and arbitrary bytes are data. | Impure, SIGPIPE-safe, and write-all. Success writes the complete slice in order. A partial-then-error closes/poisons and returns the mapped error; later operations replay it without I/O. | Data is borrowed synchronously and never retained or copied. No allocation. | `std.http`; runtime key `HttpUpgradeWrite`, existing A20. | Empty/nonempty, short writes/EINTR/EPIPE/timeout, exact bytes, no-copy counter, sticky error, and platform parity. |
| `u.deadline(timeout_ns: i64) -> Result<(), Error>` | Bound live receiver then timeout once. The value must be `1..=86400000000000`; invalid aborts before clock/state work. | Impure. Captures one monotonic start plus budget and replaces any prior deadline. Every later read/write recomputes the same budget's remaining duration before each syscall, rounds positive native waits up so they do not expire early, and returns `Error.Timeout` without another syscall after exhaustion. A native timeout wakeup rechecks the clock and retries when budget remains. It never resets the budget per call, frame, or partial transfer. A fresh live handle cannot fail this operation; a poisoned handle replays its sticky error without changing state. | Mutably borrows the handle; retains only fixed-size clock/budget state and allocates nothing. `borrow mut` excludes overlap with read/write/shutdown. | `std.http`; runtime key `HttpUpgradeDeadline`, existing A04 `i32 @SYM(ptr, i64)`. | negative/zero/exact/next/max, replacement, cumulative partial read/write and multi-frame exhaustion, early native wakeup, ceil/recheck boundaries, poisoned replay, no-call-after-expiry, and overlap owners. |
| `u.shutdown() -> Result<(), Error>` | Bound live or already-spent receiver once. | Impure and terminal. A live handle invokes native `shutdown(SHUT_RDWR)` once, treats `ENOTCONN` as already shut down, closes without retry while ignoring the cleanup-close status, and becomes spent. Any other shutdown failure is mapped and returned after that close. A previously successful shutdown is idempotent `Ok` without a syscall. A poisoned handle returns its sticky error without a syscall after ensuring its fd was already closed. No WebSocket frame is written. | Retains the small spent handle until Drop; shutdowns and closes the fd at most once. Drop of a still-live handle performs only the same no-retry cleanup close, with no native shutdown and no surfaced error. | `std.http`; runtime key `HttpUpgradeShutdown`, existing A03. `HttpUpgradeFree` uses A62. | live/spent/poisoned, peer-already-closed, shutdown errno, repeated shutdown, Drop order, shutdown/close-once and no-frame counters. |
| `pkg.web.types.UpgradeAccepted { response: response_builder, selected: string }` | Exact field/source order. `selected` is protocol-defined opaque metadata; `""` is the allocation-free absent value. | Move. It is legal only as the payload of `UpgradeDecision.Accept`. | Owns one response builder and one string. Moving transfers both; ordinary Drop frees both if unpublished. Dispatch transfers `selected` to pump only after upgrade success and drops it before any failure fallback otherwise. | `pkg.web.types` nominal interface definition and complete reachable graph. | formation/order/move/drop/interface/whole-per-unit owners. |
| `pkg.web.types.UpgradeDecision { Accept(UpgradeAccepted), Reject(response_builder), Failed(Error) }` | Closed discriminator/source order exactly `0..=2`. Produced by one prepare callback. | Move. Accept attempts `respond_upgrade`. Success invokes pump; on Err, dispatch logs the ordinary handler method/path/error diagnostic once and tries the fixed 500 through the same ctx, which writes only when validation left ctx unspent and otherwise fails silently after a committed-write failure. Reject uses ordinary `ctx.respond` with no handler-error log. Failed logs once and writes the fixed 500. | Exactly one active payload owner; branch move/drop and early-exit cleanup are exhaustive. The Accept error drops selected before constructing the fallback builder. | `pkg.web.types` ordinary sum identity. | every tag/payload/control/cleanup/fallback/log path, malformed checked HIR, and interface mutation owners. |
| `pkg.web.types.UpgradeHandler { prepare: fn(Ctx, slice<str>) -> UpgradeDecision, pump: fn(Ctx, http_upgrade, string) -> Result<(), Error> }` | Exact field/source order. Both callbacks are noncapturing/copyable function values under the shipped function-value rules. | Copy. `prepare` runs once after middleware and before an Upgrade write. `pump` runs once only after a successful 101, owning the transport and selected string. Pump `Ok` emits no log; pump `Err` uses the same exact method/path/error diagnostic as Stream and cannot send another HTTP response. | Two 16-byte function values; no allocation/Drop. The pump owns and must consume or Drop the handle before return. | `pkg.web.types`; function signatures, effect facts, and complete nominal graph enter interface identity. | direct/imported function values, signature/effect mismatch, pump Ok/Err logging, whole/per-unit dispatch, and no-capturing-environment owners. |
| `pkg.web.upgrade(method: str, pattern: str, values: slice<str>, prepare: fn(Ctx, slice<str>) -> UpgradeDecision, pump: fn(Ctx, http_upgrade, string) -> Result<(), Error>) -> Route` | Arguments once left-to-right. No defaults. It performs no protocol validation; ordinary route validation later owns method/pattern, and `values` are opaque protocol configuration retained in the Copy route. | Pure constructor. Produces one ordinary route with `Handler.Upgrade`, empty `stream_type`, and exact values. It participates in the same radix priority, 405 `Allow`, prefix grouping, and middleware chain. HEAD fallback applies only to `Respond`, never Stream/Upgrade. | Returned Route is Copy and borrows method/pattern/values for the `serve` lifetime. No allocation. | `pkg.web` source and `pkg.web.types.Route`, whose exact new trailing field is `upgrade_values: slice<str>`. Router cache/source identity includes it. | mixed Respond/Stream/Upgrade routing, group/group_with copying, 404/405/HEAD/middleware, route validation, and zero hot-path allocation owners. |
| `pkg.ws.Message { Text(string), Binary(array<u8>), Close(Close) }` | Closed discriminator/source order exactly `0..=2`. Receive never exposes continuation, Ping, Pong, or raw control frames. | Move. Text is one complete valid UTF-8 message; Binary one complete byte message; Close the validated peer close. | Each nonempty payload owns ordinary heap storage; canonical empty string/array allocations follow existing representations. No payload borrows the connection or scratch. | `pkg.ws` nominal sum and reachable definitions enter interface/cache identity. | variant/order/interface, empty/nonempty allocation, move/drop/control, and whole/per-unit owners. |
| `pkg.ws.Close { code: Option<i64>, reason: string }` | Exact field/source order. `None` means an empty close payload. Received `Some(code)` admits `1000,1001,1002,1003,1007,1008,1009,1010,1011,1012,1013,1014`, registered `3000,3003,3008`, or private `4000..=4999`; all other values are invalid. Reason is the remaining valid UTF-8, possibly empty. | Move because reason is owned. A one-byte payload, invalid code, or invalid UTF-8 is never returned. | Owns only the reason string. | `pkg.ws` nominal interface definition. | empty/code-only/code+reason, every allowed/forbidden code boundary, invalid length/UTF-8, field/order and cleanup owners. |
| `pkg.ws.route(pattern: str, protocols: slice<str>, pump: fn(pkg.web.types.Ctx, http_upgrade, string) -> Result<(), Error>) -> pkg.web.types.Route` | Arguments once left-to-right. No defaults. Each server protocol must be a nonempty RFC token; duplicate byte-exact names abort. Empty list means select none and ignore valid client offers. A nonempty list requires a client offer match; the first server-list entry offered by the client wins. Pattern validation remains `pkg.web`'s startup owner. | Pure except invalid static configuration abort. Returns a GET Upgrade route using the package handshake prepare function. Middleware runs first and may answer before handshake. The selected protocol reaches the pump as an owned string; `""` means none. | Route retains pattern/protocol views for `serve`; no constructor allocation. The handshake clones only a nonempty selected protocol. | Ordinary `pkg.ws` source over `pkg.web.upgrade`; SHA-1 helper stays private and absent from the public interface. Root/internal source hashes and `pkg.web` dependency identity drive caches. | valid/invalid/duplicate protocol tables, empty/required/first-server selection, mixed web routing/middleware, whole/per-unit, and vendorable subtree owners. |
| `pkg.ws.receive(borrow mut connection: http_upgrade, max_message_bytes: i64) -> Result<Message, Error>` | Receiver then bound once. `max_message_bytes` must be `0..=536870912`; invalid aborts before allocation/I/O. Zero admits only zero-byte Text/Binary messages. | Impure. Reads exactly one application-visible event: a complete Text/Binary message or peer Close. It assembles continuation fragments, permits interleaved control frames, automatically replies to every Ping with an identical Pong and continues, and consumes/ignores every Pong. Client frames must be masked; server input with RSV set, reserved opcode, nonminimal length, invalid control shape, invalid continuation sequence, cumulative excess, invalid text UTF-8, or invalid Close is failed with the exact close policy below. Protocol/text/limit failure best-effort writes 1002/1007/1009 respectively, then shuts down and returns `Error.Invalid`; a close-write failure wins with its transport error. A valid peer Close is echoed byte-exact, shutdown, and returned; echo failure returns the transport error instead. Abrupt EOF is `Error.NotFound`; other transport errors pass through. | After valid bound checking, each call owns one fixed-capacity 32 KiB `buffer` allocation and one initially empty heap-mode `array_builder<u8>` accumulator. Its initialized length never exceeds `max_message_bytes`; first/nonempty growth uses the shipped `max(4, needed).next_power_of_two()` capacity rule, so retained payload capacity never exceeds the fixed 512 MiB global cap. Masking removal appends each decoded payload byte once. Control-only paths and zero-length messages allocate no accumulator payload. Binary success transfers the builder allocation directly into the returned array. Text success builds the byte array, validates its complete view, clones it once into the returned string, then frees the staging array. Close allocates only a nonempty returned reason string. Every result is independent of scratch/connection; every error frees unpublished storage and leaves the handle spent/poisoned. | `pkg.ws` ordinary source owns RFC state; `std.http` owns exact reads/writes. No WebSocket parser/runtime key, sidecar, registry, or HIR/MIR operation exists. | opcode/FIN/RSV/mask/7-16-64-bit length/fragment/control-interleave Cartesian product, exact/next cap, UTF-8 splits, close codes, automatic reply/failure precedence, allocation/growth/copy/cleanup, and official-vector differential owners. |
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
receive/send operation takes an exclusive call-bounded borrow. A valid peer Close is already echoed
and the transport already shut down when `Message.Close` is observed. Falling out or returning an
error before a Close simply drops the handle and closes the TCP connection without fabricating a
WebSocket close frame.

## Handshake contract

`pkg.ws.route` installs one package-owned prepare callback. The ordinary router admits only exact
GET to that row; another method follows the existing 404/405 selection and never invokes prepare.
After exact-GET route selection and a middleware Proceed, prepare validates the request in this
exact order, stopping at the first failure and performing no SHA-1, base64 encoding, response write,
or transport publication first:

1. defensively recheck the selected method is exact uppercase `GET`, then require the parsed request
   body is empty;
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

Failure inside prepare produces a normal empty-body 400 response. A version failure additionally emits
`Sec-WebSocket-Version: 13`. No invalid request invokes the pump. `Origin` is application policy and
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

`ctx.respond_upgrade` repeats the HTTP-version and 101/framing safety checks at the ownership
boundary. Package validation decides the normal 400 response; the lower check prevents malformed
checked HIR or another Upgrade protocol from publishing an invalid transport.

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
| Close | Empty payload maps to `Close { code: None, reason: "" }`. Length one, an inadmissible wire code, or invalid UTF-8 reason fails. A valid payload is echoed exactly and the transport is closed before `Message.Close` is published. |
| Text | UTF-8 validation covers the complete reassembled message, not individual fragments. Invalid final text sends 1007 and returns `Error.Invalid`. |
| Limit | The cumulative decoded data payload is checked before every payload read/copy. Exact `max_message_bytes` succeeds; the next byte sends 1009 without reading that frame payload. Control payloads do not consume the data-message cap. |

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
Pong, or Close echo replaces the protocol result because it is the last observable operation needed
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

No A124 shape is consumed. The keyed inventory grows by nine only when implementation activates the
complete capability. Header rows borrow the request context represented by `http_headers` and never
retain names/tokens. All pointer/length pairs validate negative/overflow/null products before slice
formation. `HttpRespondUpgrade` first requires a writable aligned output slot and zeroes it; an
invalid slot returns `AL_INVALID` without inspecting or consuming either input. It then requires and
takes a nonnull aligned builder before validating ctx, so every later status consumes that builder;
a null/misaligned builder or ctx returns `AL_INVALID`, and semantic validation returns the ordinary
`Error.Invalid` status. Free is null-safe. Read clears the
buffer before dereference/I/O, never grows past its fixed capacity, and publishes length only after
complete success. Upgrade takes the builder on entry, but takes the request fd only after all
validation and serialization succeed; the output remains null until the response head writes fully.
No unwind crosses any row.

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
| Formation and type placement | Type name, constructor direct Ok carrier, local/by-value/borrow/borrow-mut, function-value pump signature; reject every collection/aggregate/tag/capture/task/parallel/out/extern/global/return path and malformed future variant. | Parameterized sema + checked-HIR type-placement sweep, variant tripwire, whole/per-unit pump compilation. |
| Construction and ownership | Builder validation, fd move-in, ctx source spending, output publication, handle move/local replacement, pump move-in, normal return, `?`/`else`/`match`/`map_err`, branch/loop joins, early exit, Drop. | Runtime allocation/fd counters plus MIR source-null/cleanup matrix and socket E2E. |
| Header views | Count/token validation/contains across repeated rows and every split; invalid args; ctx lifetime; no allocation. | Runtime differential oracle and web handshake raw-request vectors. |
| Web dispatch | Respond/Stream/Upgrade x static/param/wildcard x method/HEAD/405 x group/group_with x middleware Proceed/Respond/Failed. Prepare Accept/Reject/Failed; upgrade validation/write failure; pump Ok/Err. | `apps_web_upgrade` owner with route-table and socket assertions; existing web suites remain unchanged. |
| RFC handshake | Every seven-step validation row, duplicate headers, token case/grammar, canonical key, version rejection header, server-order subprotocol selection, extensions ignored, SHA-1/base64 golden. | Independent raw HTTP oracle plus RFC 6455 accept vector; browser client interop. |
| Frame grammar | FIN/RSV/opcode/mask, 7/16/64-bit minimal lengths, control size/fragment, data/continuation state, arbitrary TCP splits/coalescing, mask positions. | Independent frame encoder/oracle, exhaustive bounded mutation corpus, randomized fragmentation differential test. |
| Message/control | Text/Binary/fragmented UTF-8, Ping automatic Pong, Pong consume/ignore, Close empty/code/reason, outgoing send, timed complete close handshake, every allowed/forbidden code. | Raw peer wire captures and typed result assertions. |
| Bounds and precedence | negative/zero/exact/next message cap, every length arithmetic edge, malformed-plus-oversized, cap-plus-EOF, protocol-close write/shutdown failure, no overread. | Pairwise multi-invalid matrix, read/copy counters, exact boundary twins. |
| Allocation and cleanup | Header stack storage, fixed scratch, partial-message storage across control policy, result ownership, no send payload copy, OOM abort, every unpublished failure, fd close once. | Allocation/copy/fd counters and failpoints after each allocation/read/write/publication boundary. |
| ABI and lowering | Nine exact keys/symbols/shapes/attributes; pointer/null/length/capacity/output validation; HIR/MIR operation records; LLVM calls; rt-LTO on/off. | Registry/export/compatibility goldens, direct runtime matrix, malformed HIR/MIR, optimized/unoptimized and whole/per-unit parity. |
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
  has one response/status/header/pump rule;
- FIN x RSV x opcode x mask x inline/16/64-bit length x fragmentation x interleaved control x
  zero/exact/next cap has one read/copy/result/close rule;
- every text input is UTF-8 by type, handshake token/key grammar is ASCII-exact, embedded NUL is
  rejected in header tokens but remains data in WebSocket Text/Binary payloads, and no native
  pointer is formed before its validation phase;
- every multi-invalid input follows the stated precedence before allocation, read, write, fd move,
  or result publication;
- every native scalar width, parameter order, pointer role, status, attribute, output initialization,
  and activation count is fixed in both directions without consuming A124;
- the generic Upgrade seam cannot expose a raw fd, smuggle the handle through an aggregate/capture,
  or let request views outlive the spent context retained by the pump;
- Ping/Pong processing keeps partial message state call-local and introduces no sidecar, registry,
  hidden heartbeat, or result variant;
- all examples parse with accepted syntax and declarations remain separate from positional calls;
  and
- no implementation cell consumes a WebSocket client, HTTP/2 extended CONNECT, TLS termination,
  permessage-deflate, arbitrary extensions, raw frames, background heartbeats, async scheduling,
  connection broadcast registry, or standalone listener.

## References

- [RFC 6455 — The WebSocket Protocol](https://www.rfc-editor.org/rfc/rfc6455.html)
- [RFC 9110 — HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html)
- [IANA WebSocket Protocol Registries](https://www.iana.org/assignments/websocket/) — snapshot
  retrieved 2026-09-03; registry last updated 2026-06-10. The close-code rows in the ledger are
  pinned to that snapshot and do not widen automatically.
