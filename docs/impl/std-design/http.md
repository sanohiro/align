This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.http — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/http.md)

## Overview

An HTTP/1.1 primitive, NOT a framework (draft §18.2). Built on std.net sockets. Members: request,
response, header, method, status, client, server primitive. Connection reuse per the net rail.
**TLS is the hidden dependency**: HTTPS needs an FFI TLS engine (BoringSSL/rustls-ffi class, like
compress/crypto's borrow-the-engine). v1 is **plaintext HTTP/1.1 only**; HTTPS is deferred within
M11 until the TLS FFI wrapper lands (record, don't half-ship). HTTP/3, routing, middleware = pkg,
not std.

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
  the reuse path (see `bench/http_client/README.md`). The `get_many` bounded-concurrency scaling shape
  (R5) is a later slice; R6's keepalive latency/throughput gate — the part that gates **module**
  completion — is met.

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
4. server primitive (serve/accept, caller writes response). **Surface settled 2026-07-10** (two
   independent design reviews: language-purity lens + systems-evolution lens; both ratified — full
   surface in Signatures above). The settled decisions:
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
     future `ctx.respond_stream(rb) -> Result<http_stream, Error>` (rb built header-only) with Move
     `http_stream.send(chunk) -> Result<(), Error>` + Drop = terminal chunk + close. Needs a
     chunked **write** path (new, non-conflicting with CL-only parse). The v1 surface already
     admits it (`.body()` is optional), so nothing is painted in.
   - **R-requirements: R1/R2/R4 apply and are required** (zero-copy request offset table; memchr
     scan; one-write respond). No server bench gate in v1 — a light accept→respond round-trip bench
     arrives with keepalive/concurrency, where a reuse path first exists.
5. [DEFERRED to post-TLS] HTTPS via the FFI TLS wrapper.

## Known v1 limitations (Slice 2/3)

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
- **`https://` rejection is coarse (DC-1, low).** `https://` is correctly rejected pre-connect (P1's
  security intent is met — never a silent plaintext downgrade), but it maps to the **bare
  `Error.Invalid`**, indistinguishable from any other malformed URL. The design's aspiration of a
  clear "HTTPS not supported in v1 (TLS wrapper pending)" message is therefore **unmet**. This is
  structural, not a fix we can slot in: the `Error` enum carries **no message payload**, so there is
  no mechanism to attach the string. Do not invent a new one for this — the message-carrying error
  story is a separate cross-cutting decision. Recorded as a known v1 limitation tied to the
  message-less `Error` enum; revisit if/when `Error` grows a payload.

## Pitfalls

- **P1 (TLS defer honesty)**: v1 is plaintext only. Do NOT silently accept `https://` URLs and
  send plaintext — reject `https://` with a clear "HTTPS not supported in v1 (TLS wrapper
  pending)" `Error.Invalid` until the TLS slice lands. Silent downgrade is a security footgun
  (Nothing-hidden violation). **v1 caveat (DC-1):** the rejection is correct but coarse — it is the
  bare `Error.Invalid` with no attached message, so the "HTTPS not supported" wording above is an
  aspiration the message-less `Error` enum cannot yet carry (see Known v1 limitations).
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
- `https://` → `Error.Invalid` (P1)
- CRLF in header → rejected (P6)
- response body view escaping resp → compile error (P3)
- pool reuses a conn across 2 gets
- Move-rejection + unbound-receiver rejected
- import-required
- `bench/http_client` numbers recorded vs a Rust baseline (R6 — completion is benchmark-gated)
