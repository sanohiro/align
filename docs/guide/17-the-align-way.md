# The Align way

> 🌐 **English** · [Japanese](./ja/17-the-align-way.md)

The idioms, collected. Each of these was earned somewhere in the previous chapters; here they are as the checklist an experienced Align programmer runs on autopilot.

## Describe transformations, don't iterate

Reach for `map`/`where`/`reduce` before you even think the word "loop" — there is no `for` or `while` to reach for anyway. The pipeline fuses to one vectorized pass. Genuinely sequential control — pump to EOF, retry, converge — is the one `loop` expression with a `break value`; bulk keyed aggregation is `group_by`; recursion is for recursive problems (parsers, trees), never for iteration. If you're walking an index inside a `loop`, you've missed a pipeline.

```align
total := xs.map(f).where(p).sum()
```

## Handle errors with `?`, not branches

Failable functions return `Result`; call them with `?`; convert error types visibly with `map_err`; `match` at the point of final consumption when the reason matters. If the reason truly does not matter, `result else fallback` discards the error visibly and supplies a fallback. Let `main() -> Result<(), Error>` do the exit-code plumbing. Absence is still `Option`, a different thing from failure, and the signature says which one you mean.

## Choose the lifetime, then stop thinking about memory

- Local value → nothing to do.
- A phase's worth of allocations → `arena {}`, and `.clone()` the survivors out.
- Text assembly → `builder`, never `+` in a loop.
- Someone else's data → a view (`str`, `slice`), free.

One decision per phase of work. The compiler enforces the rest — moved values, escapes, drops — so once it compiles, the memory is right.

## Lay bulk data out as SoA

If a hot path touches one or two fields of many rows, transpose: `to_soa()` at the point data enters, or decode JSON straight into `soa<T>`. Repeated aggregation over a string key → `dict_encode` once. AoS is for data you touch whole and rarely.

## Parallelism is two words

`par_map` when the per-element function is expensive (measure — the sequential loop is vectorized and often wins); `task_group`/`spawn` with `wait()?` for heterogeneous jobs. Purity is inferred, so if it compiles, it's race-free; if it doesn't, the compiler just found your hidden side effect.

## Let the compiler see the shape

Align's speed comes from what the compiler can *prove*: contiguous memory, no aliasing, non-null, cold error paths, arena lifetimes. Every restriction in the language — no null, one error model, Move types, inferred regions, terminated pipelines — exists to keep those proofs alive without annotations. Working with the grain (pipelines over index games, `Result` over sentinels, arenas over scattered ownership) is what keeps the inference fed. **Idiomatic Align is fast Align — the two are the same thing by design.**

## Trust, but verify with the tools

`alignc check` in the edit loop. `alignc fmt --write` before committing. When the lints speak — huge copy, unnecessary heap, lossy cast — they are the performance model pointing at a line; the fix is usually an idiom from this list. And when you wonder whether a pipeline really vectorized: `alignc emit-llvm`. Never argue about performance you can dump.

## Nothing hidden — read code by its keywords

Everything costly or dangerous in Align announces itself with a greppable word: allocation (`arena`, `heap.new`, `builder`, `.clone()`, `.to_array()`), failure (`Result`, `?`), mutation (`mut`, `out`), threads (`par_map`, `spawn`), the unchecked world (`unsafe`, `raw`, `extern`). A reader — human or AI — audits a file by scanning for those words, and their absence is a guarantee, not a hope. Write code that keeps this property: the next reader of your program is the point of the language.
