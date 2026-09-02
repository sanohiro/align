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

**Module status: v1 COMPLETE** (Slices 1–6 shipped; client-side TLS is Slice 5). The first
post-`pkg.db` convergence item, client streaming receive, is **IMPLEMENTED 2026-08-30**: the
dependent raw `http_read_stream` boundary and its consuming `http_sse_stream` transition are both
shipped. Server-side TLS, client certs, custom CA, session resumption, and
revocation remain separate backlog items.

## Signatures

v1 proposal, Fable's settled shapes:

```text
// Client
cl := http.client()                         // owns a connection pool (Move)
cl.max_response_body_bytes(limit: i64)      // 0 = whole-body fixed default / streaming no cumulative cap
cl.get(url: str) -> Result<response, Error>
cl.post(url: str, body: bytes) -> Result<response, Error>
cl.request(req: request) -> Result<response, Error>
cl.request_stream(req: request) -> Result<http_read_stream, Error>
                                             // consumes req; returned Move stream borrows cl
stream.status() -> i64
stream.header(name: str) -> Option<str>       // view into stream's retained final head
stream.read(out: mut buffer) -> Result<i64, Error>
                                             // de-framed bytes; overwrites out; 0 = complete
events := stream.sse()                        // consumes stream at its current body position
events.status() -> i64
events.header(name: str) -> Option<str>
events.last_event_id() -> str                  // current persistent state; view into events
events.retry_ms() -> Option<i64>               // current persistent state
events.next(out: mut buffer) -> Result<Option<http_sse_event>, Error>
                                             // WHATWG event; three str views borrow out's fresh generation
                                             // retry_ms is an inline Copy value

http_sse_event {
  event: str                                 // "message" when the event field is empty
  data: str                                  // data lines joined by one LF
  last_event_id: str                         // persistent latest valid id, initially empty
  retry_ms: Option<i64>                      // persistent latest representable retry, initially None
}
// Request/response building
r := http.request(method: str, url: str)    // builder (Move — owns header list + body buf)
r.max_response_body_bytes(limit: i64)       // 0 inherits the client; positive limits only narrow it
r.header(name: str, value: str)
r.body(data: bytes)
resp.status() -> i64
resp.header(name: str) -> Option<str>       // view into resp
resp.body() -> bytes                         // view into resp (region-bound)
// Server primitive (not a framework) — surface settled 2026-07-10 (two-lens design review)
srv := http.serve(host: str, port: i64) -> Result<http_server, Error>
srv := http.serve_shared(host: str, port: i64) -> Result<http_server, Error>
                                             // the prefork sibling: same bind + SO_REUSEPORT, so N
                                             // workers each own a listener on ONE port (item 9 ①)
srv.accept() -> Result<http_request_ctx, Error>   // one request; caller writes the response.
                                             // Yields the next request off a KEPT-ALIVE connection
                                             // before accepting a new one (item 9 ②) — same surface
ctx.method() -> str                          // view into ctx (region-bound)
ctx.path() -> str                            // view into ctx (region-bound)
ctx.headers() -> http_headers                // the parsed header table as a Copy, non-owning VIEW
                                             // (region-bound to ctx, like ctx.body(); item 10).
                                             // A struct field type, so a Copy per-request context
                                             // can carry it — `http_headers` is a GLOBAL type name
hs.get(name: str) -> Option<str>             // RFC 9110 §5.1 case-insensitive lookup; the returned
                                             // str views the request buffer and INHERITS `hs`'s
                                             // region (so a wrapper taking the view through a
                                             // parameter compiles — item 10 ④)
ctx.body() -> bytes                          // view into ctx (region-bound)
rb := http.response(status: i64)             // response_builder (Move — owns header list + body buf;
                                             // the build-dual of `request`; named apart from the
                                             // parsed read-view `response`)
rb.header(name: str, value: str)             // bound receiver; CR/LF/NUL aborts (P6)
rb.body(data: bytes)                         // optional — a bodiless response is legal and frames
                                             // as Content-Length: 0 (except 1xx/204/304)
ctx.respond(rb) -> Result<(), Error>         // consumes BOTH ctx and rb; one-write serialize (R4);
                                             // PARKS an eligible 1.1 connection for keep-alive (no
                                             // Connection header), else closes the accepted fd;
                                             // a HEAD request gets the body SUPPRESSED, its
                                             // Content-Length kept (RFC 9110 §9.3.2; W4)
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
- `http_read_stream` and `http_sse_stream` are dependent **Move** resources. Each owns one checked-out
  plaintext/TLS connection plus bounded decoder state and retains a shared borrow of the creating
  `client` so exact completion can return the connection to that client's pool. The client may serve
  other shared-borrow request operations while a stream lives, but it cannot be moved or dropped
  before the stream. Both types are nameable as bare locals/parameters/returns. Their only storage
  carriers are the finite, acyclic grammar `C ::= stream | Option<C> | Result<C,N> |
  Result<N,C> | Result<C,C>`, where `stream` is either dependent stream and `N` is any otherwise
  valid type whose storage graph contains no stream. Only the active builtin tag owns a stream.
  Every other storage edge is rejected by default if its descendant graph contains one: this
  includes user structs/sums, anonymous tuples, collections, boxes/builders/tasks, closure
  environments, and parallel elements/results. A noncapturing function-value signature may name
  `C` as a parameter or result because the function value stores no stream; a capture may not.
  Generic substitution is checked on the complete concrete graph. The classifier exhaustively
  matches every `Ty`/`Scalar` variant with no wildcard, so a future constructor fails the compiler
  tripwire until it is classified. `request_stream` produces the first stream inside `Result`;
  `sse()` transfers the same owner and borrow into an `http_sse_stream` and nulls the source.
- `http_sse_event` is a compiler-provided Copy record. Its three `str` fields view the fresh
  generation written into the caller's `out` buffer by `next`; they cannot survive the next mutable
  use, replacement, or Drop of that buffer. The record owns no allocation and has no Drop.
- Move-rejection at the `scalar_arg` choke point except own-constructor Result Ok positions (net
  template).

## Effect classification

HTTP operations that connect, accept, send, receive, finish, reject, or otherwise touch a socket are
**Impure**. In-memory constructors, parsers, builder/setter operations, and getters retain their
existing **Pure** classification. For the streaming surface specifically, `request_stream`, `read`,
and `next` are Impure; `sse`, `status`, `header`, `last_event_id`, and `retry_ms` are Pure. The stream
types are not legal parallel elements/results or closure/task captures, so this ownership-only
transition and the Pure getters cannot smuggle a live connection into a Pure parallel closure.

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
   rejected (CL-duplication smuggling guard). The original slice rejected `chunked`; Request 4 now
   de-chunks supported HTTP/1.1 responses through the same R1-honouring decoder. Caps: ≤ 128 headers,
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
   is `Error.Invalid` at request time (P1 — never a silent plaintext downgrade). Framing supports
   Content-Length, HTTP/1.1 chunked, and read-to-close; Request 4 replaced the original CL-only path.
   The parser
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
   `ctx.method()/path()` (`str` views), `ctx.headers()` (a Copy `http_headers` view of the parsed
   header table; `hs.get(name)` is the case-insensitive `Option<str>` lookup — item 10, which
   REPLACED `ctx.header(name)`), `ctx.body()` (`slice<u8>` view) — all region-bound to `ctx` (#297); `http.response(status)` ->
   `response_builder` (Move, distinct Ty + display name from the parsed `response`) + `rb.header(name,
   value)` (bound receiver, P6 CR/LF/NUL **abort**) + `rb.body(data)` (optional); `ctx.respond(rb) ->
   Result<(), Error>` (**consumes BOTH** ctx and rb — MIR nulls both slots like `cl.request(req)`;
   serialize = status line + headers + auto Content-Length (0 for a bodiless body-allowed status);
   ONE write, R4;
   MSG_NOSIGNAL/SO_NOSIGPIPE; closes the fd, v1 one-request-per-conn). **W4 (2026-07-21):
   `respond` to a HEAD request suppresses the body bytes and keeps their `Content-Length` (RFC
   9110 §9.3.2) — enforced at the protocol boundary so any caller answering HEAD through a bodied
   builder (incl. pkg.web's HEAD→GET routing) is RFC-correct by construction; `respond_stream` /
   `reject` are unchanged (a stream has no HEAD form).** The **NEW**
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
   - **Auto-header policy (mirror of client serialize):** auto `Content-Length` on every response
     whose status may carry a body — the set length, or `0` when no body was set (amended 2026-07-21
     with keep-alive: an unframed response means "read until close", which forbids a persistent
     connection and there is no legitimate use for it — close-delimited framing is
     `respond_stream`'s 1.0 mode). `1xx`/`204`/`304` carry no body and get no framing header;
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
   truncation / poison / Align-client chunked dogfood / the double-consume + bodied-abort
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
   - Request 4 later removed the original client asymmetry: Align's own whole-body client now
     de-chunks the streaming server response and exposes the decoded payload.

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

9. **Prefork listener + server-side connection keep-alive — DESIGNED + SHIPPED 2026-07-21**
   (consumer = pkg.web concurrent serve, `pkg-design/web.md` → "Concurrent serve").
   Two std changes; keep-alive lands first (independently testable on the v1 sequential loop).
   - **① `http.serve_shared(host, port) -> Result<http server, Error>`** — identical to
     `http.serve` plus `SO_REUSEPORT` on the listener, so N workers each bind their OWN listener
     on one port and the kernel balances connections. A SIBLING op, not a flag: `http.serve`
     keeps strict-bind semantics (an accidental second server must still fail loudly; port
     sharing is an explicit choice — the `respond`/`respond_stream` sibling precedent, no bool
     traps). Portability: Linux balances properly; macOS accepts the option for TCP with
     unspecified distribution quality — record, don't gate (the bench box is Linux).
   - **② Connection keep-alive, entirely inside `accept`/`respond` — the loop shape is
     unchanged for every caller.** A **bounded parked SET per server handle** (256 connections;
     at capacity the least-recently-served one is closed to make room). The design opened with a
     single slot — "the v1 one-conn-at-a-time posture, made explicit" — and that was **corrected
     during implementation**: with one slot, every new connection evicted the previous one, so a
     client that had just been told the connection is persistent lost its next request (a failure
     a client cannot safely retry for a POST). Serving is still strictly one request at a time;
     the parked connections are idle, not in flight:
     - **Eligibility** (computed at parse time, carried on the ctx): the request is HTTP/1.1,
       has no `Connection: close` header, and left **no residual bytes** past its own body in
       the parse buffer (a pipelining client is answered then closed — residual carry-over is
       deliberately NOT built; real keep-alive clients await the response, so residual ≈ never).
       1.0 keep-alive (legacy `Connection: keep-alive`) is not supported — close, as today.
     - **`ctx.respond`**: eligible + write succeeded → the fd is PARKED into the server's slot
       instead of closed, and the auto `Connection: close` header is **omitted** (absence = 
       persistent is the 1.1 default; fasthttp does the same — leanest bytes on the bench path).
       Ineligible → `Connection: close` + close, exactly today.
     - **The RESPONSE is now always self-delimiting** (settled while implementing; RFC 9112 §6.3).
       `respond` used to emit `Content-Length` only when a body was SET, so a bodiless `200` was
       framed "until the connection closes" — un-keep-alive-able, and indistinguishable on the wire
       from a truncated stream. **Amended outright:** a response whose status may carry a body is
       framed either way (the set length, or `0` when no body was set); `1xx`/`204`/`304` carry no
       body and get no framing header; a body SET on one is **rejected** (`Err`, the same treatment
       a caller-supplied `Content-Length` gets in the same function) rather than silently dropped —
       such a response is terminated at the first empty line whatever its fields say, so those bytes
       would be read as the START OF THE NEXT RESPONSE on a kept-alive connection, and silently
       discarding caller data is the one thing this boundary must not do. `respond_stream` rejects
       the same statuses (a stream cannot follow a response that has already ended). HEAD stays
       silent suppression — that one is driven by the REQUEST, which the builder cannot see.
       Keep-alive therefore depends only on the REQUEST, and bodiless responses (`web.status(201)`)
       stay on the connection.
       `respond_stream`, `reject`, and
       every error path keep today's close-always semantics (a stream's terminator is its close;
       the reject window is an error path — recorded, not worth a second framing mode).
     - **`srv.accept()`**: nothing parked → plain `accept(2)`, as today. Otherwise
       `poll({…parked, listener}, infinite)`; a parked connection readable → claim it out of the
       set and parse the NEXT request from it (a fresh parse buffer — zero-copy views stay
       per-request); listener readable → take the new connection, **leaving the parked set
       untouched** — the set is bounded where it grows (`respond` evicts the coldest when parking
       into a full one), so accept needs no valve of its own, and one there would also fire for
       connections that never join the set at all. Genuine descriptor pressure is answered by the
       `NoFds` path below, which spends a connection only when `accept` actually ran out of fds.
       (An accept-time valve WAS shipped in #595 and removed in #597 for exactly this reason: it
       killed a warm connection for every `Connection: close` or malformed request that arrived at
       capacity, permanently shrinking the warm set, and no test observed it. Its one real service —
       reaping a parked connection whose peer vanished WITHOUT a FIN, which is silent and so never
       reported by `poll` — is now `SO_KEEPALIVE`, set on every accepted connection: the kernel's
       probes turn such a connection into a hangup this loop closes. Hours, not milliseconds, but
       bounded; under real descriptor pressure `NoFds` reclaims immediately. The cost of the removal
       is the worst-case descriptor count per worker: MAX parked **+ 1** in flight.) The readiness
       scan starts at a **rotating** index
       rather than always preferring parked: a "parked first" scan lets busy keep-alive clients
       starve the listener outright — with its own `SO_REUSEPORT` queue no sibling worker can
       drain it, so new connections would sit until the backlog dropped SYNs. Parked EOF / parse
       error → close that one, look again. No idle timeout: idle parked fds simply wait in
       `poll`, which IS accept's normal idle state. `POLLNVAL` is watched alongside
       `POLLHUP`/`POLLERR` — without it an invalid fd would report a revent no branch matches and
       the wait would spin. The `accept` surface and `Result` are unchanged.
     - **A malformed request no longer surfaces from `accept` at all** (corrected here — it was
       the pre-keep-alive behaviour too, and prefork made it fatal). A smuggling/bare-LF/TLS-to-a-
       plaintext-port request is a PER-REQUEST fault while the listener is perfectly healthy, so
       `accept` closes that connection and keeps waiting — exactly what the parked path already
       did. Returning it handed every caller a `Result` that killed the accept loop
       (`srv.accept()?`) the first time a scanner connected; with prefork, one such connection
       per worker took the whole server down. Only a real `accept(2)` failure returns an error,
       which is what makes `srv.accept()?` correct in a serve loop.
     - **Nor does a transient `accept(2)` errno** — the same argument, applied to the syscall
       itself. One classification (`classify_accept_error`) decides all of it, in three cases:
       - **Noise → `Again`.** `EINTR`; **`ECONNABORTED`** (the client vanished between its SYN and
         the `accept`, so the connection that would have been returned no longer exists — and
         otherwise a client could kill a worker by connecting and immediately resetting); and, on
         Linux, the **already-pending network errors** accept(2) explicitly says to "treat like
         EAGAIN by retrying" (`ENETDOWN`, `EPROTO`, `ENOPROTOOPT`, `EHOSTDOWN`, `ENONET`,
         `EHOSTUNREACH`, `EOPNOTSUPP`, `ENETUNREACH`) — each describes THE CONNECTION, not the
         listener. That last group is the one that actually fires on Linux, which usually completes
         the handshake and reports a reset later; `ECONNABORTED`-from-`accept` is mostly a BSD
         event. **`Again` returns to the WAIT, never to an immediate re-`accept`** — the listener
         is blocking, so retrying in place would park the thread there and stop the parked
         keep-alive connections (which share that one `poll`) from being served until an unrelated
         new connection arrived. `http_accept_conn` therefore performs exactly ONE `accept` and the
         retry lives in the caller's loop.
       - **`EMFILE`/`ENFILE` → `NoFds`**, recoverable descriptor exhaustion: `accept` gives a
         descriptor back and retries. **Which one it spends is chosen from this wait's `revents`:**
         the coldest parked connection *with no readable request* — closing one whose next request
         has already arrived would drop a request the client never sees answered — falling back to
         the coldest outright if every one is readable, because an exhausted table must still make
         progress. It is **paced at one connection per 10 ms of waiting** (`http_yield_for_fds`):
         prefork workers share the process descriptor table but each owns a SEPARATE parked set, so
         the descriptors a worker lacks are usually a sibling's (and `ENFILE` is system-wide, where
         giving back our own may not help at all) — unpaced, one worker would burn its entire warm
         set in a tight loop over pressure it did not cause. The pacing state is per `accept` call,
         which is exactly where a burn-down could run: a call that keeps failing to accept cannot
         spend a second connection without first waiting. (It resets when the call returns — but a
         call returns only by handing back a request or by failing `Fatal`, and neither is a loop.)
         With nothing left to spend it just backs off.
       - **Anything else → `Fatal`**, returned unchanged: a genuine listener-level fault is the
         only `accept` failure a serve loop should ever see.

       The result is a degradation (a warm connection is spent, paced, to serve the waiting request)
       where the server previously died. The **noise half of the rule is one
       predicate shared with std.net's `tcp_accept`** — it had the identical hole, and an accept
       loop is an accept loop. Exhaustion is NOT shared: a raw listener holds no parked set to give
       back from, so `net`'s caller keeps that decision.
     - **An interim (`1xx`) response never parks** — it is not a complete response, so a client
       that got one waits for the final one; the connection closes, as before keep-alive.
     - **Drop-order safety (the one sharp edge):** the ctx must not park into a freed server.
       The set is a runtime-internal refcounted cell (`Arc<Mutex<ParkSlot>>`) held by the
       server handle AND cloned into each ctx at accept — one refcount bump per request, no
       user-visible allocation, uncontended by construction (prefork gives every worker its own
       server handle, so the mutex never crosses threads). Server dropped first → the cell is
       marked dead and `respond` just closes; ctx dropped first → refcount releases. No sema/
       region surface is added for a runtime lifetime detail (rejected: tying ctx's region to
       srv — heavier, and wrong for the free-standing-handle model shipped in item 4).
   - **Test matrix (spec):** two requests over one connection E2E (same socket, both 200, views
     correct per request); `Connection: close` request honored; 1.0 closed; pipelined
     (residual) request answered-then-closed; the coldest parked fd evicted when a new connection is
     PARKED into a full set (arriving is not enough — see the valve note above); parked EOF
     recovery; stream/reject conns always closed; HEAD (suppressed body) + keep-alive compose;
     keepalive × pkg.web serve E2E (loop unchanged); `serve_shared` double-bind succeeds while
     plain `serve` double-bind still fails; prefork E2E — W workers, concurrent clients, one
     held-open stream while others answer.
   - **Shipped record.** Runtime: `SO_REUSEPORT` behind a shared `tcp_listen_impl(…, reuseport)` +
     `align_rt_http_serve_shared`; the parked set as `Arc<Mutex<ParkSlot>>` (`Live(Vec<fd>)`/`Dead`,
     the latter set by `HttpServer::drop`, which also closes every still-parked fd); eligibility
     computed in `http_read_request` (`http_request_wants_close` + the residual check) and carried on
     the ctx; `align_rt_http_accept` restructured around `http_wait_parked_or_listener` (a new
     `poll(2)` extern) + `http_accept_conn` (exactly ONE `accept` per call), whose failures go
     through `classify_accept_error` (`Again`/`NoFds`/`Fatal`, its noise half the
     `accept_errno_is_noise` predicate shared with the net rail) and, on exhaustion,
     `http_yield_for_fds` → `http_relieve_fd_pressure`;
     `http_serialize_head(rb, persistent, extra)` omitting the
     `Connection` line on the keep-alive path. Language: `ExprKind::HttpServe`/`Rvalue::HttpServe`
     gained a `shared: bool` (a FIELD, not a variant — every analysis pass keeps treating it as
     `http.serve`), and `http.serve_shared` dispatches through the same `check_http_serve`. Tests:
     11 runtime keep-alive units (two-requests-one-connection, the three ineligibility rules, HEAD
     composition, new traffic NOT evicting a parked connection, the park-time capacity valve evicting
     the coldest — and a one-shot request at capacity evicting NOBODY (#597, the removed accept-time
     valve), parked-EOF recovery, fd hygiene, bodiless-response framing, a bodiless STATUS
     rejecting a set body, an interim response never parking, and four clients parked at once) +
     the `serve_shared` double-bind unit + the three `accept`-errno units (the classification table
     including the pending-network family; the reclaim choosing an idle connection over one with a
     request waiting, and the one-per-backoff pacing; and an out-of-process descriptor-exhaustion
     E2E under a lowered `RLIMIT_NOFILE` — with the table full the parked connection is reclaimed
     and the pending request still answered, guarded against libtest's exit-0-on-no-match by
     asserting the child's own "1 passed" summary);
     driver `m11_http_server.rs` (`serve_shared` E2E + gates),
     `apps_web_root.rs` (keep-alive × the pkg.web loop, a second client not costing the first its
     connection, and a malformed request not killing the loop), and `apps_web_prefork.rs`
     (concurrent clients; one listener per worker read out of `/proc/net/tcp`; a held-open stream
     occupying one worker while the others serve; the out-of-range `workers` aborts).
   - **Behavioral note for callers/tests:** an eligible HTTP/1.1 request now leaves the connection
     OPEN, so a client that reads to EOF blocks until the server exits or capacity pressure evicts
     the parked connection. One-request-per-connection clients must say `Connection: close` (the
     driver tests' shared `one_shot` helper) or frame their read by `Content-Length`.

10. **`ctx.headers()` — the detached header-table view — SHIPPED 2026-07-21** (branch
    `http-headers-view`; consumer = `pkg.web`'s `web.header(c, name)`, `pkg-design/web.md` → ctx
    accessors). The design below is the record; ①–⑨ shipped as written, and **What actually shipped**
    at the end records the four places implementation taught something the design did not say.

    **The problem, precisely.** A framework's per-request context is a **Copy struct of views that
    owns nothing** — pkg.web's `Ctx` exists in that shape for load-bearing reasons (an owning `Ctx`
    would be consumed by its own accessors, and a failed handler would have consumed the handle the
    framework still needs to answer 500 through). Every other accessor rides a view the struct can
    carry: `method`/`path`/`query` are `str`, `body` is `slice<u8>`. **A header lookup cannot,
    because the name is not known until the handler asks** — the value being borrowed is the whole
    parsed table, not one span. So either the framework re-implements RFC 9110 lookup over a raw
    head view (a second implementation of something std.http already has — against One way), or
    std.http hands out a value that IS the table, borrowed. This is that value.

    - **① Surface.** `ctx.header(name)` is **replaced** — not supplemented — so the lookup has one
      spelling:
      ```text
      ctx.headers() -> http_headers        // a Copy, non-owning VIEW of the parsed header table;
                                           // region-bound to ctx exactly like ctx.body()
      hs.get(name: str) -> Option<str>     // RFC 9110 §5.1 case-insensitive lookup; the returned
                                           // str views the request buffer, region-bound to `hs`
      ```
      `ctx.header(name)` becomes `ctx.headers().get(name)` at every call site (there are no Align
      ones outside the docs — this costs nothing today and buys one mechanism forever). The parsed
      **response** keeps `resp.header(name)`: nothing needs to detach it from a value the caller
      already owns, and a shared view type would have to carry a discriminant for two unrelated
      runtime structs. That asymmetry is deliberate and recorded here rather than papered over.
    - **② Representation: the ctx pointer itself.** `align_rt_http_ctx_header` already takes
      `*const HttpRequestCtx` + the name and writes an `AlignStr` — so the view is *the same
      pointer*, and `hs.get(name)` lowers to **the existing call**. **No runtime code is added at
      all**; `ctx.headers()` lowers to `Rvalue::Use` of the ctx operand. The whole enabler is a
      type-system change.
    - **③ The type: `Ty::HttpHeaders` — Copy, non-owning, region-tracked.** The precedent to copy is
      `Ty::JsonDoc`/`Ty::JsonScanner` (Copy + `tracks_region` + `ty_may_borrow`), not the Move
      handles. It is a bare 8-byte pointer, which no Copy type is today, so `ty_size_align` needs its
      own `(8, 8)` arm rather than the `(16, 8)` catch-all.
    - **④ Region semantics — the one line the whole design turns on.** `region_of`'s
      `HttpCtxMethod | HttpCtxPath | HttpCtxHeader | HttpCtxBody` arm caps the result at
      `Frame.shorter(region_of(ctx))`. Inherited by the lookup, that cap makes
      `fn header(c: Ctx, name: str) -> Option<str> = c.headers.get(name)` — the pkg.web wrapper, and
      the whole point — **reject at compile time** ("cannot return a view that borrows local
      storage"). Verified on today's compiler with the equivalent `ctx.header` wrapper. So the two
      operations must be split:
      - `ctx.headers()` (**new** `ExprKind::HttpCtxHeaders`) keeps the cap: `Frame.shorter(region_of(ctx))`.
        A view minted from a local handle cannot leave the frame that owns the handle.
      - `hs.get(name)` (the **existing** `ExprKind::HttpCtxHeader`, its operand re-pointed from the
        handle to the view) **inherits**: `region_of(hs)`. Through a parameter — where the caller
        provably outlives the call — that is `Static`, and the wrapper compiles. This is exactly the
        rule `str`/`slice` views already follow through parameters; it is not a new exception.
    - **⑤ The soundness checklist a new `Ty` does NOT get for free.** Adding a `Ty` variant is
      compiler-forced through **four** passes (`ty_mentions_slice`, `tracks_region`, and the two
      `ty_name`s). Everything else is a `matches!` list or a `_ =>` arm that fails **open**. Three
      are fatal if missed, and the first two are a PAIR — either one alone produces the same silent
      use-after-free (`hs := ctx.headers()` … `ctx.respond(rb)?` … `hs.get("host")` reading a freed
      buffer):
      - **`ty_may_borrow`** — without it the `Let` records no borrow provenance for the view at all.
      - **`borrow_sources_inner`** — its tail is `_ => BorrowRoots::new()`, so a new `ExprKind` is
        NOT forced here even though the other eight passes are exhaustive. `HttpCtxHeaders` must map
        to the ctx's storage roots. (The existing `HttpCtxHeader` arm already reads
        `storage_roots(operand)` and needs no change — `storage_roots`'s `_ => borrow_sources(e)`
        fallthrough then chains a temporary view correctly, given the two additions above.)
      - **`scalar_type`'s pointer arm** — miss it and the `_ =>` falls through to `int_type`'s
        `_ => i32`, silently truncating a pointer. This exact bug already happened once for
        `Ty::Fn`.
      Then: `is_field_ok` (or `Ctx` cannot carry it), `resolve_type` — as a **global surface name**,
      no import required, like `http_request_ctx`/`response_builder`/`http_stream`, so that
      `pkg.web.types` stays the dependency-free leaf it is designed to be — and `ty_size_align`.
      **`ty_size_align` is lint precision, not safety:** its only consumer is the huge-struct-copy
      lint; the real layout comes from `scalar_type` + `field_abi_align` (`_ => 8`, already right).
      Note `Ty::HttpRequestCtx` — a shipped struct-field type — has the same 16-vs-8 over-report
      today; fix both, and add rows to `sema_and_codegen_struct_layout_agree`, which is a
      hand-written table with no row for a Move-handle, `Ty::Fn`, or `Ty::Slice` field either.
      Deliberately **not** added: `ty_is_move`, `is_owned_droppable`, `handle_free_fn`,
      `null_moved_source`, the `DropPlan` owned-leaf set (it must not make its enclosing struct Move), and —
      importantly — **no `Scalar` variant**, which keeps the view out of `Option`/`Result` payloads
      and array elements by fail-closed default (worth a tailored diagnostic: today it reports
      "must be a scalar (composite payloads are not supported yet)", which is the right answer with
      the wrong story).
    - **⑥ Dispatch and effect, both easy to get wrong.** `"get"` is already claimed by a catch-all
      arm (`"get" if recv_ty != Ty::HttpClient => check_box_get`), which would swallow `hs.get(name)`
      into a *"'get' takes no arguments"* diagnostic — a bad message, not a build failure, so nothing
      catches it. The new arm goes **above** it, exactly where the `json.doc` arm sits for the same
      reason. The receiver **place-gate** (`Local | Field`) that `check_http_ctx_method` applies must
      be kept for `ctx.headers()` (it still rejects `srv.accept()?.headers()`) and **not** inherited
      by `hs.get(name)` — the mandated spelling `ctx.headers().get(name)` has a non-place receiver,
      and the view owns nothing to drop. MIR routes through the `lower_http` dispatch list, not a new
      inline arm in `lower_expr` (the `expr_depth` headroom note, #296). Effect: **Pure** — a pointer
      copy, and the lookup is a read-only scan of an immutable buffer, which is what lets a handler
      reading headers stay legal under `par_map`/`task_group`.
    - **⑦ Not in v1: iteration.** `hs.count()`/`hs.name(i)`/`hs.value(i)` would serve a proxy that
      forwards every header, and the runtime already has the spans. It is deferred, not rejected:
      lookup is the REST need, and each accessor is another node in a `Ty` sweep that fails open. If
      a consumer needs enumeration, it is three sibling nodes on the same view — no new type.
    - **⑧ Alternatives, rejected with reasons.** *Pre-extract the headers into a `slice<str>` field*
      (no new `Ty` at all) is the strongest one and loses twice: the runtime holds offset spans, not
      `AlignStr`s, so materializing the slice is **an allocation per request** — on the 4.1 µs budget
      that is the current perf target — and the lookup would then live in pkg.web, a second RFC 9110
      implementation. *Let `Ctx` borrow the `http_request_ctx`* is dead on arrival: the handle is
      Move, so `Ctx` becomes Move and every reason recorded in `types.align` reappears.
      *Generalize to a full detached request view* (`.method()/.path()/.body()` on one value) is the
      natural "shouldn't this generalize?" question and loses today: those three already work on the
      handle, pkg.web's `path`/`query`/`pattern` are DERIVED rather than passed through, so `Ctx`
      would not collapse anyway, and each extra accessor is another node through a sweep that fails
      open. The name stays header-shaped because that is what it is for.
    - **Test matrix (spec):** wrapper-through-parameter compiles and returns the view (the property
      that motivates the split); a view minted from a LOCAL handle cannot be returned, nor `break`
      out of a loop, nor survive its ctx across a `serve` iteration; `hs.get()` after
      `ctx.respond(rb)` is a compile error — on a **bare local**, since any `str` field in an
      enclosing struct supplies a borrow root and masks the hole; `hs.get()` inside a **stream pump**
      AFTER `ctx.respond_stream(rb)` must **compile and work** (that path borrows the ctx and never
      frees it); case-insensitive hit + miss (`Option`) E2E through pkg.web; the view as an array
      element / `Option` payload is rejected; a `Ctx` carrying it stays Copy (no drop emitted, no
      double free) with a struct-layout row asserting sema and codegen agree.
    - **⑨ Removal sweep for `ctx.header(name)`.** Zero Align call sites anywhere (no `.align` file,
      no Rust-embedded test program) — confirmed. What changes: this file (§ surface + Slice
      breakdown), its ja mirror, `docs/open-questions.md`, and the compiler sites — the method
      dispatch arm, `check_http_ctx_method`'s `"header"` case, and the "try method / path / header /
      …" suggestion string. `rb.header` / `r.header` / `resp.header` are different receivers and stay.
    - **What actually shipped (2026-07-21) — including where the design was incomplete.** ①–⑧ are as
      designed: `Ty::HttpHeaders` (Copy, `tracks_region`, `ty_may_borrow`, no `Scalar`, its own
      `(8, 8)` arm), `ExprKind::HttpCtxHeaders` lowering to `Rvalue::Use`, the existing
      `HttpCtxHeader` node re-pointed at the view, the region split (`Frame` cap on `headers()`,
      inherit on `get`), the `hs.get` dispatch arm above `check_box_get`, Pure, and **zero** new
      runtime code. `Ty::HttpRequestCtx`'s 16-vs-8 over-report is fixed with it, and
      `sema_and_codegen_struct_layout_agree` gained rows for a Move-handle field, a `http_headers`
      field, a `Ty::Fn` field, a `slice<T>` field and the pkg.web `Ctx` shape itself. Four
      corrections to the design's own account:
      - **⑤ under-counts the compiler-forced passes for the new `ExprKind`.** `region_of` and
        `slice_is_local` are both exhaustive over `ExprKind`, so the *region* rule — the one the
        whole design turns on — is **fail-closed**, not fail-open. The fail-open surface is only
        what ⑤ names at the `Ty` level (`ty_may_borrow`, `scalar_type`) plus the one `ExprKind` tail
        it correctly identifies (`borrow_sources_inner`). Each was mutation-checked: dropping
        `ty_may_borrow` or the `borrow_sources_inner` arm is caught by the bare-local
        use-after-`respond` test **and** the cross-iteration test; dropping the `scalar_type` arm
        breaks every pkg.web E2E; dropping `tracks_region`, `region_of` or `slice_is_local` fails
        the build.
      - **⑤'s "tailored diagnostic" needed TWO sites, not one.** There are two `payload_scalar`s —
        a free one (with a `what` label) and a `Checker` method — and the method one hardcoded
        `"Option payload"` for every caller, so an *array element* rejection reported itself as an
        Option payload. The method now takes the position it is checking, which fixes that
        pre-existing mislabel for every type, not just this one.
      - **The test matrix's "cannot survive its ctx across a `serve` iteration" was only half true,
        and the other half was a pre-existing hole — FIXED 2026-07-22, not in this slice.**
        `MoveCheck` used to end a borrow generation when the owner was **moved or reassigned**, not
        when it was **dropped at the end of a loop iteration**, and `Region::Frame` cannot tell "this
        frame" from "this iteration". The pkg.web shape was always safe because `ctx.respond(rb)`
        MOVES the handle every pass — that case was already rejected. A loop body that merely lets
        the ctx drop leaked the view into the next pass, general to every view over a Move handle
        (reproduced identically on a plain `str` from `ctx.path()`, and on a plain `string` with no
        std handle in sight). `loop_moves` now ends the generation of the loop's per-iteration drop
        set at the back-edge and at every `break`; see `docs/open-questions.md` next to #460. **Two
        claims here were wrong**: the `arena {}` variant is not an instance of the bug at all (a
        heap-owned local inside `arena {}` is dropped at *function* exit — `emit-mir` shows the drop
        after `arena_end` — and genuinely arena-allocated storage is already caught by the region
        rule), and this type is not reliably louder than a `str`: both are shape- and
        allocator-dependent UB.
      - **⑨'s suggestion string is unreachable for the removed name — so the removed name got its
        own arm.** The ctx-method dispatch arm is name-guarded (`"method" | "path" | "headers" | …`),
        so `ctx.header(x)` never reaches `check_http_ctx_method`, and the "try …" list it would have
        landed in is not where a caller of the old spelling arrives; the generic *"unknown method"*
        was. Review round 1 asked for the hint to be real, so a `"header" if recv_ty ==
        Ty::HttpRequestCtx` arm now **errors** with the replacement spelled out — it resolves
        nothing, so it is a diagnostic, not a compat path. (The suggestion string was updated too; it
        still serves an unknown method that IS in the guard list.)

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
  - **→ RESOLVED (align-llm Request 2). See "I/O timeouts" below.** align-llm (LLM API calls to
    endpoints that can black-hole a connection) supplied the concrete client demand and the missing
    configuration surface. The net-rail non-blocking-`connect` substrate + `SO_RCVTIMEO`/`SO_SNDTIMEO`
    and a per-client-default / per-request-override `timeout(ns)` — the two things this note said were
    missing — are **IMPLEMENTED** (the G3-1 deferral is fully resolved on the client). The
    server-side escalation above remains its own post-v1 hardening item.
  - **Sub-case — HEAD / 304 framing (inherited from Slice 1/2).** A `HEAD` response, or a `304 Not
    Modified`, legitimately carries a `Content-Length` header **but no body**. The v1 read loop frames
    purely by `Content-Length` (it does not special-case the request method or status), so it would
    wait for body bytes that never arrive → the same indefinite block as above. v1's surface does not
    expose `HEAD` conveniently (only `get`/`post`/`request`), but a caller-built `request` with method
    `HEAD` hits this. **DESIGNED for align-llm Request 4 below; implementation pending.** The same
    capability adds method/status-aware framing, informational-response advancement, and client-side
    chunk de-framing without adding a second transport surface.
- **~~`https://` rejection is coarse (DC-1, low).~~ RESOLVED by Slice 5.** `https://` no longer maps
  to `Error.Invalid` at all — it routes to the verified TLS path. A verification failure is now the
  distinct `Error.Denied`; a bad TLS transport is `Error.Code`; a protocol violation is
  `Error.Invalid`. (The message-less `Error` enum is still a broader story, but the specific DC-1
  "HTTPS not supported" debt is gone — HTTPS *is* supported.)

