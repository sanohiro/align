# Align v0.10.1 Release Notes

Align v0.10.1 collects the language, ownership, JSON, pipeline, library, and performance work since v0.9.0. Align remains pre-release. Update the compiler and runtime together, and rebuild cached interfaces, generated programs, and consumer binaries.

## Release recovery

The v0.10.0 tag did not produce published release assets: prebuilt cache warming
failed on the known `pkg.web` structured-task callback compatibility gap.
This release retains the compiler's conservative task access checks and warms
an explicit dependency-closed corpus: `pkg.db`, `pkg.frame`, `pkg.auth`,
`pkg.kv`, `pkg.csv`, `pkg.template`, and `pkg.s3` (21 package modules plus the
generated entry). Every selected unit must compile and pass cache verification.
Compiler failures now print their captured diagnostics in release logs.

Full `pkg.web` imports, including `pkg.ws`, still fail the task capture check;
choosing one worker does not avoid it. Their source contracts remain documented,
but this release does not repair that compatibility gap or prebuild their cache.
Package sources continue to be vendored separately from compiler distribution.

## Language, ownership, and data

- Fixed arrays support owned `string` elements (`[string; N]`) and ordinary dynamic-array headers (`[array<T>; N]`) with recursive Move and Drop handling. Local fixed scalar and text arrays admit shared and exclusive whole-array borrows.
- Direct owning record-field extraction transfers arrays, nested Move records, and tuples through the existing ownership model. Flat tuples compose with `Option`, `Result`, user sums, ordinary record fields, and callbacks.
- Copy `rng` state composes inside ordinary concrete, generic, and nested records, advancing through explicit mutable borrowing without heap allocation.
- Indexed array fields on owning records support scalar `.len()` queries, independent Copy element reads (`rows[r].values[i]`), and element writes on owned scalar-array fields.
- Shared sum-payload matches preserve disjoint field origins across repeated calls. Mutable-borrow checks separate storage authority from borrowed contents and distinguish mutable-output dependencies from possible mutation across direct and known noncapturing indirect calls.
- Typed `array_builder<T>` accumulators support nonconsuming indexed reads of independent Copy elements before `build()`.

## Pipelines, concurrency, and diagnostics

- Explicit comparator sorting (`sort_by(cmp)`) supports scalar Copy elements and independent Copy records. `sort_by_key` also accepts independent Copy records, and `scan(init, f)` admits Copy record inputs and independent Copy record accumulators.
- Sequential `par_map` fallback preserves original HIR capture identities across sibling branches without cloning stage expressions.
- Structured tasks (`task_group`) progress during caller work while protecting shared captures and support returning independent inline Copy records.
- `explain-opt` surfaces standalone LLVM vectorization-profitability refusals and reports vectorization blockers without speculative remedies.

## JSON, libraries, and documentation

- JSON decoding supports standalone owned `string` roots, `array<string>` roots, and direct owned record-array (`array<Record>`) roots with complete indexed grammar validation. Recursive JSON graphs compose borrowed `str` and owned `string` fields across decode and encode paths.
- `json.doc.keys()` materializes object keys in document order in linear time within an explicit arena, pairing directly with `elems()`.
- `pkg.unicode` adds fallible Unicode 16 NFC normalization (`unicode.nfc`) with independent owned output and conformance coverage.
- `std.cli` adds finite floating-point options, explicit option occurrence counts, and parsing from borrowed `slice<str>` argument slices.
- `std.fs` adds `open_ro_regular` for regular-file admission and positional reads, plus advisory available-byte queries on retained directory handles.
- Runnable examples, bilingual guides, the CLI walkthrough, and the task-based API index have been refreshed.

## Correctness and performance

- Ownership and lowering fixes cover indexed-assignment target liveness after eager operands, complete record-field replacement publication, deferred aggregate source transfers, named-arena loop validation, native JSON arena-decoder allocation authority and union error cleanup, nominal validated `core.codec` batch results, and borrowed view fields in codec records.
- Escape analysis streamlines singleton generation renames, observer scans, and projected header scans.
- Runtime fast paths reduce work in HTML, percent, and form length counting and unchanged/prefix bulk copies, named timestamp calendar parsing, RFC 1123 name dispatch, short and block decimal parsing, builder two-digit integer emission, and path extension delimiter search. Retained benchmarks document local measurements and limits rather than a general application-speed guarantee.

## Compatibility and known limits

- Compiler interfaces, checked-HIR/MIR validation inventories, and the native runtime ABI have changed since v0.9.0. Old runtime archives, cached artifacts, and compiled consumers must not be mixed with this compiler.
- K1/plan61 remains deferred: known gaps in interprocedural write-authority and temporal borrow validation (including text-builder byte views across helpers) are not closed by this release.
- General exclusive borrowing of partial Move fields, exclusive matched owned sum payloads, and Move element sorting remain outside the delivered surface.
- Provider delivery in Align is separate from external consumer adoption (`align-llm`, `align-media`, and `align-voice`), which remains owned by each consumer repository.

Release artifacts target Linux x86_64, Linux ARM64, and macOS Apple Silicon. Each package contains `alignc`, `align-repl`, the matching runtime archive, and the prebuilt first-party package cache; native dependencies remain explicit.
