This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.net — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/net.md)

## Overview

Low-level sockets: tcp, udp, dns, socket. Syscall-backed. The keystone reuse: a connected
socket's fd plugs into the **existing M9 reader/writer** unchanged — polymorphism lives in
construction (a net-side constructor returning an fd-owning handle), the read/write/Drop-closes-fd
machinery is identical (draft §18.2 io principle; realized by reader/writer being fd-generic). So
net adds socket lifecycle + DNS, NOT a new I/O path.

## Signatures

v1 proposal — draft §18.2 lists members only; these are Fable's settled shapes:

```text
// TCP client
tcp.connect(host: str, port: i64) -> Result<tcp_conn, Error>   // DNS + connect; keepalive ON by default
c.reader() -> reader          // borrow an M9 reader over the socket fd
c.writer() -> writer          // borrow an M9 writer over the socket fd
// TCP server
tcp.listen(host: str, port: i64) -> Result<tcp_listener, Error> // bind+listen; SO_REUSEADDR
l.accept() -> Result<tcp_conn, Error>
// UDP
udp.bind(host: str, port: i64) -> Result<udp_socket, Error>
u.send_to(data: bytes, host: str, port: i64) -> Result<i64, Error>
u.recv_from(buf: mut buffer) -> Result<datagram, Error>   // fills caller buffer, returns {n, peer}
// DNS
dns.resolve(host: str) -> Result<array<string>, Error>    // owned IP strings
```

## Type & ownership classification

