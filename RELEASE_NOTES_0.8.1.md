# Align v0.8.1 Release Notes

Align v0.8.1 collects the language, compiler, standard-library, and reliability work since v0.7.5. The v0.8.0 tag did not produce a release because its prebuilt-cache verification found nondeterministic object bytes. Align remains pre-release; rebuild programs and libraries with the matching compiler and runtime archive.

## Language and data

- Fixed-size inline arrays can be stored in records and passed across module boundaries. JSON decoding keeps their storage inline instead of allocating a separate array buffer.
- `match` accepts integer and character value/range patterns and exact string-literal patterns.
- Checked typed byte views and `as_bytes` provide explicit, provenance-checked reinterpretation without a copy.
- Scoped `float(reassoc)` and `float(contract)` express the two named floating-point relaxations. `core.math` includes `exp`, `exp2`, `log`, `log2`, and `log10`.

## Compiler and performance

- Sum types, `Option`, and `Result` use explicit-tag union storage. Aggregate calls and returns use the revised transport and drop-state model.
- The compiler carries scalar sign/zero extension facts at call boundaries, a per-symbol runtime-effects model, and small public function bodies through interfaces for eligible cross-module inlining.
- Loop facts materialize borrowed view headers once, narrow bounds checks, and version admitted counted loops while retaining a checked fallback.

## Runtime and libraries

- `std.http` servers can set an inbound request-body cap before `accept()`. A declared over-limit body returns `Error.Invalid`, closes that connection, and leaves the listener usable.
- The standard library adds owned host observations, incremental SHA-256 contexts, retained-root filesystem operations, process controls, and JSON encoding paths. Buffer and HTTP operations have explicit bounds and ownership checks.

## Reliability

- Fixes cover fallible owned-field replacement, borrowed fixed-array producer certification, resource formation diagnostics, and borrowed-byte range facts.
- Tagged `array_builder` elements now pass codegen type validation, and invalid constant annotations produce one primary diagnostic.
- The nightly full-suite detector now has bounded shards and explicit failure reporting; its deep-pipeline owner follows string-dispatch edges and inspects the optimized kernel behind an export wrapper.
- Parameter transport now emits functions in deterministic module order, so cache-disabled and prebuilt-cache builds produce byte-identical objects.

This release includes changes to the pre-release interface format and compiler/runtime ABI. Rebuild cached interfaces, generated programs, and consumer binaries when updating the toolchain.
