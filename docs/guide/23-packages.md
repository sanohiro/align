# Packages: vendored source, pkg.web, and pkg.jwt

> 🌐 **English** · [Japanese](./ja/23-packages.md)

`core` is the language's data layer, `std` is the OS boundary, and `pkg` is the source-package layer for frameworks and domain libraries. The package foundation and the first-party `pkg.web` and `pkg.jwt` packages are available today. What is deliberately still missing is a registry or fetch tool.

## A package is a source tree

A package root is `pkg/<name>.align`, with optional submodules below `pkg/<name>/`. The ordinary module rule does all the work:

```text
main.align
pkg/
  jwt.align
  web.align
  web/
    types.align
    cookie.align
    internal/
      router.align
```

`import pkg.web` resolves to `pkg/web.align`; `import pkg.web.cookie` resolves to `pkg/web/cookie.align`. Calls and types remain fully qualified, such as `pkg.web.get(...)` and `pkg.web.types.Ctx`.

Vendoring means copying that source subtree into the consuming project. In this repository, [apps/web/pkg](../../apps/web/pkg) and [apps/jwt/pkg](../../apps/jwt/pkg) are package-author workspaces; copy or merge their `pkg/` directories into your application's root. They are not embedded in the `alignc` archive, Debian package, or Homebrew formula.

There is no package manifest, lockfile, registry, version solver, or download command. Imports plus the filesystem are the dependency graph, and one `pkg/<name>` exists per source tree. Updating or auditing a dependency means updating or auditing the vendored source.

## The two enforced package boundaries

The compiler checks two path rules on every import:

- An `internal` module is importable only from the subtree rooted at its parent. `pkg.web` may import `pkg.web.internal.router`; `main` and `pkg.jwt` may not.
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

There is no application-state parameter in the handler shape yet, and no database package. Those are current limitations, not hidden framework facilities.

## `pkg.jwt`

`pkg.jwt` implements compact JSON Web Tokens with HS256. Claims remain JSON text: `core.json` owns their schema, while the package owns the signed envelope.

```align
import pkg.jwt

pub fn main() -> Result<(), Error> {
    claims := "{\"sub\":\"42\",\"exp\":2000}"
    token := pkg.jwt.encode_hs256(claims, "secret")
    decoded := pkg.jwt.decode_hs256(token, "secret")?
    print(pkg.jwt.time_claims_valid(decoded, 1000))
    return Ok(())
}
```

Verification pins the algorithm to HS256 instead of trusting the token's `alg` field, and compares signatures in constant time. `time_claims_valid` checks optional `exp` and `nbf` NumericDate claims separately from signature verification. HS384/512, RSA, ECDSA, and public-provider OIDC verification are not exposed until the corresponding audited crypto primitives exist.