- `tcp_conn`, `tcp_listener`, `udp_socket` are **Move types** (new `Ty::TcpConn`/`Ty::TcpListener`/
  `Ty::UdpSocket`), each owns one fd, Drop = close(fd) — the reader/writer/buffer Move precedent
  exactly. Rejected as array/slice/vec/box elements and as Option/Result payloads at the
  `scalar_arg` choke point, EXCEPT the Result Ok payload positions their own constructors return
  (connect/listen/accept/bind return `Result<T, Error>`) — allow those Ok positions like
  reader/writer were (`Scalar::Buffer` #346 template).
- `c.reader()`/`c.writer()` return **borrowed** M9 reader/writer over the conn's fd
  (`owns_fd: false` — the conn still owns and closes it). So the reader/writer's region is bound
  to the conn `c`; using them past c's Drop is rejected (`region_of(TcpReader) = region_of(c)`).
  This is the #297-trap-aware arm.
- `dns.resolve` → owned `array<string>` (deep-drop like `read_dir` #339). `datagram`/`response`
  are small structs (Copy) carrying counts + owned peer/body as appropriate.
  - **Slice 4 v1 shape (shipped):** `recv_from` returns the received **count** only —
    `Result<i64, Error>`, mirroring `reader.read` exactly (fill the caller's buffer, return the byte
    count). The ideal `datagram {n, peer}` return is **deferred**: a `Result` `Ok` payload is a single
    `Scalar` (there is no `Scalar::Tuple`), and the peer address is an owned `string`, so `{n, peer}`
    would require synthesizing a builtin Move struct-with-owned-field aggregate — a magic special-case
    that "ideal form or defer" forbids. It waits for first-class builtin-struct returns. The socket
    already receives the peer at the syscall (`recvfrom`); v1 simply discards it (null `src_addr`).

## Effect classification

All net ops are **impure** (syscalls) — never in a `par_map` closure.

## Error policy

Syscall failures go through the **shared errno→Error table** (M9): ECONNREFUSED/ETIMEDOUT/
EHOSTUNREACH → `Error.Code(errno)` (no dedicated variant in v1 — extend the table only if a
consumer needs to branch on them), ENOENT-class DNS failure → a resolve-specific `Error.Invalid`
or `Error.Code`. Partial read/write handled by the reused reader/writer (already correct).
Connection reset mid-stream surfaces as a read/write Error.

## Concurrency model

The recorded rail (open-questions "Network std rails"): connection reuse by default (keepalive
ON). net provides the **substrate** for bounded-concurrency batching — `task_group` + the
`par_map` blocking pool (NOT a new async runtime; `io_uring` is a later Linux backend, not the
semantic model). The concrete batched API (`get_many`, pipelined write-then-read) lives **one
layer up in `std.http`** (`cl.get_many`) — it operates on HTTP request/response types, which are
`std.http`'s, so it must NOT sit in `std.net` (a net→http dependency would be a layering
violation / circular dependency; see http.md). net stays byte-stream generic. A connect-per-request
loop to one static host is a lint target (post-v1 lint, record but don't implement in the module).
HTTP/3, TLS, socket tuning (TFO/REUSEPORT/thread-per-core) are pkg, not std.

## New machinery required

3 new Move `Ty` (TcpConn/TcpListener/UdpSocket) + runtime structs + Drop(close); socket-lifecycle
runtime fns (socket/connect/bind/listen/accept, getaddrinfo for `dns.resolve`, sendto/recvfrom);
reuse M9 reader/writer verbatim for the byte path (the win); `region_of` arms binding borrowed
reader/writer to their conn; the `task_group` + blocking-pool substrate that `std.http`'s
`get_many` builds on (batching itself is http's, not net's). No new effect, no new I/O path, no
async runtime.

## Slice breakdown

1. `dns.resolve` alone (getaddrinfo → owned `array<string>`) — smallest, no Move type, validates
   the errno path + deep-drop.
2. `tcp_conn` Move type + `connect` + `reader()`/`writer()` borrow (the reader/writer reuse — the
   core proof) + Drop-closes-fd + full Gate-1 sweep.
3. `tcp_listener` + `listen` + `accept` (server side).
4. `udp_socket` + `bind` + `send_to` + `recv_from`.

(The batched `get_many` rail is implemented in `std.http`, not here — it needs HTTP types. net
just supplies the `task_group` + blocking-pool substrate, already available.)

## Pitfalls (implement carefully)

- **P1 (Move sweep ×3)**: three new Move Ty must be swept through every pass like reader/writer
  (`ty_is_move`/`tracks_region`/`null_moved_source`/drop/`MoveCheck`/`EscapeCheck`/`region_of`/
  finalize/MIR/codegen/print). Highest risk; a miss = fd double-close or leak.
- **P2 (borrowed reader/writer region, #297)**: `c.reader()`/`writer()` borrow the conn's fd
  (`owns_fd:false`). Their region MUST be `region_of(c)`, not Static — else a reader outlives its
  conn's `close(fd)` = use-after-close. Explicit `region_of` arm + escape test. This is the
  subtle one: the reader is itself a Move type but here it's a NON-owning borrow, so it must NOT
  close the fd on its own Drop (`owns_fd:false` already handles this in runtime, but the region
  binding is new).
- **P3 (fd double-close)**: conn owns the fd; `reader()`/`writer()` borrows must set
  `owns_fd:false` so only the conn's Drop closes. Verify no path closes twice.
- **P4 (batching lives in http, not net)**: the batched `get_many` takes HTTP request/response
  types, so it belongs in `std.http` (`cl.get_many`), NOT `std.net` — putting it here would make
  net depend on http (a layering violation / circular dependency). net only exposes the substrate.
  *(Superseded detail, corrected 2026-07-10 when the get_many design settled — see http.md slice-plan
  item 6: the CPU-sized `par_map` pool is the wrong shape for I/O-bound batching, so get_many uses
  its own bounded blocking-worker claim loop; and per-slot `Err` is inexpressible — `Result` is a
  `Ty`, not a `Scalar`, so array elements can't carry it — making the batch **all-or-Err** with the
  lowest-index error, matching the frozen `Result<array<response>, Error>` signature.)*
- **P5 (DNS owned strings deep-drop)**: `array<string>` from `resolve` must deep-free each IP
  string (`read_dir` #339 template).
- **P6 (bound-receiver, #337/#338)**: conn/listener/socket are owned Move — unbound temporaries
  can't be receivers in v1 (bind first). `tcp.connect(...).reader()` remains rejected after the
  2026-07-15 general Move-temporary cleanup fix because receiver stable-address semantics are a
  separate surface decision.

## Test checklist

- `dns.resolve` localhost → contains 127.0.0.1
- connect to a local listener + round-trip bytes through reader/writer
- reader used past conn Drop → compile error (P2)
- accept loop serves N clients
- udp `send_to`/`recv_from` round-trip
- fd not double-closed (the RSS/fd-count test pattern)
- conn/listener as array element → rejected
- unbound-temporary receiver → rejected
- import-required
- (Integration tests need a loopback listener in-process — the m9 io test harness pattern.)

**Note**: v1 is blocking sockets on the blocking pool. Non-blocking/epoll/io_uring is a later
Linux backend behind the same signatures, not a semantic change.
