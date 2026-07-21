This directory holds the authoritative per-package design docs for **first-party `pkg` libraries**,
at the same depth as `../std-design/` (signatures, Move/effect classification, error policy, slice
plan, pitfalls, test anchors). First-party packages are developed in this repo and **distributed
with the system** as vendorable subtrees (pkg-foundation model: consumers copy `pkg/<name>/` into
their project; a future fetch tool automates the copy). They remain ordinary pkg-layer packages —
explicitly vendored, never ambiently resolvable.

# pkg — web

> 🌐 **English** · [Japanese](./ja/web.md)

## Status

**DESIGN v2 (2026-07-20, owner-directed; attribution corrected same day).** The owner's brief,
restored after a lost conversation record and now pinned here so it cannot be lost again: **the
deliverable is an Align-idiomatic, data-oriented, blazing-fast zero-copy REST framework — speed is
the headline, and bloat is rejected** (a small surface; nothing speculative). **References are
instrumental, not mandates:** when the owner asked "which existing framework matches this vision?",
*Claude* named **Go's Fiber** (fasthttp's zero-allocation philosophy) as the closest analogue —
the owner did not choose it, and a better existing reference may replace it at any point. The
router references are likewise instrumental: **httprouter/fasthttp** (the radix lineage) and
Rust's **matchit** (the minimal fastest-in-class radix matcher). The judging criterion for every
decision is Align-ness (data-oriented / nothing hidden / one way / minimal), never "because
framework X does it". **The router is a first-class requirement**: the first consumer app
(OpenAI-compatible, fixed paths) would not need one, but a REST framework does — so it gets a
deliberately Align-idiomatic design (below), not an afterthought. The gateway / LLM apps are
merely the framework's first consumers ("what we build with it happens to be LLM-related") — they
do not shape this design. Plan of record: `../15-pkg-web-plan.md`. Hard compiler prerequisite:
**F1 field-eligibility widening** (see Prerequisites).

## Minimalism (owner constraint)

The surface is exactly: routing, ctx accessors, responders, middleware-lite, SSE sugar — nothing
else. NO template engines, static-file servers, sessions, websockets, ORM hooks, config systems,
or lifecycle callbacks: each is a separate package if a consumer ever demands it. Every addition
must name its consumer; "frameworks have this" is not a reason ("one way" + the no-bloat brief).

## Why Align can win this

Fiber is fast because fasthttp refuses per-request allocation and reuses buffers — but Go still
pays GC, interface boxing, and copies at the `string([]byte)` boundary. Align's semantics make
Fiber's discipline the *default*: `std.http` already parses a request into ONE buffer + an offset
table (R1), every accessor is a `str`/`slice<u8>` **view** (zero-copy by construction, enforced by
regions — a leaked view is a compile error, not a use-after-free), SIMD JSON decodes into
view-bearing structs, and the per-request `arena {}` resets in O(1) with no GC ever. The framework's
job is to add **routing + ergonomics without breaking that chain**: nothing on the hot path may
copy request bytes or touch the heap. That is the existence claim, and W5/W7 make it a measured,
regression-pinned number.

## The performance contract (design invariants, bench-pinned)

```text
1. Zero request-byte copies      — path, params, query, headers, body: all views into the
                                   request buffer. The framework never materializes a string
                                   from request data. (.clone() is the app's explicit escape.)
2. Zero heap allocation/request  — framework hot path allocates nothing on the heap; per-request
                                   scratch lives in the request arena (O(1) bulk reset).
                                   Handlers allocate only what they visibly write.
3. O(segments) dispatch          — a startup-built radix structure (static > param > wildcard
                                   priority, httprouter/matchit semantics) stored the ALIGN way:
                                   flat contiguous arrays (a node table + an edge table indexed
                                   by offsets), not pointer-chasing heap nodes — the router is
                                   itself data-oriented (cache-line-friendly walks; the same
                                   design move as soa/tape/offset-table elsewhere in Align).
                                   No per-request pattern parsing, no regex, no map lookups.
                                   Param values land in a fixed slot array, not a map.
4. Zero-copy output              — response bodies encode straight into the response writer
                                   (the library-foundations "zero-allocation output" pattern);
                                   json.encode composes with the builder → socket path.
5. Startup-total validation      — the route tree is built and checked ONCE at serve() (conflict/
                                   ambiguity abort); the request path does no validation work.
```

