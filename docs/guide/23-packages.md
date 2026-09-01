# Packages: vendored source, pkg.web, pkg.frame, pkg.auth, and pkg.db

> 🌐 **English** · [Japanese](./ja/23-packages.md)

`core` is the language's data layer, `std` is the OS boundary, and `pkg` is the source-package layer for frameworks and domain libraries. The package foundation and the first-party `pkg.web`, `pkg.frame`, `pkg.auth`, and `pkg.db` packages are available today. What is deliberately still missing is a registry or fetch tool.

## A package is a source tree

A package root is `pkg/<name>.align`, with optional submodules below `pkg/<name>/`. The ordinary module rule does all the work:

```text
main.align
pkg/
  db.align
  auth.align
  frame.align
  web.align
  db/
    sqlite.align
    postgres.align
    pool.align
  web/
    types.align
    cookie.align
    internal/
      router.align
```

`import pkg.web` resolves to `pkg/web.align`; `import pkg.web.cookie` resolves to `pkg/web/cookie.align`. Calls and types remain fully qualified, such as `pkg.web.get(...)` and `pkg.web.types.Ctx`.

Vendoring means copying that source subtree into the consuming project. In this repository, [apps/web/pkg](../../apps/web/pkg), [apps/frame/pkg](../../apps/frame/pkg), [apps/auth/pkg](../../apps/auth/pkg), and [apps/db/pkg](../../apps/db/pkg) are package-author workspaces; copy or merge their `pkg/` directories into your application's root. They are not embedded in the `alignc` archive, Debian package, or Homebrew formula.

There is no package manifest, lockfile, registry, version solver, or download command. Imports plus the filesystem are the dependency graph, and one `pkg/<name>` exists per source tree. Updating or auditing a dependency means updating or auditing the vendored source.

## The two enforced package boundaries

The compiler checks two path rules on every import:

- An `internal` module is importable only from the subtree rooted at its parent. `pkg.web` may import `pkg.web.internal.router`; `main` and `pkg.auth` may not.
- A module below `pkg/` may import only `core`, `std`, or another `pkg` module. It cannot reach back into the consuming project's modules.

These rules keep package internals private and the dependency direction one-way without adding another visibility syntax or build language.

## `pkg.web`

`pkg.web` is a zero-copy REST framework over `std.http`. A unary handler receives a Copy context made of request views, builds a response, and returns it. The framework retains the request handle, so it can turn an unmatched path into 404, a method mismatch into 405, and a handler error into 500.

```align
import pkg.web
import pkg.web.types

fn hello(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {
    return pkg.web.text(pkg.web.param(c, "name"))
}

pub fn main() -> Result<(), Error> {
    routes := [
        pkg.web.get("/hello/:name", hello),
    ]
    return pkg.web.serve("127.0.0.1", 8080, routes, 1)
}
```

Route constructors cover the usual HTTP methods plus `any`; patterns support static segments, `:param`, and a trailing `*wildcard`. `group` and `group_with` add prefixes and ordered middleware. Request accessors include `param`, `query`, `has_query`, `header`, `body`, and `body_str`; responders include `text`, `json`, `status`, `status_text`, and `status_json`.

`serve(host, port, routes, workers)` makes concurrency visible at the call site. One worker runs on the calling thread; multiple workers use separate `SO_REUSEPORT` listeners. Streaming routes use `stream`, and `sse` is the Server-Sent Events specialization. Malformed route tables and impossible worker counts abort at startup as programmer errors.

Public companion modules provide focused, composable pieces:

- `pkg.web.cookie` reads request cookies and builds injection-checked `Set-Cookie` values.
- `pkg.web.cors` makes CORS policy decisions without silently permitting an invalid wildcard-plus-credentials policy.
- `pkg.web.multipart` walks `multipart/form-data` bodies as zero-copy `Part` views. The application supplies `pkg.web.body(c)` and owns the iteration offset.

There is no application-state parameter in the handler shape yet. That is a current limitation, not a hidden framework facility. Database access is a separate package, `pkg.db`, below.

## `pkg.frame`

`pkg.frame` performs bounded stable inner joins over validated `core.codec` i64 or string columns. It returns owned `RowPair { left, right }` source ordinals in left-major, right-ascending order, so it does not materialize or retain either input batch. The required `max_pairs` makes duplicate-key fanout and output allocation visible. Nullable/composite keys, outer joins, adaptive build-side selection, parallelism, and spill are deliberately absent; the exact surface is in [`pkg.frame`'s design](../impl/pkg-design/frame.md).

## `pkg.auth`

`pkg.auth` assembles three bounded authentication protocols from the audited `std.crypto` primitives: HS256 tokens, canonical Argon2id password records, and opaque 256-bit session tokens. Claims remain JSON text and all policy stays explicit at the call site.

```align
import pkg.auth
import std.encoding

fn main() -> Result<(), Error> {
    key := encoding.hex_decode("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")?
    claims := "{\"sub\":\"42\",\"exp\":2000}"
    token := pkg.auth.encode_hs256(claims, key.bytes())?
    verified := pkg.auth.verify_hs256(token, key.bytes(), 1000000000000)?
    print(verified)

    policy := pkg.auth.Argon2Policy{m_cost: 65536, t_cost: 3, parallelism: 1}
    phc := pkg.auth.password_hash("correct horse".bytes(), policy)?
    print(pkg.auth.password_verify("correct horse".bytes(), phc, policy)?)
    print(pkg.auth.session_token())
    return Ok(())
}
```

Verification authenticates the compact bytes before parsing JSON, pins the algorithm to HS256, and compares signatures in constant time. The required `now_ns` checks optional integer-form `exp` and `nbf` claims without a hidden clock read. Password verification enforces the caller's three work ceilings before Argon2 runs. There is no default password policy, key lookup, issuer/audience policy, cookie, or session store; the exact bounds and errors are recorded in [`pkg.auth`'s design](../impl/pkg-design/auth.md).

## `pkg.db` — complete committed roadmap

`pkg.db` is the first-party database package, vendored the same way as the other three: [apps/db/pkg](../../apps/db/pkg) holds `pkg/db.align` plus the `pkg.db.sqlite`, `pkg.db.postgres`, and `pkg.db.pool` modules beneath it.

Complete: the first public release scope. Typed static queries and commands are checked against real schema metadata at compile time, execute on both SQLite and PostgreSQL, and regenerate that metadata offline. Prepared statements, transactions, typed row streams with deadlines and cancellation, compound one-to-many and many-to-one outputs, migration lifecycle tooling, and read-only catalog inspection with `EXPLAIN` are all in.

The complete committed roadmap has also shipped: bounded batch and SoA delivery, PostgreSQL-native single-row and portal-batch delivery, the explicit fixed-capacity non-waiting `pkg.db.pool`, driver-explicit dynamic SQL, and proved SQLite scalar functions. The final cross-rail audit runs every owner suite in the required local and CI gate. Broader logical types, PostgreSQL COPY and callbacks, and SQLite collations remain explicitly consumer-gated future surfaces rather than incomplete D1–D14 work.

The compiler side is already in the binary you have: `alignc db prepare`, `db migrate`, `db status`, `db check`, and `db repair` (chapter [16](16-toolchain.md)) drive the checked metadata and the migration catalog. `docs/impl/pkg-design/db.md` is the contract of record.

Chapter [24](24-database.md) is the working guide to that shipped surface, task by task.
