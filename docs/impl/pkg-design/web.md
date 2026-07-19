This directory holds the authoritative per-package design docs for **first-party `pkg` libraries**,
at the same depth as `../std-design/` (signatures, Move/effect classification, error policy, slice
plan, pitfalls, test anchors). First-party packages are developed in this repo and **distributed
with the system** as vendorable subtrees (pkg-foundation model: consumers copy `pkg/<name>/` into
their project; a future fetch tool automates the copy). They remain ordinary pkg-layer packages —
explicitly vendored, never ambiently resolvable.

# pkg — web

> 🌐 **English** · [Japanese](./ja/web.md)

## Status

**PROPOSAL (2026-07-20)** — the surface below is designed and internally consistent, but three
forks (marked ⚖) await owner settlement before implementation. Plan of record:
`../15-gateway-workspace-plan.md` (this doc is its F2; the framework is the deliverable — the
gateway app is explicitly LATER, owner 2026-07-20). Hard prerequisite: **F1** (non-capturing fn
values as struct fields / array elements). F0 (pkg-foundation rules) enables `internal` modules and
layering enforcement but does not block building the package itself.

## Overview

`pkg.web` is the REST-API server framework: the **policy layer** over the `std.http` protocol floor
(`serve`/`accept`/`ctx.*`/`respond`/`respond_stream`). The layering rule (recorded 2026-07-18):
protocol — spec-defined, one correct answer — lives in `std.http`; convention/policy — route
patterns, handler shape, response sugar — lives here. `pkg.web` re-implements no protocol.

References: **Go 1.22 `net/http` ServeMux** is the primary design reference — method-aware pattern
routing (`{param}` wildcards), automatic 404/405 semantics, a standard-library-grade minimal
surface. chi/httprouter are the performance reference (radix-tree dispatch — deferred; v1 is a
linear scan, see Pitfalls P2). Middleware-chain frameworks (chi/gin/echo style) are explicitly NOT
the model for v1: middleware needs capturing escaping closures (deferred language feature) and is
policy Align has one-way alternatives for (a wrapper fn calls the next fn directly).

The route table is **data** — a visible array value of Copy structs, no registration side effects,
no globals, no reflection ("nothing hidden"; the compiler sees every route):

```align
import pkg.web

fn list_models(ctx: http_request_ctx, params: slice<str>) -> Result<(), Error> { ... }
fn get_model(ctx: http_request_ctx, params: slice<str>) -> Result<(), Error> {
  id := params[0]                              // the {id} capture, in pattern order
  ...
}

fn main() -> Result<(), Error> {
  routes := [
    web.get("/v1/models", list_models),
    web.get("/v1/models/{id}", get_model),
    web.post("/v1/chat/completions", chat),
  ]
  web.serve("127.0.0.1", 8080, routes)
}
```

## Signatures (proposed)

```text
// route construction — ⚖ FORK A (recommended form shown; see Forks)
web.get(pattern, handler)    -> route        // per-method constructors; pattern is a str literal
web.post(pattern, handler)   -> route
web.put(pattern, handler)    -> route
web.delete(pattern, handler) -> route

// the one handler signature ("one way"):
//   fn(ctx: http_request_ctx, params: slice<str>) -> Result<(), Error>
// `params` = the {name} captures as str views in pattern order (region-bound to ctx's
// request buffer). A handler responds via ctx (std.http: ctx.respond / ctx.respond_stream —
// both consume ctx) and uses params strictly before that consume (borrow-checked).

// serving
web.serve(host, port, routes) -> Result<(), Error>
//   Impure. Sequential accept loop (v1 recorded direction). Per request: match method+path
//   against the table → dispatch the handler; no match on path → 404; path matches but
//   method doesn't → 405; handler Err → 500 (after best-effort error response) — all three
//   automatic responses are framework policy with fixed minimal JSON bodies.

// request sugar
web.body_str(ctx) -> Result<str, Error>      // ctx.body() bytes as validated UTF-8 str view
//   JSON in: req: ChatReq := json.decode(web.body_str(ctx)?)?   (core.json as-is; no wrapper)

// response sugar
web.json(ctx, x)              -> Result<(), Error>  // 200, content-type: application/json,
                                                    // body = json.encode(x); consumes ctx
web.status_json(ctx, code, x) -> Result<(), Error>  // same with an explicit status code
web.no_content(ctx)           -> Result<(), Error>  // 204, empty body; consumes ctx
```

```text
// types
route   — Copy struct { method (tag-only enum), pattern: str, handler: fn(...) -> Result<(), Error> }
//        Copy because every field is Copy (a str view of a literal, a tag, a fn pointer):
//        the route table is plain data — buildable in a literal, storable, passable.
```

**Pattern syntax (⚖ FORK B, recommended form):** `/`-separated segments; a literal segment matches
byte-exactly; a `{name}` segment matches exactly one non-empty segment and captures it (order of
appearance = index in `params`). Consistent with Align's existing `{...}` hole syntax in
`template` literals (the same visual language for "a hole to be filled"), and with Go 1.22
ServeMux. No regex segments, no optional segments, one pattern form. A trailing `{name...}`
tail-wildcard (Go-style) is deferred until a consumer needs it. Trailing-slash: exact match only
(no implicit redirect — hidden behavior).

**Matching semantics (fixed, not configurable):** longest-literal-prefix wins over wildcard at the
same position (Go semantics — `/v1/models/featured` beats `/v1/models/{id}`); two routes that can
tie are a **construction-time abort** (duplicate/ambiguous table is a bug, caught at `serve`
startup, not per request). Query strings are not part of the pattern (use the std.http query floor).
Percent-decoding of captured segments follows the std floor (see std-prerequisites).

