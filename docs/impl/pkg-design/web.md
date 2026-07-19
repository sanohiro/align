This directory holds the authoritative per-package design docs for **first-party `pkg` libraries**,
at the same depth as `../std-design/` (signatures, Move/effect classification, error policy, slice
plan, pitfalls, test anchors). First-party packages are developed in this repo and **distributed
with the system** as vendorable subtrees (pkg-foundation model: consumers copy `pkg/<name>/` into
their project; a future fetch tool automates the copy). They remain ordinary pkg-layer packages —
explicitly vendored, never ambiently resolvable.

# pkg — web

> 🌐 **English** · [Japanese](./ja/web.md)

## Status

**DESIGN v2 (2026-07-20, owner-directed).** The owner's brief, restored after a lost conversation
record and now pinned here so it cannot be lost again: **the deliverable is a blazing-fast,
zero-copy REST framework — speed is the headline, not a byproduct.** The primary reference is
**Go's Fiber** (the zero-allocation philosophy of its fasthttp foundation + Express-derived
ergonomics); the router follows the **httprouter/fasthttp radix-tree lineage** (a separate,
deliberate reference — Fiber for the framework model, the radix router for dispatch). The
gateway / LLM apps are merely the framework's first consumers ("what we build with it happens to
be LLM-related") — they do not shape this design. Plan of record: `../15-pkg-web-plan.md`.
Hard compiler prerequisite: **F1 field-eligibility widening** (see Prerequisites).

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
3. O(segments) dispatch          — a startup-built radix tree (static > param > wildcard priority,
                                   httprouter semantics); no per-request pattern parsing, no
                                   regex, no map lookups. Param values land in a fixed slot
                                   array, not a map.
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

```align
import pkg.web

// handlers: ONE signature — fn(c: web.Ctx) -> Result<(), Error>
fn get_model(c: web.Ctx) -> Result<(), Error> {
  id := web.param(c, "id")               // str view into the request path
  m := lookup(id)
  web.json(c, m)                          // encode → response writer; consumes c
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
//   per request: parse (std.http, zero-copy) → radix dispatch → handler; automatic responses:
//   no path match → 404, path-but-not-method → 405 (with Allow), parse error → 400,
//   handler Err → 500 (best-effort). Fixed minimal JSON bodies; the loop never dies per-request.

// ctx accessors (all Pure; all return views region-bound to c)
web.param(c, name)   -> str              // named :param capture (fixed slot array; total —
                                         //   a name not in the pattern is a startup-checkable
                                         //   bug → abort at tree build if statically absent)
web.query(c, name)   -> Option<str>      // std.http query floor (percent-decoded per RFC 3986)
web.header(c, name)  -> Option<str>
web.body(c)          -> slice<u8>
web.body_str(c)      -> Result<str, Error>    // UTF-8-validated view
//   JSON in: req: ChatReq := json.decode(web.body_str(c)?)?   — core.json, view-decoding

// responders (Impure; consume c — Move discipline, mirrors ctx.respond)
web.json(c, x)               -> Result<(), Error>   // 200 + application/json + json.encode(x)
web.status_json(c, code, x)  -> Result<(), Error>
web.text(c, s)               -> Result<(), Error>   // 200 + text/plain
web.status(c, code)          -> Result<(), Error>   // status + empty body
```

```text
// types
web.Ctx    — the per-request context struct: the std.http request handle + the param slot
             array (names from the matched route — Static; values — views into the path).
             A Move struct (it owns the request handle); consumed exactly once by a responder.
Route      — Copy struct { method (tag-only enum), pattern: str, handler: fn(Ctx) -> Result<(), Error> }
```

**Pattern syntax (Fiber/httprouter lineage — settled by the restored reference):** `/`-separated;
literal segments match byte-exactly; `:name` matches exactly one non-empty segment and captures;
a trailing `*name` captures the rest (tail wildcard). Priority at each tree node: **static >
`:param` > `*wildcard`** (httprouter semantics — `/v1/models/featured` beats `/v1/models/:id`).
Two routes that can tie → startup abort. No regex, no optional segments, exact trailing-slash
matching (no hidden redirects). Query strings are never part of the pattern.

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

## Middleware (designed now, lands later — W6)

Fiber's `c.Next()` chain needs capturing closures (deferred). The v1-compatible model is a
**non-capturing pre-handler list** threaded by Move: `fn(c: Ctx) -> Result<Option<Ctx>, Error>` —
return `Some(c)` to proceed (ctx handed back), `None` after responding (halt), `Err` for 500.
Groups carry the list: `web.group_with(prefix, [auth, log], routes)`. Covers auth/logging/CORS
headers without closures; stateful middleware waits for the capturing-closure feature and a real
consumer. Verify in F1 probing: `Option<Ctx>` (an Option of a Move struct) — if the payload gap
bites, the fallback shape is a two-variant enum `Verdict { Proceed(Ctx), Done }` (Move-enum
payloads shipped in J2).

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
- **W6 — middleware-lite + SSE sugar** (with the std SSE floor) — gated on the first consumers.
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
