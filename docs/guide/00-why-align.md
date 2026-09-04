# Why Align

> 🌐 **English** · [Japanese](./ja/00-why-align.md)

Align is an AOT-compiled, data-oriented programming language. Before introducing the syntax, this chapter explains its design priorities and how they shape the programs you write.

> **Status.** This guide follows the implementation in the current repository. A packaged release may predate a feature described here; chapter [01](01-getting-started.md) explains how to build from source. Examples include complete programs, fragments to place in a function, and explicitly marked compile errors. Align is still in 0.x and does not promise backward compatibility.

## Four-way alignment

Align is designed for four participants: the **Human** and **AI** that read and write code, the **Compiler** that analyzes memory use and parallelism, and the **Hardware** that runs it. Language features must serve all four. This guides decisions such as omitting macros, visible lifetime annotations, and inheritance hierarchies.

## Nothing hidden

Allocation, errors, side effects, parallelism, and `unsafe` are always visible in the source. There are no hidden copies, no exceptions thrown from nowhere, no threads spawned behind your back. If a line allocates, you can see it. Recoverable errors use `Result`. Bounds violations and invalid integer division terminate the program instead of returning `Err` (chapter [04](04-errors.md)). This is not ceremony — it is what lets both a human and a compiler reason about the code locally.

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

Chapters 01–05 cover the foundations: the toolchain, expressions, data modeling, errors, and memory. Chapters 06–12 cover pipelines and data processing. Chapters 13–18 cover the standard library, unsafe, FFI, tooling, and idioms. Chapters 19–22 apply these ideas to larger systems, and chapters 23–25 introduce packages, databases, and vector search.

[The Little Aligner](../little-aligner/README.md) uses short questions and answers in the style of *The Little Schemer*, focusing on pipelines, data layout, and ownership. This guide adds practical coverage of tools, libraries, and packages. You can use the workbook to practice the language concepts and return here when building a program.
