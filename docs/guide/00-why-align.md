# Why Align

> 🌐 **English** · [Japanese](./ja/00-why-align.md)

Align is an AOT-compiled, data-oriented programming language. Before the syntax, the mindset — because Align asks you to write differently than a scalar, object-oriented language does.

> **Status.** Align is pre-release. The compiler (`alignc`) builds real native executables, and everything in this book compiles and runs today unless a section is explicitly marked **implementation in progress** — those parts are designed (the design lives in the spec) but not built yet, and the book says so wherever it applies. There is no stability promise: pre-release, the language changes outright, with no deprecation cycles.

## Four-way alignment

Most languages optimize for one reader: the human. Align optimizes for four at once — the **Human**, the **AI** that reads and writes code, the **Compiler** that must infer memory and parallelism, and the **Hardware** that runs it. A design only ships when it serves all four. That is why Align has no macros, no visible lifetimes, no inheritance hierarchies: each would help one reader at the expense of another.

## Nothing hidden

Allocation, errors, side effects, parallelism, and `unsafe` are always visible in the source. There are no hidden copies, no exceptions thrown from nowhere, no threads spawned behind your back. If a line allocates, you can see it. If it can fail, it returns `Result`. This is not ceremony — it is what lets both a human and a compiler reason about the code locally.

## One way to do things

Align prefers convergence over expressiveness. One error model (`Result<T, E>` + `?`). One optional model (`Option<T>`, no null). One ownership model (value / arena / explicit heap). One parallel model (`map`/`reduce`/`chunks`/`task_group`). When there is one obvious way, the human doesn't choose, the AI doesn't guess, and the reader doesn't decode someone else's cleverness.

## Data-oriented at the core

The center of Align is not the object — it is the array. Real programs spend their time walking over collections of data: transforming, filtering, summing. Align makes that the natural thing to write, and lowers it to tight, cache-friendly, SIMD-friendly machine code. You write `prices.map(with_tax).where(in_stock).sum()`; the compiler fuses it into one loop with no intermediate arrays. The speed comes from ordinary code lowering well, not from hand-written intrinsics.

## What this means when you write Align

- You will write very few explicit loops. Align has **no `for` and no `while`**: you describe transformations as pipelines, and the one `loop` expression is reserved for genuinely sequential control — read until EOF, retry until success.
- You will not manage memory by hand, but you will decide *where* data lives (a value, an arena, the heap).
- You will handle errors as values, with `?`, not with try/catch.
- You will lay data out as structure-of-arrays when it's processed in bulk, and the compiler rewards you for it.

## How to read this book

Chapters 01–05 get you writing programs: the toolchain, the expression-oriented core, data modeling, errors, and memory. Chapters 06–12 are the heart of the language: pipelines, strings, JSON, generics and modules, closures and parallelism, data-oriented layout, and explicit SIMD. Chapters 13–16 cover the standard library and the edges (unsafe, FFI, tooling). Chapter 17 closes with the idioms that make Align code fast and idiomatic.

Prefer learning by drilling? [The Little Aligner](../little-aligner/README.md) teaches the same material as a question-and-answer workbook, in the style of *The Little Schemer*.

Every code block in this book is real: it compiles with today's `alignc` unless marked otherwise.
