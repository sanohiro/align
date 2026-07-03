# Thinking in pipelines

This is the heart of Align. You rarely write a `for` loop; you describe a transformation over a collection and let the compiler generate the loop — fused, branch-minimal, vectorizable.

## The shape

```align
total := prices.map(with_tax).where(in_stock).sum()
```

Read left to right: take `prices`, apply `with_tax` to each, keep the ones where `in_stock` holds, sum the result. Crucially, **no intermediate arrays are built** — `map`, `where`, and `sum` fuse into a single counted loop. This is stream fusion, done by the compiler, not by you.

## The stages

- `map(f)` — transform each element.
- `where(p)` — keep elements matching a predicate. (`where(.active)` keeps rows whose `active` field is true.)
- `.field` — project a field out of each struct.
- `reduce(init, f)` / `sum()` — collapse to a single value. A pipeline ends in a reduction.
- `chunks`, `partition` — the wider family (with more, like `sort` and `scan`, arriving as the language grows; see the spec for the current set).

## Why this is fast

A hand-written version in a scalar language allocates a temporary array per step and walks memory three times. Align's fused loop walks once, keeps intermediates in registers, and hands the vectorizer a clean counted loop with no aliasing and cold error paths. The "obvious" code *is* the fast code — you are not giving up speed for clarity, which is the whole point.

## A worked example

Summing the after-tax price of in-stock items, over an array of structs:

```align
Item { price: f64, active: bool }

fn with_tax(p: f64) -> f64 = p * 1.08

fn main() -> i32 {
    items := [
        Item { price: 100.0, active: true },
        Item { price: 50.0,  active: false },
        Item { price: 200.0, active: true },
    ]
    total := items.where(.active).price.map(with_tax).sum()
    // (100.0 + 200.0) * 1.08 = 324.0
    print(total)
    return 0
}
```

One loop. No temporaries. The `where(.active)` becomes a branch that skips to the next iteration; the `.price` is a field load; `with_tax` inlines; `sum` accumulates.

## The habit

When you find yourself about to write `for`, stop and ask: what is the transformation? Then write it as a pipeline. Explicit loops are for the rare case the pipeline vocabulary genuinely can't express — and that case is rarer than you think.
