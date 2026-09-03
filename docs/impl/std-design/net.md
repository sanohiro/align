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
consumer needs to branch on them). Resolver failures are EAI values, not errno:
`EAI_NONAME`/`EAI_NODATA` → `Error.Invalid`; every other nonzero EAI value →
`encoded := AL_CODE.saturating_add(eai.saturating_abs())`, then
`Error.Code(encoded - AL_CODE)`. Partial
read/write is handled by the reused reader/writer. Connection reset
mid-stream surfaces as a read/write Error; the implemented `pkg.kv` prerequisite below has closed
the former SIGPIPE hole, so a write to a closed peer cannot terminate the process first.

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
> SHIPPED in #634, threading its effective timeout through this same `align_rt_tcp_connect`
> parameter. The accepted-design prerequisite below tightens positive-timeout mode transitions and
> quantization without changing this public surface or an ABI identity.

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

The **connect** deadline lives in `align_rt_tcp_connect`: a positive `timeout_ns` uses
non-blocking mode; immediate zero succeeds, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` enter
`poll(POLLOUT)`, and every other immediate errno is mapped. Readiness is resolved through
`SO_ERROR`; poll timeout returns `AL_TIMEOUT`. The implemented prerequisite below makes mode
installation and restoration checked.
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

Connect to a black-holed address with a logical wait deadline → `Err(Timeout)` without early
expiry. A conn whose peer accepts then never sends, with `read_timeout_ns` set → a read returns
`Err(Timeout)` after its configured blocking wait. `ns == 0` preserves blocking behavior.

---

## Checked shared timeout substrate (`pkg.kv` prerequisite 1 — IMPLEMENTED 2026-09-02)

> **Status:** implemented as the first independently useful prerequisite. It changed no public
> signature, compiler operation, runtime symbol, ABI shape, registry key, or row count. At that
> boundary the checked package row remained inactive; it is now active with `pkg.kv`.

For every usable resolver address and positive `timeout_ns`, `align_rt_tcp_connect` records a
monotonic start and positive `Duration` budget immediately before the first `F_GETFL`, then checks
`F_GETFL` and `F_SETFL(flags | O_NONBLOCK)`. Either failure records its fixed errno-mapped status,
closes that candidate, and continues to the next address without calling `connect`. After checked
installation, exactly one immediate `connect` is issued: zero succeeds,
`EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` enter the wait, and every other errno is mapped immediately.
Either immediate terminal result wins even if the budget is simultaneously exhausted. The
in-progress path continues against the same start/budget pair; it never forms an absolute
`start + budget`, so `Instant::checked_add` overflow cannot turn a huge positive timeout into an
unbounded wait:

- each iteration subtracts `start.elapsed()` from the budget; a positive remainder is rounded up to
  the next millisecond and saturated at `i32::MAX` for one `poll`, so the complete positive i64
  range remains bounded through repeated chunks; an exhausted remainder returns `AL_TIMEOUT`
  before another poll and does not issue a final zero-timeout `poll` call;
- EINTR recomputes the remainder, any other poll error is mapped immediately, and zero from `poll`
  causes another monotonic recheck and another poll only when time remains, or `AL_TIMEOUT` without
  another poll when the budget is exhausted; and
- a positive readiness/error event wins over a simultaneously exhausted budget and is resolved
  through `SO_ERROR`.

Every immediate or polled success then checks `F_GETFL` and
`F_SETFL(flags & !O_NONBLOCK)`. A restoration failure records that status, closes the candidate, and
continues. No connection is published until blocking mode was checked-restored. The nonpositive
raw-ABI blocking path stays unchanged: public HTTP callers reject negative values before this ABI,
and raw `tcp.connect` supplies zero. DNS and the sum across multiple addresses have no end-to-end
deadline; scheduler/kernel delay may return after an address's logical deadline.

A nonzero `getaddrinfo` result returns before address iteration:
`EAI_NONAME`/`EAI_NODATA` maps to `AL_INVALID`, while every other symbolic EAI value maps to
`AL_CODE.saturating_add(eai.saturating_abs())`. The connection output remains null,
transient host/service storage drops, no address-list owner escapes, and no socket is attempted.
Symbolic EAI owners pin both categories, null output, cleanup, and zero socket calls.

After successful resolution, resolver order is observable. Unsupported families, null addresses,
and zero address lengths are skipped without changing the last failure. The first successful usable
address wins. No usable address returns `AL_INVALID`; if every attempted candidate fails, the
runtime returns the last status from socket creation, nonblocking `F_GETFL`, nonblocking `F_SETFL`,
an immediate connect errno, poll error/timeout, `getsockopt(SO_ERROR)` failure, nonzero `SO_ERROR`,
blocking-restore `F_GETFL`, or blocking-restore `F_SETFL`. Mixed-address owners put a skipped entry
and a later success after each failure class; all-failure variants pin the last attempted status and
close count.

The same prerequisite fixes the shared socket-timeout conversion used by public
`read_timeout_ns`/`write_timeout_ns` and the now-active checked package row. Every positive nanosecond
value becomes `ceil(ns / 1000)` microseconds and then a normalized
`timeval { tv_sec, tv_usec: 0..999999 }`; exact microseconds remain exact, and zero retains the
existing clear/no-timeout meaning. The option bounds one blocking wait for progress, not the total
duration of a multi-read or multi-write operation.

The active source-reachable `TcpConnSetIoTimeout` consumer requires every non-null compatible caller
to hold one live, unfreed connection exclusively across the call, with no live reader/writer shell
derived from it and no other value retaining one at entry, and no overlapping read/write/
configuration/reader-or-writer construction/free/Drop. It uses the exact normalized `timeval` and
installs receive before send. With entry option states `{R0,S0}` and requested state `T`, receive
failure performs exactly one `setsockopt`, returns its mapped status, makes no send call, and leaves
`{R0,S0}`; send failure performs exactly two calls, returns the send mapped status, and leaves
`{T,S0}`; success performs exactly two calls, returns zero, and leaves `{T,T}`. Either option failure
requires the compatible caller to retire the connection, perform no read/write/configuration/
reader-or-writer construction/retry, and free/Drop it exactly once; its zero-derived-shell entry
state leaves no shell cleanup to order against that close. Success preserves usability and may
construct derived shells afterward, but a later overwrite requires all such shells and retaining
values to Drop first. The package calls only on a fresh unpublished clear/clear connection before
shell construction and closes either failure without reopening resolution or trying another
address. Owners pin live/exclusive/zero-derived-shell entry preconditions, pre-armed states, option
order, call counts, returned status, retry prohibition, zero overlapping/post-failure constructor
calls, retirement, and close/Drop. One structural owner classifies retainers by target-connection
provenance through the complete active recursive Drop graph: direct/buffered reader, writer, or
logger-owned writer leaves in locals/calls, struct fields, nested `Option`/`Result`, and admitted
user-sum paths, plus elements of source-constructed fixed arrays of retaining structs. That last
path composes the existing struct-field and fixed Move-struct-array rules and does not admit a
direct handle array element. Derived from
the canonical formation/Drop graph with a new-edge tripwire, it crosses inactive/moved-out states,
other/mixed-connection shells, and zero/one/multiple target leaves; only zero is compatible. Every
positive carrier class completes configure-construct-move-into-move-out-where-supported-or-
recursive-Drop-reconfigure. Direct handle collections/boxes/tuples and direct reader/writer user-sum
payloads remain formation negatives; nameable dynamic-array/slice shapes for retaining structs/sums,
the admitted non-tuple shapes' user-struct-field closure, and the direct dynamic-array/slice element,
tuple, and builtin `Option`/`Result` edges admitted for `DynStructArray` retain explicit no-live-
producer owners.

The ceil-to-microsecond conversion also serves shipped `std.http` plain/TLS/pool rearming. The
poll-millisecond helper also serves `process.command`; that consumer adopts the same monotonic
start-plus-budget arithmetic and ceil conversion for the complete positive-i64 range while keeping
its existing post-syscall timeout-wins precedence. These are one shared prerequisite, not divergent
package-local conversions.

Acceptance owners cover exact/next and maximum-positive ns, us, ms, chunk, and deadline boundaries;
failed `F_GETFL`/`F_SETFL` installation and restoration on immediate and polled success; early
zero-result recheck versus exhausted/no-call poll, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` versus other
immediate errno, EINTR remainder recomputation, readiness at the deadline, every resolver skip and
last-status failure class, symbolic EAI branch, mixed-address close/continuation, no early expiry, a blocking-mode probe
on every published connection, exact-timeval and pre-armed receive/send state plus caller-retirement products, HTTP
plain/TLS/pool rearm, and command pipe-drain/post-EOF reap.

