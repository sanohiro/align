# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repository is

This is the **design specification + early implementation of "Align"**, an AOT-compiled, data-oriented programming language. The authoritative design lives in Markdown (`draft.md` + `docs/`); the compiler implementation has begun under `crates/` (Rust workspace, milestone M0).

Two kinds of work coexist:
- **Docs** (`draft.md`, `docs/`): design spec. "Correctness" = internal consistency across documents. Editing rules below still apply.
- **Code** (`crates/`): the `alignc` compiler. `cargo build` / `cargo test` apply. Code must stay consistent with the spec, not redefine it.

## Document layout and roles

- `draft.md` — the **authoritative, most complete** spec (Language Specification Draft v0.1, sections 1–21). When the design needs detail, this is the source of truth. Written in English prose with `align` code blocks.
- `docs/language-spec.md` — a condensed (English) summary of `draft.md`. Keep it consistent with `draft.md`; it is a digest, not an independent spec.
- `docs/design-notes.md` — the **rationale** ("why") behind each decision. Consult before changing any design choice; a change that contradicts a stated principle here needs justification.
- `docs/history.md` — chronology of decisions and rejected alternatives (e.g. exceptions, GC, visible lifetimes were all rejected). Use to avoid re-proposing already-discarded ideas.
- `docs/non-goals.md` — explicit out-of-scope items. Check before adding any feature; many "obvious" additions (OOP, async-everywhere, trait/template complexity, GC, framework-in-core) are deliberately excluded.
- `docs/open-questions.md` — design-decision tracker, split into **Settled** / **Open** / **Future**. Read the Settled section before proposing anything — those decisions are locked (see "Settled decisions" below). Genuinely open items are tied to milestones; new design discussion belongs here until resolved.
- `docs/impl/` — **implementation plan** for an actual compiler (does not exist yet). `00-overview.md` (strategy: Rust + LLVM-via-MIR + walking-skeleton-then-widen) → `01-pipeline.md` (stages/IR boundaries) → `02..06` (per-stage design) → `07-roadmap.md` (milestones M0–M8). These describe how `draft.md` will be built; they must stay consistent with the spec, not redefine it.

The two language layers — `draft.md` (detailed) and `docs/language-spec.md` (summary) — can drift. When editing one, check the other.

## Core design invariants (do not violate when extending the spec)

These are the load-bearing principles. Any proposed syntax or feature must respect them or it contradicts the project's identity:

- **Four-way alignment**: every design serves Human + AI + Compiler + Hardware simultaneously, not just human ergonomics.
- **One way to do things** — prefer convergence over expressiveness. One error model (`Result<T,E>` + `?`), one optional model (`Option<T>`, no null), one ownership model (value / arena / explicit heap), one parallel model (`map`/`reduce`/`chunks`/`task_group`).
- **Nothing hidden**: allocation, errors, side effects, parallelism, and `unsafe` must always be visible in source. No hidden copies, exceptions, or thread creation.
- **Compiler-friendly by restriction**: restrictions exist to let the compiler infer contiguous memory, no-alias, non-null, arena lifetime, cold error paths — *without* Rust-style visible lifetimes.
- **Data-oriented core**: array/slice processing is the center of the language. SIMD/cache/GPU friendliness comes from normal `map`/`reduce`/`scan`/`filter`/`mask` code lowering well, not from hand-written intrinsics.
- **AI-friendliness is a constraint, not a feature**: avoid macros, complex generics, multiple paradigms, lifetime annotations.

## Settled decisions (do not re-litigate)

These are locked. Full rationale + record locations in `docs/open-questions.md` Settled.

- **Compiler implemented in Rust.** Backend = **LLVM, but always lower through a backend-agnostic MIR** (never "C-backend-first"). Semantics live in MIR; `MIR → LLVM` is pure lowering.
- **Syntax = Go style.** Newline terminates a statement; `;` is an optional separator only for cramming multiple statements on one line. Braces `{}` delimit blocks → indentation is insignificant (NOT Python). A line starting with `.`/binary-operator continues the previous line (multi-line chains).
- **Expression-oriented.** `if` / `match` / `else`-unwrap / `arena` / blocks are expressions; a block's trailing expression is its value. Single-expression function bodies use `fn f() -> T = expr`.
- **Type declarations are keyword-less.** `Name { field: Type, ... }` = struct, `Name { Variant, Variant(payload) }` = sum type — disambiguated by content. Fields/variants are `,`-separated.
- **Integer overflow = defined two's-complement wrap** (no UB, zero-cost, doesn't block SIMD). Explicit `checked_*`/`saturating_*`/`wrapping_*` ops. Div-by-zero etc. is a hard error, never silent.
- **Ownership = a property of the type** (array/string/buffer/heap are Move; primitives/small-structs/slice are Copy). No `owned` keyword. Lifetimes are inferred (regions), never written.
- **Purity is inferred** (effect inference); `par_map`-style closures must be Pure.
- **Formatter normalizes only meaningless variation** (spacing, `;` placement, trailing comma, alignment); it does NOT force one-line ↔ multi-line. "One way" = one correct *formatting* per layout, not one allowed layout.

## Current status & next step (handoff)

