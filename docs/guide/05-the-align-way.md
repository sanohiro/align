# The Align way

A short collection of the idioms that make Align code fast and idiomatic — the "culture," the way an experienced Align programmer reaches for things. Each is a habit the earlier chapters introduced; here they are as a checklist.

## Describe transformations, don't iterate

Reach for `map`/`where`/`reduce` before you reach for a loop. The pipeline fuses to one pass; a hand-rolled loop with temporaries does not. If you're writing `for`, ask whether the pipeline vocabulary can say it — usually it can.

```align
// idiomatic
total := xs.map(f).where(p).sum()

// avoid, when a pipeline expresses it
// mut total := 0
// for x in xs { ... }
```

## Handle errors with `?`, not branches

A function that can fail returns `Result`; call it with `?`. The happy path reads top-to-bottom; the error path is the cold edge the compiler lays out away from the hot code.

```align
import std.fs

fn load() -> Result<Config, Error> {
    raw := fs.read_file(path)?
    cfg := parse(raw)?
    return Ok(cfg)
}
```

## Choose the lifetime, then forget about memory

Local value → just a value. A phase that allocates a lot → wrap it in `arena {}`. You make that one decision; the compiler does the rest. No `free`, no leak, no dangling.

## Lay bulk data out as SoA

If you process a collection repeatedly, use `soa<T>`. Cache density and vectorization follow from the layout. AoS is for data you touch whole and rarely.

## Let the compiler see the shape

Align's speed comes from the compiler being able to prove things: contiguous memory, no aliasing, non-null, cold error paths, arena lifetimes. Every restriction — no null, one error model, Move types, inferred regions — exists so the compiler can infer those properties *without* you writing annotations. Working with the grain (pipelines over loops, `Result` over sentinels, arenas over scattered allocation) is what keeps that inference alive. Idiomatic Align is fast Align — the two are the same thing by design.

## Nothing hidden

If a line allocates, you can see it (`heap.new`, an `arena` block, a `string` builder). If it can fail, it returns `Result`. If it runs in parallel, you wrote `par_map` or `task_group`. There is no hidden cost to hunt for — which is what makes Align code, once written, easy to trust.
