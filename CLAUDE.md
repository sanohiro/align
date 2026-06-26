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

- **Phase: M0–M3 COMPLETE; M4 done through slices + reduce; M5 started (strings).** The Rust workspace under `crates/` (all 8 crates per `docs/impl/00-overview.md`: `align_span` `align_diag` `align_ast` `align_lexer` `align_parser` `align_sema` `align_mir` `align_codegen_llvm` `align_runtime` `align_driver`) flows end-to-end: `lexer → parser → sema → MIR → LLVM → executable`. `cargo build` / `cargo test` are green.
- **What works today:** `alignc run examples/min.align` compiles `fn main() -> i32 { x := 1; return x }` to a native executable and returns exit code 1. Subcommands: `check` / `emit-mir` / `emit-llvm` / `build` / `run`. Integer literals infer width from context (`x := 1; return x` in an `-> i32` fn → `x: i32`); unconstrained ints default to `i64`. Arithmetic `+ - * / %` with correct precedence. An integration test compiles+runs and asserts the exit code (`crates/align_driver/tests/m0.rs`).
- **Toolchain:** Rust 1.96, LLVM 19 via `inkwell` (`llvm19-1`). The Debian llvm-19 is shared-only (no `libPolly.a`), so `llvm-sys` is forced to dynamic linking via the `prefer-dynamic` feature + `.cargo/config.toml` (`LLVM_SYS_191_PREFER_DYNAMIC=1`). For M0 the generated `main` is the C entry (crt0 calls it); `align_runtime` is a stub, wired for real at M2 (Result-returning `main` via `align_rt_start`).
- **Where things are:** spec = `draft.md` (authoritative). Design rationale = `docs/*.md`. Implementation plan = `docs/impl/00–07`. Decisions = `docs/open-questions.md` Settled (and "Settled decisions" above).
- **Phase: M1 COMPLETE** (`docs/impl/07-roadmap.md`). Delivered across all stages: `if`/comparison/`bool`, `mut` + reassignment, multi-arg fns + calls; **builtin `print`** (integers only, decimal + newline, via `align_rt_print_i64`); **structs** (keyword-less `Name { field: Type }` decl, `Name { field: value }` literal, `base.field` read/write); and **the full primitive set** — `i8..u64`, `f32`/`f64`, `char`.
  - **Runtime/linking:** `align_runtime` builds as a staticlib (`crate-type = ["lib","staticlib"]` → `target/<profile>/libalign_runtime.a`); the driver links it (+ `-lpthread -ldl -lm`) into every executable. `print` is a sema builtin (not a user fn); codegen widens its integer arg to i64.
  - **Structs (M1 cut):** primitive fields only; a struct lives in its slot (construct via `:=` → field stores; field read = GEP+load; `mut base.field = v`). Passing/returning/copying a whole struct value is rejected and waits for the move model (M3). Field access `p.x` is a 2-segment `Path` resolved in sema; `Ty::Struct(id)` indexes `Program.structs`.
  - **Floats/char:** float literals (incl. `1e3` exponents) infer `f32`/`f64` like ints (default `f64`); `fadd`/`fsub`/`fmul`/`fdiv`/`frem`/`fcmp`, unary `fneg`. **No implicit int↔float coercion** (mixing is a type error). `char` = 32-bit Unicode scalar with escape literals (`'\n'` etc.), equality/ordering only — arithmetic on `char` is rejected. `print` stays integer-only until `std.io` (M5). codegen values are now `BasicValueEnum` (int+float), not `IntValue`.
  - **Parser note:** type-annotated `let` without `mut` (`x: T := v`) is now recognized (dispatch widened to `ident :`).
