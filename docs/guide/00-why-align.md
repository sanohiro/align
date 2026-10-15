# Why Align

> 🌐 **English** · [Japanese](./ja/00-why-align.md)

Align is an AOT-compiled, data-oriented programming language. Before introducing the syntax, this chapter explains its design priorities and how they shape the programs you write.

> **Status.** This guide follows the implementation in the current repository. A packaged release may predate a feature described here; chapter [01](01-getting-started.md) explains how to build from source. Examples include complete programs, fragments to place in a function, and explicitly marked compile errors. Align is still in 0.x and does not promise backward compatibility.

## Four-way alignment

Align is designed for four participants: the **Human** and **AI** that read and write code, the **Compiler** that analyzes memory use and parallelism, and the **Hardware** that runs it. Language features must serve all four. This guides decisions such as omitting macros, visible lifetime annotations, and inheritance hierarchies.

## Nothing hidden

Allocation and copies use operations such as `builder`, `.to_array()` and `.clone()`. Parallel work uses `par_map` or `spawn`, and native pointer operations require `unsafe`. Recoverable errors use `Result`; bounds violations and invalid integer division terminate the program instead of returning `Err` (chapter [04](04-errors.md)). These conventions help readers locate operations with costs or failure paths.

## One way to do things

Align uses `Result<T, E>` and `?` for errors, `Option<T>` for absence, and one ownership model for values, arenas and explicit heap allocation. Parallel work uses `par_map` and structured `task_group` scopes; ordinary `map`/`reduce`/`chunks` are sequential. Keeping these conventions consistent limits the number of mechanisms a reader must learn.

## Data-oriented at the core

Arrays and slices are central to Align. Pipelines express transformations, filtering and reductions; for example, `prices.map(with_tax).where(in_stock).sum()` fuses into a loop without intermediate arrays. Columnar storage can improve locality for field-wise scans. Whether a particular program benefits depends on its data, operations and target hardware.

## What this means when you write Align

- Use pipelines for collection transformations and `loop` for sequential control, such as reading to EOF or retrying. Align has no `for` or `while`.
- You will not manage memory by hand, but you will decide *where* data lives (a value, an arena, the heap).
- You will handle errors as values, with `?`, not with try/catch.
- Consider structure-of-arrays for repeated field-wise scans, and measure it against your existing layout.

## How to read this book

Chapters 01–05 cover the foundations: the toolchain, expressions, data modeling, errors, and memory. Chapters 06–12 cover pipelines and data processing. Chapters 13–18 cover the standard library, unsafe, FFI, tooling, and idioms. Chapters 19–22 apply these ideas to larger systems, and chapters 23–25 introduce packages, databases, and vector search.

Chapter [26](26-performance.md) gives a measurement-driven optimization workflow; chapter [27](27-multimodal-reference.md) combines the shipped libraries in a bounded application.

[The Little Aligner](../little-aligner/README.md) uses short questions and answers in the style of *The Little Schemer*, focusing on pipelines, data layout, and ownership. This guide adds practical coverage of tools, libraries, and packages. You can use the workbook to practice the language concepts and return here when building a program.