Bench anchors (W5/W7): `bench/web_router` (dispatch ns/op vs a hand-written `match` — must be
within noise), `bench/web_e2e` (req/s: pkg.web vs a raw `std.http` accept loop — framework overhead
must be ≈ zero; plus a same-box Go Fiber comparison as the external honesty check — the target is
**competitive-or-better vs Fiber** on plaintext + JSON echo shapes).

## Surface (Fiber-informed, Align-idiomatic)

> **Call convention (owner-decided 2026-07-20):** call sites are **fully qualified** — `pkg.web.get(...)`,
> `pkg.web.param(c, "id")`, `c: pkg.web.Ctx` — per §17 D3 (no import aliases; provenance visible at
> every use). The bare `web.` / `web.Ctx` in the examples below is shorthand for `pkg.web.` and will be
> spelled out when the public surface lands (W2). This was reconciled when F0 settled D3.

```align
import pkg.web

// handlers: ONE signature — fn(c: web.Ctx) -> Result<response_builder, Error>
// The handler BUILDS a response and hands it back; the framework writes it.
fn get_model(c: web.Ctx) -> Result<response_builder, Error> {
  id := web.param(c, "id")               // str view into the request path
  m := lookup(id)?                        // `?` works: a failure becomes a 500
  web.json(json.encode(m))
}

fn main() -> Result<(), Error> {
  routes := [
    web.get("/v1/models", list_models),
    web.get("/v1/models/:id", get_model),
    web.post("/v1/chat/completions", chat),
  ]
  web.serve("127.0.0.1", 8080, routes)
}
```

No app object, no registration side effects, no globals, no reflection: the route table is a
visible array **value** of Copy structs; `serve` compiles it into the radix tree at startup.
(Align has no user-defined methods, so Fiber's `c.Params("id")` becomes the qualified
`web.param(c, "id")` — same ergonomics, one call convention.)

**Who owns the request handle (settled by the owner, 2026-07-20): `serve` does.** The first
implementation gave it to the handler — `Ctx` was a Move struct owning it, and responders consumed
it. Building the framework on that shape produced three dead ends, all the same root cause: every
accessor borrows from the context, so `web.param(c, name)` would consume the very context the
handler still had to answer through; reading a param and then responding was rejected outright (a
live borrow while `c` is moved); and "handler Err → 500" could not be implemented, because a handler
that fails has already consumed the handle and left nothing to respond through. Moving the handle to
`serve` dissolves all three: a handler becomes a function of the request that builds a response, and
the framework — which still holds the connection — writes it or answers 500. The compiler enabler
this needed was making `response_builder` a nameable type and a legal `Result` payload
(`docs/impl/std-design/http.md`).

## Signatures

