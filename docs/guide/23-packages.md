# Packages: selecting and vendoring source libraries

> 🌐 **English** · [Japanese](./ja/23-packages.md)

`core` provides language-level operations, `std` provides OS and service APIs, and `pkg` provides source libraries for applications. The packages below are implemented. Choose one by the work you need to do, then copy its source into your project. A registry and fetch tool are not yet available.

## A package is a source tree

A package root is `pkg/<name>.align`, with optional submodules below `pkg/<name>/`. The ordinary module rule does all the work:

```text
main.align
pkg/
  db.align
  auth.align
  frame.align
  kv.align
  web.align
  db/
    sqlite.align
    postgres.align
    pool.align
  kv/
    internal/
      resource.align
  web/
    types.align
    cookie.align
    internal/
      router.align
```

`import pkg.web` resolves to `pkg/web.align`; `import pkg.web.cookie` resolves to `pkg/web/cookie.align`. Calls and types remain fully qualified, such as `pkg.web.get(...)` and `pkg.web.types.Ctx`.

Vendoring means copying a package's source into the consuming project. Copy or merge the `pkg/` directory from the appropriate workspace into your application's root:

| Work | Package | Source workspace |
|---|---|---|
| HTTP routes, middleware, and SSE | `pkg.web` | [apps/web/pkg](../../apps/web/pkg) |
| WebSocket server routes | `pkg.ws` and its `pkg.web` dependency | [apps/ws/pkg](../../apps/ws/pkg) plus [apps/web/pkg](../../apps/web/pkg) |
| HTML text with escaping | `pkg.template` | [apps/template/pkg](../../apps/template/pkg) |
| CSV into typed columns | `pkg.csv` | [apps/csv/pkg](../../apps/csv/pkg) |
| Joins over typed columns | `pkg.frame` | [apps/frame/pkg](../../apps/frame/pkg) |
| Tokens, password hashes, and session tokens | `pkg.auth` | [apps/auth/pkg](../../apps/auth/pkg) |
| RESP2 key-value access | `pkg.kv` | [apps/kv/pkg](../../apps/kv/pkg) |
| SQLite and PostgreSQL | `pkg.db` | [apps/db/pkg](../../apps/db/pkg) |

These sources are not embedded in the `alignc` archive, Debian package, or Homebrew formula. When trying a newly added package, use a compiler that includes its support; a packaged release may lag the current repository. Copy a package's internal modules as well as its root file.

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

## `pkg.ws`

`pkg.ws` adds WebSocket server routes to `pkg.web`; it requires both source trees in the table above. Put `pkg.ws.route(pattern, protocols, pump)` in the same route array as your REST or SSE handlers. The pump receives the owned `http_upgrade` connection and selected protocol string. `pkg.ws.receive` takes an explicit message-size limit and returns a complete `Text`, `Binary`, or `Close` message; use `send_text`, `send_binary`, and `close` for replies.

A live WebSocket occupies one server worker. Choose the worker count for simultaneous long-lived connections as well as ordinary HTTP requests; there is no hidden thread per connection. This is a server package, not a WebSocket client. The [WebSocket design](../impl/pkg-design/ws.md#public-use) provides a complete pump and the handshake requirements.

## `pkg.template`

For HTML output, use `pkg.template` rather than inserting untrusted text directly into a language `template` string. After vendoring the package, this is a complete program:

```align
import pkg.template

fn greeting(name: str) -> string {
    mut output := pkg.template.html()
    pkg.template.raw(output, "<p>")
    pkg.template.write(output, name)
    pkg.template.raw(output, "</p>")
    return pkg.template.to_string(output)
}

fn main() -> i32 {
    print(greeting("<Align>"))
    return 0
}
```

It prints `<p>&lt;Align&gt;</p>`. `write` escapes text; `raw` inserts trusted markup unchanged. `to_string` consumes the Move builder and returns an owned string. Escaping is suitable for element text and already-quoted attribute values; it does not validate URLs, JavaScript, CSS, or attribute names. A plain language `template` formats text but does not HTML-escape it. See the [HTML builder design](../impl/pkg-design/template.md).

## `pkg.csv`

`pkg.csv.decode` reads an in-memory UTF-8 CSV document into `soa<R>`. Supply a named destination arena and a `pkg.csv.DecodeOptions` value with `header`, `line_ending`, and `max_rows`. There are no inferred defaults: choose `Header.Present` or `Header.Absent`, and `LineEnding.Lf` or `LineEnding.CrLf` explicitly. The binding annotation, such as `rows: soa<Trade>`, supplies the row type, following the same expected-type pattern as JSON decoding.

With a header, fields are selected by name; without one, columns must match the record's declaration order and exact width. The result belongs to the destination arena. File reads are a separate `std.fs` operation, and errors use `pkg.csv.Error`, which you must handle or map before returning from a `main` that uses builtin `Error`. This first API decodes a complete document; it does not stream or encode CSV. See the [CSV design and example](../impl/pkg-design/csv.md#public-use).

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

The password portion of this example needs OpenSSL 3.2 or newer for Argon2id; see chapter [01](01-getting-started.md).

Verification authenticates the compact bytes before parsing JSON, pins the algorithm to HS256, and compares signatures in constant time. The required `now_ns` checks optional integer-form `exp` and `nbf` claims without a hidden clock read. Password verification enforces the caller's three work ceilings before Argon2 runs. There is no default password policy, key lookup, issuer/audience policy, cookie, or session store; the exact bounds and errors are recorded in [`pkg.auth`'s design](../impl/pkg-design/auth.md).