## I/O timeouts (align-llm Request 2 — DESIGNED 2026-07-24, IMPLEMENTED 2026-07-24)

Resolves the G3-1 deferral above. Motivated by `align-llm`'s LLM API calls (`POST
/v1/chat/completions`) to endpoints that can hang or black-hole a connection: without a timeout, one
request stalls the whole verify/repair loop indefinitely. Source:
`../align-llm/docs/align-requests.md` Request 2 (priority: high).

### Surface — one `timeout(ns)`, client default + per-request override

Both are in-place setters on the existing bound-local Move builders (the `.header()`/`.body()`
idiom — return `()`, no new argument on the frozen `get`/`post`/`request` signatures):

```text
cl := http.client()
cl.timeout(ns: i64)              // default timeout for every request on this client   -> ()
r := http.request("POST", url)
r.timeout(ns: i64)               // per-request override (0 = "use the client default") -> ()
```

`ns == 0` on the client means "no timeout" (current blocking behavior — backward-safe default). A
request's own `timeout` overrides the client's; an unset request timeout inherits the client's.
Negative `ns` is rejected at build time (abort, like `r.header()` on CR/LF).

**One knob, not connect/read/write separately** (owner criteria: one way, human-understandable). A
single `ns` is applied as the deadline for **each** blocking operation — connect, send, and receive —
so a peer that never accepts hits the connect bound and a peer that accepts then never responds hits
the receive bound. This is a per-operation deadline, **not** a single wall-clock deadline across the
whole request (which would need deadline arithmetic threaded through connect+send+recv and buys
little for this workload). Documented as per-op; it satisfies the gate (a hung request returns within
the test's bounded detector tolerance after reaching the logical wait deadline) with the simplest
surface. Scheduler/kernel delay is not a strict public wall-clock ceiling.

### A timeout is `Error.Timeout`

Consistent with "transport/TLS/malformed-message failures are errors; an HTTP status is data," a
timeout surfaces as the **`Error.Timeout`** variant (the shared new variant — canonical definition in
`process.md`'s "Shared prerequisite: the `Error.Timeout` variant"; `AL_TIMEOUT = 4`). It is distinct
from `Error.Code` (a transport errno), `Error.Denied` (a TLS verification failure), and a normal
`Ok(response)` carrying a 4xx/5xx status.

### Runtime design (raw-libc sockets; all hook sites already located)

- **connect timeout** — the net-rail substrate (net.md). `align_rt_tcp_connect` (currently a blocking
  `connect` at runtime `:679`, "no connect timeout" note at `:621`) gains a `timeout_ns` parameter:
  set the fd non-blocking, `connect` (expect `EINPROGRESS`), `poll(POLLOUT)` with the ns deadline,
  check `SO_ERROR`; on poll-timeout return **`AL_TIMEOUT`**. `timeout_ns == 0` keeps today's blocking
  path unchanged. `http_connect_fd` (`:14748`) passes the effective request timeout down.
- **read timeout** — `SO_RCVTIMEO` on the conn fd (`plain_read`/`read` at `:14001`; the TLS path's
  `SSL_read` uses the same fd, so the socket option bounds it too). Expiry yields `EAGAIN`/
  `EWOULDBLOCK`; at the read site that is converted to **`AL_TIMEOUT`** (not the generic errno path —
  we know it is our deadline).
- **write timeout** — `SO_SNDTIMEO` on the fd (`http_send_all`/`send` at `:14511`). Symmetric.
- The effective timeout is set on the fd right after connect (both plain and TLS), before the first
  I/O. A pooled keepalive conn reused for a new request re-applies the new request's effective
  timeout (the pool stores conns, not deadlines).

### New machinery

`Error.Timeout` + `AL_TIMEOUT` (shared, see process.md). `align_rt_tcp_connect` gains a `timeout_ns`
arg (net.md Slice, shared with `std.net`). A `timeout_ns` field on the client + request handles
(`HttpClient` / `HttpRequest` runtime structs) with the new setters `align_rt_http_client_timeout` /
`align_rt_http_timeout`, and sema `HttpClient`/`HttpRequest` method dispatch for `timeout`. The
effective-timeout resolution (request override else client default) happens in `http_client_perform`.

### As built (2026-07-24)

- **Language.** New HIR/MIR/codegen variants `HttpRequestTimeout { req, ns }` /
  `HttpClientTimeout { client, ns }` (`Ty::Unit`, i64 arg, bound-local receiver — the exact
  `r.body`/`c.timeout_ns` shape), swept through every sema pass (EffectScan — **Pure**, a field store;
  `region_of`→Static; `slice_is_local`; EscapeCheck walk/visit; MoveCheck — borrow-only, consumes
  nothing; `borrow_sources`; finalize) and lowered through `lower_http` → `align_rt_http_timeout` /
  `align_rt_http_client_timeout`. A negative `ns` aborts at the setter; a non-i64 arg is a type error.
- **Effective timeout.** `http_client_perform` resolves `req.timeout_ns > 0 ? req : client` (the
  client default is an `AtomicI64` so `get_many`'s shared workers read it safely) and threads it as
  ONE `ns` into: `http_connect_fd`/`http_tls_connect` (→ `align_rt_tcp_connect(…, ns, …)`) for the
  connect + handshake, and `http_arm_conn_timeout(fd, ns)` (both `SO_RCVTIMEO` + `SO_SNDTIMEO`) armed
  on **every** request — fresh AND reused pooled conn — before the exchange. **Re-arm/clear:** because
  `SO_*TIMEO` persists on a pooled fd, a reused conn is always re-armed, and `ns == 0` **clears** it
  back to a zero `timeval` = "block forever" (so a conn pooled by a timeout-armed request never carries
  a stale deadline into a later no-timeout request). A fresh conn with `ns == 0` skips the arm entirely
  (byte-identical to the pre-timeout path — no `setsockopt`).
- **Expiry → `AL_TIMEOUT`.** Plaintext `plain_read`/`http_send_all` map a blocking-fd
  `EAGAIN`/`EWOULDBLOCK` via `io_read_write_status` (a blocking socket yields `EAGAIN` only when a
  deadline is armed, so with `ns == 0` the mapping is byte-identical). **TLS** is the subtle case: a
  `SO_*TIMEO` expiry makes the underlying `recv`/`send` return `EAGAIN`, which OpenSSL surfaces as
  `SSL_ERROR_SYSCALL` **or** `SSL_ERROR_WANT_READ`/`WANT_WRITE` (version-dependent). `tls_read`/
  `tls_write_all` capture `errno` **before** `SSL_get_error`, and when a deadline is armed
  (`has_deadline`) key on `err ∈ {SYSCALL, WANT_READ, WANT_WRITE}` **and** `errno ∈
  {EAGAIN, EWOULDBLOCK}` → `AL_TIMEOUT` (otherwise `WANT_*` retries and `SYSCALL` maps its errno,
  exactly as before — so `ns == 0` is byte-identical, no spin). The handshake (`SSL_connect`) runs over
  the same armed fd and maps the identical condition to `AL_TIMEOUT`.

### Checked shared-timeout prerequisite (`pkg.kv` prerequisite 1 — IMPLEMENTED 2026-09-02)

> **Status:** implemented. The shipped timeout surface and ABI remain unchanged; this prerequisite
> changes no public signature, compiler operation, runtime symbol, ABI shape, registry key, or row
> count. The planned checked package row remains inactive.

HTTP connect inherits the checked net-rail transition recorded in `net.md`. For each usable
resolver address and positive effective timeout, the runtime records a monotonic start and positive
`Duration` budget immediately before the first `F_GETFL`, checks both that call and
`F_SETFL(flags | O_NONBLOCK)`, and closes/continues on either failure before `connect`. The immediate
call classifies zero as success, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` as in progress, and every other
errno as a terminal failure; either immediate terminal result wins over simultaneous budget
exhaustion. It never
forms an absolute `start + budget`, so `Instant::checked_add` overflow cannot turn any positive i64
timeout into an unbounded wait. Each positive remainder is rounded up to the next millisecond and
saturated at `i32::MAX` for one `poll`; an exhausted remainder returns `AL_TIMEOUT` before another
poll. `EINTR` recomputes the remainder. A zero poll result triggers a fresh monotonic check and
re-polls only while time remains. A positive readiness/error event already returned by `poll` wins
over simultaneous budget exhaustion and is resolved through `SO_ERROR`. Immediate and polled
success both require checked `F_GETFL` plus `F_SETFL(flags & !O_NONBLOCK)` restoration before HTTP
can receive the socket; restoration failure closes that candidate and continues to the next
address.

