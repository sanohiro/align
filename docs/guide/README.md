# The Align Guide

> 🌐 **English** · [Japanese](./ja/README.md)

A hands-on introduction to writing Align. Start at [00](00-why-align.md) and read the foundations in order; later chapters introduce tools and libraries as you need them. For the full language specification, see [draft.md](../../draft.md).

**[The Little Aligner](../little-aligner/README.md)** practices pipelines, data layout, and ownership through short questions and answers, in the tradition of *The Little Schemer*. You can start with either book and consult the other when you want a different explanation or more practice.

## Reading and running the examples

Some examples are complete programs; others are fragments that use declarations introduced nearby. Type and function declarations belong at file scope, and executable statements go inside `main`. Explicitly marked errors show what the compiler rejects; they are exercises in reading diagnostics. Features still awaiting implementation are marked separately.

Use `alignc check file.align` to check a program and `alignc run file.align` to execute it. Test files use `alignc test file.align`; see [chapter 16](16-toolchain.md). For short expressions, use `align-repl`. Start independent examples with `:clear`: each entry reruns the accumulated program, including earlier side effects. Installation and the first session are covered in [chapter 01](01-getting-started.md).

## Part I — Foundations

- [00 — Why Align](00-why-align.md)
- [01 — Getting started](01-getting-started.md) — install, `align-repl`, the first program
- [02 — Language basics](02-language-basics.md)
- [03 — Modeling data: structs, sum types, match](03-modeling-data.md)
- [04 — Errors: Option, Result, and `?`](04-errors.md)
- [05 — Memory: value, arena, heap](05-memory.md)

## Part II — The heart of the language

- [06 — Pipelines: the data-processing core](06-pipelines.md)
- [07 — Strings and text](07-strings-and-text.md)
- [08 — JSON](08-json.md)
- [09 — Generics and modules](09-generics-and-modules.md)
- [10 — Closures and parallelism](10-closures-and-parallelism.md)
- [11 — Data-oriented design: SoA and grouped aggregation](11-data-oriented.md)
- [12 — Explicit SIMD: vecN, masks, alignment](12-simd.md)

## Part III — The standard library and the edges

- [13 — std: files, I/O, and the OS boundary](13-std-os.md)
- [14 — std: encoding, regex, rand, cli](14-std-encoding-rand-cli.md)
- [15 — The edges: unsafe and C FFI](15-unsafe-and-ffi.md)
- [16 — The toolchain: alignc, tests, align-repl, formatting, and lints](16-toolchain.md)
- [17 — The Align way](17-the-align-way.md)
- [18 — std services: network, HTTP, processes, compression, crypto](18-std-services.md)

## Part IV — Designing Systems without Objects

- [19 — Unlearning objects](19-unlearning-objects.md)
- [20 — Beyond arenas: pools and lifetimes](20-beyond-arenas.md)
- [21 — State machines](21-state-machines.md)
- [22 — Building a system: ECS](22-building-a-system.md)

## Part V — Packages

- [23 — Packages: vendored source and choosing a library](23-packages.md)
- [24 — Databases: pkg.db in practice](24-database.md)
- [25 — Vector search through pkg.db](25-vector-search.md)
