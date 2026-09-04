# The Align way

> 🌐 **English** · [Japanese](./ja/17-the-align-way.md)

Here are the idioms from the previous chapters, collected as a checklist for writing and reviewing Align code.

## Describe bulk transformations with pipelines

Use `map`/`where`/`reduce` for bulk transformations. The compiler can fuse pipeline stages and vectorize the resulting pass. Use `loop` with a `break value` for sequential control such as reading to EOF, retrying, or convergence; use `group_by` for bulk keyed aggregation. Recursion suits recursive problems such as parsers and trees. If a loop advances an index, check whether a pipeline can express the operation directly.

```align
total := xs.map(f).where(p).sum()
```

## Propagate errors with `?`

Failable functions return `Result`; call them with `?`; convert error types visibly with `map_err`; `match` at the point of final consumption when the reason matters. If the reason truly does not matter, `result else fallback` discards the error visibly and supplies a fallback. Let `main() -> Result<(), Error>` do the exit-code plumbing. Absence is still `Option`, a different thing from failure, and the signature says which one you mean.

## Choose a lifetime for each phase

- Local value → nothing to do.
- A phase's worth of allocations → `arena {}`, and `.clone()` the survivors out.
- Text assembly → `builder`, never `+` in a loop.
- Someone else's data → borrow a view (`str`, `slice`) without copying the data.

Choose how long the data must live. The compiler then checks moves and region escapes and inserts the required cleanup.

## Lay bulk data out as SoA

If a hot path touches one or two fields of many rows, transpose: `to_soa()` at the point data enters, or decode JSON straight into `soa<T>`. Repeated aggregation over a string key → `dict_encode` once. AoS is for data you touch whole and rarely.

## Choose between data parallelism and tasks

Use `par_map` when the per-element function is expensive; measure first, because a vectorized sequential pipeline may be faster. Use `task_group`/`spawn` with `wait()?` for independent jobs with different operations. Parallel closures must be Pure. Purity is inferred, so a closure that changes state or performs I/O is rejected.

## Let the compiler see the shape

Align's restrictions give the compiler information it can use for optimization: contiguous memory, no aliasing, non-null values, cold error paths, and arena lifetimes. Pipelines, `Result`, and explicit allocation preserve that information without extra annotations. These idioms help the compiler optimize the code; measure the result on your workload.

## Trust, but verify with the tools

Use `alignc check` while editing and `alignc fmt --write` before committing. Investigate warnings about large copies, unnecessary heap allocation, and lossy casts. To check whether a pipeline vectorized, inspect its optimization remarks with `alignc explain-opt` or its IR with `alignc emit-llvm`. Use a benchmark to measure execution time.

## Nothing hidden — read code by its keywords

Look for the words that mark allocation (`arena`, `heap.new`, `builder`, `.clone()`, `.to_array()`), failure (`Result`, `?`), mutation (`mut`, `out`, `borrow mut`), threads (`par_map`, `spawn`), native resources (`resource`), and unchecked operations (`unsafe`, `raw`, `extern`). They help a reader find the operations that deserve closer inspection. Keep these choices visible in your own APIs so the next reader can understand their costs and requirements.