The same prerequisite makes each positive HTTP socket timeout use
`ceil(ns / 1000)` microseconds in a normalized `timeval { tv_sec, tv_usec: 0..999999 }` for both
`SO_RCVTIMEO` and `SO_SNDTIMEO`. Exact microseconds remain exact, the next nanosecond rounds up, the
maximum admitted value remains representable, and zero keeps the shipped clear/block-forever
meaning on a reused connection. These conversions do not expire a logical timeout early; scheduler
or kernel delay may still make an operation return after it.

Acceptance owners cover exact/next/maximum ns-to-ms and ns-to-us boundaries, failed checked mode
installation/restoration, immediate errno classes, `EINTR`, early-zero versus exhausted/no-call
poll behavior, readiness in flight at expiry,
re-arm/zero-clear on pooled connections, normalized `timeval` fields, and no early expiry. They
are active with the prerequisite and complement the already-shipped Request 2 tests above.
`http_timeout_quantization_plain_tls_pool_rearm` directly observes the normalized receive/send pair
on fresh plaintext, dependent response-stream, TLS-handshake, request-I/O, pooled rearm, and pooled
zero-clear paths.

### Test / gate

A request to an endpoint that **accepts the connection but never responds** reaches its configured
logical wait deadline and returns `Err(Timeout)` instead of blocking indefinitely (the Request-2
acceptance gate). A connect to a black-holed (never-accepting) address does the same. The tests use a
generous upper tolerance to detect a stall; that tolerance is not a public wall-clock guarantee. `ns == 0`
preserves the current blocking behavior. A normal fast request is unaffected. **Shipped tests:**
`align_runtime` units — setter store + request-overrides-client, accept-then-no-response → `AL_TIMEOUT`
(read path, `SO_RCVTIMEO` + plaintext mapping end-to-end), a full-backlog loopback black hole →
`AL_TIMEOUT` (connect path, Linux), and fast-response-unaffected — plus `crates/align_driver/tests/
http_timeout.rs` E2E through the Align surface (`cl.timeout` / `r.timeout` → `Err(Error.Timeout)` on a
silent server, per-request override, inert-when-fast, and the unbound-receiver / non-i64 compile
gates). The runtime owner above covers positive TLS handshake/request arms and pooled TLS zero-clear;
the positive TLS round-trip is not drivable from the driver harness because its `#[cfg(test)]` trust
hook is absent there, as recorded in Slice 5. The `has_deadline` mapping in `tls_read`/
`tls_write_all` is the one place the TLS transport differs, documented above. Every pre-existing
http/TLS/pool/get_many/stream test passes unchanged (the effective-0 byte-identical invariant).