```text
// routes (per-method constructors — the Fiber/Express reading; one way to write a GET route)
web.get(pattern, handler)     -> route
web.post(pattern, handler)    -> route
web.put(pattern, handler)     -> route
web.delete(pattern, handler)  -> route
web.patch(pattern, handler)   -> route

// grouping (pure data: prefix + routes → prefixed routes; no closures involved)
web.group(prefix, routes)     -> array<route>

// serving — Impure; sequential accept v1 (concurrency is the recorded, measured follow-up)
web.serve(host, port, routes) -> Result<(), Error>
//   startup: build + validate the radix tree (duplicate/ambiguous → abort with the pattern).
//   per request: parse (std.http, zero-copy) → split the request-target into path + query →
//   radix dispatch → handler → WRITE what it returned. Automatic responses: no path match → 404,
//   path-but-not-method → 405 (with Allow), handler Err → 500. The loop never dies per-request.

// ctx accessors (all Pure; all return views region-bound to c)
web.param(c, name)   -> str              // named :param capture (fixed slot array; total —
                                         //   a name not in the pattern is a startup-checkable
                                         //   bug → abort at tree build if statically absent)
web.query(c, name)   -> Option<str>      // std.http query floor (percent-decoded per RFC 3986)
web.header(c, name)  -> Option<str>      // NOT SHIPPED YET — see the note below
web.body(c)          -> slice<u8>        // SHIPPED 2026-07-21: `Ctx.body` carries the zero-copy
web.body_str(c)      -> Result<str, Error>    //   view; body_str = `.as_str()` (validated view)
//   JSON in: req: ChatReq := json.decode(web.body_str(c)?)?   — core.json, view-decoding
//   `web.header` blocker (recorded 2026-07-21): the Copy `Ctx` owns nothing, and an
//   arbitrary-name header lookup cannot ride a single stored view the way `body` does — it needs
//   either a raw-head `str`/`slice<u8>` view field + a pkg.web-side RFC 9110 lookup (duplicating
//   std.http's, against One way), or a std.http enabler exposing the parsed header table to a
//   detached view (the ideal shape — e.g. `ctx.headers()` as a view value the Ctx can carry).
//   Design the enabler first; do not ship a second lookup.

// responders (Pure; they BUILD a response — they do not touch the request handle, so a handler may
// call accessors and responders in any order)
web.json(body)               -> Result<response_builder, Error>  // 200 + application/json
web.status_json(code, body)  -> Result<response_builder, Error>
web.text(s)                  -> Result<response_builder, Error>  // 200 + text/plain
web.status_text(code, s)     -> Result<response_builder, Error>
web.status(code)             -> Result<response_builder, Error>  // status + empty body
//   `body` is the ENCODED document, not a value: Align has no user-written generics, so a
//   `json(x)` that encoded `x` is not expressible — and `web.json(json.encode(m))` is the better
//   reading anyway, since encoding's allocation stays visible in the handler (Nothing hidden).
```

```text
// types
web.Ctx    — the per-request context: a **Copy** struct of views (method, path, query, and the
             matched pattern). It owns NOTHING — the request handle stays with `serve`, and the
             views are valid for the handler call.
Route      — Copy struct { method: str, pattern: str,
                           handler: fn(Ctx) -> Result<response_builder, Error> }
```

**Pattern syntax (Fiber/httprouter lineage — settled by the restored reference):** `/`-separated;
literal segments match byte-exactly; `:name` matches exactly one non-empty segment and captures;
a trailing `*name` captures the rest (tail wildcard). Priority at each tree node: **static >
`:param` > `*wildcard`** (httprouter semantics — `/v1/models/featured` beats `/v1/models/:id`).
Two routes that can tie → startup abort. No regex, no optional segments, exact trailing-slash
matching (no hidden redirects). Query strings are never part of the pattern.

## Router internals (the W1 implementable spec)

The route table (visible data) compiles at `serve()` startup into a **flat radix structure** —
contiguous arrays, offset-indexed, zero pointers (the Align design move: soa/tape/offset-table):

```text
Node  { first_edge: i64, n_edges: i64,     // static children, sorted by label for binary search
        param_child: i64,                  // -1 or node index (the ONE :param child)
        wild_leaf: i64,                    // -1 or leaf index (the ONE trailing *name)
        leaf: i64 }                        // -1 or leaf index (a route ends at this node)
Edge  { label: str, node: i64 }            // label = one full literal segment (byte-compare)
Leaf  { method_handlers: array over Method // per-method handler fn or absent → the path's
                                           //   method set IS this row (405 Allow comes free)
        param_names: slice<str>, n_params: i64 }   // pattern-order names for web.param lookup
```

