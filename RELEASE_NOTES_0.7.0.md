# Align v0.7.0 Release Notes

Align v0.7.0 adds bounded XML and CSV readers, typed Redis operations,
composable WebSocket server support, and an HTML text builder. The new surfaces
keep ownership, allocation, limits, and failure explicit.

## Bounded XML and CSV

`std.xml` consumes one owned UTF-8 string and validates the complete supported
XML 1.0 document before publishing a forward-only reader. It exposes Start,
End, and Text events, lexical names, and source-order attributes. Cursor-bound
views cannot survive advancement without an explicit clone. Element depth and
attributes per element are each limited to 256. DTDs, entity declarations, and
processing instructions are rejected; namespaces remain lexical. This is not a
DOM, streaming input API, or general entity processor.

`pkg.csv` decodes in-memory UTF-8 CSV directly into an arena-backed `soa<R>`.
The caller selects header handling, line endings, the destination region, and a
row limit. Named headers support typed column projection. String cells borrow
the input where possible; doubled-quote normalization uses the output arena.
The first capability does not include encoding, streaming, file loading, or
dialect inference.

## Application packages

`pkg.kv` provides a typed, single-owner Redis RESP2 client over plaintext TCP.
GET returns an optional owned string; SET has explicit condition and expiry
options; single-key DEL reports whether a key was removed. Connection and I/O
timeouts and response bounds are explicit. Shared TCP timeout handling and
connection-derived writers were hardened, including SIGPIPE suppression.

`pkg.ws` composes RFC 6455 server handshakes and bounded message handling with
the existing `pkg.web` routes, middleware, and worker model. A protocol-neutral
HTTP/1.1 Upgrade boundary transfers the accepted byte transport. WebSocket
connections occupy their worker; the package adds no hidden thread or second
concurrency model.

`pkg.template` provides one Move-owned HTML text builder. Ordinary writes
escape text by default, `raw` explicitly appends trusted markup, and
`to_string` consumes the builder and transfers the output without copying. It
is not a contextual template language or an HTML sanitizer.

## Compiler and learning workflow

The XML capability includes typed ownership and cleanup across control flow,
generic carriers, whole-program compilation, and per-unit interfaces. Producer
validation authenticates protected values and call arguments before LLVM
lowering, including existing string-bearing array, SoA, subslice, and JSON
scanner paths.

The bilingual Little Aligner guides now make the native AOT REPL the practical
reading workflow. The post-XML consolidation plan is recorded separately; its
future qualification and optimization work is not part of this release.

## Distribution and compatibility

Release artifacts target Linux x86-64, Linux AArch64, and macOS Apple Silicon.
Archives contain `alignc`, `align-repl`, and the matching runtime archive.
Versioned builds use the existing dist profile and compiler PGO pipeline; LLVM
22 and native linker dependencies remain explicit.

**Align makes no backward compatibility guarantees during the 0.x series.**
Rebuild all Align programs and vendored packages with v0.7.0. Do not reuse
compiler interfaces, objects, or cache artifacts from v0.6.0. The new checked
operations, runtime surfaces, and ownership validation are part of one
compiler/runtime version.
