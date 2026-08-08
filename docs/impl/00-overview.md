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

## Crate Structure (current)

A Rust workspace. Split crates per stage, matching IR boundaries to crate boundaries.

```text
alignc/                  workspace root
  crates/
    align_span/          source positions / file management, including registered static inputs
                        (depended on by all stages)
    align_diag/          shared foundation for diagnostics (errors/warnings)
    align_ast/           syntax tree shared by parser, sema, and formatter
    align_lexer/         source → tokens
    align_parser/        tokens → AST
    align_sema/          name resolution + type inference/checking + move/arena checking → typed HIR
    align_mir/           HIR → MIR conversion + MIR optimization (fusion etc.)
    align_codegen_llvm/  MIR → LLVM IR → object
    align_runtime/       minimal runtime (arena allocator etc.). Linked into output
    align_interface/     canonical per-unit public interfaces + hashes/codecs, including
                        public static-Query contracts
    align_hash/          canonical hashing shared by language/runtime/cache identity
    align_fmt/           token-preserving source formatter
    align_driver/        CLI, import DAG, object cache, parallel codegen, link
                        (end-to-end tests live under this crate)
```

Stage responsibilities and IR-boundary details in `01-pipeline.md`.

## Current implementation notes (2026-08-08)

String comparison is complete for both `str` and owned `string`: owned operands use the ordinary
non-consuming `str` borrow before lowering, including mixed and generic `Eq`/`Ord` comparisons
(`draft.md` §5).

## Required native-library boundary (settled 2026-07-27)

Ordinary Align packages must be able to wrap persistent native state without receiving a
compiler-known handle type for every library. The design of record is
`17-library-boundary-prerequisites.md`:

```text
recursive tagged Move payloads
borrowed parameters + inferred return provenance
package-defined opaque Move resources + exactly-once Drop
named arena region capabilities
deterministic compiler-registered static source inputs
region-backed plain-struct builders
nested generic package composition + closed RegionPlain bounds
```

These are general language/compiler mechanisms. `pkg.db` is their first complete consumer, but
neither ownership checking nor MIR lowering may dispatch on a `pkg.db`, `std.http`, or other
package name. Existing compiler-known std handles migrate only when their owning package is
implemented in ordinary Align; this work does not create an `std.http` dependency for `pkg.db`.

Static inputs are a narrow extension of the source pipeline. The driver discovers exact files
referenced by recognized static constructors, registers them in `align_span`, and incorporates
their hashes into the producer action and implementation identities. The public semantic contract
is encoded by `align_interface`; producer-only bytes and generated thunks stay in the object and
a separate versioned artifact. No build script, manifest language, directory scan, environment
read, or build-time database connection is introduced.

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
11-release-distribution.md      CI targets, release artifacts, Homebrew, and apt distribution
11-parallel-execution-optimization.md  parallel correctness, low-lock runtime, and range-IR audit
12-pipeline-closure-memory-io-simd-audit.md  pipeline legality, closure ABI/lifetime, allocation, I/O, and SIMD audit
13-string-array-allocation-short-input-audit.md  text/array ownership, copy counts, and short-input audit
14-llm-inference-focus-audit.md  measured model-loading/layout/routing/profiling priorities
16-test-policy.md     ordinary PR gate and focused/full/performance test policy
17-library-boundary-prerequisites.md  borrow/resource/region/static-input/builder prerequisites
18-pkg-db-review.md   database design feasibility review, findings, and revised delivery gates
source-correctness-fixes-2026-07-13.md  implemented correctness fixes and their permanent regression gates
```

## Invariants (upheld in the implementation too)

The design invariants in `draft.md` / `docs/design-notes.md` remain binding at the implementation stage. In particular:

- allocation / error / side effects / parallelism / unsafe must be **traceable even in generated code** (nothing hidden).
- Restrictions are an information source for compiler inference. Infer no-alias / non-null / arena lifetime / cold error path without exposing lifetimes in source (`03-types.md`).
- Achieve, in MIR, the lowering by which `map` / `reduce` / `scan` / `filter` / `mask` vectorize naturally (`04-mir.md`).