Build (startup, plain heap — freed at serve exit): insert each route segment-by-segment; a
literal segment adds/finds a static edge; `:name` claims the node's single param child (two
different `:a`/`:b` names at the same position = conflict → **abort** naming both patterns);
`*name` claims the single wildcard leaf (must be trailing; conflicts abort); a duplicate
(method, path) leaf → abort. Sort each node's edges; store param names per leaf.

Match (per request, zero alloc): split the path by `/` (in place — offsets, no copies); walk from
the root; at each node try static edges (binary search on the segment) FIRST, else the param
child (capture the segment view into the fixed slot array `params[i]`), else the wildcard leaf
(capture the uneaten remainder incl. `/`s); at the end, the leaf's method row gives the handler
(present → dispatch; absent but row non-empty → 405 + Allow from the row; no leaf → 404). Static
beats param beats wildcard at EVERY node, **with backtracking** (matchit semantics — settled
2026-07-21 by the #591 review): when the preferred branch dead-ends deeper in the path, the walk
unwinds and tries the next alternative, so `{/a/featured, /a/:id/versions}` routes
`/a/featured/versions` to the `:id` row. The oracle's `match_score` is **fixed-width base-3,
left-aligned to the path's segment count** (static 2 / param 1 / wildcard 0, the wildcard's
absorbed positions zero-filled) — i.e. genuinely lexicographic left-to-right, which makes the
walk's first success equal the oracle's max for EVERY table, so no route-set shape needs a
build-time ambiguity abort. (Two review findings settled this: an earlier draft said
no-backtracking + abort, but that abort would have rejected exactly such realistic tables, and
the linear scan — production dispatch before the tree — already matched them; and the oracle's
original un-shifted fold compared MAGNITUDES, ranking `/:cat/:slug` over `/assets/*file` on
`/assets/logo` against the httprouter/matchit/Fiber reference — the left-alignment fixed the
oracle to the documented left-to-right intent.) Duplicate (method, path) rows and conflicting
param names remain build-time aborts.
`web.param(c, "name")` = linear scan of the ≤ n_params name views (n is tiny; no map).

## Prerequisites (compiler / std — the 土台)

- **F1 — field-eligibility widening (the one hard language slice).** `web.Ctx` and `Route` need
  struct fields beyond today's whitelist (probed 2026-07-20: `fn` in a field errors "struct fields
  must be a primitive scalar, str, or a plain struct"): ① a **fn value** field (Copy pointer —
  the `Route.handler`; effect bits flow via FnTy, #465), ② a **Move handle** field
  (`http_request_ctx` inside `Ctx` — makes `Ctx` a Move struct; drop/move machinery for Move
  fields already exists via the J3a Move-enum-field work), ③ a **`slice<str>`** field (the param
  slots — view slices, region-tracked like `str` fields). Each reuses existing classification
  machinery; the slice widens `is_field_ok` + the layout/drop/region sweeps. Capturing escaping
  closures stay OUT (unchanged deferral).
- **F0 — pkg-foundation rules** (`internal` + pkg-layering import checks + spec text): enables
  `pkg.web.internal.*` modules (the radix tree lives there) — parallelizable with F1.
- **std.http floor items (consumer arrived):** `ctx.query` + percent-decode (protocol → std);
  SSE event framing (WHATWG) when the first streaming consumer lands (the LLM app — W6+).

## Move/effect classification

```text
Route / route table   Copy data (str view of a literal + tag + fn pointer); Static; never dropped
the radix tree        built once inside serve (arena- or startup-heap-owned; freed at serve exit)
web.Ctx               Move struct (owns the request handle); created by serve per request,
                      consumed exactly once by a responder; params are views (never dropped)
web.serve             Impure; borrows the table; runs until setup-Err
accessors             Pure; views region-bound to c (escape past the responder = compile error)
responders            Impure; consume c
handlers              Impure allowed; called through Route.handler (FnTy effect bits, fail-closed)
```