## Client response framing (align-llm Request 4 — DESIGNED + IMPLEMENTED 2026-08-14)

`align-llm`'s C1 provider adapters already parse SSE events, but provider responses commonly use
HTTP/1.1 `Transfer-Encoding: chunked`. Before Request 4, the whole-body client rejected that framing
before the provider could see the body. Request 4 completes the existing `cl.request` boundary: it
de-frames a chunked response into the same owned `response` and zero-copy `resp.body()` view. It adds no public
type, method, option, ABI entry point, streaming-input handle, or provider-specific abstraction.

This is a framing capability, not a configurable receive-bound capability. The existing fixed
`HTTP_MAX_BODY` ceiling remains the decoded-body ceiling and an excess remains `Error.Invalid`.
align-llm Request 5 separately owns the public client/request cap and its limit-specific error. If
Request 5 lands after this capability, Request 5 owns their combined cap/framing adoption gate.

### Public-contract ledger

| Surface / state | Exact contract |
|---|---|
| Existing calls | `cl.get(url)`, `cl.post(url, body)`, `cl.request(req)`, and every `cl.get_many` worker use this framing engine. `http.parse(bytes)` uses the same complete-response decoder with ordinary method semantics because it has no request method. No signature changes. |
| Ownership and views | Success returns the existing Move `response`. It owns one retained byte buffer containing the exact final response head followed by the contiguous decoded body. Header spans still index the exact final-head bytes; `status()`, `header()`, and `body()` keep their existing lifetime and allocation rules. Interim heads, chunk lines, delimiters, and trailers are not retained. |
| Request-method selection | Only exact uppercase `HEAD` selects HEAD response semantics. Exact uppercase `CONNECT` is rejected as `Error.Invalid` before DNS, connect, write, pool access, or any other network side effect because this whole-body API cannot return a tunnel. Lowercase or mixed-case `head` and `connect` are ordinary extension methods. `http.parse(bytes)` uses ordinary non-HEAD semantics. |
| Response-head wire syntax | Every interim and final head uses exact CRLF. Its status line is exactly `HTTP/1.0 SP 3DIGIT SP [reason-phrase] CRLF` or `HTTP/1.1 SP 3DIGIT SP [reason-phrase] CRLF`; the code is `100..=999`, and a reason phrase contains only HTAB, SP, visible ASCII, or obs-text. Every field name is a nonempty RFC `token` immediately followed by `:`; field values contain only HTAB, SP, visible ASCII, or obs-text. Bare LF, an unknown version, extra/missing status separators, obs-fold, whitespace before `:`, or another control byte is `Error.Invalid`. A supported chunked transfer coding additionally requires HTTP/1.1. The whole head is validated before framing or body suppression is selected. |
| Informational responses | Every `100..=199` response except `101` is an interim head. It is validated, must contain neither `Content-Length` nor `Transfer-Encoding`, consumes no payload, is discarded, and parsing continues at the next co-read byte. Interim and final head wire spans, including each terminating empty line, share one cumulative `HTTP_MAX_HEADER_BLOCK = 262,144` byte allowance. Each head independently obeys `HTTP_MAX_HEADERS = 128`. `101` is `Error.Invalid`; no response or upgraded handle is exposed. |
| Bodyless final responses | A final response to exact `HEAD`, and final `204` and `304`, produces a zero-length body and performs no payload, chunk-terminator, or trailer read. Exact `HEAD` except at status `204`, and every `304`, may carry a syntactically valid `Content-Length` of any decimal magnitude or one supported `Transfer-Encoding: chunked` value as metadata; the exact final header remains discoverable. `204` permits neither field regardless of method. All heads still reject malformed/conflicting lengths, unsupported transfer codings, and simultaneous `Content-Length` plus `Transfer-Encoding` before suppression. |
| Transfer-Encoding | Combine all field values in wire order and split them on commas with optional SP/HTAB. The only supported sequence is exactly one case-insensitive `chunked` coding with no parameter. Empty elements, repeated `chunked`, another coding, or malformed token syntax are `Error.Invalid`. A supported transfer coding and any `Content-Length` together are `Error.Invalid`. |
| Content-Length / close-delimited | Existing decimal, equal-duplicate, fixed-cap, truncation, and close-delimited behavior remains. For payload-bearing responses, `Content-Length` is selected before close-delimited; a supported chunked field is selected before either. Read-to-close responses remain non-reusable. |
| Chunk-size line | `HTTP_MAX_CHUNK_LINE = 8,192` wire bytes includes the terminating CRLF. A line is one or more ASCII hexadecimal digits followed by zero or more RFC 9112 chunk extensions and exact CRLF. Extension names are RFC `token`; an optional value is a `token` or quoted-string with valid quoted-pair escaping. Bare LF, invalid grammar, missing CRLF within the guard, or a byte beyond the guard is `Error.Invalid`. Guard excess wins before syntax or magnitude state that depends on the excess byte; a syntactically valid magnitude above the fixed decoded-body ceiling is then `Error.Invalid` without target conversion. |
| Cumulative chunk framing | `HTTP_MAX_CHUNK_FRAMING = 262,144` wire bytes counts every chunk-size line including its CRLF and every CRLF following a nonzero payload, through and including the zero-chunk line. It excludes payload and trailers, which have their own limits. Bytes already co-read are charged before another read. A terminal zero line ending exactly at the allowance is accepted; a byte needed beyond it is `Error.Invalid` without an over-guard probe. This bound is independent of decoded length and therefore bounds many-tiny-chunk and extension work. |
| Chunk payload | A nonzero size is checked against cumulative decoded length and `HTTP_MAX_BODY` before target conversion, reserve, or another payload read. Exactly that many payload bytes followed by exact CRLF are required. Zero selects trailers. Truncation at the size line, payload, payload CRLF, zero chunk, or trailers is `Error.Invalid`; no partial response succeeds. Empty data chunks are impossible because zero is terminal. |
| Trailers | `HTTP_MAX_TRAILER_BLOCK = HTTP_MAX_HEADER_BLOCK = 262,144` wire bytes counts from the first byte after the zero-chunk line through the terminating empty CRLF. Trailer lines require exact CRLF, an RFC-token field name immediately followed by `:`, and a value containing only HTAB, SP, visible ASCII, or obs-text. `Content-Length` and `Transfer-Encoding` trailer fields are invalid. Trailer count consumes the unused portion of the final head's `HTTP_MAX_HEADERS` budget. Trailers are validated incrementally but never retained, merged, or exposed. A valid terminator ending exactly at the guard is accepted; recognizing one beyond the guard is invalid without an over-guard probe. |
| Errors and precedence | Request validation precedes network work. For each available head: exact wire syntax, `Content-Length` normalization/conflicts, and transfer-coding syntax/conflicts precede method/status body selection. Per-line and cumulative framing/trailer guards precede grammar state requiring an excess byte; complete in-guard grammar precedes decoded-size checks; decoded-size excess precedes payload/trailer reads. Malformed/truncated framing is `Error.Invalid`; an armed I/O deadline remains `Error.Timeout`; other transport/TLS failures keep their existing taxonomy. Every failure leaves the output slot null. |
| Connection fate | A bodyless, Content-Length, or terminal-chunk-plus-valid-trailers response is self-delimited. It is pooled only when the final head is keep-alive eligible, the Align scratch has no residual byte, and a TLS connection reports neither pending application bytes (`SSL_pending`) nor buffered record/plaintext work (`SSL_has_pending`). Chunk metadata and trailers must be fully consumed before pooling. Read-to-close, `Connection: close`, `101`, malformed/truncated framing, fixed-cap excess, any scratch residual, or either TLS-pending condition closes rather than pools. A zero-response-byte failure on a reused idle connection retains the existing one fresh retry; any response byte makes the failure non-retryable. |
| Plaintext / TLS | One parser and state machine consumes the existing `Conn` abstraction for both transports. The same read limits, errors, cleanup, pooling rules, and timeout behavior apply. TLS adds no plaintext-only staging or alternate framing path. |
| Allocation / performance | Header and chunk discovery use one `HTTP_CLIENT_READ_CHUNK = 32,768`-byte reusable scratch allocation. Let `HTTP_MAX_RESPONSE_LOGICAL = HTTP_MAX_HEADER_BLOCK + HTTP_MAX_BODY = 1,074,003,968`. The response accumulator uses exact-capacity boxed byte allocations, not `Vec`'s allocator-dependent reserve heuristic: each checked geometric target is at most `HTTP_MAX_RESPONSE_LOGICAL`, and the final allocation is converted allocation-preservingly into the response `Vec`. Its stored Rust capacity plus scratch therefore has a steady ceiling of `HTTP_MAX_RESPONSE_LOGICAL + scratch = 1,074,036,736` bytes. During growth, old capacity is strictly below the new capacity; old + new + scratch is strictly below `2 * HTTP_MAX_RESPONSE_LOGICAL + scratch = 2,148,040,704` bytes. That transient ceiling counts both simultaneously live Rust allocation layouts but excludes allocator bookkeeping and allocator-internal transition space outside those layouts. The accumulator grows only from validated final-head and decoded-payload progress, never a peer-declared size alone. Parser state is constant-size; no chunk table, trailer table, separately accumulated second body, or per-chunk allocation is permitted; the counted old/new pair exists only while geometrically relocating this single accumulator. Allocation failure is the existing fatal OOM behavior, not `Error.Invalid`. |
| Consumer acceptance | A provider fixture returns two SSE chunks and a terminal zero chunk; both OpenAI-compatible and llama.cpp adapters receive only their concatenated payload. Direct fixtures prove status/header/body preservation, every malformed/truncated case, bodyless and informational framing, plaintext/TLS parity, sequential reuse, and close-delimited non-reuse. |

