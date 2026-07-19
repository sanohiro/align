# pkg foundation + pkg.web framework plan (framework-first)

> Status: plan record, 2026-07-20. Owner-directed: **the framework comes first — including the pkg
> foundation** — executed in a separate session; this document is that session's source of truth.
> This SUPERSEDES the 2026-07-18 direction note "framework by extraction, gateway app first" (in
> `open-questions.md` → REST-gateway runway): the owner re-decided 2026-07-20 to build the pkg
> foundation and the `pkg.web` framework deliberately, with the OpenAI-compatible gateway as the
> framework's first consumer/validation app rather than its predecessor. Read together with:
> `open-questions.md` → "Build system / package layout — pkg-foundation design (PROPOSAL 2026-07-19)"
> (the package model; the owner's framework-first directive is its implementation go-ahead),
> `work/proposals/api-server-db.md` (the performance blueprint), `draft.md` §18.3 (pkg layering),
> and `docs/impl/std-design/http.md` (the std.http protocol floor pkg.web builds on).

## 1. Decisions (recorded)

**D-A — monorepo, no separate repository pre-release.** The framework and its consumer app live in
THIS repo. A package is defined by its path shape, not a repo boundary (pkg-foundation D2/D4), so
location is a management choice — and pre-release the monorepo wins decisively: (1) the
no-backward-compat discipline (CLAUDE.md) requires language changes and framework follow-ups to
land in the **same commit**; (2) the framework + app are the compiler's best end-to-end integration
test, exercised in CI every PR; (3) later extraction to a standalone repo is trivial by
construction — the subtree is already in distribution shape (vendoring IS copying). Revisit only
when the language stabilizes AND an external consumer wants the framework independently.

**D-B — one workspace hosts both: `apps/gateway/`.** Per pkg-foundation D4 an author's workspace =
entry files at its root + the package under its `pkg/`. The framework is developed at
`apps/gateway/pkg/web/` and the gateway entry (`apps/gateway/main.align`) is its first consumer —
the same directory is simultaneously the framework author's workspace and the shipped app.

```text
align/                            ← this repo
├── crates/                       ← the compiler (Rust)
└── apps/
    └── gateway/                  ← Align workspace
        ├── main.align            ← the gateway app (module main) — pkg.web's first consumer
        └── pkg/
            └── web/…             ← THE framework (module pkg.web.*), plus pkg/web.align root
```

**D-C — the router gate is smaller than recorded.** The 2026-07-18 note gated a router on
"fully-escaping fn values". Probed 2026-07-20: a **non-capturing** fn value is already Copy/`Static`
(`Ty::Fn`, a bare pointer, no environment) — what is missing is only its admission as a **struct
field / array element** plus calling through a field (today: `struct fields must be a primitive
scalar, str, or a plain struct for now, got fn#0`, and `r.handler(x)` parses as an unknown method).
That is a small, well-scoped language slice (F1 below). The genuinely deferred item — heap-owned
**capturing** closures that escape — stays deferred (its consumer would be stateful middleware;
none exists yet).

**D-D — the gateway fronts a stub backend behind a narrow seam.** The align-LLM engine (doc-14) does
not exist yet; the app must not wait for it. v1 serves a deterministic stub completion behind ONE
function boundary (`engine.generate(...)`), so the full HTTP/JSON/SSE/arena path is real and
benchmarkable now and the engine slots in later without reshaping the app.

**D-E — all other language/std prerequisites are shipped.** JSON completeness J1–J5 (nested/
optional decode+encode, the multimodal `content` union, `json.doc`, `json.scan`), `std.http`
server primitives (M11) + SSE `respond_stream` (M12), arena-scoped requests. Verified 2026-07-19/20:
multi-level `pkg.*` resolution (root module + submodule + absolute in-package imports) compiles and
runs on the current `alignc` unchanged.

## 2. Phases (execute in order; each slice lands PR → review → merge)

**F0 — pkg foundation v1 (compiler + spec).** The pkg-foundation proposal's v1 scope, now
owner-directed to implement: ① the `internal` path rule (D7 — an import path containing an
`internal` segment is legal only for importers whose module path starts with the prefix up to the
`internal` parent); ② the pkg-layering rule (D8 — a module under `pkg/` may import only
`core`/`std`/`pkg`); both are import-edge checks at resolution (`load_units`/sema). ③ Spec text:
`draft.md` §17 (the two rules) + §18.3 (replace the placeholder with the package model),
`language-spec.md` digest, `design-notes.md` rationale; move the open-questions entry PROPOSAL →
Settled on landing. Acceptance: positive + negative import tests for both rules; the pkg-foundation
smoke shape (root module + submodule + internal) pinned as a driver test.

**F1 — fn values as struct fields / array elements (the router prerequisite).** Admit `Ty::Fn`
(non-capturing, Copy, `Static`, effect-bit-carrying per #465) as a struct field and as an
array/slice element; call through a field/element (`r.handler(x)` where `handler: fn(i64) -> i64`
— note the parser currently reads this as a method call; dispatch must check field-of-fn-type
first). Sweep the usual exhaustive passes (field-eligibility, layout/ABI = bare pointer, Move/Copy
classification = Copy, effect propagation through indirect calls — the #465 machinery already
consumes `FnTy` effects). NOT in scope: capturing closures escaping (unchanged deferral).
Acceptance: a `Route { path: str, handler: fn(ctx) -> R }` table in an array, dispatched by lookup,
end-to-end; effect inference still fails closed on unknown targets.

**F2 — pkg.web design doc — WRITTEN 2026-07-20 (PROPOSAL).** `docs/impl/pkg-design/web.md` (+
`ja/` mirror), at std-design depth: signatures, Move/effect classification, error policy, W1–W5
slice plan, pitfalls, and the std.http prerequisites (query/percent-decode, SSE framing — the
"consumer arrived" items). Primary reference: **Go 1.22 `net/http` ServeMux** (method-aware
`{param}` pattern routing, automatic 404/405); chi/httprouter as the performance reference
(radix tree deferred; v1 linear scan, bench-gated). Three forks (⚖ A route constructors /
B pattern syntax / C params delivery) await owner settlement before W1 starts; recommendations
are in the doc. **Distribution (owner question answered 2026-07-20):** `pkg.web` is a
**first-party package shipped with the system** — the align repo/release carries it as a
vendorable subtree; consumers copy `pkg/web/` into their project (later automated by the fetch
tool). It stays an ordinary pkg-layer package — explicitly vendored, never ambiently resolvable
(no search paths; pkg-foundation D2/D6 hold).

**F3 — pkg.web implementation slices** (per the F2 doc's slice plan; expected shape: types+router
core → request/response sugar → SSE helpers → hardening), developed at `apps/gateway/pkg/web/`
with unit-style example entries beside it.

**F4 — the gateway app on pkg.web.** `apps/gateway/main.align` implements the OpenAI-compatible
surface THROUGH the framework — its acceptance doubles as the framework's validation:

```text
POST /v1/chat/completions   stream:false → one chat.completion object
POST /v1/chat/completions   stream:true  → SSE chat.completion.chunk deltas + data: [DONE]
GET  /v1/models             → the stub model list
else                        → 404 / 405 via the router; malformed JSON → 400 OpenAI error object
```

Per-request `arena {}`; str views off the socket buffer; `Option` params; the multimodal `content`
union; sequential accept loop. CI wiring: a driver integration test (pattern:
`crates/align_driver/tests/m11_http.rs` in-process server tests) builds the workspace with the
in-tree `alignc`, runs it on an ephemeral port, and asserts round-trips — the app is exercised on
every PR. A `bench/gateway` throughput anchor lands with hardening (the api-server-db.md
flat-memory claim gets a number).

## 3. Deferred / out of scope (with reasons)

- **Capturing escaping closures** — unchanged deferral; the consumer would be stateful middleware,
  which is itself deferred out of pkg.web v1.
- **Real inference** — doc-14's separate track; the D-D seam isolates it.
- **Concurrent accept / task_group serving** — sequential is the recorded v1; measured follow-up.
- **`pkg.db.*` drivers** — api-server-db.md §2's rules activate with their first consumer.
- **The fetch tool / registry / compiled distribution** — pkg-foundation D11/D12, consumer-gated.
- **A separate repo for the framework** — pre-release monorepo per D-A.
