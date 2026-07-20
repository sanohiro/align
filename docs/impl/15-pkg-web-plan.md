# pkg.web plan — the zero-copy REST framework (foundation included)

> Status: plan record, 2026-07-20 (v2 — reframed per the owner's restored brief). **The deliverable
> is `pkg.web`: a blazing-fast, zero-copy REST framework for Align** — primary reference Go's
> **Fiber** (fasthttp's zero-allocation philosophy + Express-derived API), router reference the
> **httprouter/fasthttp radix-tree lineage**. The gateway / LLM apps are merely the framework's
> first consumers and are explicitly LATER ("what we build with it happens to be LLM-related").
> This supersedes both the 2026-07-18 "extraction over invention" note and this doc's own v1
> (gateway-centric) framing. Execution happens in a separate session; this doc + the design doc
> are that session's source of truth. Read: `pkg-design/web.md` (THE design — surface, performance
> contract, W1–W7 slices), `open-questions.md` → pkg-foundation proposal (the package model),
> `work/proposals/api-server-db.md` (the end-to-end performance blueprint).

## 1. Decisions (recorded)

**D-A — monorepo; first-party; shipped with the system.** `pkg.web` is developed in this repo and
distributed with it as a vendorable subtree (consumers copy `pkg/web/`; the future fetch tool
automates the copy; never ambiently resolvable — pkg-foundation D2/D6). No separate repository
pre-release: language changes and framework follow-ups must land in the same commit
(no-backward-compat discipline); the framework is the compiler's best end-to-end CI test; later
extraction stays trivial (the subtree is distribution-shaped).

**D-B — the framework author workspace is `apps/web/`.** Per pkg-foundation D4: the framework
lives at `apps/web/pkg/web/` with example/test entry files beside it. The LLM gateway app comes
later in its own workspace (or as an entry beside the framework — decided when it starts); it is
NOT part of the framework's acceptance.

```text
align/
├── crates/                       ← the compiler (Rust)
└── apps/
    └── web/                      ← Align workspace = pkg.web author workspace
        ├── example_api.align     ← example/test entries (module main; compiled only if reached)
        └── pkg/
            ├── web.align         ← module pkg.web (root)
            └── web/…             ← submodules incl. pkg.web.internal.* (the radix tree)
```

**D-C — performance is the headline.** The design doc's performance contract (zero request-byte
copies, zero per-request heap allocation, O(segments) radix dispatch, zero-copy output,
startup-total validation) is the framework's existence claim, bench-pinned in W5 and externally
compared against Go Fiber in W7 (target: competitive-or-better on plaintext + JSON echo).

**D-D — surface decisions settled by the restored Fiber reference** (no open forks): per-method
constructors (`web.get/post/...`), `:name` + `*name` pattern syntax (Fiber/httprouter lineage,
static > param > wildcard priority), one-ctx handlers (`fn(c: web.Ctx) -> Result<(), Error>`) with
named param access `web.param(c, "id")`. Full surface: `pkg-design/web.md`.

**D-E — the compiler prerequisite is F1 field-eligibility widening**, probed 2026-07-20: struct
fields today are limited to "primitive scalar, str, or a plain struct"; `Route`/`Ctx` need ① fn
value fields, ② a Move-handle field (`http_request_ctx`), ③ `slice<str>` fields. Each reuses
existing classification machinery (FnTy effects #465; Move-field drop from J3a; str-view region
tracking). Capturing escaping closures stay deferred (middleware-lite runs without them —
design doc "Middleware").

## 2. Phases (execute in order; each slice lands PR → review → merge)

- **F1 — field-eligibility widening (compiler; the hard gate).** ①②③ above through the usual
  exhaustive sweeps (`is_field_ok`, layout/ABI, Move/Copy + drop, region/escape, MIR, codegen).
  Acceptance: a `Route { pattern: str, handler: fn(...) }` array dispatched by lookup runs
  end-to-end; a Move-handle-bearing struct moves/drops exactly once; a `slice<str>` field is
  region-tracked like a `str` field; effect inference stays fail-closed through stored fn values.
  Probe first, per item — any that already works shrinks the slice.
- **F0 — pkg-foundation v1 (parallelizable with F1).** The `internal` path rule + the
  pkg-layering rule (import-edge checks) + spec text (draft §17 rules, §18.3 the package model,
  language-spec digest, design-notes rationale); move the open-questions proposal → Settled.
  Acceptance: positive/negative import tests; the verified pkg smoke shape pinned as a driver test.
- **F2 — the design: DONE** (`pkg-design/web.md` + ja, 2026-07-20 v2 — Fiber-informed,
  performance-contract-first). Revisit only if F1 probing invalidates a shape (e.g. `Option<Ctx>`
  for middleware — fallback recorded in the doc).
