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
// Server primitive (not a framework)
srv := http.serve(host: str, port: i64) -> Result<http_server, Error>
srv.accept() -> Result<http_request_ctx, Error>   // one request; caller writes the response
// Batched client (the rail — moved here from net; see Concurrency in net.md)
cl.get_many(urls: slice<str>, max_concurrency: i64) -> Result<array<response>, Error>
```

## Type & ownership classification

- `client`, `request`, `http_server`, `http_request_ctx` are **Move types** (own pooled conns /
  header lists / body buffers / the accepted socket). reader/writer Move precedent + the net Move
  types they wrap.
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
   end at the conn close → not reused), **and** carried **no bytes beyond the framed message** (leftover
   ⇒ dirty ⇒ dropped). **Stale-conn retry:** a reused idle conn the server has since dropped fails
   before any response byte; that ONE case is transparently retried once on a fresh conn (the request
   was almost certainly never processed — the failure is the idle-close race). A fresh conn's failure,
   or any mid-response failure, surfaces directly. **SIGPIPE:** the client write path uses
   `send(MSG_NOSIGNAL)` (Linux) / `SO_NOSIGPIPE` (macOS) so writing to a dropped reused conn returns
   `EPIPE` (→ retry) instead of killing the process — no global signal handler installed. **Pool
   bounds:** ≤ 8 idle conns per host (overflow closed, not pooled); an idle conn older than 90 s is
   reaped on take (avoids a doomed reuse+retry). **R6 met:** `bench/http_client` (below) records the
   pool at **2.86× keepalive speedup** (floor 1.48×) and **parity with hand-written Rust `std::net`**.
   Tests: `align_runtime` units (pool reuses one conn across 3 gets; `Connection: close` not pooled;
   stale-conn retry; `http_head_keep_alive` decision table) + a driver test (two gets reuse one conn,
   observed via the server's accept count).
4. server primitive (serve/accept, caller writes response).
5. [DEFERRED to post-TLS] HTTPS via the FFI TLS wrapper.

## Known v1 limitations (Slice 2/3)

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