The framework is **pure Align** — no `unsafe`, no FFI, no new runtime symbols; it is the proof the
pkg layer needs nothing user code doesn't have.

## Error policy

`serve` returns `Err` only for setup (bind/listen/tree-build abort is a startup abort, not Err —
programmer error). Per-request: framework maps malformed requests to 400, unmatched to 404/405,
handler `Err` to 500 — fixed minimal JSON bodies, loop continues. Application error vocabularies
(e.g. the OpenAI error object) are app policy via `web.status_json`. Nothing request-derived can
panic; everything is `Result` or a view.

## Middleware (redesigned 2026-07-21 for the settled ownership model — lands W6)

Fiber's `c.Next()` chain needs capturing closures (deferred). The framework-owns-the-handle model
makes the v1 shape simpler than the original Move-threading design: `Ctx` is Copy, so a
pre-handler neither consumes nor returns it —

```text
fn(c: Ctx) -> Result<Option<response_builder>, Error>
//   None      -> proceed to the next pre-handler / the handler
//   Some(rb)  -> short-circuit: serve writes rb, the handler never runs (auth reject, redirect)
//   Err       -> 500, same as a handler Err
```

`Option<response_builder>` is a legal payload since #583. Groups carry the list:
`web.group_with(prefix, [auth, log], routes)`. Covers auth/logging/CORS headers without closures;
stateful middleware waits for the capturing-closure feature and a real consumer.


## Streaming (SSE + generic) — designed 2026-07-21, lands W6

**The problem.** A handler is `fn(Ctx) -> Result<response_builder, Error>` — it builds ONE complete
response. SSE and LLM token streams instead hold the connection and write incrementally, so
streaming needs a second interaction model. The settled ownership rule extends rather than breaks:
**the framework owns the request handle for the whole request; a stream handler additionally owns
the response STREAM** — which only exists once the response head is committed, the exact moment the
framework's ability to answer differently (404/405/500) has ended anyway. Nothing is given up.

### Surface

```align
// The second (and last) handler signature, scoped to stream routes. Borrows the request through
// `c` (valid for the whole call — serve still holds the handle) and OWNS the response stream.
fn events(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {
  web.send_event(s, "tick")?
  s.finish()
}

routes := [
  web.get("/v1/models", list_models),
  web.sse("/v1/events", events),                                   // GET; text/event-stream
  web.stream("POST", "/v1/chat/completions", "application/x-ndjson", chat),
]
```

```text
web.sse(pattern, pump)                       -> route   // method GET (EventSource always GETs),
                                                        //   Content-Type text/event-stream
web.stream(method, pattern, content_type, pump) -> route // the general form
web.send_event(s, data)  -> Result<(), Error>           // one `data: {data}\n\n` frame, one send;
                                                        //   single-line data (multi-line = caller)
```

### Types

```text
Handler {
  Respond(fn(Ctx) -> Result<response_builder, Error>),
  Stream(fn(Ctx, http_stream) -> Result<(), Error>),
}
Route { method: str, pattern: str, stream_type: str, handler: Handler }
//   stream_type: the stream head's Content-Type; "" on Respond routes (never read).
```

One table, one dispatch: stream routes go through the same radix tree, the same method resolution,
and contribute to 405 `Allow` like any row. `Handler` is the Align-idiomatic or-kind — a sum type,
NOT two fn fields with filler fns (rejected: a filler is a magic sentinel) and NOT a second route
table (rejected: splits priority/405 across tables).

### serve semantics

