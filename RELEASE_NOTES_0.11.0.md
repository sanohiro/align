# Align v0.11.0 Release Notes

Align v0.11.0 collects the JSON, ownership, concurrency, numeric and native-boundary capabilities delivered since v0.10.2. Align remains pre-release. Update the compiler and runtime together, and rebuild generated programs and consumer binaries.

## JSON and bounded output

- `json.encode` and `json.encode_bounded` accept stable nested record fields without transferring their owners. Existing typed schemas and output bytes are preserved.
- Both encoders accept named `json.doc` roots and subtrees, including scalar values and null. Default output is compact JSON preserving member order, duplicate occurrences, original string escapes and exact number tokens, including numbers outside machine numeric ranges. Missing returns `Error.Invalid`.
- An optional final `json.encode_options` Copy record selects spaces after commas and colons and incremental string re-escaping. All three Boolean fields are explicit; omission preserves the original compact output. Every policy preserves exact numeric tokens. Bounded output counts separators and escapes before growth, without a proportional normalization buffer.
- Document encoding borrows its input and parse arena only through the call and returns an independent owned string. Exact-fit limits succeed; negative or exceeded limits return `Error.Invalid` without a partial result.

## Ownership, tasks and records

- Structured tasks can return self-contained owned data, including strings, buffers, ordinary dynamic arrays and supported record/tuple/sum compositions. After a successful wait, `get()` consumes an owning task handle and transfers its result once; Copy results remain repeatable. Group cleanup joins workers and drops untaken results. Owned and exclusive captures remain excluded.
- Heap-built owning records admit primitive Copy fixed-array fields, including zero-length fields, with inline storage and ordinary construction, transfer, replacement and cleanup.
- Indexed fixed-array fields support independent Copy reads and chained field projection, including `rows[i].shape[j]` and `frames[i].points[j].x`. These operations do not create retained views or implicitly clone owning fields.
- Compiler fixes preserve disjoint sibling snapshots across direct field transfers, admit `resource_ref` leaves in supported borrowed sum projections, certify initialized `out slice<T>` reads across imported/per-unit calls, and resolve imported record types in extern declarations.
- Regression coverage qualifies deferred optional-record initializer transfers while retaining rejection of reads after a completed move.

## Numeric, compression and native capabilities

- Explicit integer vectors support bitwise and shift operators with the existing scalar rules. Floating-point scalars and vectors gain `round_even()` without changing `round()`.
- `expm1()`, `log1p()`, `sin()`, `cos()` and `atan2()` are available on f32/f64 and their explicit vectors. Target-native evaluation and reported scalar fallback remain explicit; cross-platform bit identity and universal SIMD or accuracy guarantees are not promised.
- `std.compress` adds zlib and raw DEFLATE compression/decompression. All four decompression families accept an optional trailing output limit, defaulting to 1 GiB. The limit must be between zero and 1 GiB inclusive; out-of-range limits abort before decoding. Output growth stays within the inclusive caller limit; framing, checksum and complete-input requirements remain format-specific. The output bound excludes native workspace, allocator rounding and transient realloc overlap.
- `unsafe { raw.try_alloc(size) }` returns `Result<raw, Error>`: invalid sizes return `Invalid`, zero returns a null pointer without allocation, and positive allocation refusal returns `Code(12)`. Raw pointers compose through supported Option/Result, sum, tuple and ordinary record carriers without acquiring pointee ownership. Allocation, initialization and matching release remain explicit caller obligations.
- C-layout records admit positive fixed arrays of numeric C scalars, including raw load/store and supported SysV/AArch64 by-value ABI transport. Zero-length C arrays and nonnumeric array fields remain excluded.
- Linux child launch avoids copying the parent's address space while retaining launch-error reporting, signal and file-descriptor behavior.

## Compatibility and known limits

- Private runtime signatures and compiler artifact identities changed. Install `alignc`, `align-repl` and the matching runtime archive as one set; do not mix older runtime archives or generated objects with this compiler. The interface wire format remains version 21, while compiler identity prevents reuse of incompatible artifacts.
- K1/plan61, general interprocedural view/write authority, externally owned writable views, retained indexed-field slices, general exclusive owned-field/payload projection and Move-element sorting retain their separate prerequisites. This release does not close those deferred capabilities.
- Provider delivery is separate from adoption and acceptance in `align-llm`, `align-media` and `align-voice`. Native Codex/template/tool/image qualification remains consumer-owned.

Release artifacts target Linux x86_64, Linux ARM64 and macOS Apple Silicon. Packages contain `alignc`, `align-repl`, the matching runtime archive and the compiler-exact first-party package cache. Native dependencies remain explicit. Versioned artifacts use the existing thin-LTO and compiler-PGO distribution workflow.

Implementation and verification: [changes since v0.10.2](https://github.com/sanohiro/align/compare/v0.10.2...v0.11.0), including PRs #1385–#1406. Individual capability owners, independent reviews, bounded gates and platform CI qualify the delivered surfaces; deferred and consumer-owned acceptance is not implied.
