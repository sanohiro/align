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
  web.serve("127.0.0.1", 8080, routes, 4)
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

// serving — Impure; `workers` request loops (prefork, SO_REUSEPORT — see "Concurrent serve")
web.serve(host, port, routes, workers) -> Result<(), Error>
//   startup: validate the table (malformed → abort with the pattern); `workers < 1` aborts too.
//   per request: parse (std.http, zero-copy) → split the request-target into path + query →
//   radix dispatch → handler → WRITE what it returned. Automatic responses: no path match → 404,
//   path-but-not-method → 405 (with Allow), handler Err → 500. The loop never dies per-request.
//   `workers == 1` runs that loop inline on the calling thread (no threads at all); `>= 2` spawns
//   that many workers, each with its OWN listener. A connection is kept alive inside std.http, so
//   the loop shape is identical either way.

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
  s.send_event("tick")?
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
s.send_event(data)       -> Result<(), Error>           // one `data: {data}\n\n` frame, one send;
                                                        //   single-line data (multi-line = caller);
                                                        //   send_event("") = a legal empty event
```

**`send_event` is a `http_stream` METHOD, not a `web.*` free fn** (revised while shipping enabler
5 — the surface was first sketched as `web.send_event(s, data)`). Two reasons, one decisive: a
pkg-level free fn takes a Move handle BY VALUE — Align has no user-fn borrow params (borrowing is
the std bound-receiver mechanism, the recorded `io.copy` restriction) — so
`web.send_event(s, …)?; s.finish()` cannot compile: the wrapper would consume the very stream the
pump still has to finish. And SSE event framing was already committed as a std.http floor item
("SSE event framing (WHATWG) when the first streaming consumer lands", Prerequisites above), so
the framing lives with the other stream writes — `send` / `send_event` / `finish` / `reject`, one
method family on one handle. pkg.web ships NO wrapper (the `web.header` no-duplication rule).

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
    match ctx.respond_stream(rb) {
      Ok(s) => pump(c, s) else {}                     // Err after the head: nothing to answer
      Err(e) => {}                                    // client already gone -> next request
    }
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

### Ordering constraint (hard) — LIFTED 2026-07-21

v1 `serve` was sequential — **an open stream starved every other client** — so streaming shipped
test-only, gated on the concurrent-serve follow-up. That follow-up is now SHIPPED ("Concurrent
serve (prefork)" below): a stream occupies exactly one of `W` workers and the other `W - 1` keep
serving, which turns the gate into a visible sizing decision (`workers >= expected concurrent
streams + 1`). Pinned by `crates/align_driver/tests/apps_web_prefork.rs`
(`a_held_open_stream_occupies_one_worker_while_the_others_serve`). Nothing in the streaming design
changed shape with concurrency, exactly as this note predicted.

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
5. **pkg.web wiring — DONE (2026-07-21).** `Handler` (`Respond`/`Stream`) + `Route.stream_type` in
   `types.align`; `web.stream` / `web.sse` constructors; `serve`'s stream arm exactly as the
   pseudocode above; and `s.send_event(data)` as the std.http method (WHATWG `data: {data}\n\n`,
   ONE write sharing the lazy head's buffer — head + framing + event in a single `send`; an empty
   event is a real frame, unlike `send("")`'s no-op; runtime `align_rt_http_stream_send_event` over
   the shared `http_stream_send_parts`). En route, fixed a **MoveCheck false positive**: a
   match-arm binding consumed inside a `loop` body poisoned the back-edge fixpoint (arm bindings
   never cleared their moved bit on (re)initialization, unlike `Let` — exactly serve's
   `Ok(s) => pump(c, s)`). E2E: `crates/align_driver/tests/apps_web_stream.rs` (3 — SSE frames +
   mid-pump `param`/`has_query`/`body` reads, the reject 4xx window with the loop surviving, and
   one-table coexistence: stream-route 405 `Allow` + 404), `m12_http_stream.rs` (+1 `send_event`),
   runtime unit framing test, and a sema regression pin for the MoveCheck fix. **No longer
   test-only**: concurrent serve shipped the same day (the prefork section below), so the hard
   ordering note is lifted and a stream costs one worker, not the server.

### Backlog (recorded, not v1)

Heartbeat/keep-alive comments, `event:`/`id:` fields + `Last-Event-ID` resume, multi-line
`send_event` data splitting, per-request head customization on stream routes, and stream
timeouts/backpressure — each waits for a consumer.

## Concurrent serve (prefork) + connection keep-alive — designed + SHIPPED 2026-07-21

**The problem, twice over.** v1 `serve` is one blocking loop: an open SSE/chat stream starves
every other client (the hard ordering note above — production streaming is gated on this), and
one-request-per-connection makes the W5/W7 bench meaningless (a keep-alive'd Fiber measures
requests; a close-per-request Align would measure TCP handshakes). One design covers both.

### The design: prefork, not shared state

`serve` gains a visible worker count. `W` workers each own their **own** listener on the same
port (`SO_REUSEPORT` — the fasthttp/nginx prefork lineage) and each run the EXISTING sequential
request loop, unchanged. The kernel balances incoming connections across the listeners.

```text
pkg.web.serve(host, port, routes, workers) -> Result<(), Error>   // outright signature change
//   workers == 1  -> exactly today's loop, on the calling thread (no task_group, zero threads)
//   workers >= 2  -> task_group { spawn W workers }; each worker: its OWN http.serve_shared
//                    listener + the unchanged accept/dispatch/respond loop
//   workers <  1  -> startup abort (the validate class: a programmer-config error)
//   workers >  process.cpu_count() + 1 -> the SAME abort (found while implementing): a worker never
//                    returns, and task_group runs its tasks on a pool sized by the available
//                    parallelism plus the calling thread, so tasks past that count never start at
//                    all. Aborting keeps the promise the parameter makes instead of silently
//                    serving with fewer loops. NOTE this makes the cap machine-dependent: a source
//                    line that names a fixed count aborts on a smaller box (or under a cgroup CPU
//                    quota, which `cpu_count()` respects) — write `workers = process.cpu_count()`.
```

- **Nothing hidden, by parameter:** thread creation is visible at every call site — `serve(...,
  4)` says four threads in the source. `spawn` inside `serve` is ordinary pkg-level Align
  (`task_group { spawn(fn { worker(...) }) }`), not runtime magic.
- **No shared mutable state, by construction.** The alternative — one listener handle shared by
  N workers — was rejected by the language itself: a Move `http server` handle cannot be
  captured by value into N closures, and no borrow crosses a `spawn`. `SO_REUSEPORT` dissolves
  the sharing: each worker owns its own Move listener, its own parked keep-alive slot, its own
  request loop — zero locks, zero contention, the same "no shared mutable state" rule `spawn`'s
  by-value capture already enforces (draft §Task Group).
- **The route table is the only shared input** — a `slice<Route>` of Copy rows, captured by
  value (a 16-byte view descriptor) into each worker; the backing array outlives the structured
  `task_group` by regions. **Probed 2026-07-21: this whole shape compiles and runs correctly
  TODAY** — Impure spawn bodies, loop-spawned workers each capturing the slice view + their
  worker index, indirect calls through the fn-value `Route.handler` on real threads, `wait()?`.
  **There is no compiler enabler**; the arc is std.http work only.
- **Error semantics (partial degradation).** A worker that hits a listener-level fault returns
  its `Err` and dies; the other workers keep serving. `wait()?` joins ALL tasks, so `serve`
  returns — with the first error — only when EVERY worker has died. Per-request faults never
  kill a worker (unchanged). **Transient `accept(2)` errnos never surface either** (shipped in
  std.http, http.md item 9): `EINTR`/`ECONNABORTED` retry, and `EMFILE`/`ENFILE` reclaim the
  coldest parked keep-alive connection and retry. So "a listener-level fault" now means exactly
  that — a worker no longer dies because a client reset its SYN or the fd table filled up. No
  errno reaches Align's `Error` for it; the classification lives entirely under `accept`.
- **Streaming unblocked.** An open stream occupies exactly its own worker; `W - 1` keep
  serving. Production streaming's gate becomes "run with enough workers", a visible capacity
  decision in the app's source — record `workers >= expected concurrent streams + 1` as the
  sizing rule of thumb in the fn doc.
- **Sizing:** the bench gate runs `workers = process.cpu_count()`; the fn doc recommends it as the
  default, and it is also the cap (above). That accessor is a std addition this arc required — the
  recommended sizing was previously unwritable in Align (`std-design/process.md`).

### Keep-alive rides entirely in std.http

The request loop does not change for keep-alive — `srv.accept()` simply learns to yield the
next request from a kept-alive connection before accepting a new one, and `ctx.respond` learns
to hand an eligible connection back instead of closing it. The full protocol design (eligibility,
the single parked slot, poll preference, the no-pipelining rule, the `Connection` header change,
drop-order safety) is std.http item 9 — `docs/impl/std-design/http.md`. pkg.web's serve loop is
byte-identical before and after; only the prefork wrapper above is pkg-side work.

### Slices (implementation order)

1. **std.http `http.serve_shared(host, port)`** — DONE. The `SO_REUSEPORT` listener as a SIBLING op
   (`http.serve` keeps strict-bind semantics: an accidental double server must still fail
   loudly; reuse is an explicit choice, the `respond`/`respond_stream` sibling precedent).
2. **std.http keep-alive** — DONE (item 9 ②: parked slot + poll + eligibility, tested against the
   sequential serve before prefork existed). **One behavioral consequence for every caller:** an
   eligible 1.1 request leaves the connection open, so a read-to-EOF client must now send
   `Connection: close` (the driver tests' shared `one_shot` helper) or frame by `Content-Length`.
3. **pkg.web prefork** — DONE. `serve(host, port, routes, workers)`, the `workers == 1` inline
   path (strict bind, zero threads), the `task_group` wrapper, and every call site updated
   outright. The worker body is the factored-out `worker(host, port, routes, shared)`, whose bind
   line is `srv := if shared { http.serve_shared(…)? } else { http.serve(…)? }` — an ordinary
   value-carrying `if`, so no new nameable `http_server` type was needed. Needed NO compiler
   enabler, as probed — but it DID need one std addition, `process.cpu_count()`, because
   `task_group` dispatches onto a pool sized by the available parallelism: more never-returning
   workers than that would never start, so `serve` aborts above the cap and the recommended
   `workers = cores` sizing is now writable. `apps_web_prefork.rs` counts the listeners actually
   bound (`/proc/net/tcp`), so "spawned W tasks" can never again be mistaken for "W loops run".
4. **W5 bench gate** (`bench/web_router`, `bench/web_e2e` keep-alive'd, `workers = cores`) —
   only now is the Fiber comparison honest. THE REMAINING SLICE. **Its first measurement
   (2026-07-21) found and fixed the blocker before any bench file existed:** dispatch cost
   1319 ns/op on a 6-route table and 708 ns/op on a 2-route one — scaling with TABLE SIZE, because
   `best_path_route` rebuilt the radix structure per call, contradicting contract item 3 ("a
   startup-built radix structure … no per-request pattern parsing"). The tree is now built ONCE by
   `router.build_tree(routes)` in `worker`, before the accept loop, as a single flat `array<i64>`
   (offset header + ten contiguous columns), and matched per request by `tree_best_path` —
   **1319 → 57 ns/op**, a 23× drop. The two O(table) scans inside dispatch went with it (per-node
   edge chains + a same-pattern route chain, both built once). `dispatch_routes` /
   `method_not_allowed` / `allow_methods` take the built tree; `best_path_route` remains as the
   build-and-match convenience for the differential tests. **Item 3 is still not met.** `bench/web_router` measures it (6-route vs 128-route
   table) and reports a slope, not 1.00×. Three things about that number, all found by an
   adversarial review of the bench itself: the remaining cost is the **per-node sibling scan** (a
   node's static edges are a linked chain with a string compare each — a miss on a flat 128-route
   namespace still costs ~0.4 µs and the per-route slope is unchanged by the chains, because the
   chain IS the node's children); the published row **conflates path depth and chain position with
   table size** (depth-matched and head-positioned, the 128-route table is flat — 49.9 ns vs
   57.0 ns), so it is a report rather than a gate until it measures the same path against both
   tables; and one ordering bug was caught there — appending edges at the chain HEAD made
   first-registered routes the last candidate (`/r0` 23.7 → 394.5 ns/op), now fixed by appending at
   the tail. The remaining levers are a sibling index (first-byte bucket / sorted edge run) and the
   per-edge `Route` struct copy forced by `routes[i].pattern` being rejected through a
   `slice<struct>`.

## Slices (F3 of the plan)

- **W1 — router core.** Pattern parse + validation; the **radix tree** (static/param/wildcard
  nodes, priority order, conflict detection) + matcher as pure functions over path segments;
  param slot capture. Unit-tested against a linear-scan oracle (differential). Needs F1①.
- **W2 — Ctx + serve + dispatch.** `web.Ctx` (needs F1②③); the accept loop over std.http;
  automatic 404/405/400/500; `group`. Integration tests via the in-process server pattern
  (`crates/align_driver/tests/m11_http.rs`).
- **W3 — accessors + responders.** param/query/header/body/body_str; json/status_json/text/status.
- **W4 — hardening.** First slice SHIPPED 2026-07-21: **startup table validation**
  (`router.validate`, pure diagnosis; `serve` prints to stderr + `process.abort()` before binding
  — the error policy's "startup abort, not Err"): known uppercase method or `""`, leading-`/`
  pattern, named `:`/`*` segments, `*` tail-only, no parameter name twice in a pattern, a
  non-empty `stream_type` on every Stream row (an empty one would emit a blank `Content-Type:`,
  and `stream_type == ""` is the invariant the HEAD fallback reads as "a Respond row"), and no
  duplicate PATH CLAIM the later row can never win — same method twice (also what duplicated the
  405 `Allow` join) or any row after an any-method route on that claim; parameter names don't
  affect a claim (`/a/:x` ≡ `/a/:y`). Specific-then-`any` on one pattern stays legal (the fallback
  direction). **HEAD is RFC-correct** (9110 §9.3.2): std.http's `respond` suppresses the body for
  a HEAD request at the protocol boundary (its `Content-Length` still sent), and `serve` routes a
  HEAD with no explicit row to the path's GET handler (Respond rows only — a stream head has no
  HEAD form, so stream-only GET keeps HEAD at 405). **Automatic 404/405/500 carry the fixed
  minimal JSON bodies** (`{"error":"not found"}` / `"method not allowed"` / `"internal error"`,
  `Content-Type: application/json`). Tests: `apps_web_validate.rs` (9 aborts + the legal-shadow
  serve), `apps_web_root.rs` HEAD/body matrix, runtime serializer unit. Keep-alive reuse is
  SHIPPED (std.http item 9 ②; `apps_web_root.rs` keep-alive E2E). Remaining W4: route-tree
  edge matrix (deep paths, long segments, empty table), malformed-request matrix, and the
  handler-`Err` logging story (W5+).
- **W5 — the router/e2e bench gate. MET.** `bench/web_e2e` prices the framework at **0.8 µs per
  request** over the same responses written directly on `std.http` (CONNS=1 ping-pong; 0.98–1.00×
  at 32 connections). It also prices everything else, which is why it was built before optimising
  the router further: **Align's whole protocol path is 4.1 µs/req** above a minimal-Rust floor,
  dispatch is 35 ns, so the router is **0.9% of Align's own path** and its remaining levers are
  worth ~0.1%. The real budget is the 4.1 µs — candidates inside it: the `poll` syscall keep-alive
  adds per request, and sharing one buffer between the parse and the response write.
  `bench/web_router` remains a report (its scaling row conflates depth and chain position); making
  it a gate is recorded there.
- **W6 — middleware-lite + streaming** — both DESIGNED (sections above, 2026-07-21). Streaming is
  **WIRED, pinned E2E, and no longer production-gated**: concurrent serve SHIPPED 2026-07-21 (the
  prefork section above), so a stream costs one worker instead of the whole server.
  Middleware-lite remains designed-only.
- **W7 — the external comparison. DONE.** Same box, same generator, same request, both sides in
  their prefork configuration (Fiber's own throughput recommendation, and the analogue of
  `serve(..., workers)`): **pkg.web 491,505 req/s vs Fiber-prefork 374,393 — 1.31×**, plus 1.19× on
  single-connection latency (70.2 vs 81.2 µs) and 2.5× at the p99 tail (210 vs 533 µs). Against Go's
  `net/http`: 5.9×. In protocol-path terms, measured against a minimal-Rust floor: **Align 4.1 µs
  per request, Fiber 17.9 µs, net/http 37.7 µs.** Numbers, the non-prefork Fiber row (4.3× slower
  than its own prefork mode), and the caveats — our own generator rather than `wrk`, WSL2 loopback,
  32 connections is a small load, neither side tuned — are in `bench/web_e2e/README.md`.

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