```text
match r.handler {
  Respond(h) => rb := answer(h, c); ctx.respond(rb) else {}          // unchanged
  Stream(pump) => {
    rb := http.response(200)
    rb.header("Content-Type", r.stream_type)
    rb.header("Cache-Control", "no-cache")            // a cached stream is nonsense; always set
    s := ctx.respond_stream(rb) else { <skip pump> }  // client already gone -> next request
    pump(c, s) else {}                                // Err after the head: nothing to answer
  }
}
```

- `respond_stream` is a **non-consuming** bound receiver (std.http change ①, below): `ctx` stays in
  serve's frame, so `c`'s views stay valid for the whole pump call — this is what makes
  `fn(Ctx, http_stream)` well-formed under the borrow rules. The fd is lifted into the stream; `ctx`
  is spent (a second respond is `Err`); its drop frees the parse buffer only.
- **The head is lazy** (std.http change ②): `respond_stream` stores the head; the first `send` (or
  `finish`) writes it. Before that, `s.reject(rb) -> Result<(), Error>` (std.http change ③)
  discards the stored head and writes `rb` as a complete NORMAL response instead — after a send it
  is `Err`. This is what gives a stream route its 4xx window: parse/validate the request inside the
  pump, `return s.reject(...)` on bad input, stream on good input — one fn, no separate validate
  phase (rejected: a per-route validate fn doubles the parse work and bloats Route).
- After the first send there is NO error window, by HTTP's own rules: a pump `Err` mid-stream just
  ends the stream (drop closes the fd; the client sees termination). Same silent-`Err` posture as a
  handler Err — the W4 logging story covers both.
- The loop never dies per request, unchanged.

### Ordering constraint (hard)

v1 `serve` is sequential — **an open stream starves every other client**. Streaming must land
together with (or after) the recorded concurrent-serve follow-up; shipping it on the sequential
loop is test-only. This is an ordering note, not a design dependency: nothing above changes shape
with concurrency.

### Enablers (probed 2026-07-21; in implementation order)

1. **`http_stream` nameable in source — DONE.** A `resolve_type` entry, the exact #583
   `response_builder` pattern; `http_stream` was already a full `Scalar`/`Ty` (the `respond_stream`
   `Ok` payload, with `.send`/`.finish`), so only the source spelling was missing. Pinned by
   `crates/align_driver/tests/http_stream_nameable.rs` (param/return spelling, nullary, still not an
   array element).
2. **fn value as enum variant payload — DONE.** A new `Scalar::Fn(u32)` variant (there was none — a
   fn value was `Ty::Fn` with no scalar form, so a variant payload could not represent one). A fn
   value is Copy `{fn_ptr, env_ptr}` (16 bytes, 8-align), so a fn-only enum is non-Move and never
   dropped; a mixed enum's tag-switched drop skips the fn slot. The #583 checklist was swept —
   `scalar_to_ty`, MIR `sort_key_order` (fail-closed arm), codegen `scalar_bytes` (unreachable), and
   the codegen `scalar_type` fn arm that reserves the 16-byte slot instead of the catch-all's silent
   `i32`. Construction compares fn payloads **by signature, not `fn_types` id** (each `fn` expr
   interns a fresh `FnTy`). `ty_to_scalar(Ty::Fn)` stays `None` (fn is a variant payload only, not an
   `Option`/`Result`/`box` payload). Pinned by `crates/align_driver/tests/fn_variant_payload.rs`
   (dispatch, the real `Handler` signature, Copy/no-drop, cross-module round-trip via
   `align_interface`, the `Route { handler: Handler }` array shape, mixed fn+Move-array drop, and a
   wrong-signature reject). **Deferred (fail-closed, no consumer):** a *generic* sum type with a fn
   payload — rejected at the template payload resolver — is not shipped half-built.
3. **A fn-value signature with a Move-handle param — DONE (verified via a Move-value proxy).** #573
   nulls the owned arg in the caller's frame after an INDIRECT call; a 200k-loop test drives an owned
   `array<i64>` (the `http_stream` stand-in) by value through a match-extracted fn payload and
   asserts no double-free (completion, not a signal exit) plus a move-after-use reject. The real
   `http_stream` receiver awaits enabler 4.
