> 🌐 **English** · [Japanese](./ja/README.md)

A hands-on introduction to writing Align — not the spec (that's draft.md), but how to think and write in Align. Chapters are ordered; start at 00. Every example compiles with today's `alignc` unless marked *implementation in progress*.

Prefer learning by doing? **[The Little Aligner](../little-aligner/README.md)** covers the same ground as question-and-answer drills, in the style of *The Little Schemer*.

## Part I — Foundations

- [00 — Why Align](00-why-align.md)
- [01 — Getting started](01-getting-started.md)
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
- [14 — std: encoding, rand, cli](14-std-encoding-rand-cli.md)
- [15 — The edges: unsafe and C FFI](15-unsafe-and-ffi.md)
- [16 — The toolchain: alignc, the formatter, the lints](16-toolchain.md)
- [17 — The Align way](17-the-align-way.md)
- [18 — std services: network, HTTP, processes, compression, crypto](18-std-services.md)

## Part IV — Designing Systems without Objects

- [19 — Unlearning objects](19-unlearning-objects.md)
- [20 — Beyond arenas: pools and lifetimes](20-beyond-arenas.md)
- [21 — State machines](21-state-machines.md)
- [22 — Building a system: ECS](22-building-a-system.md)
