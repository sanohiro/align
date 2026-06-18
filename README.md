# Align

Align is an AOT-compiled, data-oriented programming language designed to align
human intent, AI generation, compiler optimization, and modern hardware.

> Less code. Predictable performance.

This is an early-stage project. The design lives in `draft.md` + `docs/`; the
compiler (`alignc`) is being implemented in Rust under `crates/`.

## Status

Milestones **M0 (walking skeleton)** and **M1 (the bones of the language)** are
complete: programs flow end to end through
`lexer → parser → sema → MIR → LLVM → executable`. M1 adds functions and calls,
`if`/comparisons/`bool`, `mut` reassignment, structs (declaration, literal,
field access), the full primitive set (`i8..u64`, `f32`/`f64`, `char`), and a
builtin `print` wired to the runtime.

```sh
cargo build
cargo test
cargo run --bin alignc -- run examples/point.align    # prints 3, 10; exits 13
cargo run --bin alignc -- run examples/circle.align   # float arithmetic; exits 1
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
