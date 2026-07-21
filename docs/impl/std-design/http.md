This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.http — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/http.md)

## Overview

An HTTP/1.1 primitive, NOT a framework (draft §18.2). Built on std.net sockets. Members: request,
response, header, method, status, client, server primitive. Connection reuse per the net rail.
**HTTPS/TLS on the client is SHIPPED** (Slice 5): `https://` works transparently through
`cl.get/post/request` + `cl.get_many` over OpenSSL libssl (mandatory verification against the system
trust store + hostname binding), dynamically linked alongside crypto's libcrypto. Server-side TLS
stays deferred (client-first). HTTP/3, routing, middleware = pkg, not std.

**Module status: COMPLETE** (Slices 1–6 shipped; client-side TLS is Slice 5). Server-side TLS,
client certs, custom CA, session resumption, and revocation are the recorded post-v1 backlog.

## Signatures

v1 proposal, Fable's settled shapes:

```text
// Client
cl := http.client()                         // owns a connection pool (Move)
cl.get(url: str) -> Result<response, Error>
cl.post(url: str, body: bytes) -> Result<response, Error>
cl.request(req: request) -> Result<response, Error>
// Request/response building
r := http.request(method: str, url: str)    // builder (Move — owns header list + body buf)
r.header(name: str, value: str)
r.body(data: bytes)
resp.status() -> i64
resp.header(name: str) -> Option<str>       // view into resp
resp.body() -> bytes                         // view into resp (region-bound)
// Server primitive (not a framework) — surface settled 2026-07-10 (two-lens design review)
srv := http.serve(host: str, port: i64) -> Result<http_server, Error>
srv.accept() -> Result<http_request_ctx, Error>   // one request; caller writes the response
ctx.method() -> str                          // view into ctx (region-bound)
ctx.path() -> str                            // view into ctx (region-bound)
ctx.header(name: str) -> Option<str>         // view into ctx (region-bound)
ctx.body() -> bytes                          // view into ctx (region-bound)
rb := http.response(status: i64)             // response_builder (Move — owns header list + body buf;
                                             // the build-dual of `request`; named apart from the
                                             // parsed read-view `response`)
rb.header(name: str, value: str)             // bound receiver; CR/LF/NUL aborts (P6)
rb.body(data: bytes)                         // optional — a header-only response is legal
ctx.respond(rb) -> Result<(), Error>         // consumes BOTH ctx and rb; one-write serialize (R4);
                                             // closes the accepted fd (v1: one request per conn)
// Batched client (the rail — moved here from net; see Concurrency in net.md)
cl.get_many(urls: slice<str>, max_concurrency: i64) -> Result<array<response>, Error>
```

## Type & ownership classification

- `client`, `request`, `http_server`, `http_request_ctx`, `response_builder` are **Move types**
  (own pooled conns / header lists / body buffers / the listening or accepted socket). reader/writer
  Move precedent + the net Move types they wrap. `response_builder` is deliberately a distinct type
  from the parsed read-view `response`: build (header-list → serialize) and parse (offset-table →
  views) never share a usage site, so one overloaded type would add an internal Parsed|Built branch
  to every getter for zero convergence gain. The symmetry that matters is by direction and holds:
  `response_builder` ≅ `request` (builders), `http_request_ctx` reads ≅ `response` reads (views).
- **`response_builder` is a nameable type and a valid `Option`/`Result` payload** (2026-07-20). It
  was originally neither: unspellable in source, and refused by `scalar_arg` outright, on the
  reasoning that `http.response` returns one directly so no API would ever wrap it. pkg.web's
  ownership decision needs exactly that — a handler that BUILDS a response and hands it back
  (`fn(Ctx) -> Result<response_builder, Error>`) so the framework keeps the request handle and can
  still answer when the handler fails. It is now admitted on the same terms as `http_request_ctx`:
  legal as a payload, still refused as an array/slice/box element, where an element read copies the
  handle and both copies would free it.

  This is sound because the builder **owns every byte it holds and borrows nothing** —
  `rb.header(name, value)` stores `String::from_utf8_lossy(..).into_owned()` and `rb.body(data)`
  stores `data.to_vec()`. That is what lets a builder outlive the locals its header/body were built
  from, and why it is not region-tracked. **A zero-copy `rb.body` would therefore be a breaking
  change, not an optimization**; `response_builder_payload.rs` pins the copy semantics from both
  sides (survival, and byte-exact bytes off the wire from a handler whose body came from a dead
  local).