- **M2 COMPLETE (vertical slice)** (`docs/impl/07-roadmap.md`). Delivered: `Option<T>` (`Some`/`None`/`else`-unwrap), `Result<T,E>` + `?` (MIR cold `Err` edge), and `pub fn main() -> Result<(), Error>`.
  - **Type repr (keeps `Ty: Copy`):** a `Scalar` enum + `Ty::Option(Scalar)` / `Ty::Result(Scalar, Scalar)` / `Ty::ErrCode`. **Payloads are scalar-only** (M2 cut); a constructor (`Some`/`Ok`/`Err`) resolves its payload to a concrete scalar there (literals default), so no inference var lives inside a composite. `Some`/`None`/`Ok`/`Err`/`error` are **sema builtins** (not user fns), like `print`. `Ty::Error` is the type-check sentinel; the language `Error` type is `Ty::ErrCode` (an i32 code — placeholder; the real Error sum-type design stays Open).
  - **Aggregates:** `Option<T>` = `{i8 tag, T}`, `Result<T,E>` = `{i8 tag, T ok, E err}` (plain structs, insert/extract value; codegen values are `BasicValueEnum`). `?` lowers to a tag branch: `Err` early-returns an `Err` of the function's own return type (cold edge); `Ok` continues unwrapped.
  - **main ABI:** `fn main() -> i32` stays the C entry; `fn main() -> Result<(), Error>` is lowered as `align_main` and codegen emits a C `main` wrapper → on `Err(code)` calls runtime `align_rt_report_error(i32)` and returns the code, else 0. New unit literal `()`; new `?` token; generic type syntax `Name<...>`; type-annotated-let-without-`mut` now parses.
- **M3 COMPLETE (vertical slice)** (`docs/impl/07-roadmap.md`). Delivered: `arena {}` regions with a real runtime bump allocator (chunked, pointer-stable; `align_rt_arena_begin/alloc/reset/end`) and bulk free; the **heap box** `box<T>` (`heap.new(x)`, `.get()` copies the scalar out, `.clone()` deep-copies) as the anchor Move type; **move/use-after-move** checking and **arena escape** checking (HIR flow analyses `MoveCheck`/`EscapeCheck` in `align_sema`).
  - **Box/arena cut:** `box<T>` payload is a scalar; `heap.new`/`clone` REQUIRE an arena (free-standing heap with per-binding drop insertion is deferred). `Ty::Box(Scalar)` + `Ty::ArenaHandle` (codegen: opaque pointers). Method syntax `recv.method(args)` works only for the builtins `heap.new`/`box.get`/`box.clone` via 2-segment-path call dispatch.
  - **Cleanup:** arena lowering pushes the handle on a `Builder.arenas` stack; the block end frees it, and every exit out of the arena (`return`, `?`) frees all open arenas first (`emit_arena_cleanup`). `return` no longer makes a dead block — `lower_block` stops on divergence.
  - **Escape:** a box's region = arena nesting depth where allocated; escaping to a shallower depth (return, assign-to-outer, arena block value) is a compile error. Anonymous `arena {}` only (named `arena a {}` deferred; recorded in open-questions).
  - **Parser fix:** a block's trailing expression before `}` is its value even on its own line (the settled expression-oriented rule); a diverging block (`{ … return }`) takes the expected type, so it fits any value position.
  - **Deferred:** whole-struct move/copy (pass/return/copy) + the Copy/Move size threshold — structs stay slot-only until M4 (arrays/large aggregates).
- **M4 core slice COMPLETE** (`docs/impl/07-roadmap.md`). Delivered: fixed-length scalar arrays from literals `[...]` (`Ty::Array(Scalar, N)`), and the fused pipeline `[...].map(f).where(p).sum()` lowering to **one counted loop** in MIR (map = inline call, where = branch skipping to the increment, sum = accumulator — no intermediate arrays). `map`/`where` take named functions; `sum` is the reduction terminal (pipelines must end in a reduction). General generics deferred — these are compiler-known builtins, monomorphic per element type.
  - **Slice 0 (foundation):** `.` is now a postfix operator (`ExprKind::FieldAccess`); method chains `a.f(x).g()` parse as nested FieldAccess+Call. Struct field access / box methods / `heap.new` migrated off the old 2-segment-path hack.
  - **Element typing:** an inline literal source takes its element type from the first stage's parameter (or, with no stages, the `sum` result type); `.sum()` honors the expected type so mismatches are caught.
  - **Struct arrays (done):** `Ty::StructArray(id, n)` (AoS; struct literals allowed as array elements). Pipeline stages now include `Project` (`.field`, struct→scalar) and `WhereField` (`where(.active)`, keep on a bool field) — the draft.md §8 shape `[...].where(.active).pay.sum()` runs as one fused loop. New `ExprKind::FieldShorthand` for the `.field` predicate argument; `.field` postfix in a sum-chain is a projection. MIR `StoreElemField`/`IndexField` GEP `[0, i, field]` into `[N x %Struct]`.
  - **Deferred (later M4 / M5):** dynamic-length `array`/`slice` + array type annotations, array-valued results (materialization), more stages/terminals (`reduce`/`scan`/`filter`/`partition`/`sort`/`chunks`), `out` args, and named-function `map` over struct elements (needs struct-by-value params, deferred since M1). Arrays aren't Move-checked yet.