The shipped direct owners are `socket_timeout_timeval_quantization`,
`tcp_connect_timeout_budget_quantization`, `tcp_connect_transition_and_address_matrix`,
`tcp_connect_resolver_status_and_order_matrix`, `tcp_connect_positive_timeout_publishes_blocking_fd`,
`http_timeout_quantization_plain_tls_pool_rearm`, and `command_timeout_budget_quantization`, plus the
pre-existing end-to-end timeout and command cleanup/precedence owners.

---

## SIGPIPE-safe connection-derived writer (`pkg.kv` prerequisite 2 — IMPLEMENTED 2026-09-02)

> **Status:** implemented as the second independently useful safety prerequisite. No public
> signature, compiler operation, runtime symbol, ABI shape, registry key, or row count changed. At
> that boundary the package source and checked-timeout row remained inactive; both are now active.

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

The existing keyed `IoWriterWriteBuilder` identity, A19
`i32 @align_rt_io_writer_write_builder(ptr, ptr)` declaration,
`unsafe extern "C" fn(*mut Writer, *mut Builder) -> i32` Rust ABI, attributes, and inclusion in the
then-shipped 330/347/355 keyed/base/maximum counts did not change at the writer-prerequisite
boundary. The later `pkg.kv` row made the totals 330/348/356, and `pkg.csv` subsequently makes them
331/349/357. Its source-visible builder overload
borrows the builder bytes and delegates to the hardened `IoWriterWrite` row, so it cannot bypass the
socket sink policy.

Acceptance owners include failed macOS/BSD install with no send followed by retry, overlapping
shells with success/failure in both orders, shell Drop without option clear, and connection close
discarding the setting. Linux/macOS subprocess closed-peer tests cover direct slice and builder
overloads, logger, and `io.copy` routes and must return `Error`, never die from SIGPIPE. Direct
partial/EINTR/timeout/zero-progress tests and file/std writer parity remain separate owners.

The shipped direct owners are `tcp_writer_complete_send_transition_matrix`,
`tcp_writer_macos_nosigpipe_state_matrix`, `tcp_writer_generic_fd_parity_and_socket_lifecycle`, and
`tcp_writer_closed_peer_routes_do_not_sigpipe`.

Both prerequisites have landed. The `TcpConnSetIoTimeout` row and its `pkg.kv` package consumer
have also landed and are active together. Exact package consumption and the implementation
boundary are in `../pkg-design/kv.md`; the active one-row delta and unchanged prerequisite ABI
identities are recorded in `../20-runtime-abi-ledger.md`.