- **Phase: M0 + M1 COMPLETE.** The Rust workspace under `crates/` (all 8 crates per `docs/impl/00-overview.md`: `align_span` `align_diag` `align_ast` `align_lexer` `align_parser` `align_sema` `align_mir` `align_codegen_llvm` `align_runtime` `align_driver`) flows end-to-end: `lexer → parser → sema → MIR → LLVM → executable`. `cargo build` / `cargo test` are green.
- **What works today:** `alignc run examples/min.align` compiles `fn main() -> i32 { x := 1; return x }` to a native executable and returns exit code 1. Subcommands: `check` / `emit-mir` / `emit-llvm` / `build` / `run`. Integer literals infer width from context (`x := 1; return x` in an `-> i32` fn → `x: i32`); unconstrained ints default to `i64`. Arithmetic `+ - * / %` with correct precedence. An integration test compiles+runs and asserts the exit code (`crates/align_driver/tests/m0.rs`).
- **Toolchain:** Rust 1.96, LLVM 19 via `inkwell` (`llvm19-1`). The Debian llvm-19 is shared-only (no `libPolly.a`), so `llvm-sys` is forced to dynamic linking via the `prefer-dynamic` feature + `.cargo/config.toml` (`LLVM_SYS_191_PREFER_DYNAMIC=1`). For M0 the generated `main` is the C entry (crt0 calls it); `align_runtime` is a stub, wired for real at M2 (Result-returning `main` via `align_rt_start`).
- **Where things are:** spec = `draft.md` (authoritative). Design rationale = `docs/*.md`. Implementation plan = `docs/impl/00–07`. Decisions = `docs/open-questions.md` Settled (and "Settled decisions" above).
- **Phase: M1 COMPLETE** (`docs/impl/07-roadmap.md`). Delivered across all stages: `if`/comparison/`bool`, `mut` + reassignment, multi-arg fns + calls; **builtin `print`** (integers only, decimal + newline, via `align_rt_print_i64`); **structs** (keyword-less `Name { field: Type }` decl, `Name { field: value }` literal, `base.field` read/write); and **the full primitive set** — `i8..u64`, `f32`/`f64`, `char`.
  - **Runtime/linking:** `align_runtime` builds as a staticlib (`crate-type = ["lib","staticlib"]` → `target/<profile>/libalign_runtime.a`); the driver links it (+ `-lpthread -ldl -lm`) into every executable. `print` is a sema builtin (not a user fn); codegen widens its integer arg to i64.
  - **Structs (M1 cut):** primitive fields only; a struct lives in its slot (construct via `:=` → field stores; field read = GEP+load; `mut base.field = v`). Passing/returning/copying a whole struct value is rejected and waits for the move model (M3). Field access `p.x` is a 2-segment `Path` resolved in sema; `Ty::Struct(id)` indexes `Program.structs`.
  - **Floats/char:** float literals (incl. `1e3` exponents) infer `f32`/`f64` like ints (default `f64`); `fadd`/`fsub`/`fmul`/`fdiv`/`frem`/`fcmp`, unary `fneg`. **No implicit int↔float coercion** (mixing is a type error). `char` = 32-bit Unicode scalar with escape literals (`'\n'` etc.), equality/ordering only — arithmetic on `char` is rejected. `print` stays integer-only until `std.io` (M5). codegen values are now `BasicValueEnum` (int+float), not `IntValue`.
  - **Parser note:** type-annotated `let` without `mut` (`x: T := v`) is now recognized (dispatch widened to `ident :`).
- **Next action: M2** (`docs/impl/07-roadmap.md`) — `Option<T>`/`else`-unwrap, `Result<T,E>` + `?` (desugared in MIR to early-return + cold path), `pub fn main() -> Result<(), Error>` via `align_rt_start`, and a single `Error` type (finalize the error-type design in `open-questions.md` during M2).
- **Note (perf, not blocking):** linking the Rust std staticlib makes the output ~5 MB and pulls `libgcc_s` (unwinding). Acceptable for now; the small-binary / fast-startup lever (own itoa + direct `write`, drop std, `panic=abort`) is recorded in `docs/open-questions.md` Future and revisited at M5 (`std.io`).
- **No design item is blocking.** Open items wait on their milestone (error type → M2, explicit-allocator arena → M3, generics → M4, etc.).
- **Note on continuity:** prior decisions also lived in this machine's Claude memory (`~/.claude/.../memory/`), which does NOT transfer between machines — but everything durable is already captured in `draft.md` + `docs/open-questions.md`, so this repo is self-sufficient.

## Conventions when editing

- **Language: English only.** Everything in this project is written in English — code comments, identifiers, CLI output, diagnostic messages, commit messages, and all documentation (`draft.md`, `docs/`, `docs/impl/`). Align is a personal project intended to become globally adopted, so do not introduce Japanese. (The repo was originally written with Japanese docs; it was converted to English on 2026-06-17.)
- Match the existing house style: terse declarative sentences, fenced code blocks tagged `align` for language examples and `text` for bullet-like lists of concepts.
- Library is layered `core` (language-intrinsic primitives) → `std` (OS boundary) → `pkg` (frameworks/ecosystem, kept out of core/std). Place any new library surface in the correct layer; `draft.md` §18 defines the boundaries.
- When changing a design decision, update *all* of: `draft.md`, the `docs/language-spec.md` digest, the rationale in `docs/design-notes.md`, the relevant `docs/impl/*.md`, and the **Settled** section of `docs/open-questions.md` (move items out of **Open** as they settle).
