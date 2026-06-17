# Align

Align is an AOT-compiled, data-oriented programming language designed to align
human intent, AI generation, compiler optimization, and modern hardware.

> Less code. Predictable performance.

This is an early-stage project. The design lives in `draft.md` + `docs/`; the
compiler (`alignc`) is being implemented in Rust under `crates/`.

## Status

Milestone **M0 (walking skeleton)** is complete: a minimal program flows end to
end through `lexer → parser → sema → MIR → LLVM → executable`.

```sh
cargo build
cargo test
cargo run --bin alignc -- run examples/min.align   # exits with code 1
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