4. **std.http `respond_stream` rework — DONE** (changes ①–③, shipped 2026-07-21). The ctx is
   borrowed and left SPENT on success (a later `respond`/`respond_stream` is `Err`; a validation
   `Err` leaves it unspent), the head is lazy (stored in the stream; first `send`/`finish` writes
   it), and `s.reject(rb)` answers pre-stream with a complete normal response. Full shipped record
   in `docs/impl/std-design/http.md` item 8; the M12 tests were updated outright
   (`m12_http_stream.rs`, 13 — including a mid-pump `ctx.path()` read, the exact stream-handler
   borrow shape enabler 5 needs).

### Backlog (recorded, not v1)

Heartbeat/keep-alive comments, `event:`/`id:` fields + `Last-Event-ID` resume, multi-line
`send_event` data splitting, per-request head customization on stream routes, and stream
timeouts/backpressure — each waits for a consumer.

## Slices (F3 of the plan)

- **W1 — router core.** Pattern parse + validation; the **radix tree** (static/param/wildcard
  nodes, priority order, conflict detection) + matcher as pure functions over path segments;
  param slot capture. Unit-tested against a linear-scan oracle (differential). Needs F1①.
- **W2 — Ctx + serve + dispatch.** `web.Ctx` (needs F1②③); the accept loop over std.http;
  automatic 404/405/400/500; `group`. Integration tests via the in-process server pattern
  (`crates/align_driver/tests/m11_http.rs`).
- **W3 — accessors + responders.** param/query/header/body/body_str; json/status_json/text/status.
- **W4 — hardening.** Route-tree edge matrix (conflicts, deep paths, long segments, empty table,
  `*` tails, method sets); malformed-request matrix; keepalive reuse.
- **W5 — the router/e2e bench gate.** `bench/web_router` + `bench/web_e2e` vs raw std.http
  (≈ zero overhead required) — the performance contract becomes a pinned regression.
- **W6 — middleware-lite + streaming** — both DESIGNED (sections above, 2026-07-21); implementation
  gated on the streaming enablers and (for streams in production) concurrent serve.
- **W7 — the external comparison.** Same-box Fiber (Go) plaintext + JSON-echo benches; record the
  numbers and the gap analysis in this doc.

## Pitfalls

- **P1 — one handler signature, forever.** `fn(Ctx) -> Result<(), Error>`. App state (DB pools…)
  is a future single deliberate decision (likely an explicit state param changing the fn type once
  — never per-app drift).
- **P2 — the radix tree is the design, not an optimization.** Linear scan exists only as the W1
  differential-testing oracle. (Fiber/httprouter is the reference precisely for dispatch.)
- **P3 — params/views escape discipline.** A view stored past the responder consume is a compile
  error by design (#460 liveness); document `.clone()` as the explicit escape — never eager-copy
  "to be safe" (that breaks invariant 1).
- **P4 — no hidden response state.** Responders consume `Ctx` (Move); no builder-inside-ctx
  mutation pattern. Headers beyond the sugar → std.http `response_builder` directly.
- **P5 — nothing on the hot path may allocate.** Every W-slice PR states where its bytes live
  (view / arena / startup); the W5 bench is the enforcement, but review checks it first.
- **P6 — 405 needs the per-path method set** from the tree (Allow header) — design it into the
  node layout in W1, not bolted on in W4.

## Test anchors (planned)

Workspace `apps/web/` (the framework author workspace: `pkg/web/` + example/test entries beside
it); driver integration tests `apps_web_*` (W2/W4 matrices); `bench/web_router` / `bench/web_e2e`
(W5) / the Fiber comparison (W7). The LLM gateway app (later, separate) is the first full-surface
consumer — not part of this package's acceptance.
