# Implementation Strategy (Overview)

The top-level document for implementing `draft.md`. All subsequent `docs/impl/*.md` follow this strategy.

## Decisions

```text
Implementation language   Rust
Backend                   LLVM (the one real target), but always through MIR
Approach                  Fix the whole design first / drive a vertical skeleton through first
```

## Why these three

### Implementation language: Rust

The standard choice for compiler implementation. Lexer/parser generators, LLVM bindings (`inkwell`), and strong types + ownership for self-checking are all available. It also serves as the foundation for later writing Align in Align (self-hosting).

### Backend: straight to LLVM, but through MIR

A staged strategy of "C backend first → LLVM later" is **not** adopted. Reasons:

- The core of Align is **deterministically lowering** `vecN<T>` / `maskN<T>` / loop fusion **to vector instructions**. Going via C would depend on the host C compiler's auto-vectorization, which breaks the "predictably fast" identity.
- Migrating to LLVM later would be a major rewrite.

Instead, **a backend-agnostic intermediate representation, MIR, is always interposed**. All of Align's semantics (arena / move / fusion / SIMD-ization decisions) live on the MIR side, and `MIR → LLVM` is limited to a pure final-stage lowering. This means:

- If we later want to add a C backend or text output for debugging, it is just "add one lowering" and not a rewrite.
- Backend-driven decisions do not leak as far as the type checker.

Details in `04-mir.md` / `05-backend-llvm.md`.

### Approach: whole-design → vertical-slice skeleton → fleshing out

The most dangerous thing in compiler development is "building out one stage before the whole pipeline is connected." Building out a complete type system before codegen, then finding at the codegen stage that the shape of the type information does not fit and rewriting the type checker — this is the real nature of a major rewrite.

The countermeasure is to separate two axes.

```text
Axis A  Feature coverage     few features → add features
Axis B  Pipeline traversal   does source → executable connect end to end
```

The danger is in axis B. Therefore:

> The design (these impl docs) fixes the whole picture first.
> The implementation drives the smallest vertical-slice skeleton (walking skeleton) through end to end first, then plugs features into it.

First complete a skeleton in which an `x := 1`-level program flows through lexer → parser → typecheck → MIR → LLVM → executable. Once the skeleton is through, `map` / `where` / arena / JSON become a matter of just **plugging** into the same pipeline, and no stage gets rewritten.

## Crate Structure (proposal)

A Rust workspace. Split crates per stage, matching IR boundaries to crate boundaries.

```text
alignc/                  workspace root
  crates/
    align_span/          source positions / file management (depended on by all stages)
    align_diag/          shared foundation for diagnostics (errors/warnings)
    align_lexer/         source → tokens
    align_parser/        tokens → AST
    align_ast/           AST definitions
    align_sema/          name resolution + type inference/checking + move/arena checking → typed HIR
    align_mir/           HIR → MIR conversion + MIR optimization (fusion etc.)
    align_codegen_llvm/  MIR → LLVM IR → object
    align_runtime/       minimal runtime (arena allocator etc.). Linked into output
    align_driver/        CLI: alignc build / run. Connects the stages
  tests/                 end to end (.align → run → output comparison)
```

Stage responsibilities and IR-boundary details in `01-pipeline.md`.

## Document Index

```text
00-overview.md        this document. Overall strategy
01-pipeline.md        pipeline stages and IR boundaries
02-frontend.md        lexer / parser / AST
03-types.md           type system / inference / move & arena checking
04-mir.md             MIR design (backend-agnostic core)
05-backend-llvm.md    MIR → LLVM lowering / SIMD / arena codegen
06-runtime-std.md     minimal runtime and core/std bootstrap
07-roadmap.md         milestones M0..Mn
08-memory-model-v2.md owned heap/drop + inferred borrow-region implementation design
08-nested-structs.md  nested aggregate ownership/lowering implementation record
09-explain-opt.md     optimized-IR / optimization-remark implementation record
10-cache-first-optimization.md  cache identity, incremental-build, and CPU-locality audit
11-parallel-execution-optimization.md  parallel correctness, low-lock runtime, and range-IR audit
12-pipeline-closure-memory-io-simd-audit.md  pipeline legality, closure ABI/lifetime, allocation, I/O, and SIMD audit
13-string-array-allocation-short-input-audit.md  text/array ownership, copy counts, and short-input audit
source-correctness-fixes-2026-07-13.md  implemented correctness fixes and their permanent regression gates
```

## Invariants (upheld in the implementation too)

The design invariants in `draft.md` / `docs/design-notes.md` remain binding at the implementation stage. In particular:

- allocation / error / side effects / parallelism / unsafe must be **traceable even in generated code** (nothing hidden).
- Restrictions are an information source for compiler inference. Infer no-alias / non-null / arena lifetime / cold error path without exposing lifetimes in source (`03-types.md`).
- Achieve, in MIR, the lowering by which `map` / `reduce` / `scan` / `filter` / `mask` vectorize naturally (`04-mir.md`).
