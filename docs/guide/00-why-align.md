# Why Align

Align is an AOT-compiled, data-oriented language. Before the syntax, the mindset — because Align asks you to write differently than a scalar, object-oriented language does.

## Four-way alignment

Most languages optimize for one reader: the human. Align optimizes for four at once — the **Human**, the **AI** that reads and writes code, the **Compiler** that must infer memory and parallelism, and the **Hardware** that runs it. A design only ships when it serves all four. That is why Align has no macros, no visible lifetimes, no inheritance hierarchies: each would help one reader at the expense of another.

## Nothing hidden

Allocation, errors, side effects, parallelism, and `unsafe` are always visible in the source. There are no hidden copies, no exceptions thrown from nowhere, no threads spawned behind your back. If a line allocates, you can see it. If it can fail, it returns `Result`. This is not ceremony — it is what lets both a human and a compiler reason about the code locally.

## One way to do things

Align prefers convergence over expressiveness. One error model (`Result<T, E>` + `?`). One optional model (`Option<T>`, no null). One ownership model (value / arena / explicit heap). One parallel model (`map`/`reduce`/`chunks`/`task_group`). When there is one obvious way, the human doesn't choose, the AI doesn't guess, and the reader doesn't decode someone else's cleverness.

## Data-oriented at the core

The center of Align is not the object — it is the array. Real programs spend their time walking over collections of data: transforming, filtering, summing. Align makes that the natural thing to write, and lowers it to tight, cache-friendly, SIMD-friendly machine code. You write `prices.map(withTax).where(inStock).sum()`; the compiler fuses it into one loop with no intermediate arrays. The speed comes from ordinary code lowering well, not from hand-written intrinsics.

## What this means when you write Align

- You will write very few explicit loops. You describe the transformation, not the iteration.
- You will not manage memory by hand, but you will decide *where* data lives (a value, an arena, the heap).
- You will handle errors as values, with `?`, not with try/catch.
- You will lay data out as structure-of-arrays when it's processed in bulk, and the compiler rewards you for it.

The rest of this guide teaches those four habits in order.
