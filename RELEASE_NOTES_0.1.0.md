# Align v0.1.0 Release Notes

Welcome to the v0.1.0 release of Align. This is the first public milestone of the Align language, encompassing everything from the initial M0 walking skeleton through the M15 separate compilation and M14 post-upgrade (ThinLTO and PGO) waves.

## Summary (M0–M15 + M14)

This release ships the full core language and toolchain pipeline:
- **Core Compiler**: `lexer → parser → sema → MIR → LLVM → native` pipeline with parallel and cached per-unit codegen.
- **Language Features**: `if` / `match` / `else`-unwrap, `Option`/`Result` with `?`, `arena`/`box` with move & escape checking, `loop` with unified break, strings + `json` parsing.
- **Data-Oriented Core**: Fused array pipelines (`map`, `where`, `sum`, `reduce`, `group_by`), stable adaptive sorting, and SIMD primitives (`vecN`, `soa`, `maskN`).
- **Parallelism**: Safe parallel mapping (`par_map`), first-class closures (with escape-checked captured environments), and `task_group` on real threads.
- **Standard Library**: `io`, `fs`, `path`, `env`, `time`, `encoding`, `rand`, `cli`, `net`, `process`, `compress`, `crypto`, and `http` (with HTTPS/TLS via libssl).
- **Optimization**: ThinLTO for cross-unit inlining and profile-guided optimization (PGO) instrumentation/use.
- **FFI**: `unsafe` blocks, raw pointers, and `extern "C"` functions (C-ABI calling).

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.**
As we iterate towards a stable 1.0, the language syntax, standard library APIs, and ABI may break without warning or legacy fallbacks.

## Known Intentional Limitations

The following features are deliberately deferred or currently unsupported in v0.1.0:

- **Fully-escaping function values**: Returning closures from functions, storing them in structs, or keeping them in arrays is deferred until the heap-owned environment/drop model is settled. Closures currently only survive as scoped captures (e.g., inside `map` or `task_group`).
- **`extern "C"` export-of-body**: You can call C functions from Align, but exporting an Align function to be called from C with the C ABI is not yet supported.
- **Windows Support**: Align currently targets Linux (x86-64) and macOS (Apple Silicon). Windows is explicitly unsupported at this time.
- **AArch64 SIMD limits**: While x86-64 uses AVX2 for many hot paths, some `aarch64` SIMD paths (such as UTF-8 validation and stable compaction) remain on the scalar production path because current NEON translations did not meet the strict no-regression performance gates. (Base64 and Hex encode do have native NEON dispatch).
