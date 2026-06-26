# Align

Align is an AOT-compiled, data-oriented programming language designed to align
human intent, AI generation, compiler optimization, and modern hardware.

> Less code. Predictable performance.

This is an early-stage project. The design lives in `draft.md` + `docs/`; the
compiler (`alignc`) is being implemented in Rust under `crates/`.

## Status

Milestones **M0–M3** are complete: programs flow end to end through
`lexer → parser → sema → MIR → LLVM → executable`. M1 adds functions and calls,
`if`/comparisons/`bool`, `mut` reassignment, structs (declaration, literal,
field access), the full primitive set (`i8..u64`, `f32`/`f64`, `char`), and a
builtin `print` wired to the runtime. M2 adds `Option<T>` with `else`-unwrap,
`Result<T, E>` with `?`, and a `Result`-returning `main` (mapped to an exit code).
M3 adds the memory model: `arena {}` regions with bulk free, the heap box
`box<T>` (`heap.new`/`.get()`/`.clone()`), move / use-after-move checking, and
arena escape checking. M4 (core slice) adds fixed-length arrays and the fused
pipeline `[...].map(f).where(p).sum()` — lowered to a single loop with no
intermediate arrays.

```sh
cargo build
cargo test
cargo run --bin alignc -- run examples/arena.align     # arena + heap box; exits 42
cargo run --bin alignc -- run examples/pipeline.align  # fused map/where/sum; exits 24
```

`alignc` subcommands: `check`, `emit-mir`, `emit-llvm`, `build`, `run`.

## Performance & portability

Align targets the cloud/container reality of *build once, run on a varied fleet*. The default build
uses a **safe, portable per-architecture baseline** (`x86-64-v2` on amd64, `armv8-a`/NEON on arm64),
so one binary runs across mixed Intel/AMD/Graviton and feature-masked hosts. More aggressive targets
are **opt-in, never the default** (`--target-cpu native` for a source build on the host you run on,
or a higher baseline for a fleet you control). Wide SIMD on a varied fleet comes from **runtime
CPU-feature dispatch in the library** (one binary picks AVX2/NEON at run time, falling back safely) —
not from a fixed high baseline. See `draft.md` §3.4 and `docs/open-questions.md` ("Build targets &
portability"). The default build now targets that portable baseline (`x86-64-v2` on amd64, the
`armv8-a` floor on arm64); `alignc build|run … --target-cpu native` opts into the build host's exact
CPU. *(The codegen baseline + opt-in is in place; library runtime-dispatched SIMD still lands with the
std/runtime layer — the current backend builds scalar IR and leans on LLVM `-O2` autovectorization.)*

## Requirements

- Rust (stable)
- LLVM 19 (`llvm-config` on `PATH`), a C compiler (`cc`) for linking

## Layout

- `draft.md` — authoritative language specification
- `docs/` — design rationale, history, non-goals, open questions
- `docs/impl/` — compiler implementation plan (stages, MIR, backend, roadmap)
- `crates/` — the `alignc` compiler workspace

## License

MIT