- **F3 — pkg.web implementation, W1–W7** (per the design doc): W1 router core (radix tree +
  differential oracle) → W2 Ctx/serve/dispatch (+ the std.http query floor lands here if not
  before) → W3 accessors/responders → W4 hardening → W5 bench gate (zero-overhead + contract
  regression) → W6 middleware-lite + SSE sugar (consumer-gated) → W7 the Fiber comparison,
  numbers recorded into the design doc.
- **F4 — the first consumer app (LATER, separate).** The OpenAI-compatible LLM gateway on
  pkg.web (stub engine seam; SSE; per-request arena). Not part of the framework's acceptance;
  scheduled when the owner calls it.

## 2b. REST-completeness backlog (owner directive 2026-07-20)

**Scope rule: `pkg.web` is a general REST API / web-server framework. The LLM gateway is only its
first consumer — never a reason to omit or defer a feature.** The owner pushed back on exactly that
framing ("LLMゲートウェイで必要ない機能は全て削ろうとするのが気に入らない"): scoping by one
consumer makes the support surface undefined and forces repeated "actually, that isn't supported"
conversations. Judge every candidate by *does a REST API / web server commonly need this?*
Layering still applies — the right answer is often "build it in `std` or its own `pkg`", but that
must mean **build it there**, never **skip it**. (The design doc's "no bloat" constraint governs the
framework's *core surface*, not standard web functionality.)

Status of the things a REST/web server is expected to have:

```text
DONE   JSON (decode/encode/doc/scan)            core.json
DONE   HTTP client/server + SSE/chunked         std.http
DONE   base64 / base64url / hex / utf8_valid    std.encoding
DONE   sha256/512, hmac_sha256, argon2id,       std.crypto
       AEAD, constant_time_equal, random
DONE   gzip / zstd                              std.compress
DONE   JWT (HS256; alg-pinned, CT-compared)     pkg.jwt          (2026-07-20)
DONE   URL/percent encode+decode (RFC 3986)     std.encoding     (2026-07-20)
DONE   application/x-www-form-urlencoded        std.encoding     (2026-07-20)
DONE   query-string lookup (zero-alloc, escaped   pkg.web.internal.query (2026-07-20)
       keys matched without materializing)
DONE   Cookie parse / Set-Cookie build          pkg.web.cookie   (2026-07-20)
DONE   HTML escaping                            std.encoding     (2026-07-20)
DONE   CORS decisions (allowlist, wildcard+cred  pkg.web.cors     (2026-07-20)
       rejection, Vary, preflight methods)      — header emission wires in at serve
TODO   multipart/form-data (uploads)            pkg.web or pkg
NEXT   fn VALUE with a `Result` return          compiler (FnTy.ret is a Scalar today)
NEXT   fn VALUE called with a STRUCT argument   compiler (closure ABI aggregate-arg path)
LATER  JWT HS384/512, RS256/ES256               needs std.crypto hmac_sha384/512, RSA/ECDSA
```

**The two `NEXT` compiler items gate the designed handler contract.** `Route.handler` is
`fn(Ctx) -> Result<(), Error>`; a fn *value* cannot carry a `Result` return, and calling one with a
struct argument aborts (F1① only exercised scalar parameters). Until both land, a matched handler is
called directly rather than through the route table's field — the shapes otherwise compose and are
proven over a real socket (`apps_web_serve.rs`).

**OAuth 2.0 / OIDC** (owner asked 2026-07-20) is a package (`pkg.oauth`) over these parts, not a
separate subsystem:

```text
client flows (authorization-code + PKCE)   BUILDABLE NOW — authorize URL (percent_encode),
                                           token exchange (std.http client + form_encode body),
                                           token JSON (core.json), PKCE S256 (sha256 + base64url),
                                           state/nonce (crypto.random)
bearer-token parsing on the resource side  BUILDABLE NOW — header read + pkg.jwt
validating a PUBLIC provider's token       BLOCKED on RS256: Google / Auth0 / Okta / Entra all sign
                                           with RS256, so this needs std.crypto RSA verification
                                           (EVP over the already-linked libssl — a bounded addition,
                                           not a redesign), then JWKS fetch (https + JSON) on top
```

A `TODO` here is committed work with a home, not a maybe. Discovering a *further* gap later is
normal; designing to one consumer is not.

## 3. Deferred / out of scope (with reasons)

- **Capturing escaping closures** — unchanged deferral; middleware-lite (non-capturing,
  Move-threaded) covers auth/logging/CORS; stateful middleware waits for the feature + consumer.
- **Concurrent accept / task_group serving** — sequential v1 per the recorded direction; the
  concurrency step is measured AFTER the single-core hot path is proven (W5), not assumed.
- **DB drivers (`pkg.db.*`)** — api-server-db.md §2 activates with their first consumer.
- **Fetch tool / registry / compiled distribution** — pkg-foundation D11/D12, consumer-gated.
- **A separate repo** — pre-release monorepo per D-A.