The `Content-Length` arbitrary-magnitude allowance above is metadata-only for `HEAD` and `304`.
Normalize its decimal magnitude by removing leading zeroes and comparing digit sequences; do not
convert it to `usize`, compare it with `HTTP_MAX_BODY`, reserve from it, or read a body. Equal
duplicate values are equal by normalized magnitude (`003` equals `3`). Payload-bearing lengths keep
the existing target-representable and global-cap requirements until Request 5 supplies its wider
arbitrary-magnitude, limit-specific comparison.

### Streaming decoder and retained layout

The parser separates the bytes it is discovering from the bytes the returned handle owns:

```text
fixed read scratch -> interim head (validate, discard)
                   -> final head (copy exact bytes once, keep header spans)
                   -> chunk line / CRLF / trailers (validate, discard)
                   -> decoded chunk payload (append or read directly into response buffer)

response.buf = exact final head || decoded payload
response.body_start = final head length
response.body_len = cumulative decoded payload length
```

`HttpHead` gains an explicit framing classification instead of rejecting chunked during header
scan. The transport loop owns request-method/status selection, interim advancement, byte guards,
and reuse. The complete-buffer `http.parse` path calls the same state machine with ordinary method
semantics. As before, bytes after the first complete self-delimited response do not join its body;
the new retained layout discards them instead of keeping unreachable tail capacity. Existing header
lookup remains byte-exact because final-head bytes are not rewritten and trailer bytes never enter
`HttpHeaderSpans`.

A fixed scratch read may co-read bytes beyond the boundary that completes a head, size line, or
trailer line. Those bytes stay within the one 32 KiB scratch allowance and are fed to the next state;
after a failure becomes recognizable, no byte is copied into retained body storage and no later
transport read occurs. Every post-zero-chunk read is clamped to the remaining trailer allowance.
Every chunk-framing and post-zero-chunk byte already co-read is charged to its cumulative allowance
before another read. Reads are clamped to the current line, cumulative framing, or trailer remainder,
whichever is least. There is no one-byte over-guard probe for a chunk line, cumulative chunk framing,
or trailer block.

### Framing and reuse matrix

| Method / final status | Valid metadata | Selected body | Reusable after exact completion |
|---|---|---|---|
| exact `HEAD`, final status other than `204` | no framing field, arbitrary valid CL, or supported HTTP/1.1 chunked TE | zero bytes; consume no body framing | yes when final head is keep-alive and scratch/TLS have no residual or pending bytes |
| any method, `204` | neither CL nor TE | zero bytes | same |
| ordinary method, `304` | no framing field, arbitrary valid CL, or supported HTTP/1.1 chunked TE | zero bytes; consume no body framing | same |
| ordinary method, other final status + supported HTTP/1.1 chunked TE | TE only | decoded chunks through valid trailers | yes after terminal chunk/trailers and no scratch/TLS residual or pending byte |
| ordinary method, other final status + CL | CL only | exactly CL bytes | existing rule |
| ordinary method, other final status + neither | none | read to EOF | never |
| any, interim non-`101` | neither CL nor TE | zero; advance to next head | not a completion |
| any, `101` or any invalid/conflicting framing | any | none; fail | never |

### Implementation closure matrix

This matrix is authoritative for the implementation PR. The implementation follows this reviewed
strategy without reopening it; a change to the public rows, allocation strategy, or error/reuse
precedence reopens the matrix and requires a fresh design review.

| Closure axis | Required implementation evidence | Owner |
|---|---|---|
| Header formation and validation | `HttpHead` records exact version/status/header spans plus normalized CL/TE facts. A parameterized table covers exact CRLF, both admitted versions, malformed/unknown versions, three-digit status grammar/range, reason bytes, token names with immediate colon, values/obs-fold, CL duplicates/leading zeroes, TE token lists, HTTP/1.0 chunked rejection, CL+TE, per-head header count, and bodyless forbidden/permitted metadata. | `align_runtime` HTTP parser units |
| Request validation | Exact `CONNECT` fails before pool lookup/connect/write; case variants reach a fixture. Exact `HEAD` alone selects suppression. | runtime side-effect counter fixture + driver E2E |
| Interim construction/discard | Same-read and split-read `100`, `102`, `103`, and `199` preserve co-read final bytes; framing fields, `101`, cumulative head byte excess, malformed/truncated interim heads fail with no final handle. At most one header-span table is live. | runtime parameterized state-machine owner |
| Chunk move-in / compaction | Same-read, split-at-every-boundary, many-tiny-chunk, extensions, uppercase/lowercase hex, embedded NUL payload, exact decoded cap, and exact cumulative-framing cap cases produce one retained final head plus byte-exact decoded body. | runtime decoder matrix + allocation instrumentation |
| Chunk malformed / early exits | Empty/invalid/overlong size line, cumulative framing cap + 1, over-global magnitude, truncated payload, missing payload CRLF, missing zero chunk, and EOF/error/timeout at every state produce no response and no later read. | runtime fault-injection matrix |
| Trailer validation / discard | Empty trailers, exact guard, guard+1, continuous unterminated input, invalid name/value, forbidden framing fields, shared header-count boundary, duplicate name with final header, and split-at-every-boundary retain no trailer bytes/offsets and never alter header lookup. | runtime trailer matrix + structural instrumentation |
| Bodyless / final selection | `HEAD`, `204`, and `304` cover absent/CL/chunked metadata Cartesian rows, including values above the global cap; no terminator/trailer read occurs. Lowercase method controls are payload-bearing. | runtime framing matrix + plaintext driver fixture |
| Response move-out / Drop | Success boxes exactly one response; every failure leaves null and frees scratch/accumulator. Existing response array success/error Drop paths remain exact when `get_many` mixes chunked and ordinary responses. | live-response/allocation counters + `get_many` owner |
| Pool and replacement | Chunked, bodyless, and CL first responses reuse one plaintext/TLS connection for a second response only after full self-delimitation. Close, scratch residual, `SSL_pending`, `SSL_has_pending`, malformed, cap, `101`, and read-to-close cases close; stale zero-byte retry remains one fresh attempt. A TLS co-read fixture separately places next-response bytes in Align scratch and in OpenSSL pending storage. | runtime pool matrix, plaintext and verified-TLS |
| Whole / per-call consumers | `get`, `post`, `request`, `get_many`, and `http.parse` share framing behavior appropriate to their method knowledge; no package/compiler/ABI surface changes. | runtime units + `crates/align_driver/tests/http_chunked_client.rs` |
| Plaintext / TLS parity | Identical byte partitions and failures traverse `Conn::Plain` and `Conn::Tls`; timeout mapping and teardown remain transport-specific only below `Conn`. | verified-TLS runtime fixtures paired with plaintext vectors |
| Resource and allocation parity | Instrumented accumulator allocation-layout capacity obeys the exact steady and transient numeric ceilings in the ledger, including the old allocation during growth, plus one 32 KiB scratch; allocation-preserving conversion keeps the final response capacity within the same bound. Exact cumulative-framing cap and cap + 1 prove bounded metadata work; chunk/trailer structural state is constant-size. No growth target uses an unvalidated CL/chunk magnitude. | runtime capacity/high-water instrumentation |

No benchmark is a correctness gate: this capability changes accepted framing without making a new
throughput claim. The existing R1/R4 requirements still apply, so the owner records allocation and
copy counts and rejects a regression that adds per-chunk allocation or a second body buffer.

### Delivery and adoption

The implementation is one runtime/owner-test capability PR. No compiler, package, or ABI layer
changes. Its hand-written diff exceeds roughly 1,000 lines because the strict head parser,
incremental chunk/trailer decoder, exact-capacity accumulator, plaintext/TLS reuse verdict, and their
single owner matrix form one producer-to-consumer failure domain: splitting them would leave a
dormant parser or duplicate framing, cleanup, and allocation proof across temporary boundaries. The
implementation updates the historical item-7 client-asymmetry tests outright. After merge,
align-llm rebuilds and pins Align in its next permitted
consumer-prerequisite wave, switches its provider stream fixtures from Content-Length to chunked,
and proves valid SSE, malformed/truncated rejection, and final status/header/body preservation. The
sibling request register remains the lifecycle owner for that adoption evidence.

## Bounded client response bodies (align-llm Request 5 — DESIGNED 2026-08-14, IMPLEMENTED 2026-08-15)

Provider calls need an operation-sized receive limit, not a check after the whole-body client has
already allocated the response. Request 5 adds one client default and one request override to the
Request 4 framing engine. It adds no streaming-input handle, second decoder, provider policy,
ambient configuration, or package API.

### Public-contract ledger