## Move/effect classification

```text
route            Copy value (str view + tag + fn pointer); never dropped; region = Static
                 for literal patterns (a computed pattern str would region-bind the route —
                 legal but unusual)
routes table     array<route> / fixed array — plain data, Copy elements
web.serve        Impure (network); borrows the table (never consumed); runs until Err
handlers         Impure allowed (they do I/O by nature); called through the stored fn value —
                 effect bits flow through FnTy (#465 machinery, already shipped)
web.json / etc.  Impure (socket write); CONSUME ctx (mirror ctx.respond)
web.body_str     Pure; returns a str view region-bound to ctx
```

The framework is **pure Align** — no `unsafe`, no FFI, no new runtime symbols. It is deliberately
the proof that the pkg layer needs nothing the language doesn't already give user code.

## Error policy

- `web.serve` returns `Err` only for setup failures (bind/listen). Per-request errors never kill
  the loop: a handler `Err` → the framework sends a minimal 500 JSON body (best-effort) and
  continues; a malformed request line/headers (std.http parse `Err`) → 400 and continue.
- Automatic bodies are minimal fixed-shape JSON (`{"error":{"code":404}}` style). Application
  error shapes (e.g. the OpenAI error object) are app policy — built by the app with
  `web.status_json`; the framework does not define a rich error vocabulary.
- No panics reachable from request data: pattern-table validation aborts at startup
  (programmer error), everything request-derived is `Result`.

## std.http prerequisites (consumer-arrived; std-side slices, not pkg.web code)

Recorded 2026-07-18 as std-bound-when-consumer-arrives; `pkg.web` is that consumer:

1. `ctx.query(name) -> Option<str>` + percent-decode (RFC 3986 — protocol, one correct answer).
2. SSE event framing helper (WHATWG-defined) — needed by the first streaming consumer
   (the gateway app later, not `pkg.web` v1 itself → lands with W4, may slip after v1).
3. (verify at W2) an accepted-request path/method accessor surface sufficient for dispatch —
   `ctx.method()` / `ctx.path()` already ship.

## Forks awaiting owner settlement (⚖)

- **A — route constructors:** per-method `web.get/post/put/delete(pattern, handler)`
  (RECOMMENDED: the universal REST reading; exactly one way to write a GET route; no
  stringly-typed method) vs one `web.route(m, pattern, handler)` with a `Method` enum (more
  literally "one constructor", but every call site grows a tag argument). Not both.
- **B — pattern syntax:** `{name}` (RECOMMENDED: consistent with `template` holes + Go 1.22) vs
  `:name` (Sinatra/Express/chi lineage). Not both.
- **C — params delivery:** positional `params: slice<str>` in pattern order (RECOMMENDED:
  zero-allocation, no map type, order visible in the pattern) vs named lookup
  (`web.param(params, "id")` — linear scan sugar; could be added later ON TOP of positional
  without breaking the signature). Settle whether positional-only is acceptable for v1.

## Slices (F3 of the plan; each lands PR → review → merge)

- **W1 — types + match engine.** `route`/`Method`, pattern parse + validation, the matcher
  (longest-literal-wins, ambiguity abort) as pure functions over `str`/segments. Unit-testable
  without sockets. Needs F1 (fn field) for the `route` struct itself.
- **W2 — serve + dispatch.** The accept loop over `std.http` serve/accept; method+path dispatch;
  automatic 404/405/400/500; params capture into the per-request flow. Integration tests via the
  in-process server pattern (`crates/align_driver/tests/m11_http.rs`).
- **W3 — request/response sugar.** `body_str`, `json`, `status_json`, `no_content`.
- **W4 — SSE sugar** (with/after the std SSE framing floor) — gated on the first streaming
  consumer; may land with the gateway app rather than v1.
- **W5 — hardening + bench.** Route-table edge matrix (ambiguity abort, empty table, deep paths,
  long segments); `bench/web_router` — dispatch overhead vs a hand-written `match` must be
  near-zero (the framework's existence claim); record the number.

## Pitfalls

- **P1 — the handler fn type must stay ONE type.** `fn(http_request_ctx, slice<str>) ->
  Result<(), Error>` — resist per-app generic handler signatures; app state threading (DB pools
  etc.) is a later, deliberate design (likely an explicit state param — but that changes the fn
  type and the route struct, so it must be one decision, not per-app drift).
- **P2 — linear-scan dispatch is v1-correct, not v1-lazy.** Fixed small REST tables (< ~100
  routes) scan faster than tree setup amortizes; the bench (W5) records the crossover evidence.
  A radix tree is a MEASURED follow-up (chi/httprouter reference), not a default.
- **P3 — params are views region-bound to ctx.** A handler that stores a param past
  `ctx.respond` consume is a borrow error by design (the #460 liveness machinery catches it);
  document `.clone()` as the escape hatch — do not "fix" by copying params eagerly.
- **P4 — no implicit response mutation.** Helpers consume ctx exactly like `ctx.respond`
  (Move discipline); there is no response-builder-carried-in-ctx pattern — a handler that wants
  headers uses std.http's `response_builder` directly.
- **P5 — startup validation must be total.** Every table defect (duplicate route, ambiguous
  pair, malformed pattern, empty pattern) aborts at `serve` startup with the offending pattern
  in the message — never a per-request surprise.

## Test anchors (planned)

`apps/gateway/pkg/web/` unit-style example entries per slice; driver integration tests
(`apps_web_*`) for W2's dispatch matrix; `bench/web_router` (W5). The gateway app (F4, later)
is the full-surface validation consumer.