## `pkg.kv`

`pkg.kv` is a synchronous plaintext RESP2 client for typed GET, SET, and single-key DEL operations. Connection and I/O timeouts and the maximum response size are explicit. The Move `client` is used through one `borrow mut` operation at a time, and Drop is its public close operation.

```align
import pkg.kv

fn open_store() -> Result<pkg.kv.client, pkg.kv.Error> {
    options := pkg.kv.ClientOptions {
        connect_timeout_ns: 1000000000,
        io_timeout_ns: 1000000000,
        max_response_bytes: 1048576,
    }
    return pkg.kv.connect("127.0.0.1", 6379, options)
}

fn create_session(
    borrow mut store: pkg.kv.client,
    key: str,
    payload: str,
) -> Result<bool, pkg.kv.Error> {
    options := pkg.kv.SetOptions {
        condition: pkg.kv.SetCondition.IfAbsent,
        expires_in_ns: Some(900000000000),
    }
    return pkg.kv.set(store, key, payload, options)
}
```

`get` returns an owned optional string, `set` reports whether its `Always` / `IfAbsent` / `IfPresent` condition applied, and `delete` reports whether the key existed. Transport, oversized, or malformed replies retire the client; a later operation then returns `Closed`. There is no default endpoint, authentication, TLS, RESP3 negotiation, generic command API, pipeline, retry, redirect, pool, transaction, script, or pub/sub surface. Root `pkg.kv` and its private `pkg.kv.internal.resource` implementation ship together; the exact bounds and error rules are recorded in [`pkg.kv`'s design](../impl/pkg-design/kv.md).

## `pkg.db`

`pkg.db` is the first-party database package, vendored the same way as the other four: [apps/db/pkg](../../apps/db/pkg) holds `pkg/db.align` plus the `pkg.db.sqlite`, `pkg.db.postgres`, and `pkg.db.pool` modules beneath it.

`pkg.db` provides typed static queries and commands for SQLite and PostgreSQL. Queries can be checked at compile time using schema metadata produced by `alignc db prepare`. The package also supports prepared statements, transactions, typed row streams, one-to-many and many-to-one results, migrations, and read-only catalog inspection with `EXPLAIN`.

For larger workloads, it offers bounded batches and SoA views, PostgreSQL single-row and portal-batch delivery, a fixed-capacity non-waiting pool, and explicit dynamic SQL. PostgreSQL execution supports deadlines and cancellation; SQLite scalar functions use callbacks checked by the compiler. Broader logical types, PostgreSQL COPY and callbacks, and SQLite collations remain future work, to be designed around concrete consumer needs.

The compiler side is already in the binary you have: `alignc db prepare`, `db migrate`, `db status`, `db check`, and `db repair` (chapter [16](16-toolchain.md)) drive the checked metadata and the migration catalog. `docs/impl/pkg-design/db.md` is the contract of record.

Chapter [24](24-database.md) is the working guide to that shipped surface, task by task.