| Surface / state | Exact contract |
|---|---|
| Public methods | `cl.max_response_body_bytes(limit: i64) -> ()` mutates a bound `http.client`; `r.max_response_body_bytes(limit: i64) -> ()` mutates a bound `http.request`. Like the existing timeout/header/body configuration methods, both are Pure setters that borrow rather than consume their Move receiver and perform no I/O. A request remains consumed only by `cl.request(r)`. |
| Input and defaults | `limit == 0` clears the stored value. A cleared client means fixed `HTTP_MAX_BODY = 1,073,741,824`; a cleared request inherits its client. A positive value must be `1..=HTTP_MAX_BODY` and target-`usize` representable. A negative, larger, unrepresentable, or null-handle input aborts before changing the previous value and before network work. No ambient input participates. |
| Selection | `get` and `post` use the client value. `request` uses `min(positive client or HTTP_MAX_BODY, positive request or HTTP_MAX_BODY)`. A bound is *explicit* when either stored value is positive, including positive `HTTP_MAX_BODY`; zero/unset is not explicit. `get_many` snapshots the client value once before workers start and passes it to every exchange. Source borrowing excludes a concurrent setter; atomic native storage makes the worker snapshot race-free. |
| Success and ownership | Exact-limit payload succeeds and returns the existing Move `response`; `status()`, `header()`, and `body()` remain region-bound zero-copy views. An explicitly bounded response owns separate fixed header and body allocations until Drop; the opaque handle ABI and source ownership do not change. Neither setter retains an input view. |
| Limit result | The first recognizable payload excess under an explicit bound returns stable `Error.Code(-1)`. Code `-1` is reserved for an explicit `std.http` receive bound (this capability's body limit and, after the streaming settlement below, its caller-selected SSE output bound): it is outside HTTP status `100..=599` and the non-negative raw OS codes published by the common errno mapping. It is distinct from `Error.Invalid`, `Error.Timeout`, transport `Error.Code(errno)`, and a successful HTTP status such as 413. No partial response, body, or event is returned. |
| Native status mapping | Runtime uses HTTP-private `AL_HTTP_BODY_LIMIT = -1`. Only HTTP client receive Result lowerers map that negative sentinel to `Error.Code(-1)` before the shared positive category/errno mapping. The sentinel cannot collide with a saturating `AL_CODE + errno`, never enters the generic errno decoder, and adds no `Error` variant or type-layout change. |
| Content-Length | Parse every field as an arbitrary-precision normalized decimal magnitude. Syntax, equal-duplicate normalization, conflicting duplicates, and CL/TE conflict precede the cap. For a payload-bearing final response, a valid magnitude above an explicit selected cap is `Error.Code(-1)` even when also above `usize` or `HTTP_MAX_BODY`; without an explicit bound, target/global excess stays `Error.Invalid`. An explicit excess reserves nothing from the peer value and causes no later payload read. |
| Method/status composition | Request 4's exact-uppercase `HEAD`/`CONNECT`, interim, `204`, and `304` rules remain authoritative. A bodyless final response validates framing metadata but does not compare its valid arbitrary CL magnitude with the cap, allocate a body, or read payload/chunk/trailer bytes. Only the returned final payload is capped. |
| Chunked payload | Existing line/framing/trailer guards and grammar remain unchanged. Guard and complete-line syntax checks precede the cap. A valid chunk magnitude that would take cumulative decoded bytes over an explicit cap is `Error.Code(-1)` before target conversion, payload allocation, or another read; without an explicit bound, global excess remains `Error.Invalid`. A limit recognized before the terminal chunk performs no trailer read. |
| Close-delimited payload | Consume co-read payload first. Before each later read, clamp the request to remaining selected bytes plus one scratch-only probe byte. The first excess byte is `Error.Code(-1)` for an explicit bound, never enters retained payload, and causes no later read. Read-to-close remains non-reusable on success. |
| Read and error precedence | Setter/request validation precedes network work. For received bytes: complete head/framing syntax and conflicts; body selection; fixed guards; complete in-guard grammar; cap comparison; payload allocation/read. A timeout/transport error returned by a read wins when no excess byte or declared magnitude was yet available. Truncation and malformed framing stay `Error.Invalid`. |
| Co-read exception | One fixed `HTTP_CLIENT_READ_CHUNK = 32,768` scratch read may contain bytes beyond a head or chunk-line boundary. After excess is recognized, they stay scratch-only, no excess byte enters retained storage, and no later read occurs. No other probe or staging allowance exists. |
| Allocation | An explicit exchange uses one fixed `HTTP_MAX_HEADER_BLOCK` final-head allocation and one fixed scratch; after a payload-bearing final head is selected, it adds one exact `selected cap` body allocation. Neither grows or compacts, both are sized independently of peer CL/chunk magnitude, and success moves them allocation-preservingly into the opaque response. Peak Align-owned response bytes are at most `selected cap + HTTP_MAX_HEADER_BLOCK + HTTP_CLIENT_READ_CHUNK`; for 262,144 the exact ceiling is 557,056 bytes. Bodyless responses allocate no body region. Unconfigured exchanges retain Request 4's one-buffer geometric accumulator and ceilings. Allocation failure stays fatal no-unwind OOM. This explicit two-region layout deliberately amends R1's internal one-buffer rule only for configured bounds while preserving its zero-copy public requirement. |
| Structural metadata | At most one interim/final header table is live; only final-header offsets survive. Trailer fields consume the final header-count remainder without bytes/offsets. Chunk-size lines are validated by scalar incremental grammar state and retain no raw-line staging buffer. Decoder state is constant-size; no capacity depends on body length, peer magnitude, chunk count, or trailer volume. |
| Cleanup and reuse | On `Error.Code(-1)`, output stays null, accumulator/scratch are freed, the partial plaintext/TLS connection closes and is never pooled, and no stale retry occurs after any response byte. The client remains usable via a fresh connection. Exact bounded CL/chunked/bodyless success retains Request 4's reuse verdict; close-delimited success remains non-reusable. |
| Batch | `get_many` runs every worker to completion, stores by input ordinal, and returns the lowest-index error independent of completion order. Any error yields no response array and frees successful siblings. A limit competes by ordinal like malformed, timeout, or transport errors. Each worker owns its own byte allowance. |
| Plaintext / TLS | One selection and decoder state serves `Conn::Plain` and `Conn::Tls`. Align-owned TLS application scratch is inside the ceiling; kernel and opaque libssl buffers are excluded and may not derive capacity from peer length, chunk magnitude, selected cap, or accumulated body. |
| Compiler/runtime owner | Sema owns exact receiver/method/arity/type and bound-local checks. Checked HIR owns receiver/`i64` envelopes. MIR owns setter rvalues and HTTP-private limit mapping. LLVM declares/calls setters and maps HTTP results. Runtime owns storage, snapshot, selection, decoder outcome, allocation, cleanup, and batch order. Package code owns none. |
| ABI and identity | Add `void @align_rt_http_max_response_body_bytes(ptr, i64)` and `void @align_rt_http_client_max_response_body_bytes(ptr, i64)` to ABI row A66. No other signature, type tag, interface record, or handle ABI changes. Method spellings and MIR discriminants entered interface-format-6 and cache identities; the shipped Request 9 format-7 bump preserves those records, and exact edit/revert restores the prior hash. |
| Prerequisite and adoption | Request 4 at `f04672bce6f8689c9b219d0a20e770571e2d638b` supplies framing. Align implementation precedes align-llm adoption. The sibling pins the merge, sets 262,144 at `provider_http`, proves the combined framing/cap/cleanup matrix, and keeps HTTP status handling distinct. |

### Framing and limit matrix

| Final selection | Explicit bound | Excess result / connection |
|---|---:|---|
| exact `HEAD`, `204`, or `304` bodyless | either | no comparison; empty body and normal bodyless reuse verdict |
| Content-Length / chunked / close-delimited payload | no | fixed global excess is `Error.Invalid`; close |
| Content-Length payload | yes | normalized magnitude above cap is `Error.Code(-1)` before reserve/read; close |
| chunked payload | yes | checked cumulative decoded excess is `Error.Code(-1)` after valid guarded size line; no response/trailer read; close |
| close-delimited payload | yes | co-read then remaining-plus-one scratch probe; excess is `Error.Code(-1)`; close |
| malformed/conflicting/truncated framing | either | `Error.Invalid`; close |
| deadline / transport failure before recognizable excess | either | existing `Error.Timeout` / `Error.Code(errno)`; close |

### Implementation closure matrix

This is one cross-layer capability: a public setter without the bounded decoder is dormant, while a
decoder-only cap is unnameable. Reusing Request 4's state machine and owners keeps the expected
hand-written change below roughly 1,000 lines.

| Closure axis | Required evidence | Owner |
|---|---|---|
| Formation and setters | Both receivers, exact `i64`, arity, bound-local use, zero clear/inherit, positive endpoints, invalid abort-before-store, whole/per-unit spelling, interface/cache edit-revert. | Sema/checked-HIR/MIR owners plus abort fixtures |
| Selection and concurrency | get/post inherit client; request min-selects both scopes; explicit `HTTP_MAX_BODY` differs from zero; `get_many` snapshots once. | runtime table + driver dispatch/batch |
| Fixed-length receive | leading-zero/equal-duplicate CL, exact cap, cap+1, above-target/global, malformed/conflicting/truncated precedence, no reserve/read from excess declaration. | runtime parser/transport matrix |
| Chunked receive | exact/cap+1, tiny chunks, oversized valid magnitude, invalid extension/guards, co-read excess, no trailer read after limit, terminal trailers/reuse. | Request 4 decoder owner + counters |
| Close-delimited receive | exact/cap+1, same/split read, one-byte probe, no post-decision read, non-reuse. | plaintext/TLS transport fixtures |
| Bodyless/interim | HEAD/204/304 arbitrary valid metadata and interim chains never compare/allocate body; malformed metadata and 101 stay Invalid; case-variant methods stay payload-bearing. | method/status matrix with cap twins |
| Move-out and cleanup | Success transfers the header/body response allocations; every failure leaves null and frees each live allocation/scratch once; later client use is clean; `?`, `else`, `match`, and `map_err` retain ordinary Result ownership. | live allocation/response/fd counters + driver consumers |
| Batch precedence | lowest-index malformed-vs-limit completion-order twins, exact-cap success, no array on failure, sibling Drop and failed-connection teardown. | `get_many` owner |
| ABI / lowering | Two A66 setters, whole/per-unit lowering, HTTP sentinel to `Error.Code(-1)`, other statuses unchanged, malformed checked HIR rejected before LLVM. | MIR/LLVM assertions, ABI tripwire, checked-HIR negatives |
| Resource ceiling | Positive 262,144 proves 557,056 maximum for CL, close, tiny-chunk, trailer, plaintext, and TLS; no peer-derived capacity; fixed structural state. | runtime high-water/failpoint instrumentation |
| Consumer discriminants | Limit is `Code(-1)`, HTTP 413 is status data, malformed is Invalid, deadline is Timeout, OS fault is non-negative `Code(errno)`. | driver match fixture + align-llm adoption |

No benchmark is required: this contract makes a resource ceiling, not a throughput claim. Runtime
instrumentation measures the ceiling directly. Changing the code, allocation strategy, public
method, framing precedence, or layer ownership reopens this ledger before implementation.

## Client streaming receive (post-`pkg.db` convergence item 1 — IMPLEMENTED 2026-08-30)

The whole-body API is the right terminal when a caller needs one owned `response`; it is the wrong
terminal for an indefinite provider stream or a large download. This capability exposes the already
incremental Request 4 decoder as one dependent read resource. Transfer framing remains inside
`std.http`, while allocation and iteration stay visible at the call site through a caller-owned
`buffer`. A consuming type transition adds WHATWG server-sent-event interpretation without a second
socket, framing decoder, automatic reconnect loop, or provider abstraction. The normative parsing
source is WHATWG HTML, “Interpreting an event stream”
(`https://html.spec.whatwg.org/multipage/server-sent-events.html#interpreting-an-event-stream`).

```text
request -> cl.request_stream(request) -> http_read_stream
http_read_stream -> repeated read(caller_buffer) -> de-framed bytes -> completion
```

For SSE, convert the same body owner before reading raw bytes. The output buffer is reused for every
event; its capacity is the explicit event-materialization bound.

```text
http_read_stream -> consuming sse() -> http_sse_stream
http_sse_stream -> repeated next(caller_buffer) -> Option<http_sse_event> -> completion
```

### Public-contract ledger

| Surface / state | Exact contract |
|---|---|
| Construction | `cl.request_stream(req: request) -> Result<http_read_stream, Error>` is the sole streaming request entry point. It requires a bound `client`, consumes and nulls the Move `request`, serializes and sends it once, advances and discards valid non-`101` informational heads, and returns only after the final head is complete and validated. It rejects exact uppercase `CONNECT` before URL, pool, DNS, connect, write, or TLS work. `get_stream` and `post_stream` aliases are not added; callers build the ordinary request explicitly. |
| Effects | `request_stream`, `read`, and `next` are Impure because they can perform network I/O. `sse()` is a Pure ownership-only type transition with no I/O/allocation; `status`, `header`, `last_event_id`, and `retry_ms` are Pure reads of retained memory. No stream type is a parallel element/result or closure/task capture, so a Pure parallel closure cannot acquire, advance, or implicitly release a live stream. |
| Head and status | A successful stream retains the exact final status line and header bytes. `status()` returns the final `i64`; `header(name)` performs the existing case-insensitive first-match lookup and returns an `Option<str>` view bound to the stream. HTTP status, including 4xx/5xx, remains data. `101`, malformed heads, framing conflicts, and invalid bodyless metadata remain `Error.Invalid`. The constructor performs no body transport read after the final head is recognizable, except that the one 32 KiB read which completes that head may already contain body bytes. |
| Raw read | `stream.read(out: mut buffer) -> Result<i64, Error>` borrows the stream mutably, sets `out.len` to zero before work, writes at most the existing capacity without growing it, and returns the decoded payload count. A positive result makes exactly those fresh bytes visible through `out.bytes()`; zero means HTTP body completion, never merely “the buffer was full.” A zero-capacity buffer aborts before stream state or I/O changes. Once at least one payload byte is available, the call performs no further transport read, though it may consume already-buffered framing. On `Err`, `out.len` remains zero and no partial bytes are published. |
| HTTP framing | The Request 4 head, interim, bodyless, Content-Length, chunk grammar, trailer, CRLF, error-precedence, and plaintext/TLS rules remain authoritative. Its whole-message cumulative chunk-framing counter does not: each body-facing `read` or `next` starts one fresh `HTTP_MAX_STREAM_CHUNK_FRAMING = 262,144` wire-byte allowance. The call charges every chunk-size line and its CRLF plus every CRLF after a nonzero payload that it processes, including bytes already co-read into scratch; a carried byte is charged by the first call which processes it and never twice. Exact allowance succeeds. Requiring another framing byte is `Error.Invalid` without an over-guard read, and any error closes the stream; a successful return replenishes the next call's allowance. `read` exposes only Content-Length bytes, de-chunked payload bytes, or close-delimited bytes; it never exposes chunk lines, payload CRLF, the terminal chunk, or trailers. Exact `HEAD`, `204`, and `304` streams are complete with an empty body and retain Request 4's arbitrary valid metadata rule. For a payload-bearing response, an unbounded stream accepts a valid Content-Length through `u64::MAX`; a larger normalized magnitude is `Error.Invalid` before return. Cumulative decoded chunked/close-delimited length overflow past `u64::MAX` is likewise `Error.Invalid`. |
| Receive limit | A positive selected `max_response_body_bytes` remains an explicit cumulative decoded-body cap for `request_stream`; exact fit succeeds and the first recognizable excess is `Error.Code(-1)`. A declared Content-Length excess fails in the constructor before body read. Chunked and close-delimited excess may fail a later `read`/`next`. When both stored values are zero, streaming has **no cumulative body cap**: unlike a whole-body response it does not allocate in proportion to total length. The fixed head guard, per-call framing/SSE work guards, caller buffer capacity, timeouts, and `u64` accounting still apply. Thus no configured option is silently ignored, while an indefinite SSE stream is possible when the caller leaves the whole-body cap unset. |
| Timeout snapshot | Construction resolves the request override or client default once. The same timeout is applied independently to connect, send, each constructor receive, and each later `read`/`next` transport receive. Expiry remains `Error.Timeout`; zero remains no timeout. A setter after stream creation cannot change the stream's snapshot. |
| SSE transition | `stream.sse() -> http_sse_stream` is Pure and consumes/nulls the raw stream without I/O, copy, allocation, or connection change. Parsing begins at the raw stream's current logical body position, including any undelivered co-read bytes. Converting before `read` interprets the complete response body and strips one leading UTF-8 BOM as WHATWG requires; converting after raw reads deliberately interprets only the remaining suffix and does not treat a suffix BOM as the response-leading BOM. `http_sse_stream` exposes the same `status()`/`header()` head views and no raw `read`, so the two body interpretations cannot be mixed after conversion. |
| SSE scope | `events.next(out: mut buffer) -> Result<Option<http_sse_event>, Error>` implements WHATWG event-stream **decoding and field interpretation**, not the browser `EventSource` networking policy. It does not validate `Content-Type`, redirect, reconnect, sleep, add `Last-Event-ID`, or classify HTTP statuses. The caller inspects status/headers, decides whether the body is SSE, and explicitly constructs any later request. |
| UTF-8 and lines | The remaining de-framed body is decoded with the WHATWG UTF-8 decode algorithm: one response-leading BOM is removed and malformed byte sequences become U+FFFD, never an invalid Align `str`. CRLF, lone LF, and lone CR terminate lines, including across transport/chunk boundaries. Field names are compared byte-for-byte with no case folding. A first `:` splits name/value and exactly one leading U+0020 after it is removed; a line without `:` has an empty value. A leading `:` line is a comment and ignored. |
| SSE fields and state | `data` appends its value plus one LF; `event` replaces the current block-local event type; valid `id` and `retry` lines replace block-local candidates. An `id` containing U+0000 and a `retry` that is empty, nondigit, or above `i64::MAX` are ignored. Unknown and case-mismatched fields are ignored. Data/event/candidate state resets after every blank-line dispatch attempt. Persistent last-event-id/retry start empty/`None` and change only at the atomic block commits below. `events.last_event_id()` returns a stream-bound view of the last committed ID and `events.retry_ms()` its current inline Copy value; the ID view cannot survive a later `next` that may commit a replacement. |
| SSE work guard | `HTTP_MAX_SSE_METADATA = 262,144`. Each `next(out)` may hand at most `u64(out.capacity) + HTTP_MAX_SSE_METADATA` de-framed source bytes to the UTF-8 decoder. The checked sum counts every source byte processed by that call, including a stripped BOM, invalid UTF-8 input, line terminators, comments, unknown fields, invalid `retry` values, control-only blocks, and dispatched data; bytes already co-read are charged before another transport read. Ending or dispatching exactly at the allowance succeeds. Requiring the next source byte is `Error.Invalid` without an over-guard body read, closes the stream, zeroes `out.len`, and publishes no event. A successful `Some` or terminal `None` ends the call; the next call receives a fresh allowance. The caller-capacity term covers event material, while the fixed term bounds syntax and arbitrarily many ignored/control-only blocks, so an unset cumulative body cap cannot make one `next` perform unbounded work. |
| Dispatch and state commit | A blank line with at least one `data` field attempts one event; a block without `data` produces no event. One trailing LF is removed from accumulated data. An empty event type becomes `"message"`; otherwise it is preserved exactly. Before parsing, `out.len` becomes zero. The caller allocation is unpublished staging until commit. For a control-only block, a valid candidate ID/retry commits atomically at its blank line, staging resets, and parsing continues; that commit survives any later block's failure in the same `next`. For a data-bearing block, output capacity is checked for exact `event \|\| data \|\| committed-or-candidate last_event_id` bytes first. Exact fit then commits its candidate ID/retry and publishes `Some` atomically; `retry_ms` is copied inline, while the three string fields are zero-copy spans over the fresh output generation. Any output-cap or later terminal error before that atomic step rolls back only the current block to the state committed by earlier blank lines, closes the stream, leaves `out.len == 0`, and publishes no event. Output-cap excess is `Error.Code(-1)`. |
| Empty and terminal cases | `data`, `data:`, and `data:` followed by one optional space can dispatch an event whose `data` is empty; two empty data lines dispatch one LF. Comments and empty/control-only blocks do not produce `Some`. HTTP completion without a final blank line discards the complete pending block, including its staged ID/retry, returns `None`, and applies the normal connection verdict; control-only blocks committed at earlier blank lines remain observable. Every later `next` returns `None` without I/O. |
| SSE storage | `next` retains the current block's de-framed source bytes in one reusable stream-owned work allocation bounded by `C + HTTP_MAX_SSE_METADATA`, where `C` is that call's output capacity. A blank line is the commit boundary: the parser scans the retained block without another allocation, validates/replaces fields and computes the exact final event/data/ID sizes, then materializes the successful event in canonical order in the still-unpublished caller allocation. Control-only blocks update retained state and clear the work buffer; a data block publishes the caller buffer; terminal incomplete input clears it without commit. The work allocation grows exactly to a newly required call bound and is retained for reuse, so no event-frequency allocation occurs after the largest supplied capacity stabilizes. The stream separately retains the committed last-event-id in one exact-capacity allocation plus inline retry state. Let old ID capacity be `K`, a committing candidate ID length be `N`, and current output capacity be `C`. `N > C` is `Error.Code(-1)` before mutation. When `N > K`, allocate exactly `K' = max(N, min(C, saturating_mul(K, 2)))`, initialize it, then free the still-live old allocation; zero grows directly to `N`. Let `M` be the largest output capacity supplied so far: retained ID capacity is at most `M`, retained block-work capacity is at most `M + HTTP_MAX_SSE_METADATA`, and the two steady capacities total at most `2 * M + HTTP_MAX_SSE_METADATA`. Work-buffer growth plus the already-retained ID is strictly below `3 * M + 2 * HTTP_MAX_SSE_METADATA`; ID growth plus the retained work buffer is strictly below `3 * M + HTTP_MAX_SSE_METADATA`. Smaller later buffers do not shrink either allocation; a dispatched event must fit its current call's capacity. The response-leading 32 KiB transport scratch and fixed head allocation are shared with raw mode. |
| Errors and precedence | Request/setter validation precedes network work. Received head/framing syntax and the current call's framing guard precede body-cap checks; a complete valid magnitude precedes conversion/accounting. For a newly available payload byte, the selected cumulative body cap is charged before the SSE source-work guard, then UTF-8 replacement and line/field interpretation precede caller output-cap comparison. Thus a same-byte body-cap excess is `Error.Code(-1)` before an SSE-work excess; a framing byte that exceeds its own allowance is `Error.Invalid` before any payload exists. A caller output-cap excess wins before event/state commit. Framing/truncation/stream-work/`u64` excess is `Error.Invalid`, deadline is `Error.Timeout`, transport/TLS keeps its existing category, and either explicit cap is `Error.Code(-1)`. Any terminal error rolls back the current incomplete/unpublished block, preserves earlier blank-line commits, poisons/closes the connection, and makes every later body call return the same stored error without I/O; Pure state getters still expose the last committed values. |
| Allocation | Construction owns one fixed `HTTP_MAX_HEADER_BLOCK = 262,144` final-head allocation, one fixed `HTTP_CLIENT_READ_CHUNK = 32,768` scratch, the header-span table, and the checked-out connection; body bytes never allocate inside a raw stream. Request serialization scratch returns to the client immediately after construction. SSE adds one reusable raw-block work allocation bounded by the current largest `C + HTTP_MAX_SSE_METADATA`, one committed-ID allocation bounded by the current largest `C`, and inline parser/retry state. The work allocation is necessary to validate a later replacement field before destroying the prior block-local candidate: for example, an `id` whose trailing NUL makes the whole line ignorable must leave an earlier valid `id` in the same block intact. A one-pass parser cannot retain both arbitrary alternatives inside the one `C`-byte caller output. The exact steady and growth ceilings are the `SSE storage` row above; they exclude the caller-owned output buffer, fixed handle/table metadata, allocator bookkeeping, and opaque kernel/libssl storage. No peer Content-Length, chunk magnitude, cumulative body length, or SSE field length can produce capacity beyond its fixed or caller-selected bound. OOM remains fatal/no-unwind. |
| Ownership and cleanup | Both stream types are Move, borrow-mutated cursors and retain a shared client borrow. The client pool remains usable by other shared calls but cannot be moved/dropped before them. `sse()` transfers every connection/scratch/head/control owner and cleanup bit exactly once. Bare and admitted builtin-tag construction, success, `?`, `else`, `match`, `map_err`, replacement, return, and ordinary Drop preserve one owner plus its client provenance; all other storage/capture/parallel carriers are rejected. A stream cannot escape its creating client and an event string cannot escape its output-buffer generation. |
| Carrier formation and provenance | Let `C ::= http_read_stream \| http_sse_stream \| Option<C> \| Result<C,N> \| Result<N,C> \| Result<C,C>`, where the graph is finite/acyclic and `N` is any otherwise valid type whose storage graph contains no stream. This least set is the complete carrier admission rule: each active tag contains zero or one stream, and moving or pattern-unwrapping it initializes the destination before clearing the complete source tag/handle. Replacement and Drop inspect every nested active tag and release exactly one stream, if present. `C` may occupy a local, a by-value/shared-borrow/mutable-borrow parameter, or a function result. An `out C`, constant/global, user native/extern signature, borrowed-place owning projection/match, and every other placement reject; ordinary return and consuming match are the one transfer paths. Direct, imported, and indirect function parameter/return summaries preserve its client dependency; a noncapturing function signature may mention `C` but the function value itself is not a carrier. A control-flow join unions possible creating clients, so none can die before the joined carrier is consumed or dropped. One cycle-safe positive classifier checks the fully substituted storage graph and exhaustively matches every `Ty` and `Scalar` discriminator without `_`: an edge through builtin `Option`/`Result` recurses, while every other enclosing storage edge returns forbidden. Thus user structs/sums, anonymous tuples, fixed/dynamic/specialized collections, slices, boxes, builders, tasks, closure environments, parallel elements/results, malformed HIR, and future unclassified constructors fail closed rather than relying on an exclusion list. |
| Connection fate | A keep-alive-eligible Content-Length body or valid terminal chunk/trailers is returned to the creating client's pool exactly when complete and only with no residual scratch or TLS-pending bytes. Bodyless completion may pool before the constructor returns. Close-delimited success closes. Drop before exact completion, caller/body limit, malformed/truncated framing, timeout, transport/TLS error, `101`, and residual bytes close and never pool; Drop performs no hidden drain. A reused stale connection is retried once fresh only if it fails before any response byte, exactly as whole-body receive. |
| Plaintext / TLS | One framing/read/SSE state machine consumes `Conn::Plain` and `Conn::Tls`. TLS verification, hostname binding, ALPN, timeout mapping, `SSL_pending`/`SSL_has_pending` reuse exclusion, SIGPIPE suppression, and teardown remain below that common owner. Plaintext and TLS expose identical body partitions and SSE events for identical application bytes. |
| Compiler/runtime owner | Sema owns the exact methods/types/effects, admitted carrier grammar, bound receiver/output checks, Move transfer, client/output provenance, parallel exclusion, and event-field types. Checked HIR independently validates every new envelope plus effect/type/region/carrier facts. MIR owns stream construction, head access, read, conversion, next, transactional state commit, status mapping, source nulling, and Drop. LLVM owns ABI declarations/calls and Result/Option/event reconstruction. Runtime owns URL/request work, pool/TLS connection, framing state, cap/timeout accounting, SSE decoding/staging/commit, buffer writes, and cleanup. Package code owns none. |
| Native ABI and identity | Add `i64 @align_rt_buffer_capacity(ptr)`, `i32 @align_rt_http_client_request_stream(ptr, ptr, ptr)`, `i64 @align_rt_http_read_stream_status(ptr)`, `i32 @align_rt_http_read_stream_header(ptr, ptr, i64, ptr)`, `i32 @align_rt_http_read_stream_read(ptr, ptr, ptr)`, `ptr @align_rt_http_read_stream_sse(ptr)`, `{ptr,i64} @align_rt_http_sse_stream_last_event_id(ptr)`, `i64 @align_rt_http_sse_stream_retry_ms(ptr)`, `i32 @align_rt_http_sse_stream_next(ptr, ptr, ptr)`, and `void @align_rt_http_read_stream_free(ptr)`. The capacity getter exposes the caller-selected read window, not the current published length, so checked lowering can abort a zero-capacity `read`/`next` before entering the stateful stream ABI; `capacity(null) = 0`. The retry getter returns `-1` for `None` and the stored non-negative value otherwise. `read` returns status separately and writes its `i64` count so private negative `AL_HTTP_BODY_LIMIT` cannot collide with a positive byte count. `next` writes one canonical 64-byte/8-aligned envelope: `u8 present`, `u8 retry_present`, six zero reserved bytes, `i64 retry_ms`, then `{ptr,i64}` for event, data, and last-event-id in that order; error/None zero the complete envelope. Both Move types share the head getters and free ABI. New type/method/HIR/MIR records enter interface and cache identity once; exact edit/revert restores the prior hashes. |
| Native input validation | Runtime entrypoints validate writable outputs before consuming a Move source or doing I/O. `capacity(null) = 0`; a nonnull buffer returns its fixed caller-selected read window even when its current published length is zero. `request_stream` first rejects a null output slot, then zeroes it; a null client or request returns `AL_INVALID` with the nonnull request still caller-owned. Once all three are valid it takes the request, and every later success/error path consumes it exactly once. `read` first requires a count slot and writes zero, then requires a nonnull buffer, zeroes its length, and requires a nonnull stream; `next` first requires and zeroes its envelope, then requires a nonnull buffer, zeroes its length, and requires a nonnull stream. Invalid handles, a zero-capacity direct-ABI buffer, or missing outputs return `AL_INVALID` without changing stream state or performing I/O; checked source calls the capacity getter and lowers the zero-capacity case to the specified abort before the stateful ABI call. `header` zeroes a valid output view and returns absent for a null stream, negative or non-`usize` name length, or a positive length with null data; zero length never forms a null slice. `status(null) = 0`, `last_event_id(null) = {null,0}`, `retry_ms(null) = -1`, and `sse(null) = null` without an ownership transition. `sse(nonnull)` takes the raw owner only after that check. The shared free is null-safe. These checks precede slice construction, dereference, allocation, ownership take, and network/pool effects in that order. |
| Prerequisite and acceptance | Request 4 framing, Request 5 explicit cap, client TLS/pool/timeouts, caller-owned `buffer`, nested `Result<Option<Record>, Error>`, and dependent-resource provenance are shipped prerequisites. Raw acceptance streams fixed/chunked/close/bodyless/interim bodies across every split boundary over plaintext/TLS, proves bounded memory and pool/Drop fate, downloads beyond 1 GiB with no explicit cap, and accepts exactly 262,144 per-call framing bytes while rejecting the next before a read and replenishing the following successful call. Compiler acceptance covers every `C` grammar production including stream-bearing `Result` Ok/Err/both arms, nested tags, two-client provenance joins, direct/imported/indirect/generic forwarding, by-value/borrow/borrow-mut parameter and return placement, exact active Drop, rejected out/global/const/native/borrowed-projection placement, one negative descendant case for every current non-builtin `Ty`/`Scalar` storage edge including anonymous tuples, malformed-HIR twins, and the future-variant tripwire. Pure/Impure direct/imported/generic/parallel twins close effect transport separately. SSE acceptance uses the WHATWG examples plus BOM/invalid UTF-8, every line ending/split, empty/multiline/control-only events, id NUL/reset/inheritance, retry valid/invalid/overflow, unknown/case fields, exact output cap/cap+1, exact/rejected-next source-work allowance for every ignored class, incomplete EOF, cumulative body cap, and reconnect-policy absence. Output/body/work/framing/timeout/transport failure is injected after every staged ID/retry/data position to prove current-block rollback, prior control-only commit preservation, and no skipped reconnect ID. |

`Error.Code(-1)` is therefore the stable **explicit HTTP receive-bound** result, not only a
whole-body allocation result: it covers the already-shipped configured cumulative body cap and the
new caller-selected SSE output-buffer cap. Both reject partial publication and close the connection.

### Framing, read, and connection matrix

| Final response/body state | Observable read/event result | Connection after exact completion |
|---|---|---|
| exact `HEAD`, `204`, or `304` | first `read` = 0 / first `next` = None; no payload/framing read | pool iff existing bodyless keep-alive/residual rules allow |
| Content-Length | exact de-framed bytes, then 0/None; declared explicit-cap excess fails constructor | pool after exact length and no residual/pending bytes |
| HTTP/1.1 chunked | payload only across arbitrary chunk/read boundaries; terminal/trailers never surface | pool only after valid zero chunk + trailers and no residual/pending bytes |
| close-delimited | payload until transport EOF, then 0/None | close, never pool |
| valid body with caller Drop before exact completion | already published bytes/events remain valid until their output owner changes | close immediately; no drain |
| malformed/truncated/cap/timeout/transport/TLS failure | no partial current read/event; stable `Err` thereafter | close; never retry after response bytes |

### Implementation closure matrix

This is a cross-cutting ownership/framing capability, so the following matrix is authoritative for
implementation. A public shape, lifetime, allocation strategy, validation order, framing/reuse
verdict, SSE projection, or ABI change reopens it and requires fresh design review.

| Closure axis | Required implementation evidence | Owner |
|---|---|---|
| Formation and type classes | Exact method/arity/receiver/output types; bound client/request/buffer; the complete finite `C` grammar accepted for each stream, including `Result` Ok/Err/both stream-bearing alternatives; locals, by-value/borrow/borrow-mut parameters, and returns admitted while out/global/const/native/borrowed-owning-projection positions reject; one cycle-safe exhaustive no-wildcard `Ty`/`Scalar` classifier admits only builtin-tag storage edges and rejects every other current/future edge, with explicit source and malformed-HIR tuple twins; captures/parallel transport and forbidden concrete generic substitutions reject; both streams are non-Copy/non-clone/non-printable/non-comparable; event nested Option record has three views plus inline Copy retry. | sema + checked-HIR parameterized placement/discriminator/reachable-type sweep + compile-time variant tripwire |
| Effects and parallel exclusion | `request_stream`/`read`/`next` Impure; ownership-only `sse` and all head/state getters Pure; effect replay agrees for direct/imported/generic calls; no capture or parallel element/result can transport a stream into `par_map`. | effect-inference owner + direct/indirect/parallel negatives |
| Construction and move-in | request validation, consume/null/free on every result, shared client retention, request scratch return, final head/interim/bodyless selection, same-read body carry, stale retry boundary. | compiler move owners + runtime constructor matrix |
| Raw move-out | zero/exact/short/full-cap reads, split at every framing boundary, buffer generation/len, no grow/allocation, terminating and error expressions, later stable terminal result. | runtime decoder owner + driver E2E |
| Explicit/unset limits | whole-body unchanged; streaming unset beyond 1 GiB; positive CL constructor exact/excess; chunked/close later exact/excess; per-call chunk framing exact 262,144 / rejected-next-byte / replenished-next-call; SSE output capacity exact/excess; private negative status never aliases byte counts. | runtime cap table + MIR/LLVM discriminant owner |
| SSE interpretation and commit | Official examples and full BOM/UTF-8/line/field/blank/EOF Cartesian cases; current-block staging versus prior blank-line commits; raw-prefix transition; output concatenation/spans/inline retry; invalid-byte replacement expansion and cap; per-`next` source-work exact `capacity + 262,144` and rejected-next-byte cases for comments, unknown fields, invalid retry, control-only blocks, and split chunked input. Successful event atomically commits ID/retry with publication; output/body/work/framing/timeout/transport failure and incomplete EOF roll back only the pending block; prior control-only commits survive later failure. | parameterized runtime parser/state oracle + driver event/reconnect consumer |
| Control-state allocation | one retained raw-block work allocation bounded by `C + 262,144`, allocation-free block scans, caller materialization with `out.len == 0`, latest committed ID overwrite, NUL/invalid retry ignore without losing an earlier same-block candidate, repeated control-only commits, one exact persistent-ID allocation, checked candidate length, exact capacity growth/reuse, both old+new transient high waters, smaller later output, rollback/error/Drop free, and no event-frequency allocation after the largest supplied capacity stabilizes. | allocation/failpoint/high-water instrumentation |
| Carrier provenance, move, and Drop | direct and nested `Option`/`Result` Some/None/Ok/Err including stream-bearing Ok/Err/both alternatives, two-client joins, direct/imported/indirect/generic parameter/return, move-in/out, pattern unwrap, `?`/`else`/`match`/`map_err`, replacement, branch/loop/early return, and active-tag Drop retain the client dependency, initialize destination before source clear, and close/pool once. A table generated from every current `Ty`/`Scalar` discriminator proves all non-builtin storage edges forbidden, including user record/sum, anonymous tuple, collection/builder/box/task, capture, and parallel shapes. | sema provenance + checked-HIR mutation/discriminator sweep + MIR cleanup + runtime live counters |
| Ownership, replacement, and return | raw→SSE transfer nulls source; client Drop/move rejected while any admitted `C` survives; other shared client requests accepted; output string view rejected after next/replacement/Drop and on escape while inline retry survives normally; all allowed exits drop once. | sema provenance owners + MIR cleanup + runtime live counters |
| Pool and failure exits | CL/chunk/bodyless reusable twins, close-delimited, residual scratch, TLS pending, mid-body Drop, every parse/cap/timeout/I/O failure, completed Drop, stale zero-byte retry exactly once, no post-error I/O. | plaintext/TLS pool matrix + fd/SSL counters |
| Generic/imported/per-unit | direct/imported/indirect helpers and noncapturing function-value signatures preserve dependency summaries for admitted `C`; unresolved higher-order calls conservatively retain every compatible client root; generic forwarding checks the fully substituted graph and rejects any non-builtin storage edge; whole/per-unit interface transport, cache edit/revert, and malformed effect/carrier/interface/checked-HIR reject before LLVM. | interface/cache/validation owners |
| ABI and layout | Ten declared symbols, including the read-window capacity getter, shared free/head ABI, SSE state getters, 64-byte envelope offsets/reserved zeroes, null/malformed-input sentinels and validation order, output clearing, pre-consumption rejection, null-safe free, signed private status mapping, cross-target pointer/`i64` layout. | ABI ledger tripwire + malformed direct-call matrix + MIR/LLVM structural assertions |
| Resource parity | Fixed 262,144 head + 32,768 transport scratch, no raw body allocation, operation-local framing and SSE-work counters, reusable SSE block work bounded by output capacity plus the fixed metadata allowance, committed control storage bounded by output capacity, both old/new growth layouts counted, no unbounded peer-derived allocation, and one connection owner throughout. | runtime high-water/allocation instrumentation |
| Consumer acceptance | Streaming download hashes byte-identically to whole-body under its cap; two provider-style SSE events arrive before connection close; caller-driven reconnect reuses surfaced id/retry but no hidden request/sleep occurs. | driver E2E; later `pkg.llm`/cloud consumer adoption |

### Delivery boundary

Implementation lands as two independently useful capability PRs against this one reviewed ledger:

1. The parameterized dependent-stream carrier owner plus `http_read_stream`
   construction/head/raw read, explicit-cap composition, ownership, TLS, and pool closure. The first
   PR closes the positive `C` grammar, exhaustive storage-edge tripwire, active-tag Move/Drop,
   dependency/effect summaries, and interface/cache transport for the raw type instead of leaving them for SSE. This is
   immediately useful for large downloads and any incremental protocol consumer.
2. The consuming `http_sse_stream` transition, WHATWG parser, transactional control-state commit,
   event projection, and caller-buffer cap. Adding the sibling type must extend the same
   parameterized carrier/effect/provenance tripwire from one member to two; an ad-hoc second set of
   matches cannot land. It reuses the first PR's only transport/framing owner and introduces no
   dormant second decoder.

The raw boundary is a stable consumer surface and a distinct failure domain; combining both would
cross every compiler layer plus the HTTP/TLS decoder and the independent Unicode/event parser in one
review. Splitting earlier would leave an unusable producer, while splitting SSE field families would
duplicate state and cleanup proof. Neither PR makes a throughput claim, so benchmarks are diagnostic
only; allocation/high-water instrumentation and owner tests are correctness gates.

The carrier/state axis was reopened after the revised-design review: admitting a dependent handle
without closing every reachable aggregate would make client lifetime conditional on an unchecked
container, while mutating reconnect state before event publication could skip an undelivered event.
A later audit found that enumerating forbidden containers had itself omitted the anonymous-tuple
formation/Drop path. The closure is therefore the positive `C` grammar plus one exhaustive
no-wildcard storage-graph classifier: every non-builtin-tag edge is forbidden without needing its
name on a prose blacklist, and a new `Ty`/`Scalar` discriminator reopens the compile-time tripwire.
The boundary above makes that carrier-provenance substrate a first-PR capability and makes SSE block
state transactional in the second, rather than distributing either proof across later fixups.

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
- item 10 — `ctx.headers()`: the wrapper-through-a-parameter compiles AND reads the table E2E; a
  view from a LOCAL handle rejected on return / `break` / wrapped in a struct / held across a serve
  iteration **that consumes the ctx** (the pkg.web shape) and across one that merely **drops** it
  (`a_view_of_a_handle_dropped_at_the_end_of_an_iteration_is_rejected` — the flipped known hole,
  fixed 2026-07-22); `hs.get()` after `ctx.respond(rb)` rejected **on a bare local** (a `str` field in an
  enclosing struct masks the hole); `hs.get()` after `ctx.respond_stream(rb)` compiles and works;
  case-insensitive hit + miss E2E through `pkg.web`; the view rejected as an `Option`/`Result`
  payload and as an array element; a struct carrying it stays Copy (no drop emitted) with a
  `sema_and_codegen_struct_layout_agree` row. (`crates/align_driver/tests/http_headers_view.rs` +
  `apps_web_root.rs::web_header_reads_the_request_header_table`.)
