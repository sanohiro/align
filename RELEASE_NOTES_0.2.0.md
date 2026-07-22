# Align v0.2.0 Release Notes

The headline of v0.2.0 is **`pkg.web` — a zero-copy REST framework written in Align** — together with the JSON and HTTP work that makes it fast, and three soundness fixes in the compiler that make it safe. 128 commits since v0.1.0.

## `pkg.web` — the REST framework

A complete, benchmarked web framework ships in this release as a vendorable first-party package (`apps/web/pkg/web/`).

- **Router**: a flat SoA radix tree built once at `serve()`, with httprouter/matchit semantics (static > param > wildcard), zero-copy `:param` / `*wildcard` capture, and a linear-scan differential oracle in the test suite.
- **Handlers**: one signature, `fn(Ctx) -> Result<response_builder, Error>`. `Ctx` is a Copy struct of views that owns nothing — the framework holds the request handle, the handler builds a response and hands it back.
- **Accessors / responders**: `param`, `query`, `has_query`, `header`, `body`, `body_str`; `text`, `json`, `status`, `status_text`, `status_json`.
- **Concurrent serve**: `serve(host, port, routes, workers)` — prefork workers each owning a `SO_REUSEPORT` listener, plus server-side keep-alive in `std.http`.
- **Streaming / SSE**: `web.sse` / `web.stream` ride the same route table as unary handlers.
- **Middleware groups**: `group` / `group_with` with prefix-aware dispatch, ordered pre-handlers, and short-circuit responses.
- **Hardening**: startup route-table validation (10 abort diagnoses), RFC-9110 HEAD, fixed JSON 404/405/500 bodies, a malformed-request matrix driven through real sockets, and handler failures logged with the real method, path and error.
- **Batteries**: `pkg.web.cookie` (RFC 6265, injection-refusing), `pkg.web.cors` (Fetch-spec decisions), `pkg.web.multipart` (RFC 7578 uploads, zero-copy `Part` views), and `pkg.jwt` (HS256, alg-pinned, constant-time compared). In `std.encoding`: RFC 3986 percent-encoding, `x-www-form-urlencoded`, HTML escaping.

**Measured** (same box, same generator, both sides in prefork; caveats in `bench/web_e2e/README.md`): **pkg.web 491,505 req/s vs Go Fiber-prefork 374,393 — 1.31×**, 2.5× better p99 tail, and 5.9× Go's `net/http`. The framework itself costs **0.8 µs per request** over writing the same responses directly on `std.http`, and dispatch is 35 ns — 0.9% of the request path.

## JSON

`core.json` is now complete enough to model real API payloads: shape-directed unions (top-level, as a struct field, and as an owned `array<Struct>` variant), enums as struct fields with tag-switched drop, `array<scalar>` and `array<Move-struct>` fields, `Option<struct>` encode, bare scalar decode targets, the lazy `json.doc` document view (`len` / `key` / `elems`), and `json.scan` streaming typed rows with `sum` / `count` / `reduce` / `any` / `all` / `min` / `max`.

## `std.http`

The protocol path was instrumented and then cut, measured against a minimal-Rust floor:

- The inbound head parse resumes across reads — a 206 KiB head arriving in 100-byte pieces went **56 ms → 0.24 ms**, closing a slowloris lever.
- A response is serialized in ONE right-sized allocation; the 32 KiB per-request memset is gone.
- The client path went **14 → 2 allocations per request** across seven slices (pool-key move, idle-bucket reuse, inline header spans, borrowed host lookup, serialization-buffer reuse), and a 200 KiB body reads **42% faster**.
- `accept(2)`'s transient errnos are classified, so a dropped connection or a full fd table no longer kills a worker.
- New: `ctx.headers()`, the detached header-table view.

## Compiler

Language and toolchain work driven by the framework, plus three soundness fixes:

- **Struct fields widened**: function-value fields, Move-handle fields, and `slice<T>` view fields — the gate `pkg.web`'s `Route` and `Ctx` needed. A function *value* may also return `Result<T, E>` and be called with a struct argument.
- **Borrow liveness ends at the owner's DROP**, not only at its move. A view assigned out of a loop body to a longer-lived local no longer reads freed memory.
- **A `template`'s hidden owner now carries borrow provenance.** `template` is the one expression whose value views storage the expression itself allocates; without provenance, a `str` bound from a template escaped its loop-edge free and *sema accepted it*. Programs that laundered such a view through a tuple, struct literal, `match` payload, `.trim()` or a slice were accepted and read freed heap — all are compile errors now. Two further holes (`EnumValue`, `RandSample`) fell out of the same sweep.
- **An owned `string` interpolates like a `str`** instead of panicking codegen, and `print` / `template` / `json.encode` now **diagnose rather than abort** on any unprintable or unencodable value. One silent-wrong-output case (`array<enum>` encoding as `null`) was fixed; a pre-existing over-rejection of nested `Option<struct>` encoding was lifted.
- **`pkg` foundation rules**: `internal` path visibility and layering are enforced at the import edge.

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.** As we iterate towards a stable 1.0, the language syntax, standard library APIs, and ABI may break without warning or legacy fallbacks. v0.2.0 does change existing surface: `web.serve` takes a `workers` argument, and an eligible HTTP/1.1 request now leaves the connection open (a read-to-EOF client must send `Connection: close` or frame by `Content-Length`).

## Known Intentional Limitations

Unchanged from v0.1.0: fully-escaping function values, `extern "C"` export-of-body, Windows, and the AArch64 SIMD limits (Base64/Hex/UTF-8 do have native NEON dispatch as of this release).

New to note in v0.2.0:

- **No database drivers.** `pkg.db.*` activates with its first consumer, so an API with persistence cannot be built yet.
- **No application state in handlers.** The handler signature carries only `Ctx`; passing a connection pool or configuration to a handler is a single deliberate design decision still ahead of us.
- **JWT is HS256 only.** HS384/512 and RS256/ES256 wait on the corresponding `std.crypto` primitives; validating a public provider's OIDC token therefore is not yet possible.
- **Multipart is not wired into the core surface** by design — it is a body codec, so an application calls `pkg.web.multipart` over `pkg.web.body(c)` itself.