- `ctx.method()/path()/header()/body()` return **views region-bound to ctx** (#297 arm), the exact
  read-duals of `resp.status()/header()/body()`.
- `response` owns its header block + body buffer (Move); `resp.header()`/`resp.body()` return
  **views region-bound to resp** (#297-aware `region_of` arm — same as net's borrowed
  reader/writer and `json.decode`).
- Move-rejection at the `scalar_arg` choke point except own-constructor Result Ok positions (net
  template).

## Effect classification

All impure (network syscalls via net).

## Error policy

Transport errors bubble from std.net (errno→Error table); HTTP-level (malformed response, bad
status line) → `Error.Invalid`. A 4xx/5xx status is NOT an error — it's a valid response with that
status (the caller branches on `resp.status()`); only transport/parse failures are `Err`. (This is
a deliberate One-way call: HTTP status is data, not a Result error.)

## Performance requirements (owner directive, 2026-07-07 — requirements, not aspirations)

The owner wants std.http **fast**. The measured rails recorded in `open-questions.md` (external
design-note review: keepalive 1.48×, pipelined write-then-read 19.1×, bounded-concurrency
`get_many` 12.8× at 64 reqs) are engineering requirements for v1, plus the zero-copy discipline
the rest of std already follows. Concretely:

- **R1 — zero-copy response**: one owned response buffer; status line / headers / body are parsed
  as an **offset table + views into that buffer** (no per-header `string` allocations, no body
  copy). `resp.header()`/`resp.body()` already return region-bound views — the internal
  representation must actually be zero-copy too.
- **R2 — SIMD-backed scanning from day one**: header/line scanning rides the runtime's existing
  memchr layer (#310: AVX2+NEON+scalar, already shipped for `str` search) — find CRLF / `:` via
  memchr, never a byte-at-a-time scalar loop. The full simdjson-style structural scan (shared
  byte-classifier with JSON) stays a recorded later optimization; memchr is free today.
- **R3 — connection reuse by default**: the pool (Slice 3) is a requirement, not an option —
  `cl.get()` to the same host:port reuses the live conn (keepalive) with zero opt-in. The
  measured 1.48× is the floor; the pipelined 19.1× shape is what `get_many` batching builds on.
- **R4 — syscall discipline on the hot path**: `TCP_NODELAY` on client conns (no Nagle-delayed
  request tails); serialize the whole request (start-line + headers + body) into one buffer and
  send it with **one write** (no per-header writes); socket reads go through the M9 buffered
  reader (no per-line read syscalls).
- **R5 — `get_many` = task_group + the ParPool claim loop** (#301) with bounded concurrency —
  the measured 12.8× I/O-overlap shape; NOT a new async runtime; `io_uring` stays a later Linux
  backend, per the recorded decision.
- **R6 — benchmark-gated completion**: a `bench/http_client` harness (local plaintext server;
  keepalive GET latency/throughput + `get_many` scaling) measured against a Rust baseline —
  the module is not "done fast" until the numbers are in its README, per the repo's
  measure-before-claiming rule. **R6 is SATISFIED as of Slice 3:** `bench/http_client` ships (drives
  the shipped pool via its C-ABI entry points against an in-process localhost server) and records
  **2.86× keepalive speedup** (floor 1.48× — MET) and **parity with hand-written Rust `std::net`** on
  the reuse path (see `bench/http_client/README.md`). **The `get_many` scaling part is
  now ALSO MET (2026-07-10, the R5 slice):** 64 GETs at degree 16 with 12 ms injected latency —
  **15.4× overlap** (ideal ≈ degree) and **1.01× of an equal-degree Rust thread pool** (parity);
  honest-reporting caveats in the bench README (quote with degree + core count). R6 is now met in
  full.

## New machinery required

Move types above + HTTP/1.1 parse/serialize over net sockets + connection pool reuse. NO new I/O
path (net's reader/writer). TLS wrapper deferred (blocks HTTPS). Header parsing = memchr-backed
scan per **R2** (the full structural-scan/byte-classifier upgrade recorded for later).

## Slice breakdown

1. request/response structs + header list + HTTP/1.1 serialize/parse (no socket yet — pure
   encode/decode, testable standalone). **DONE** (branch `m11-http-slice1-parse`). Shipped surface:
   `http.request(method, url)` (total — URL parsed at serialize, not here, so a runtime URL never
   aborts the builder), `r.header(name, value)` / `r.body(data)` (mutate in place, bound receiver,
   P6 CR/LF/NUL → abort), `http.parse(bytes) -> Result<response, Error>` (the response constructor +
   codec primitive — Slice 2's client reuses the same engine; a permanent primitive, not throwaway),
   `resp.status()` / `resp.header(name)` (case-insensitive `Option<str>` view) / `resp.body()`
   (`slice<u8>` view) — both getters region-bound to `resp` (#297). serialize stays a **runtime-only
   codec** (`align_rt_http_serialize`, one contiguous buffer per R4, unit-tested) — Slice 2's client
   renders + one-writes it, not a language builtin yet. All Slice-1 ops **Pure** (no sockets). Auto
   `Host` + `Content-Length` (iff body non-empty); a caller-supplied `Host`/`Content-Length` is
   rejected (CL-duplication smuggling guard). `chunked` Transfer-Encoding → `Error.Invalid`
   (Content-Length framing only in v1; R1-honouring de-chunking deferred). Caps: ≤ 128 headers,
   ≤ 1 GiB body. R1 zero-copy: the response owns one byte buffer + an offset table; scanning rides
   the `memchr` crate (R2).
2. client + get/post over one net `tcp_conn` (plaintext). **DONE** (branch
   `m11-http-slice2-client`). Shipped surface (behind `import std.http`, all **Impure** — network):
   `http.client()` (Move `http client` handle; a ZST in v1 — no pooled state yet, the FFI entry
   points already take `*mut HttpClient` so Slice 3 adds the pool behind the same surface),
   `cl.get(url) -> Result<response, Error>` / `cl.post(url, body) -> Result<response, Error>` /
   `cl.request(req) -> Result<response, Error>` (bound-receiver gate; `cl` borrowed, `request`
   **consumes** its Move `req`). Each performs ONE request over one fresh net `tcp_conn`: connect
   (reuses `align_rt_tcp_connect` — DNS + connect + SO_KEEPALIVE) → **TCP_NODELAY** (R4) → **one
   write** of the serialized request (R4, via the Slice-1 `http_serialize_core` — auto Host +
   Content-Length, method/header/smuggling validation) → stream the response through the socket in
   32 KiB reads (never per-line — R4) to Content-Length, then parse via the Slice-1
   `http_parse_core` (R1 zero-copy). A 4xx/5xx is `Ok(response)` (P2); `https://` / a malformed URL
   is `Error.Invalid` at request time (P1 — never a silent plaintext downgrade). Framing is
   Content-Length (or read-to-close); chunked stays `Error.Invalid` (Slice-1 policy). The parser
   was refactored to an `Incomplete`/`Invalid` split so the streaming read distinguishes "need more
   bytes" from "malformed" over one shared decoder. NO pool yet (every request connects fresh and
   closes — Slice 3 adds keepalive reuse); `get_many` / server / HTTPS remain.
3. connection pool reuse (the rail — keepalive, reuse by default). **DONE** (branch
   `http-slice3-pool`). `http.client()` is no longer a ZST: it owns a **keepalive connection pool**
   (`Mutex<HashMap<(host, port), Vec<IdleConn>>>`) behind the unchanged language surface and FFI ABI
   (the compiler already treats `HttpClient` as an opaque handle pointer, so this slice is purely a
   runtime change — no sema/MIR/codegen edits). Consecutive `get`/`post`/`request` calls to the same
   `(host, port)` **reuse a live idle conn with zero opt-in** (R3); `Drop` (`align_rt_http_client_free`)
   closes every pooled conn (P5). **Reuse-verdict (correctness-critical — a dirty conn reused would
   misframe the next response):** a finished conn is returned to the pool **iff** it was keep-alive
   (HTTP/1.1 default; `Connection: close` or a non-1.1 version → not reused — decided by
   `http_head_keep_alive` from the response head), **Content-Length-framed** (read-to-close responses
   end at the conn close → not reused), carried **no bytes beyond the framed message** (leftover ⇒
   dirty ⇒ dropped), **and** its response **fully parsed** — the pool decision runs *after*
   `http_parse_core`, so a conn whose response the streaming pass admitted but the owning parse rejects
   (an untrustworthy stream) is closed, never pooled. **Stale-conn retry:** a reused idle conn the
   server has since dropped fails before any response byte; that ONE case is transparently retried once
   on a fresh conn — and the retry **bypasses the pool** (a fresh connect, never a second pooled conn,
   since the same host can hold several corpses after a server restart). A fresh conn's failure, or any
   mid-response failure, surfaces directly. **SIGPIPE:** the client write path uses `send(MSG_NOSIGNAL)`
   (Linux) / `SO_NOSIGPIPE` (macOS) so writing to a dropped reused conn returns `EPIPE` (→ retry)
   instead of killing the process — no global signal handler installed. **Pool bounds / hygiene:** ≤ 8
   idle conns per host; an idle conn older than 90 s is reaped — on `take` *and* on `put` (so a fresh
   conn is never dropped in favour of stale ones), with the overflow conn closed only after reaping; an
   emptied bucket's key is removed from the map (no unbounded empty-`Vec` growth across many hosts).
   **R6 met:** `bench/http_client` (below) records the
   pool at **2.86× keepalive speedup** (floor 1.48×) and **parity with hand-written Rust `std::net`**.
   Tests: `align_runtime` units (pool reuses one conn across 3 gets; `Connection: close` not pooled;
   stale-conn retry; `http_head_keep_alive` decision table) + a driver test (two gets reuse one conn,
   observed via the server's accept count).
4. server primitive (serve/accept, caller writes response). **DONE** (branch `http-slice4-server`).
   Shipped surface (behind `import std.http`, the server ops **Impure**): `http.serve(host, port) ->
   Result<http_server, Error>` (Move handle owning the listening fd — wraps net's `tcp.listen`,
   SO_REUSEADDR + backlog 128, then lifts the fd out); `srv.accept() -> Result<http_request_ctx,
   Error>` (Move handle owning the accepted fd + the request parsed to a zero-copy offset table,
   mirror of `HttpResponse` R1 — streaming 32 KiB reads to the head's end + Content-Length body
   framing, reusing the Incomplete/Invalid split and the 256 KiB-head / 128-header / 1 GiB-body caps;
   a malformed request closes that conn and returns `Error.Invalid`, the listener stays alive);
   `ctx.method()/path()` (`str` views), `ctx.header(name)` (case-insensitive `Option<str>` view),
   `ctx.body()` (`slice<u8>` view) — all region-bound to `ctx` (#297); `http.response(status)` ->
   `response_builder` (Move, distinct Ty + display name from the parsed `response`) + `rb.header(name,
   value)` (bound receiver, P6 CR/LF/NUL **abort**) + `rb.body(data)` (optional); `ctx.respond(rb) ->
   Result<(), Error>` (**consumes BOTH** ctx and rb — MIR nulls both slots like `cl.request(req)`;
   serialize = status line + headers + auto Content-Length iff a body was set; ONE write, R4;
   MSG_NOSIGNAL/SO_NOSIGPIPE; closes the fd, v1 one-request-per-conn). The **NEW**
   `http_parse_request_head` for `METHOD SP target SP HTTP/1.1` implements all five inbound smuggling
   guards below. **Three new Move types** (`http_server`/`http_request_ctx`/`response_builder`) took
   the full Gate-1 twin-mirror sweep (Ty + Scalar for the two Result payloads; `response_builder` is
   Ty-only like `http request`; `null_moved_source` for the respond double-consume was the one
   easy-to-miss arm). Tests: `align_runtime` units (the request-head parser + each of the five guards
   + serialize framing + fd-leak across N cycles) + driver e2e (`m11_http_server.rs`: an Align server
   driven by a Rust client, **and a dogfood run of the shipped Align `cl.get` client against the Align
   server**, plus the Gate-1 compile rejections). **Two adjustments from the settled record, both
   recorded here:** (1) the request-line parser accepts `HTTP/1.0` **and** `HTTP/1.1` (v1 always closes
   the conn, so 1.0-vs-1.1 persistence is moot; not a guard weakening — the five guards are unchanged);
   (2) `respond` always emits `Connection: close` (RFC 9112 §9.6 **mandates** it for a non-persistent
   server — the connection-management dual of the auto Content-Length, NOT an editorial `Date`/`Server`
   header) and rejects a caller-supplied `Connection` / `Transfer-Encoding` at respond time alongside
   the settled caller-`Content-Length` rejection. HTTPS/server-keepalive/concurrent-serving stay
   deferred exactly as recorded. The settled surface (2026-07-10; two independent design reviews:
   language-purity lens + systems-evolution lens; both ratified — full surface in Signatures above)
   with its decisions:
   - **Response building = `response_builder`** (`http.response(status)` + `.header` + `.body` +
     `ctx.respond(rb)`), the exact mirror of the client `request` builder — status is a
     construction-time field like method/url; an args-form `respond(status, headers, body)` is
     inexpressible (no varargs/dict literal) and a header-less `respond(status, body)` is too
     limited for a primitive (no Content-Type).
   - **`respond` consumes both ctx and rb** (precedent: `cl.request(req)` consumes its Move `req`):
     statically forbids respond-twice and use-after-close; one-write serialize (R4).
   - **Auto-header policy (mirror of client serialize):** auto `Content-Length` iff a body was set;
     caller-supplied Content-Length rejected (smuggling guard); **no auto Date/Server** — editorial
     headers are the caller's (framework = pkg territory).
   - **v1 = one request per accepted connection** (`respond` closes the fd). Server-side keepalive
     later lands invisibly behind this surface: `respond`'s close becomes close-or-pool per the
     client Slice-3 reuse-verdict mirror, and `accept()` yields the next request off a kept-alive
     conn — no signature change (the ZST→pool precedent).
   - **`http_parse_request_head` is NEW** (the response head parser keys on `HTTP/` + status and is
     not reusable for `METHOD SP target SP HTTP/1.1`). The Incomplete/Invalid streaming split, the
     header-block scan, and the caps (256 KiB head / 128 headers / 1 GiB body) ARE reused. The
     server parse side MUST add the five inbound smuggling guards the client-lenient response
     parser lacks: (1) strict CRLF line endings — reject bare LF; (2) reject whitespace between
     field-name and colon (RFC 9110 server MUST); (3) reject Content-Length + Transfer-Encoding
     together (TE alone already → `Error.Invalid`, CL-only framing); (4) explicit target forms —
     accept origin-form (`/path`), reject absolute-/authority-/asterisk-form with `Error.Invalid`
     (v1); (5) mirror the serialize-side method-token + CR/LF/NUL guards on the inbound line.
   - **Concurrency: v1 is a sequential accept→respond loop.** `spawn` captures are Copy/scalar-only
     today, so a Move ctx cannot cross into a task — **Move-capture-into-spawn is the recorded
     prerequisite for concurrent serving** (tied to that consumer; not a Slice-4 blocker — the A5
     single-GPU gateway serializes inference anyway).
   - **SSE/streaming (runway A5) is committed to land as a sibling op, not a change to `respond`:**
     `ctx.respond_stream(rb) -> Result<http_stream, Error>` — full settled design in slice-plan
     item 7 below (2026-07-11; it AMENDS this bullet's original "Drop = terminal chunk + close":
     Drop is now close-only, `finish()` is the sole clean terminator — rationale there). The v1
     surface already admits it (`.body()` is optional), so nothing was painted in.
   - **R-requirements: R1/R2/R4 apply and are required** (zero-copy request offset table; memchr
     scan; one-write respond). No server bench gate in v1 — a light accept→respond round-trip bench
     arrives with keepalive/concurrency, where a reuse path first exists.
5. **HTTPS/TLS (client-side) — SHIPPED 2026-07-10** (design settled + implemented; branch
   `http-slice5-tls`). Zero new user-facing surface — `https://` starts working through
   `cl.get/post/request` **and** `cl.get_many` (its workers share the exchange path, so HTTPS is
   transparent in a batch); `http://` is byte-for-byte unchanged. The DC-1 coarse-`https://`-rejection
   debt retired. **Implementation notes (as built):**
   - **Conn abstraction:** one internal `Conn` enum (`Plain { fd }` / `Tls { ssl, fd }`) with
     `write_all` / `read` (→ a source-agnostic `ConnRead` = `Data`/`Eof`/`Err`) / `close` methods, so
     the streaming response loop and its Incomplete/Invalid framing split are single-sourced across
     plaintext and TLS — the client-lenient parse never forks. `http_socket_exchange` takes `&mut Conn`.
   - **Engine:** OpenSSL libssl, one `#[link(name = "ssl")]` extern block mirroring libcrypto's
     wrappers; the driver links `-lssl` alongside `-lcrypto`. One process-wide `SSL_CTX` in a
     `OnceLock`, built lazily with `SSL_CTX_set_default_verify_paths` (system store) + TLS-1.2 floor;
     thread-safe for the concurrent `SSL_new` the `get_many` workers issue.
   - **Per-conn verification (in `http_tls_connect`, all BEFORE the handshake):** `SSL_VERIFY_PEER`;
     for a DNS authority `SSL_set1_host` + `X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS` + SNI
     (`SSL_set_tlsext_host_name`); for an IP-literal authority `X509_VERIFY_PARAM_set1_ip_asc` and NO
     SNI (RFC 6066); ALPN advertises `http/1.1`. Default port 443 (http = 80).
   - **Error taxonomy:** verify failure (`SSL_get_verify_result != X509_V_OK`, checked first) →
     `Error.Denied`; handshake/transport syscall → errno-mapped `Error.Code`; TLS alert / protocol
     violation → `Error.Invalid`. `SSL*` AND fd freed on every error path (`close_tls` = one-way
     `SSL_shutdown` + `SSL_free` + `close`). `SSL_read`/`SSL_write` wrapped in `SSL_get_error`
     (`WANT_*` retry on the blocking socket, `ZERO_RETURN` = EOF, `SYSCALL`-with-errno-0 = unclean EOF).
   - **SIGPIPE:** per-thread `pthread_sigmask` block around the whole HTTPS exchange
     (handshake + I/O + teardown), draining a pending SIGPIPE via zero-timeout `sigtimedwait` before
     restoring the prior mask (a `SigpipeBlock` RAII guard, held for the perform only when the scheme
     is https). On macOS/BSD the guard is a no-op ZST — the per-socket `SO_NOSIGPIPE` set at connect
     already covers the SSL BIO's `write(2)`. Plaintext keeps `MSG_NOSIGNAL`, unchanged.
   - **Pool:** key is now `(scheme, host, port)` — a TLS conn never satisfies a plaintext bucket or
     vice versa; `IdleConn` carries the live `SSL*` (reuse = same `SSL`, no re-handshake); every
     constructor/consumer (`take_idle`/`put_idle`/client `Drop`/stale-reap/overflow) is TLS-aware.
     The stale-retry logic ports unchanged — handshake failures happen only on the fresh path, so
     they are never wrongly retried.
   - **Tests:** `align_runtime` units — taxonomy (self-signed → Denied, wrong-host-cert → Denied,
     refused → Code, garbage-TLS-server → Invalid), positive round-trips (IP path + DNS/SNI path),
     TLS pool reuse (one conn / two gets), pool scheme-keying, `get_many` over mixed http+https, and
     `/proc/self/fd` no-leak across N TLS cycles — against a local libssl test server with embedded
     PEM fixtures. The positive path uses a **test-only trust hook**: a `#[cfg(test)]` `OnceLock`
     (`TLS_TEST_CA_FILE`) that adds the test CA to the client store; it is compiled OUT of the shipped
     runtime (structurally, not a runtime guard), so release builds have no trust hook at all —
     verification stays mandatory. A driver test proves the routing change (`https://` connects
     instead of being rejected pre-connect); the positive TLS round-trip is not drivable from the
     driver harness because the `#[cfg(test)]` trust hook is absent in the driver-linked runtime.

   **Settled design (as ratified):** Zero new user-facing surface — `https://` simply starts working
   through `cl.get/post/request` (the URL scheme is the only input that should change behavior).
   - **Engine = OpenSSL libssl** (the same package as libcrypto; OpenSSL ≥3.0 for TLS), capability-
     linked together with `-lcrypto` when HTTPS is used. The *linkage* reuses crypto's settlement;
     the **trust decision is a genuinely new semantic and gets its own record (this one)**: certificates are
     **always verified** against the **system trust store** (`SSL_CTX_set_default_verify_paths()`,
     never a hardcoded path; deployment note: the `ca-certificates` package must be present or
     every handshake fails closed). No disable/custom-CA/client-cert/resumption surface in v1 (no
     config surface exists — consistent with the frozen signatures). Fail closed, always.
   - **Hostname binding is REQUIRED, not optional — chain-verify-only is a defect.** The record
     mandates the exact APIs: `SSL_set_verify(SSL_VERIFY_PEER)` + `SSL_set1_host(host)` (DNS names;
     with `SSL_set_hostflags(X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS)`) or
     `X509_VERIFY_PARAM_set1_ip_asc(host)` for IP-literal authorities, set **before** the
     handshake so OpenSSL folds hostname matching into verification; `SSL_set_tlsext_host_name`
     (SNI) from the URL host; ALPN advertises `http/1.1`; TLS ≥ 1.2.
   - **Error taxonomy:** certificate/hostname/trust verification failure → **`Error.Denied`** (a
     refused trust decision — distinguishes verify-fail from a malformed URL with zero new
     variants); handshake/transport syscall failure → the errno-mapped `Error.Code`; a TLS alert or
     protocol violation mid-response → `Error.Invalid`. fd **and** `SSL*` freed on every error
     path (crypto's discipline). Read loop wraps `SSL_read`/`SSL_write` in `SSL_get_error`
     (`WANT_*` retry / `ZERO_RETURN` = EOF / `SYSCALL` → errno / `SSL` → Invalid); the
     Incomplete/Invalid split is source-agnostic and ports unchanged.
   - **SIGPIPE:** `MSG_NOSIGNAL` cannot reach `SSL_write` (BIO writes carry no flags) and Linux has
     no `SO_NOSIGPIPE`. A process-global `signal(SIGPIPE, SIG_IGN)` was considered and REJECTED —
     it would break the recorded no-global-handler discipline. Settled mechanism: **per-thread
     `pthread_sigmask`** — block `SIGPIPE` around the TLS exchange (worker threads block it at
     start), drain a pending signal via zero-timeout `sigtimedwait` before restoring.
   - **Pool:** the key becomes **(scheme, host, port)** — a TLS conn must never satisfy a plaintext
     bucket or vice versa. Reuse = reusing the live `SSL*` (no re-handshake; not session
     resumption). The stale-retry verdict ports cleanly (handshake failures happen only on the
     fresh path, so they are never wrongly retried). Drop/expiry: best-effort one-way
     `SSL_shutdown` (don't wait for the peer), `SSL_free`, `close` — Content-Length framing makes
     truncation attacks moot (a short body is already `Error.Invalid`).
   - **Server-side TLS stays DEFERRED** — coherent, not half-shipped: the server primitive carries
     its recorded trusted-network caveat; client-first matches the align-LLM A5 consumer.
6. **`cl.get_many(urls, max_concurrency)` (R5) — design SETTLED + SHIPPED 2026-07-10** (same
   two-lens review; implementation on branch `http-get-many`). Shipped exactly as settled below,
   including the prerequisite `array<response>` opaque-Move-handle-array capability (runtime-only
   construction, `rs[i]` borrow-in-receiver-position, per-element drop) and the R5 bench (15.4×
   overlap at degree 16, Rust-pool parity — see R6 above). Settled record:
   - **Results in input order** (`urls[i]` → `results[i]`); **all-or-Err**: any transport/parse
     failure fails the whole batch with the **lowest-index** error (deterministic — matches the
     `tg_wait` convention). Per-element `array<Result<response, Error>>` is **inexpressible**
     (`Result` is a `Ty`, never a `Scalar`; array elements are `Scalar`s) — all-or-Err is the only
     honest form, recorded with a future pointer (per-slot errors wait on a `Scalar::Result`-class
     capability, if ever). 4xx/5xx stay `Ok` data. Empty `urls` → `Ok` empty array. GET-only
     (`request_many` deferred-until-consumer — the rail, not the verb set, is R5's substance).
     `max_concurrency <= 0` **aborts** (programmer bug, the `rand.range` class).
   - **Run-to-completion, no short-circuit:** there is no cancellation primitive and blocking reads
     cannot be interrupted, so on failure the remaining workers finish and their results are
     discarded; the first (lowest-index) error is reported. The no-timeout limitation is therefore
     **amplified** by batching (one stalled server holds the whole batch) — recorded; the fix
     belongs to the future deadline/structured-cancellation slice.
   - **Mechanism: a dedicated bounded blocking-I/O worker pool, NOT the CPU-sized ParPool.** The
     R5 draft said "task_group + the ParPool claim loop", but the ParPool is sized to
     `available_parallelism()` and caps I/O overlap at core count — wrong shape for I/O-bound
     batching (you want overlap ≫ cores). Settled: the runtime spawns `min(max_concurrency,
     urls.len())` scoped blocking workers that claim URL indices off a shared counter and slot
     results input-order. This is exactly the settled "async = task_group + blocking workers"
     stance; live fds are bounded by the worker count (+ ≤8 idle/host pooled on completion). The
     pipelined 19.1× rail is NOT a get_many deliverable (Slice-3's reuse verdict forbids
     undrained-conn reuse) — the 12.8× multi-conn overlap shape is.
   - **Prerequisite capability (compiler): `array<response>` — a dynamic array of opaque Move
     handles.** Today `response` is rejected as an array element (the owned-handle exclusion), so
     the frozen return type needs a narrow new capability, shipped WITH get_many as its consumer
     (the #399 `Scalar::Slice`+consumer precedent): construction **by runtime only** (user-side
     `[resp1, resp2]` literals stay rejected); `rs[i]` in receiver position is a **borrow** (bound
     method calls — `rs[i].status()`, `rs[i].body()` — views region-bound to the array; the
     owned-field-borrow precedent), moving an element out is rejected in v1; whole-array move nulls
     the source; Drop = per-element `http_resp_free` loop + storage free. Full twin-mirror sweep
     required for the new element class.
   - **Bench (closes R6's get_many part):** 64 URLs against an in-process localhost server with
     **injected per-request latency** (localhost RTT ≈ 0 would mask the overlap win), vs a Rust
     baseline using an equal-degree fixed thread pool. Honest reporting: the measured overlap
     factor + the machine's core count + parity-vs-Rust at equal degree — NOT a
     hardware-independent 12.8× claim.
7. **SSE/chunked streaming response (`respond_stream`, the runway A5 remainder) — design SETTLED
   2026-07-11, SHIPPED.** Runtime: `HttpStream { fd, framed, poisoned }` + `align_rt_http_respond_stream`
   / `_stream_send` / `_stream_finish` / `_stream_free`; the head serializer is single-sourced as
   `http_serialize_head` (respond appends CL+body, respond_stream appends TE); the request's HTTP
   version is threaded parse → `HttpRequestHead.http11` → `HttpRequestCtx.http11` → the stream's
   `framed`. Compiler: `Ty::HttpStream`/`Scalar::HttpStream` (a Move handle riding the `Result` Ok
   payload, the accept precedent), HIR `HttpRespondStream`/`HttpStreamSend`/`HttpStreamFinish`, all
   routed through `lower_http`. Tested by runtime units (frame encoder, version, shared-head parity,
   poison, empty-send no-op) + `crates/align_driver/tests/m12_http_stream.rs` (1.1 chunked / 1.0 raw /
   truncation / poison / the align-client-rejects-chunked asymmetry / the double-consume + bodied-abort
   gates). (Two-critic review, Fable synthesis.) The gateway token-streaming layer: the
   caller writes SSE `data: …\n\n` lines as body content; std.http ships the **transfer framing
   only** (the framework boundary holds).
   - `ctx.respond_stream(rb) -> Result<http_stream, Error>` — consumes BOTH ctx and rb (the
     `respond` precedent). rb must be **header-only**: a body already set is a programmer
     contract bug → **abort** (`respond` is the bodied path; the `rand.range` abort class —
     code-structure-driven, not client data). Head serialize = status + headers + auto
     `Transfer-Encoding: chunked` + auto `Connection: close` (the auto-CL mirror); **the head
     serializer is single-sourced with `respond`'s** (one shared head fn incl. the
     caller-CL/TE/Connection rejection loop and the P6 guards; respond appends CL+body,
     respond_stream appends TE).
   - **HTTP/1.0 clients (required, found by review — the version is currently parsed then
     DISCARDED):** thread the request's HTTP version parse→head→ctx→stream. For a 1.0 request
     chunked is illegal — the stream is constructed in **close-delimited raw mode** (`framed:
     bool` on the stream): no TE header, `send` writes payload bytes unframed, `finish`/Drop
     just close (read-to-close IS valid 1.0 framing).
   - **`http_stream`** (Move, owns the fd lifted out of ctx; free-standing — borrows nothing
     from ctx, no region binding; standard Move-handle exclusions). `s.send(chunk: bytes) ->
     Result<(), Error>` — one chunk frame (lowercase hex length, no `0x`, CRLF payload CRLF)
     assembled in one buffer, ONE write via `http_send_all` (MSG_NOSIGNAL/EINTR/partial-write
     discipline; EPIPE → Error). **`send("")` is a no-op returning Ok** — an empty chunk is
     the protocol TERMINATOR, and empty output steps are foreseeable gateway data (a multi-byte
     UTF-8 codepoint split across tokens detokenizes to zero bytes), not a programmer bug;
     writing nothing is the honest semantics. TCP_NODELAY is already set at accept — one send
     = one immediately-visible event (the token-streaming latency requirement).
   - **`s.finish() -> Result<(), Error>` is the SOLE clean terminator** — consumes the stream
     (a new `null_moved_source` arm, the easy-to-miss one), writes `0\r\n\r\n` (framed mode;
     trailers omitted — conformant per RFC 9112 §7.1), closes, surfaces errors. **Drop =
     close-only, NO terminal write** — this deliberately AMENDS the earlier committed bullet:
     with no write deadline in v1, a terminal write on Drop to a stalled peer would block the
     single accept loop unboundedly, and a missing terminal chunk is exactly how a chunked
     sender signals truncation — abrupt close is both safer and truncation-honest (the
     explicit-op-surfaces-errors / Drop-is-silent split, the file/conn precedent). A
     **`poisoned` flag** set by any failed `send` makes `finish` skip the terminal write,
     close, and return Err (the stream did not terminate cleanly).
   - Streaming restates the slow-loris caveat: a stream holds the single blocking accept
     thread for the whole generation by design — the trusted-network posture is load-bearing,
     not just an attack caveat.
   - Client parse stays CL-only (chunked → `Error.Invalid` on align's own client — the
     recorded asymmetry; the gateway's clients are external).

8. **`respond_stream` rework for pkg.web stream routes — DESIGNED + SHIPPED 2026-07-21.**
   pkg.web's streaming design (`docs/impl/pkg-design/web.md` → "Streaming") is the consumer; it
   requires the framework to keep owning the request context while a stream handler runs, and a
   4xx window before the head is committed. Three changes, all pre-release-outright (the M12 tests
   were updated outright, no compat path):
   - **① Non-consuming receiver.** `ctx.respond_stream(rb) -> Result<http_stream, Error>` consumes
     `rb` ONLY. The fd is lifted into the stream as today; `ctx` stays with the caller, **spent**:
     a later `respond`/`respond_stream` on it is `Err` (not abort — reachable via ordinary control
     flow, unlike the bodied-rb contract bug); its Drop frees the parse buffer and skips the fd
     close (already lifted). This is what keeps `Ctx`'s views (path/query/**body** — an LLM pump
     reads the prompt while streaming) valid for the whole pump call. Precedent: `rb.header` is
     already a mutating non-consuming bound receiver.
   - **② Lazy head.** `respond_stream` VALIDATES the rb eagerly (header-only contract, P6 guards,
     TE/Connection policy — unchanged, still abort on a bodied rb) but serializes the head into the
     stream handle instead of writing it; the first `send` (or `finish`) writes it. Observable
     change: a client sees nothing until the first event — document in the fn doc, it is the price
     of ③.
   - **③ `s.reject(rb) -> Result<(), Error>`.** Legal only before the first send (after: `Err`,
     poison untouched): discards the stored head, writes `rb` as a complete NORMAL response
     (respond's serializer, CL+body), closes. Consumes the stream. This is a stream route's only
     pre-stream 4xx/5xx path — validation happens inside the pump, `reject` answers it.
   - `send`/`finish`/Drop/poison semantics above are otherwise unchanged; `framed` (1.0/1.1) is
     chosen at `respond_stream` time as today and baked into the stored head.
   - **Shipped record.** Runtime: `HttpStream.pending_head` (taken by the first `send`/`finish`
     write attempt — committed even if that write fails; head + first chunk / head + terminator go
     out in ONE write), `align_rt_http_stream_reject`, and spent-fd (`fd < 0`) `Err` checks in both
     `respond` and `respond_stream`; a validation `Err` from `respond_stream` leaves the ctx
     UNSPENT (the caller can still `respond` an error). Language: `s.reject(rb)` via
     `ExprKind::HttpStreamReject`/`Rvalue::HttpStreamReject` (both consumed, MIR nulls both slots);
     `HttpRespondStream` now nulls only `rb`. Tests: `align_runtime` unit
     (lazy-head/reject/spent-ctx contracts) + `m12_http_stream.rs` (13: borrow-then-stream
     `ctx.path()` mid-pump, spent-ctx `respond` → `Err` E2E, reject → normal-400 E2E, late reject →
     `Err` + truncation, move-gates for reject).
   - **④ `s.send_event(data) -> Result<(), Error>` — SHIPPED 2026-07-21** (with pkg.web streaming
     enabler 5, its first consumer — the committed "SSE event framing (WHATWG) when the first
     streaming consumer lands" floor item). Wraps `data` as ONE event frame `data: {data}\n\n`
     assembled INSIDE the same buffer as the chunk framing and the (possibly still pending) lazy
     head — head + chunk framing + event in one `http_send_all` write; raw (1.0) mode writes the
     event bytes unframed. **`send_event("")` is a legal EMPTY event** (`data: \n\n`, 8 payload
     bytes — never the chunked terminator), so unlike `send("")` it is a real write and commits
     the head. Multi-line `data` is the caller's problem in v1 (a bare `\n` changes the event's
     field structure — splitting is recorded pkg.web backlog). Borrows `s` exactly like `send`
     (poison latch shared). It is a METHOD, not a `pkg.web` free fn, because a pkg-level free fn
     takes a Move handle by value (no user-fn borrow params — the `io.copy` bound-receiver
     restriction class), which would consume the stream the pump still has to finish. Runtime:
     `align_rt_http_stream_send_event` over the shared `http_stream_send_parts` helper. Language:
     `HttpStreamSend`/`Rvalue::HttpStreamSend` gained an `event: bool` (same variant, so every
     analysis pass treats it as `send` — no new-variant soundness sweep needed). Tests: runtime
     framing unit (framed empty/non-empty + raw), `m12_http_stream.rs` `send_event` E2E, and the
     pkg.web `apps_web_stream.rs` suite.

## Known v1 limitations (Slice 2/3/5)

- **HTTPS is CLIENT-SIDE ONLY (Slice 5).** Server-side TLS is deferred — `http.serve` is plaintext,
  and its recorded trusted-network caveat (below) stands. Client-first matches the align-LLM A5
  consumer; server TLS is coherent post-v1 work, not a half-ship.
- **No certificate revocation checking (Slice 5).** Verification is chain + hostname against the
  system trust store; there is no CRL / OCSP / OCSP-stapling check. A revoked-but-not-expired cert
  that still chains to a trusted root is accepted. Revocation is recorded post-v1 backlog (alongside
  client certs, custom CA, and session resumption — none of which have a config surface in the frozen
  signatures).
- **The system trust store must be present (Slice 5 deployment note).** Trust roots come from
  `SSL_CTX_set_default_verify_paths()` (never a hardcoded path). If the OS `ca-certificates` package
  (or equivalent) is absent, the store is empty and **every** HTTPS handshake fails CLOSED with
  `Error.Denied` — the correct fail-closed posture, but a deployment prerequisite worth stating: ship
  `ca-certificates` in any container/image that makes HTTPS requests.
- **SERVER-SIDE ESCALATION of the timeout gap (Slice 4, security caveat — settled 2026-07-10).**
  On the client the missing I/O deadline is a robustness gap; on the **server** it is a security
  boundary: one slow-loris client (connects, then stalls or dribbles below the caps) holds the
  single blocking accept thread forever — with v1's sequential accept loop that is a trivial
  whole-server denial of service. **The v1 server primitive is therefore unsafe on untrusted
  networks**; its recorded trust assumption is a **localhost / trusted-network gateway** (the
  align-LLM runway A5 consumer), where slow-loris is out of the threat model. A read/accept
  deadline is the **first post-v1 server hardening**, ranked above the client-side timeout note
  below.
- **No read/connect I/O timeout (G3-1, medium, inherited) — DELIBERATELY DEFERRED past Slice 3.**
  A server that completes the TCP handshake then stalls — sends nothing, dribbles bytes below the
  caps, or sends fewer than `Content-Length` and holds the socket open — blocks the calling thread
  **indefinitely**. The byte caps (256 KiB head / 1 GiB body) bound *memory*, not *time*. This is the
  net rail's documented no-timeout behavior (`align_rt_tcp_connect`), inherited on connect **and**
  read. **Slice 3 decision (recorded, not implemented):** the Slice-2 note said the timeout follow-up
  would land "alongside the Slice-3 pool work, where the pool already needs per-conn deadline
  bookkeeping." On implementing Slice 3 that phrasing proved to conflate two different things. The
  pool's deadline bookkeeping is **idle-expiry** (don't reuse a conn idle > 90 s) — which Slice 3
  **does** ship — not an **I/O deadline** on connect/read. Adding real I/O timeouts is a separable,
  larger change that does not have an ideal *http-local* form: (1) a **connect** timeout's ideal home
  is the net rail (a non-blocking `connect` + `poll` substrate — net.md already flags this as a later
  backend); doing it half-in-http would be a second, partial mechanism. (2) A **read** timeout is a
  few lines (`SO_RCVTIMEO`), but a *fixed* one silently breaks a legitimate slow/large transfer, and
  v1 has **no configuration surface** to make it per-request without expanding the frozen
  `get`/`post`/`request` signatures — a separate design decision. Per "ideal form, or defer," Slice 3
  ships the pool's idle-expiry and the SIGPIPE-safe/stale-retry robustness, and **defers I/O timeouts
  to the net-rail non-blocking/deadline substrate** (unchanged from a semantics standpoint), rather
  than bolting in a half-measure. Recorded here as the standing v1 limitation.
  - **Sub-case — HEAD / 304 framing (inherited from Slice 1/2).** A `HEAD` response, or a `304 Not
    Modified`, legitimately carries a `Content-Length` header **but no body**. The v1 read loop frames
    purely by `Content-Length` (it does not special-case the request method or status), so it would
    wait for body bytes that never arrive → the same indefinite block as above. v1's surface does not
    expose `HEAD` conveniently (only `get`/`post`/`request`), but a caller-built `request` with method
    `HEAD` hits this. Method/status-aware framing (no-body for HEAD/1xx/204/304) lands with the same
    slice that adds de-chunking; recorded here, not fixed in Slice 3.
- **~~`https://` rejection is coarse (DC-1, low).~~ RESOLVED by Slice 5.** `https://` no longer maps
  to `Error.Invalid` at all — it routes to the verified TLS path. A verification failure is now the
  distinct `Error.Denied`; a bad TLS transport is `Error.Code`; a protocol violation is
  `Error.Invalid`. (The message-less `Error` enum is still a broader story, but the specific DC-1
  "HTTPS not supported" debt is gone — HTTPS *is* supported.)

## Pitfalls

- **P1 (no silent downgrade — now via real TLS)**: `https://` must NEVER be sent as plaintext.
  Slice 5 satisfies this by connecting over verified TLS (mandatory cert + hostname verification,
  fail-closed → `Error.Denied`), not by rejecting the scheme. Silent downgrade remains a security
  footgun (Nothing-hidden violation); the guarantee is now "https means TLS," enforced by the engine.
- **P2 (status-is-data)**: 4xx/5xx must NOT map to `Err` — only transport/parse failures. A
  `get()` returning 404 is `Ok(response with status 404)`. Getting this wrong forces callers into
  awkward double-error handling.
- **P3 (response view region, #297)**: `resp.header()`/`body()` are views into resp; `region_of` =
  `region_of(resp)`, not Static. Escape past resp Drop rejected.
- **P4 (Move sweep + bound-receiver)**: client/request/server/ctx are Move — full Gate-1 sweep +
  bound-receiver gate (#337/#338); unbound temporaries can't be receivers in v1.
- **P5 (connection pool Drop)**: client owns pooled conns; Drop closes all. No fd leak across pool
  churn.
- **P6 (request smuggling / header injection)**: reject CR/LF in header names/values at build time
  (header injection → request smuggling). Validate on `r.header()`.

## Test checklist

- serialize a request → exact bytes
- parse a known response → status/headers/body
- `get()` against a local plaintext server → 200 round-trip
- 404 → `Ok(status 404)` not `Err` (P2)
- `https://` → verified TLS round-trip (Slice 5); untrusted / wrong-host cert → `Error.Denied`
- CRLF in header → rejected (P6)
- response body view escaping resp → compile error (P3)
- pool reuses a conn across 2 gets
- Move-rejection + unbound-receiver rejected
- import-required
- `bench/http_client` numbers recorded vs a Rust baseline (R6 — completion is benchmark-gated)
