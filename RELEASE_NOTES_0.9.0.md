# Align v0.9.0 Release Notes

Align v0.9.0 collects the compiler, native interoperability, buffer, and standard-library work since v0.8.1. Align remains pre-release. Update the compiler and runtime together, and rebuild cached interfaces, generated programs, and consumer binaries.

## Native interoperability and data

- C-layout records support non-owning raw-pointer fields and by-value calls and returns on ARM64 Linux/macOS and LP64 GNU/musl Linux x86_64. Native ABI handling includes memory-passed aggregates, hidden return pointers, alignment, and register exhaustion.
- Stable sorting accepts Copy string and character elements and owned `String` keys.
- Text and typed array builders expose nonconsuming `.len()` observations. Typed text-builder helpers and checked decimal text-to-`i64` conversion are available.
- Imported public constants can be folded in initializers.

## Explicit buffer allocation and growth

- Buffer constructors accept explicit payload alignment. `buffer.try_new` and `buffer.try_filled` report allocation refusal through `Result`.
- All four constructors accept an optional `buffer.page_policy`. `Default` preserves ordinary allocation; `PreferHuge` requests best-effort huge-page advice for eligible Linux allocations and falls back portably elsewhere. It guarantees neither huge-page backing nor a speedup.
- Fresh zero-filled buffers use zeroed allocator acquisition. Alignment and page preference remain attached to the complete owner through growth, moves, returns, and replacement.
- Direct and nested stable buffer fields support growth through their complete mutable owner. Self-appends within the initialized prefix avoid a temporary byte snapshot.
- Buffers expose usable read-window capacity, and filesystem reads can fill an existing buffer directly.

## Runtime, libraries, and examples

- HTTP servers expose explicit complete-request and stream-write timeout budgets. Binary and SSE stream sends avoid intermediate payload copies; fixed and close-delimited SSE receives use bulk reads.
- Files support read-only random access, exclusive creation, and explicit synchronization. `io.copy` reuses eligible buffered-reader storage.
- `std.os` provides advisory total and available RAM observations on Linux and macOS.
- New bounded examples cover retained-directory traversal, binary HTTP downloads, incremental SHA-256, SSE progress, typed CSV summaries, child-output forwarding, and multimodal job composition.
- Example output paths propagate delivery and flush errors. Fallible read-window construction preserves allocation errors.

## Correctness and performance

- Ownership and validation fixes cover moved-field nulling, mutable Copy-view effects, indexed view snapshots, owned resource match results, array-builder provenance, and owned records containing fixed arrays.
- Malformed `json.doc` input now reports `Error.Invalid` consistently through direct matches, propagation, and imported helpers.
- Text admission rejects invalid UTF-8 arguments and environment values appropriately; gzip and zstd decoding consumes complete input; finite float formatting retains complete values.
- Compiler escape analysis reduces repeated state copies, storage-generation updates, hashing overhead, and retained diagnostic-replay data. These changes preserve existing analysis rules; they do not resolve the deferred K1 work.
- Runtime improvements reduce temporary storage and repeated scans in CSV, percent/form encoding, UTF-8 replacement, Base64, filesystem paths, process observations, and SSE parsing. Retained benchmarks document workload-specific results and limits rather than a general application-speed guarantee.

## Compatibility and known limits

- Compiler interfaces and native runtime ABI have changed since v0.8.1, including buffer-constructor arguments. Old runtime archives and compiled consumers must not be mixed with this compiler.
- K1/plan61 remains deferred: known gaps in interprocedural write-authority and temporal borrow validation are not closed by this release.
- General exclusive borrowing of partial Move fields and broader builder indexed/view readers remain outside the delivered surface. C FFI boolean and character support remains deferred.
- Align-side implementation and regression coverage do not establish align-llm adoption, real model/HTTP/CUDA acceptance, or end-to-end latency improvements. Those consumer checks remain pending for the latest requests.

Release artifacts target Linux x86_64, Linux ARM64, and macOS Apple Silicon. Each package contains `alignc`, `align-repl`, and the matching runtime archive; native dependencies remain explicit.
