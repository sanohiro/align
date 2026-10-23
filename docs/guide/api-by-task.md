# Find an API by task

> 🌐 **English** · [Japanese](./ja/api-by-task.md)

Start with what you want to do. Each row points to an existing API, an example
or walkthrough, and the detailed contract for types, ownership, errors, and
limits. API names below are lookup terms, not complete call expressions.
This is an index of selected implemented capabilities, not a second API
specification or a list of everything planned.

- [Process arrays and records](#process-arrays-and-records)
- [Handle text and data formats](#handle-text-and-data-formats)
- [Read files and accept input](#read-files-and-accept-input)
- [Connect to services and run programs](#connect-to-services-and-run-programs)
- [Build an application from packages](#build-an-application-from-packages)
- [Check and test your program](#check-and-test-your-program)

`core` provides language operations; `std` provides OS and service boundaries;
`pkg` libraries are source you copy into your project. Standard-library calls
such as `fs.read_file` require `import std.fs`; package calls keep their qualified
name, such as `pkg.csv.decode`, after `import pkg.csv`. Follow the complete
[CSV-to-JSON app](practical-cli.md) to combine several rows in one project.

## Process arrays and records

| I want to… | API or language feature | Start here | Exact contract |
|---|---|---|---|
| Filter, transform, and sum values | `.where`, `.map`, `.sum`, `.reduce` | [Pipelines](06-pipelines.md) | [Arrays, slices, and pipelines](../impl/core-design/array-slice-pipeline.md) |
| Combine two inputs into caller-owned output | `zip`, `.map_into` | [Complete example](../../examples/zip_map_into.align) | [Arrays, slices, and pipelines](../impl/core-design/array-slice-pipeline.md) |
| Read or update an owned array through a helper | `borrow`, `borrow mut`, `slice<T>` | [Complete example](../../examples/borrowed_array.align) | [Memory and borrowing](05-memory.md) |
| Work in columns or aggregate by key | `soa<T>`, `.group_by` | [Data-oriented programming](11-data-oriented.md) | [SoA and grouped aggregation](../impl/core-design/soa-groupby.md) |
| Apply one Pure function to many elements in parallel | `.par_map` | [Data parallelism](10-closures-and-parallelism.md#par_map--data-parallelism) | [Language specification](../../draft.md) |
| Run independent jobs, including I/O, and collect their results | `task_group`, `spawn`, `wait`, `.get` | [Task parallelism](10-closures-and-parallelism.md#task_group--task-parallelism) | [Language specification](../../draft.md) |
| Handle a missing value or a fallible operation | `Option`, `Result`, `else`, `?`, `match`, `.map_err` | [Errors](04-errors.md) | [Option and Result](../impl/core-design/option-result.md) |

Choose an explicit materialization terminal when you need stored output.
Borrowed views must stay within their owner's lifetime. `par_map` requires a
Pure callable. Spawned tasks may perform I/O, but shared captures remain read-only
until the group joins.

## Handle text and data formats

| I want to… | API or language feature | Start here | Exact contract |
|---|---|---|---|
| Keep text after its source is gone | `str.clone()` → owned `string` | [Strings and text](07-strings-and-text.md) | [Strings and builders](../impl/core-design/string.md) |
| Assemble text without repeated concatenation | `builder`, `.write`, `.write_int`, `.to_string` | [Builder examples](07-strings-and-text.md#the-builder) | [Strings and builders](../impl/core-design/string.md) |
| Decode JSON with independently owned fields | `json.decode`, record fields of `string` / `array<T>` | [Complete owned JSON example](../../examples/json_owned.align) | [JSON types and ownership](../impl/core-design/json.md) |
| Read JSON through borrowed text or columns | `json.decode`, `str`, `soa<T>`, `arena` | [JSON](08-json.md) | [JSON types and ownership](../impl/core-design/json.md) |
| Inspect or forward JSON whose schema is not known | `json.doc`, `.get`, `.at`, `json.encode_options` | [Document views](08-json.md#when-the-schema-is-not-known) | [JSON types and ownership](../impl/core-design/json.md) |
| Write JSON, optionally with a byte cap | `json.encode`, `json.encode_bounded` | [Encoding](08-json.md#encoding) | [JSON](../impl/core-design/json.md) |
| Decode CSV into typed columns | `pkg.csv.decode`, `DecodeOptions` | [Complete app](practical-cli.md) | [CSV](../impl/pkg-design/csv.md) |
| Exchange a typed columnar binary batch | `codec.encoder`, `codec.open` | [Usage examples](../impl/core-design/codec.md#source-surface) | [Columnar codec](../impl/core-design/codec.md) |
| Encode base64 or hex, or validate UTF-8 | `encoding.base64_encode`, `encoding.hex_decode`, `.as_str` | [Encoding](14-std-encoding-rand-cli.md#stdencoding) | [Standard-library specification](../../draft.md) |
| Search text with a pattern | `regex.compile`, `.find`, `.is_match` | [Regex](14-std-encoding-rand-cli.md#stdregex) | [Regex](../impl/std-design/regex.md) |
| Normalize Unicode to NFC | `pkg.unicode.nfc` | [Source package](../../apps/unicode/pkg/unicode.align) | [Unicode normalization](../impl/pkg-design/unicode.md) |

JSON decoding takes its target type from the binding annotation, not from a
written type argument at the call. `str` views and owned `string` fields have
different lifetimes; check the JSON contract for the admitted type shapes.
Decoding arbitrary bytes as text can fail; keep binary data as bytes until a
text operation requires UTF-8.

## Read files and accept input

| I want to… | API or language feature | Start here | Exact contract |
|---|---|---|---|
| Read or write a small UTF-8 file | `fs.read_file`, `fs.write_file` | [Files in one call](13-std-os.md#files-in-one-call-stdfs) | [Filesystem](../impl/std-design/fs.md), [standard-library specification](../../draft.md) |
| Copy binary data with bounded working storage | `reader.read`, `buffer.try_new`, `writer.write` | [Stream loop](13-std-os.md#streams-reader-writer-buffer) | [I/O specification](../../draft.md) |
| Parse command-line flags | `cli.command`, `.flag_str`, `.flag_bool`, `.parse` | [App boundary](practical-cli.md#3-add-the-command-line-boundary) | [CLI](../impl/std-design/cli.md) |
| Write stdout or stderr and detect output errors | `io.stdout`, `io.stderr`, `.write` | [Complete app](practical-cli.md) | [I/O specification](../../draft.md) |
| Walk directories with explicit bounds | `fs.open_directory`, `.cursor`, `.next` | [Tree summary example](../../examples/tree_summary.align) | [Retained directories](../impl/std-design/fs.md#retained-directories-and-raw-entry-names) |
| Create a new file without replacing an existing entry | `fs.create_exclusive`, `fs.rename_no_replace` | [Exclusive publication](13-std-os.md#exclusive-publication) | [Filesystem](../impl/std-design/fs.md) |

Whole-file reads allocate for the whole input. A `reader`, `writer`, or directory
handle owns or borrows a resource according to its constructor; keep the owner
live while using derived views. File and stdout writes can fail after a prefix
has been written.

## Connect to services and run programs

| I want to… | API or language feature | Start here | Exact contract |
|---|---|---|---|
| Make an HTTP request | `http.client`, `.get`, `.request` | [HTTP](18-std-services.md#stdhttp) | [HTTP](../impl/std-design/http.md) |
| Download binary HTTP data with an explicit limit | `.request_stream`, stream `.read` | [Complete fetch example](../../examples/http_fetch.align) | [Streaming receive](../impl/std-design/http.md#client-streaming-receive-post-pkgdb-convergence-item-1--implemented-2026-08-30) |
| Connect a TCP stream or resolve a hostname | `tcp.connect`, `dns.resolve` via `import std.net` | [Networking](18-std-services.md#stdnet) | [Networking](../impl/std-design/net.md) |
| Run a child program and inspect its output | `process.command`, `.run`, `.code`, `.stdout` | [Captured output example](../impl/std-design/process.md#surface) | [Processes](../impl/std-design/process.md) |
| Compress bytes or hash a file incrementally | `compress.gzip_compress`, `crypto.sha256_stream` | [Services and hashing examples](18-std-services.md) | [Compression](../impl/std-design/compress.md), [cryptography](../impl/std-design/crypto.md) |
| Write logs through an explicit writer | `log.new`, `.line`, `.flush` | [Logging](18-std-services.md#stdlog) | [Logging](../impl/std-design/log.md) |

An HTTP error status and a child's nonzero exit code are result data: inspect
them separately from transport or launch errors. Set the limits and timeouts
the chosen API exposes; their scopes differ, and a timeout is not automatically
an end-to-end deadline. Captured output and HTTP body views depend on their
owning result handles.

## Build an application from packages

Copy each package's complete source subtree, including its internal modules and
documented dependencies. [Package setup](23-packages.md#a-package-is-a-source-tree)
explains the layout; package sources do not come inside the compiler binary.
The linked designs include limits and deferred features as well as implemented
operations, so read the status and scope of the specific operation you select.

| I want to… | Package | Start here | Exact contract |
|---|---|---|---|
| Serve HTTP routes, middleware, or SSE | `pkg.web` | [Web package](23-packages.md#pkgweb) | [Web](../impl/pkg-design/web.md) |
| Add WebSocket server routes | `pkg.ws` with `pkg.web` | [WebSockets](23-packages.md#pkgws) | [WebSockets](../impl/pkg-design/ws.md) |
| Build HTML with escaped values | `pkg.template` | [Templates](23-packages.md#pkgtemplate) | [Templates](../impl/pkg-design/template.md) |
| Join typed columns | `pkg.frame` | [Join signatures and examples](../impl/pkg-design/frame.md) | [Frame](../impl/pkg-design/frame.md) |
| Query SQLite or PostgreSQL | `pkg.db` | [Database tutorial](24-database.md) | [Database](../impl/pkg-design/db.md) |
| Verify tokens or hash passwords | `pkg.auth` | [Authentication](23-packages.md#pkgauth) | [Authentication](../impl/pkg-design/auth.md) |
| Access a RESP2 key-value service | `pkg.kv` | [Key-value example](23-packages.md#pkgkv) | [Key-value client](../impl/pkg-design/kv.md) |

CSV and Unicode packages appear in the [data-format table](#handle-text-and-data-formats).

## Check and test your program

| I want to… | Tool or feature | Start here | Exact contract |
|---|---|---|---|
| Split code into files | `module`, `import`, `pub` | [Modules](09-generics-and-modules.md#modules-are-files) | [Language specification](../../draft.md) |
| Check the program before running it | `alignc check`, `alignc check-per-unit`, `alignc build` | [App workflow](practical-cli.md#4-check-run-and-build) | [Toolchain](16-toolchain.md) |
| Write and run unit tests | `test`, `test.expect`, `test.expect_eq`, `alignc test` | [Tests in a real module](practical-cli.md#2-put-the-computation-and-its-tests-in-a-module) | [Tests](../impl/core-design/test.md) |
| Measure and inspect generated code | `alignc emit-llvm`, `alignc explain-opt`, `alignc size` | [Performance guide](26-performance.md) | [Optimization explanations](../impl/09-explain-opt.md) |

For short independent programs, browse [runnable examples](../../examples/README.md).
For a guided reading order, return to the [guide contents](README.md).
