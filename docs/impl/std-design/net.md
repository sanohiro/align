This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.net — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/net.md)

> **Status:** complete in M11. DNS, TCP client/server, and UDP are shipped.

## Overview

Low-level sockets: tcp, udp, dns, socket. Syscall-backed. The keystone reuse: a connected
socket's fd plugs into the **existing M9 reader/writer** — polymorphism lives in
construction (a net-side constructor returning an fd-owning handle), the read/write/Drop-closes-fd
machinery is identical (draft §18.2 io principle; realized by reader/writer being fd-generic). So
net adds socket lifecycle + DNS, NOT a new I/O path. The `pkg.kv` prerequisite below keeps that one
writer ABI and adds only private sink provenance so socket writes can suppress SIGPIPE safely.

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
u.recv_from(buf: mut buffer) -> Result<i64, Error>        // fills caller buffer, returns byte count (v1)
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
or `Error.Code`. Partial read/write is handled by the reused reader/writer. Connection reset
mid-stream surfaces as a read/write Error; the `pkg.kv` prerequisite below closes the current Linux
SIGPIPE hole so a write to a closed peer cannot terminate the process first.

**`l.accept()` is the exception, and deliberately so: a failure of ONE inbound connection is not a
failure of the listener.** `accept(2)` reports both through the same errno, so the ones that
describe the connection are retried internally instead of being returned — `EINTR`, `ECONNABORTED`
(the client gave up between its SYN and the accept), and on Linux the already-pending network
errors accept(2) names (`ENETDOWN`, `EPROTO`, `ENOPROTOOPT`, `EHOSTDOWN`, `ENONET`, `EHOSTUNREACH`,
`EOPNOTSUPP`, `ENETUNREACH`), which it says to "treat like EAGAIN by retrying". So an accept loop
written `loop { c := l.accept()? … }` cannot be ended by one client misbehaving — which is the
whole point of the errno above reaching `Error.Code` on `connect` but not here. Everything else,
including descriptor exhaustion (`EMFILE`/`ENFILE`), IS returned: a raw listener owns no idle
connections it could reclaim, so that decision stays the caller's. (std.http's server rail shares
this noise rule and additionally recovers from exhaustion — http.md item 9 — because it does own a
parked-connection set.)

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
reuse the M9 reader/writer ABI for the byte path (the win), with the private socket-sink hardening
recorded below; `region_of` arms binding borrowed
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
- connection-derived writer to a closed peer returns Error without SIGPIPE process termination
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

---

## I/O timeouts (align-llm Request 2 — COMPLETE: net rail #633 + http surface #634, 2026-07-24)

> **Status:** the net rail below is implemented — `align_rt_tcp_connect` gained a `timeout_ns`
> parameter (non-blocking connect + `poll(POLLOUT)` deadline; `timeout_ns == 0` is the unchanged
> blocking connect), and `c.read_timeout_ns(ns)` / `c.write_timeout_ns(ns)` (`setsockopt(SO_RCVTIMEO/
> SO_SNDTIMEO)`) set an in-place deadline whose expiry the reader/writer byte path surfaces as
> `Err(Error.Timeout)`. The raw `tcp.connect(host, port)` surface stays timeout-less and lowers a
> literal `0`. The `std.http` `cl.timeout(ns)` / `r.timeout(ns)` surface (http.md "I/O timeouts")
> SHIPPED in #634, threading its effective timeout through this same `align_rt_tcp_connect` parameter.

`std.http`'s per-request timeout (http.md "I/O timeouts") rests on the net rail, so the substrate is
designed here; net also exposes it directly for raw-socket callers. Motivated by `align-llm`'s LLM
API calls (a black-holed connection must not stall the loop). Source:
`../align-llm/docs/align-requests.md` Request 2.

### Surface

Read/write deadlines are in-place setters on the bound-local conn (the same Move-builder idiom):

```text
c := tcp.connect(host, port)?
c.read_timeout_ns(ns: i64)      // SO_RCVTIMEO; 0 = block forever (default)   -> ()
c.write_timeout_ns(ns: i64)     // SO_SNDTIMEO                                 -> ()
```

Negative `ns` is rejected at build time (abort). A read/write that exceeds the deadline returns
`Err(Timeout)` — the shared `Error.Timeout` variant (canonical definition in `process.md`;
`AL_TIMEOUT = 4`). Because a deadline expiry (`EAGAIN`/`EWOULDBLOCK` from `SO_RCVTIMEO`) is
indistinguishable from a spurious wakeup only at the syscall boundary, the read/write sites convert a
deadline-armed `EAGAIN` to `AL_TIMEOUT` explicitly (the generic errno path is unchanged for
timeout-unarmed fds).

