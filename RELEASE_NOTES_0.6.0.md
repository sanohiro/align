# Align v0.6.0 Release Notes

Align v0.6.0 turns the compiler into a more complete day-to-day toolchain. It ships the native AOT REPL and the first in-language test runner, adds bounded data and authentication packages, extends HTTP, filesystem, process, JSON, and cryptographic boundaries, and makes repeated and distributed builds substantially more reusable. The release remains faithful to Align's rules: allocation, ownership, I/O, limits, and failure stay explicit.

## Native AOT REPL and in-language tests

`align-repl` is now included in every release archive and package. It keeps one growing Align program, recompiles it through the production compiler, and executes the real native binary after each accepted entry. It is not an interpreter or a REPL-only dialect, and saved sessions are ordinary Align source.

The language now has private contextual test declarations and a dedicated runner:

```align
import core.test

test "addition" {
  test.expect_eq(20 + 22, 42)
}
```

```text
alignc test main.align
```

`test.expect` and `test.expect_eq` use ordinary Align typing and cleanup. Tests are discovered only from the explicit entry/import closure, run sequentially in isolated child executions, and remain absent from production objects and interfaces. The runner has explicit timeout and output bounds rather than an unbounded hidden harness.

## Bounded data and application protocols

`core.codec` provides a canonical v1 typed columnar envelope with `I64`, `F64`, `Bool`, and `Str` columns. `codec.open` validates the complete byte input before publishing a zero-copy, input-region-bound batch; `codec.encoder` builds the same canonical format with transactional column insertion and an owned final buffer.

`pkg.frame` operates directly on those validated column views. Its bounded stable inner joins for integer and string keys return explicit source-row pairs in deterministic order, preserve duplicate-key Cartesian products, and fail without publishing a partial result when the caller's `max_pairs` limit would be exceeded.

`pkg.auth` replaces the former `pkg.jwt` prototype with one bounded authentication package:

- HS256 JWT encoding and verification over strict JSON, canonical unpadded base64url, caller-supplied time, and constant-time tag comparison;
- canonical Argon2id password records with explicit hash policy and caller-supplied verification ceilings; and
- 256-bit CSPRNG session tokens with no hidden registry, clock, or persistence.

Malformed authenticated data is distinct from denied authentication, and all JWT segments are validated as canonical before authentication. V1 deliberately adds no cookie/session store, JWKS fetch, algorithm negotiation, or ambient authentication policy.

## Standard-library boundaries

`std.crypto` adds distinct Move-owned RS256, ES256, and Ed25519 private/public key types. Strict PKCS#8/SPKI PEM import, decoded JWK public-key construction, signing, and verification use exact algorithm-specific operations and wire signatures; there is no generic key handle or implicit algorithm selection.

`std.log` adds an explicit writer-owning logger with `Debug`, `Info`, `Warn`, `Error`, and `Off` thresholds. Record formatting is allocation-free, the first sink error is latched, and `flush` is the only source-visible error-observation path. There is no global logger, environment configuration, timestamp, or asynchronous queue.

HTTP clients gain bounded response-body capture, strict shared chunked decoding, raw response streaming, and WHATWG-style server-sent event streaming. The streaming path keeps one connection owner, exposes bounded reads, and avoids allocating a complete raw body. Status remains data rather than an error.

`std.process` adds a command-local `max_capture_bytes` limit and `run_bytes` for arbitrary binary stdout/stderr. Bounded capture uses the same timeout, kill, drain, and direct-child reap lifecycle as text capture and never performs an unbounded run followed by a length check.

`std.fs` adds exclusive create, no-replace rename, and retained-root regular-file open/create operations. The retained-root forms reject symlink traversal and operate beneath retained directory descriptors; they do not claim a general sandbox, implicit cleanup, or pair atomicity.

## Ownership, JSON, and compiler correctness

Typed JSON now supports bounded canonical encoding and recursively owned record, `Option`, and array graphs. Decoded values and encoded output follow ordinary Move and recursive Drop rules across control flow, aggregate carriers, whole-program compilation, and per-unit compilation.

Borrowed projections were completed for dynamic aggregates, sum payloads, `Option` payloads, arrays, and string arrays. Storage-generation tracking now follows those views through branches and retained temporaries, so replacement or Drop cannot silently invalidate a live view. Shared recursive Drop lowering and the corresponding container paths close the ownership side of those additions.

The compiler also tightened fail-closed checked-HIR/MIR validation, canonical type and interface identity, invalid PGO-profile diagnostics, and the production/test partition. These changes intentionally invalidate affected compiler interfaces and caches instead of accepting stale artifacts.

## Builds, caches, and release distribution

`alignc build --watch` now performs foreground transactional rebuilds with the same production semantics as one-shot builds. Ordinary non-ThinLTO multi-unit builds pipeline frontend work, code generation, and linking, while ThinLTO caches function-level native results under complete shared-definition fingerprints.

Release archives now include an immutable compiler-exact prebuilt cache for the first-party `pkg.db`, `pkg.web`, `pkg.frame`, and `pkg.auth` source units. It is a fallback behind the writable user cache, never changes package resolution, and is accepted only when the packaged compiler and native LLVM build identity match exactly.

Release artifacts continue to target Linux x86-64, Linux AArch64, and macOS Apple Silicon. Each archive contains `alignc`, `align-repl`, the matching runtime archive, licenses, and documentation; Linux also receives Debian and signed apt-repository artifacts when release credentials are available, and macOS receives the generated Homebrew formula.

## Reproducible benchmark evidence

The benchmark-evidence path now binds prepared source, Git revision and tree identity, toolchain, container image, native host qualification, fixed execution environment, exact sample schedule, report schema, and SSH signatures. Linux accepted measurements execute sealed prepared bytes and fail closed on identity drift or lifecycle races. This machinery supports performance claims; it does not weaken ordinary correctness gates or turn benchmark execution into ambient build input.

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.** v0.6.0 changes language ownership and borrow rules, test syntax, compiler interfaces, runtime ABIs, cache identities, package APIs, and diagnostics. `pkg.jwt` has been removed and replaced by `pkg.auth`; there is no compatibility alias. Rebuild all Align code with v0.6.0 and discard v0.5.0 interface, object, frontend, ThinLTO, and package-cache artifacts.

## Known Intentional Limitations

- `pkg.auth` supports HS256 only. Asymmetric JWT algorithms, JWKS discovery, cookies, session persistence, rotation, and revocation remain outside v1.
- `core.codec` has four non-null column kinds and one canonical format; nullable columns, schema registries, arbitrary Arrow IPC, and extension metadata are not included.
- HTTP streaming is client-side plaintext/TLS response consumption; HTTP/2 and HTTP/3 remain outside this release.
- `align-repl` has no in-process line editor, persistent runtime state, package/project import resolution, or REPL-only language behavior.
- Windows remains unsupported. Distributed `alignc` binaries dynamically depend on LLVM 22 and use the native capability libraries documented in `docs/impl/11-release-distribution.md`.
- Fully escaping function values and the remaining owned-value closure-capture shapes remain deferred.