- **Next action: continue M4 or start M5.** Remaining M4 (above) — most usefully dynamic arrays + `reduce`/field-projection — or **M5** (`docs/impl/07-roadmap.md`): strings/`string`/builder, `core.json`, `core.template`. M5 is where `print` gains non-integer output and the `std.fs`/zero-copy-I/O work (recorded in `open-questions.md` Future) becomes relevant.
- **Note (perf, not blocking):** binary size / startup is already good — a built example is ~16 KB (14 KB stripped), dynamically linked to `libc` + `ld` only (no `libgcc_s` in `ldd`). (The earlier "~5 MB + libgcc_s" note was stale.) The remaining small-binary / fast-startup lever (own itoa + direct `write`, drop std, `panic=abort` + strip `.eh_frame`) is recorded in `docs/open-questions.md` Future as marginal polish.
- **No design item is blocking.** Open items wait on their milestone (error type → M2, explicit-allocator arena → M3, generics → M4, etc.).
- **Note on continuity:** prior decisions also lived in this machine's Claude memory (`~/.claude/.../memory/`), which does NOT transfer between machines — but everything durable is already captured in `draft.md` + `docs/open-questions.md`, so this repo is self-sufficient.

## Conventions when editing

- **Language: English only.** Everything in this project is written in English — code comments, identifiers, CLI output, diagnostic messages, commit messages, and all documentation (`draft.md`, `docs/`, `docs/impl/`). Align is a personal project intended to become globally adopted, so do not introduce Japanese. (The repo was originally written with Japanese docs; it was converted to English on 2026-06-17.)
- Match the existing house style: terse declarative sentences, fenced code blocks tagged `align` for language examples and `text` for bullet-like lists of concepts.
- Library is layered `core` (language-intrinsic primitives) → `std` (OS boundary) → `pkg` (frameworks/ecosystem, kept out of core/std). Place any new library surface in the correct layer; `draft.md` §18 defines the boundaries.
- When changing a design decision, update *all* of: `draft.md`, the `docs/language-spec.md` digest, the rationale in `docs/design-notes.md`, the relevant `docs/impl/*.md`, and the **Settled** section of `docs/open-questions.md` (move items out of **Open** as they settle).
- **No backward compatibility (pre-release).** Align has not shipped, so nothing depends on old behavior. When a design or API changes, change it **outright** — do not keep deprecated aliases, dual code paths, old syntax accepted alongside new, compat shims, or "legacy" fallbacks. Update every call site / test / doc to the new form instead. Backward-compat layers are pure bloat here; the only correct version is the current one. (Example: when `reduce`/`scan` moved to init-first arguments, the old order was removed entirely, not aliased.)
- **Ideal form, or defer — no compromise implementations.** This is the language-design stage, so ship only what is the **ideal form** ("あるべき姿"): beautiful, internally unified/consistent, and aligned with the core design invariants (Nothing hidden / One way / four-way alignment / data-oriented). If a feature can't be done that way — it would need a magic special-case, a second mechanism for something that already has one, or it breaks a principle — do **not** ship a half-hearted version. Present the ideal design, and **defer implementation** until it can be done right. Timing is not a constraint; design quality is. (Example: `task_group`'s `spawn` takes a lambda with escape-driven first-class closures — the ideal form — deferred, rather than a magic bare-call special form that only dodged the closure work.)
