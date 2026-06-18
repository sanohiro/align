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
