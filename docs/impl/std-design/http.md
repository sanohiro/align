This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.http — implementation design (M11)

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

## New machinery required

Move types above + HTTP/1.1 parse/serialize over net sockets + connection pool reuse. NO new I/O
path (net's reader/writer). TLS wrapper deferred (blocks HTTPS). Header parsing is scalar v1
(simdjson-style structural scan is a later optimization, record only).

## Slice breakdown

1. request/response structs + header list + HTTP/1.1 serialize/parse (no socket yet — pure
   encode/decode, testable standalone).
2. client + get/post over one net `tcp_conn` (plaintext).
3. connection pool reuse (the rail — keepalive, reuse by default).
4. server primitive (serve/accept, caller writes response).
5. [DEFERRED to post-TLS] HTTPS via the FFI TLS wrapper.

## Pitfalls

- **P1 (TLS defer honesty)**: v1 is plaintext only. Do NOT silently accept `https://` URLs and
  send plaintext — reject `https://` with a clear "HTTPS not supported in v1 (TLS wrapper
  pending)" `Error.Invalid` until the TLS slice lands. Silent downgrade is a security footgun
  (Nothing-hidden violation).
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
