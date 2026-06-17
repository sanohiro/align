# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repository is

This is **not a codebase**. It contains no source code, build system, tests, or git history. It is the **design specification for "Align"**, an AOT-compiled, data-oriented programming language that does not yet have an implementation. All work here is reading, editing, and extending Markdown documents.

There are no build, lint, or test commands. "Correctness" means internal consistency of the design across documents — not compilation.

## Document layout and roles

- `draft.md` — the **authoritative, most complete** spec (Language Specification Draft v0.1, sections 1–21). When the design needs detail, this is the source of truth. Written largely in Japanese prose with `align` code blocks.
- `docs/language-spec.md` — a condensed (Japanese) summary of `draft.md`. Keep it consistent with `draft.md`; it is a digest, not an independent spec.
- `docs/design-notes.md` — the **rationale** ("why") behind each decision. Consult before changing any design choice; a change that contradicts a stated principle here needs justification.
- `docs/history.md` — chronology of decisions and rejected alternatives (e.g. exceptions, GC, visible lifetimes were all rejected). Use to avoid re-proposing already-discarded ideas.
- `docs/non-goals.md` — explicit out-of-scope items. Check before adding any feature; many "obvious" additions (OOP, async-everywhere, trait/template complexity, GC, framework-in-core) are deliberately excluded.
- `docs/open-questions.md` — design-decision tracker, split into **決着済み (settled)** / **未解決 (open)** / **将来 (future)**. Read the settled section before proposing anything — those decisions are locked (see "Settled decisions" below). Genuinely open items are tied to milestones; new design discussion belongs here until resolved.
- All `docs/` files (incl. `docs/impl/`) are Japanese documents (Japanese headings + prose, English kept for code/technical identifiers). `draft.md` uses English headings + Japanese prose.
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

These are locked. Full rationale + record locations in `docs/open-questions.md` 決着済み.

- **Compiler implemented in Rust.** Backend = **LLVM, but always lower through a backend-agnostic MIR** (never "C-backend-first"). Semantics live in MIR; `MIR → LLVM` is pure lowering.
- **Syntax = Go style.** Newline terminates a statement; `;` is an optional separator only for cramming multiple statements on one line. Braces `{}` delimit blocks → indentation is insignificant (NOT Python). A line starting with `.`/binary-operator continues the previous line (multi-line chains).
- **Expression-oriented.** `if` / `match` / `else`-unwrap / `arena` / blocks are expressions; a block's trailing expression is its value. Single-expression function bodies use `fn f() -> T = expr`.
- **Type declarations are keyword-less.** `Name { field: Type, ... }` = struct, `Name { Variant, Variant(payload) }` = sum type — disambiguated by content. Fields/variants are `,`-separated.
- **Integer overflow = defined two's-complement wrap** (no UB, zero-cost, doesn't block SIMD). Explicit `checked_*`/`saturating_*`/`wrapping_*` ops. Div-by-zero etc. is a hard error, never silent.
- **Ownership = a property of the type** (array/string/buffer/heap are Move; primitives/small-structs/slice are Copy). No `owned` keyword. Lifetimes are inferred (regions), never written.
- **Purity is inferred** (effect inference); `par_map`-style closures must be Pure.
- **Formatter normalizes only meaningless variation** (spacing, `;` placement, trailing comma, alignment); it does NOT force one-line ↔ multi-line. "One way" = one correct *formatting* per layout, not one allowed layout.

## Current status & next step (handoff)

- **Phase: design complete → implementation (M0) not yet started.** This repo is still docs-only; no code, no build/test yet.
- **Where things are:** spec = `draft.md` (authoritative). Design rationale = `docs/*.md`. Implementation plan = `docs/impl/00–07`. Decisions = `docs/open-questions.md` 決着済み (and "Settled decisions" above).
- **Next action: M0 walking skeleton** (`docs/impl/07-roadmap.md`). Stand up the Rust workspace (`align_span` → `align_lexer` → `align_parser` → `align_sema` → `align_mir` → `align_codegen_llvm` → `align_runtime` → `align_driver`, per `docs/impl/00-overview.md`) and make a trivial program (`fn main() -> i32 { x := 1; return x }` / `x := 1`) flow end-to-end: lexer → parser → sema → MIR → LLVM → executable. Widen feature-by-feature only after the skeleton runs.
- **No design item is blocking.** Open items wait on their milestone (error type → M2, explicit-allocator arena → M3, generics → M4, etc.).
- **Note on continuity:** prior decisions also lived in this machine's Claude memory (`~/.claude/.../memory/`), which does NOT transfer between machines — but everything durable is already captured in `draft.md` + `docs/open-questions.md`, so this repo is self-sufficient.

## Conventions when editing

- Match the existing house style: terse declarative sentences, Japanese prose in `draft.md`, fenced code blocks tagged `align` for language examples and `text` for bullet-like lists of concepts.
- Library is layered `core` (language-intrinsic primitives) → `std` (OS boundary) → `pkg` (frameworks/ecosystem, kept out of core/std). Place any new library surface in the correct layer; `draft.md` §18 defines the boundaries.
- When changing a design decision, update *all* of: `draft.md`, the `docs/language-spec.md` digest, the rationale in `docs/design-notes.md`, the relevant `docs/impl/*.md`, and the **決着済み** section of `docs/open-questions.md` (move items out of 未解決 as they settle).