### Connect timeout — the shared substrate

The **connect** deadline lives in `align_rt_tcp_connect` (runtime `:679`; today "no connect timeout",
`:621`): it gains a `timeout_ns` parameter — non-blocking `connect` → `EINPROGRESS` →
`poll(POLLOUT)` with the ns deadline → check `SO_ERROR`; poll-timeout returns `AL_TIMEOUT`.
`timeout_ns == 0` preserves the current blocking connect exactly. `std.http` passes its effective
request timeout through this same parameter. The raw-`net` `tcp.connect(host, port)` signature stays
timeout-less in v1 (it has no pre-connect handle to set a deadline on, and Align has no optional
args); a `tcp.connect_timeout(host, port, ns)` sibling is a recorded follow-up if a raw-socket
consumer needs a bounded connect. The point of doing it here is that the substrate exists once and
`std.http` reuses it — not a second, http-local mechanism.

### New machinery

`Error.Timeout` + `AL_TIMEOUT` (shared, see process.md). `align_rt_tcp_connect` gains `timeout_ns`.
`align_rt_tcp_read_timeout` / `align_rt_tcp_write_timeout` (`setsockopt(SO_RCVTIMEO/SO_SNDTIMEO)`) +
sema `TcpConn` method dispatch for `read_timeout_ns` / `write_timeout_ns`. No new Ty, no new I/O
path — this is socket options on the existing blocking rail.

### Test / gate

Connect to a black-holed (never-accepting) address with a bound → `Err(Timeout)` within the bound. A
conn whose peer accepts then never sends, with `read_timeout_ns` set → a read returns `Err(Timeout)`
within the bound. `ns == 0` preserves blocking behavior.

---

## SIGPIPE-safe connection-derived writer (`pkg.kv` prerequisite — DESIGN CANDIDATE 2026-09-02)

> **Status:** independently useful safety prerequisite; not implemented until the `pkg.kv` design
> is accepted. No public signature, compiler operation, runtime symbol, ABI shape, registry key, or
> row count changes.

The existing `c.writer() -> writer` remains the one TCP byte-write surface. Its private runtime
`Writer` state gains a sink kind and macOS/BSD readiness bit set to socket/not-ready only by
`align_rt_tcp_conn_writer`; standard-stream and file constructors set the generic-fd kind. A
nonempty socket-kind `w.write(...)` keeps the one
complete-write loop and exact existing Result taxonomy, with these platform rules:

- Linux calls `send(MSG_NOSIGNAL)` for every attempt.
- macOS/BSD lazily installs `SO_NOSIGPIPE` before the first send on that writer shell and caches only
  success. A failed option installation returns its fixed errno-mapped `Error` before that call
  sends bytes, leaves the shell not-ready for a later retry, and a successful installation then uses
  `send` for this and later writes.
- A partial send advances the remaining view, EINTR retries, an armed blocking-socket
  `EAGAIN`/`EWOULDBLOCK` remains `Error.Timeout`, and positive-length zero progress is deterministic
  `Error.Code(0)` rather than a spin or stale errno.

There is no process-global signal handler or mask. A failure may follow bytes already written by an
earlier attempt in the same call, so the caller receives the error and owns replay policy. File and
standard-stream writers retain the existing `write(2)` path byte-for-byte. Connection-derived
writers remain unbuffered and `owns_fd: false`; their `flush`/Drop has no pending write and only the
`tcp_conn` closes the socket. `SO_NOSIGPIPE` is monotone and idempotent per socket: overlapping
shells may each attempt it, each sends only after its own successful result, a failed shell remains
retryable, no shell Drop clears it, and connection close discards it. A logger or `io.copy` that
uses the same connection-derived writer inherits the socket sink kind instead of opening a second
path.

Acceptance owners are a subprocess closed-peer test on Linux and macOS (a regression must return
`Error`, never die from SIGPIPE), direct partial/EINTR/timeout/zero-progress mapping tests, and
file/std writer parity. Exact package consumption and the implementation boundary are in
`../pkg-design/kv.md`; the unchanged ABI identities and planned checked-timeout row are recorded in
`../20-runtime-abi-ledger.md`.
