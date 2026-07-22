# Align v0.2.0 Release Notes

The headline of v0.2.0 is **`pkg.web` — a zero-copy REST framework written in Align** — together with the JSON and HTTP work that makes it fast, and the compiler soundness fixes that landed on the way. 128 merged changes since v0.1.0.

## `pkg.web` — the REST framework

A complete, benchmarked web framework. It lives in the repo at `apps/web/pkg/` and is **vendored by copying that subtree** — it is not part of the `alignc` binary archive, the Debian package, or the Homebrew formula.

- **Router**: a flat SoA radix tree built once at `serve()`, with httprouter/matchit semantics (static > param > wildcard), zero-copy `:param` / `*wildcard` capture, and a linear-scan differential oracle in the test suite.
- **Handlers**: one unary signature, `fn(Ctx) -> Result<response_builder, Error>`. `Ctx` is a Copy struct of views that owns nothing — the framework holds the request handle, the handler builds a response and hands it back. Streaming routes take a second shape (below) in the same route table.
- **Accessors / responders**: `pkg.web.param`, `query`, `has_query`, `header`, `body`, `body_str`; `text`, `json`, `status`, `status_text`, `status_json`. Calls stay fully qualified — there are no import aliases in Align.
- **Concurrent serve**: `pkg.web.serve(host, port, routes, workers)` — prefork workers each owning a `SO_REUSEPORT` listener, plus server-side keep-alive in `std.http`.
- **Streaming / SSE**: `pkg.web.sse` / `pkg.web.stream`, dispatched from the same table as unary handlers.
- **Middleware groups**: `group` / `group_with` with prefix-aware dispatch, ordered pre-handlers, and short-circuit responses.
- **Hardening**: startup route-table validation (10 abort diagnoses), RFC-9110 HEAD, fixed JSON 404/405/500 bodies, a malformed-request matrix driven through real sockets, and handler failures logged with the real method, path and error.
- **Batteries**: `pkg.web.cookie` (RFC 6265, injection-refusing), `pkg.web.cors` (Fetch-spec decisions), `pkg.web.multipart` (RFC 7578 uploads, zero-copy `Part` views), and `pkg.jwt` (HS256, alg-pinned, constant-time compared). In `std.encoding`: RFC 3986 percent-encoding, `x-www-form-urlencoded`, HTML escaping.

**Measured**: **pkg.web 491,505 req/s vs Go Fiber-prefork 374,393 — 1.31×**, 2.5× better p99 tail (210 vs 533 µs), and 5.9× Go's `net/http`. The framework itself costs **0.8 µs per request** over writing the same responses directly on `std.http`, and dispatch is 35 ns — 0.9% of Align's own protocol path, 0.05% of a request.

*Caveats, stated up front*: same box, same generator, both sides in prefork, 32 connections; the load generator is **ours**, not `wrk`, and the run-to-run spread at 32 connections reached 18% on one build, with the headline reading taken when the box was quiet. Re-running under an independent generator is recorded as owed work. Full method and raw numbers: `bench/web_e2e/README.md`.

## JSON

`core.json` is now complete enough to model real API payloads: nested-struct decode/encode targets, `Option<T>` and `array<Struct>` struct fields, shape-directed unions (top-level, as a struct field, and as an owned `array<Struct>` variant), enums as struct fields with tag-switched drop, `array<scalar>` fields, `Option<struct>` encode, bare scalar decode targets, the lazy `json.doc` document view (`len` / `key` / `elems`), and `json.scan` streaming typed rows with `sum` / `count` / `reduce` / `any` / `all` / `min` / `max`.

## `std.http`

The protocol path was instrumented and then cut, measured against a minimal-Rust floor:

- The inbound head parse resumes across reads — a 206 KiB head arriving in 100-byte pieces went **56 ms → 0.24 ms**, closing a slowloris lever.
- A response is serialized in ONE right-sized allocation; the 32 KiB per-request memset is gone.
- The client path went **14 → 2 allocations per request** across seven slices (pool-key move, idle-bucket reuse, inline header spans, borrowed host lookup, serialization-buffer reuse), and a 200 KiB body reads **42% faster**.
- `accept(2)`'s transient errnos are classified, so a dropped connection or a full fd table no longer kills a worker.
- New: `ctx.headers()`, the detached header-table view. Also new in `std.process`: `cpu_count()`.

## Compiler

Language and toolchain work driven by the framework, plus the soundness fixes it exposed:

- **Struct fields widened**: function-value fields, Move-handle fields, and `slice<T>` view fields — the gate `pkg.web`'s `Route` and `Ctx` needed. A function *value* may also return `Result<T, E>`, be called with a struct argument, and appear as an enum variant payload.
- **Borrow liveness ends at the owner's DROP**, not only at its move. A view assigned out of a loop body to a longer-lived local no longer reads freed memory.
- **A `template`'s hidden owner now carries borrow provenance.** `template` is the one expression whose value views storage the expression itself allocates; without provenance, a `str` bound from a template escaped its loop-edge free and *sema accepted it*. Programs that laundered such a view through a tuple, struct literal, `match` payload, `.trim()` or a slice were accepted and read freed heap — all are compile errors now. Two further holes (`EnumValue`, `RandSample`) fell out of the same sweep.
- **An owned `string` interpolates like a `str`** instead of panicking codegen, and `print` / `template` / `json.encode` now **diagnose rather than abort** on any unprintable or unencodable value. One silent-wrong-output case (`array<enum>` encoding as `null`) was fixed; a pre-existing over-rejection of nested `Option<struct>` encoding was lifted.
- Earlier in the cycle: an owned argument passed through an **indirect call**, and an owned local moved into a **struct field**, both now null their source; a capturing closure's frame-escape UAF was closed at `region_of`.
- **`pkg` foundation rules**: `internal` path visibility and layering are enforced at the import edge.
- **Platform CI**: nine test-portability fixes (byte-identity gates comparing embedded paths, Mach-O symbol prefixes, Darwin bitcode wrappers, x86-64-only vector-shape gates) — the reason the macOS-aarch64 artifact is trustworthy.

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.** As we iterate towards a stable 1.0, the language syntax, standard library APIs, and ABI may break without warning or legacy fallbacks.

The behavior change to know about in v0.2.0: an eligible HTTP/1.1 request now leaves the connection **open** (server-side keep-alive), so a read-to-EOF client must send `Connection: close` or frame by `Content-Length`. `ctx.respond_stream` was widened to borrow a spent `ctx` rather than consume it — old code still compiles.

## Known Intentional Limitations

Carried over from v0.1.0: `extern "C"` export-of-body, and Windows (Align targets Linux x86-64/aarch64 and macOS Apple Silicon).

Updated in v0.2.0:

- **Capturing escaping closures are still deferred** — a closure whose captured environment must outlive its frame (returning one, storing one) waits on the heap-owned environment/drop model. *Non-capturing* function values now work as struct fields, array elements and enum payloads, which is what the router needed.
- **No database drivers.** `pkg.db.*` activates with its first consumer, so an API with persistence cannot be built yet.
- **No application state in handlers.** The handler signature carries only `Ctx`; passing a connection pool or configuration to a handler is a single deliberate design decision still ahead of us.
- **JWT is HS256 only.** HS384/512 and RS256/ES256 wait on the corresponding `std.crypto` primitives; validating a public provider's OIDC token therefore is not yet possible.
- **Multipart is not wired into the core surface** by design — it is a body codec, so an application calls `pkg.web.multipart` over `pkg.web.body(c)` itself.
