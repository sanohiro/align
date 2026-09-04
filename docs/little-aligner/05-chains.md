# 5. Chains

> 🌐 **English** · [Japanese](./ja/05-chains.md)

**Q1.** Read this aloud, left to right:

```align
scores.map(fn s { s + 5 }).where(fn s { s >= 60 }).count()
```

**A1.** "Take the scores; add five to each; keep those at least sixty; count them." If you can read it, you can write it. That is the point of chains.

---

**Q2.** With `scores := [55, 40, 90]`, what does it answer?

**A2.** `2`. Adding five gives `[60, 45, 95]`, and two of those scores are at least sixty.

---

**Q3.** Does the order of stages matter? Compare:

```align
xs.where(fn x { x > 0 }).map(fn x { x * 2 }).sum()
xs.map(fn x { x * 2 }).where(fn x { x > 0 }).sum()
```

**A3.** When doubling does not overflow, the answers agree. The *work* differs: the first filters early and doubles fewer values. Remember chapter 1: integer overflow wraps, so doubling can change a value's sign. Move a filter only when that preserves the answer for your inputs.

---

**Q4.** And these two?

```align
xs.map(fn x { x - 10 }).where(fn x { x > 0 }).count()
xs.where(fn x { x > 0 }).map(fn x { x - 10 }).count()
```

**A4.** Different answers! With `xs := [5, 15]`: the first subtracts then keeps positives — count `1`. The second keeps positives then subtracts — count `2` (a `where` after which nothing filters again). A chain is a sentence; word order is meaning.

---

**Q5.** Why don't we just write `for` loops like in C or Go?

**A5.** A pipeline names the work on each element and the result we need. We do not have to manage an index or an accumulator ourselves. The compiler can fuse the stages and, where the operations allow it, vectorize the loop.

---

**Q6.** Does chaining `map` and `where` create a temporary array for each step?

**A6.** No. `map` and `where` fuse with the ending: each element passes through the stages without an intermediate array. A chain ending in `sum` uses one loop. Materializing operations such as `scan`, `sort`, and `to_array` build an array; a later reduction reads that array in another pass. You can inspect the generated code with `alignc emit-llvm yourfile.align`.

---

**Q7.** So when I split a long chain across lines, does it cost anything?

```align
total := items
    .where(.active)
    .price
    .map(with_tax)
    .sum()
```

**A7.** Nothing. A line starting with `.` continues the chain. Layout is for the reader; the compiler sees one pipeline either way.

---

**Q8.** May I stop a chain in the middle and hold the half-done work?

```align
halfway := items.where(.active).price
```

**A8.** No — a chain must end in a reduction (`sum`, `count`, …) or a materialization (`to_array`, `sort`, `map_into`). The ending determines whether we reduce the values, allocate an array, or write into existing storage.

---

**Q9.** Then how do I *reuse* a filtered set for two questions?

**A9.** Materialize once, ask twice:

```align
active := items.where(.active).price.to_array()
print(active.sum())
print(active.max())
```

One visible allocation, two cheap reductions. (One day you'll want *many* aggregates over *groups* — that is chapter 10's `agg`.)

---

**Q10.** What does `chunks` do here?

```align
[1, 2, 3, 4, 5].chunks(2).map(fn c { c.sum() }).to_array()
```

**A10.** `[3, 7, 5]`: the sums of `[1, 2]`, `[3, 4]`, and `[5]`. `chunks(n)` splits the array into slices of up to `n` elements. The last is shorter when the length is not a multiple of `n`.

---

**Q11.** A chain that writes into memory *you* own — what is `map_into`?

```align
fn dbl(x: i64) -> i64 = x * 2

fn double_into(src: slice<i64>, out dst: slice<i64>) {
    src.map(dbl).map_into(dst)
}
```

**A11.** The zero-allocation ending: results go into the slice `dst`, which must be the same length, and which the compiler proves doesn't overlap `src`. For the hot path that recycles buffers.

`out` says that the caller provides the destination. The function writes into `dst` instead of allocating an array of its own. The destination for `map_into` is received through an `out` parameter like this one.

---

**Q12.** The habit, then. You are about to write a loop. What do you ask first?

**A12.** *"What is the transformation?"* Then you write it as stages: change-each → keep-some → collapse. If you cannot name the stages, chapter 11 (`loop`) is waiting — but ask the question first, every time.

---

**Q13.** Does fusion let the compiler move an effect across a `where`?

```align
xs.map(log).where(is_wanted).count()
xs.where(is_wanted).map(log).count()
```

**A13.** No. The first calls `log` for every input; the second calls it only for survivors. Fusion removes intermediate storage, not meaning. Sequential callables run in written stage order, and a rejected element never reaches a later stage.

---

**Q14.** So “one loop” does not mean “the order no longer matters”?

**A14.** Correct. Combining the work into one loop preserves the order of operations on each element.

---

**Q15.** Write this request: “From the temperatures in degrees Celsius, keep positive ones, express each in units of half a degree, and find the maximum.”

**A15.**

```align
temps.where(fn t { t > 0 }).map(fn t { t * 2 }).max()
```

`where` keeps positive values, `map` doubles them to change units, and `max` finds the largest.

---

**Q16.** We also need the count of those converted values. Should we materialize?

**A16.** If these are the only two questions and the input is cheap to scan, two fused reductions may be simpler. If the conversion is expensive or the result will answer many questions, materialize once:

```align
warm := temps.where(fn t { t > 0 }).map(fn t { t * 2 }).to_array()
hi := warm.max()
n := warm.count()
```

The right answer depends on reuse. “No hidden allocation” does not mean “never allocate.”

---

**Q17.** Split `[1, 2, 3, 4, 5, 6, 7]` into chunks of three elements and sum each chunk.

**A17.**

```align
[1, 2, 3, 4, 5, 6, 7]
    .chunks(3)
    .map(fn hand { hand.sum() })
    .to_array()
```

`[6, 15, 7]`. Each chunk is a view into the original array. The last contains only one element; it needs no padding or copying.

---

**Q18.** Why does Q17 end in `to_array` rather than `sum`?

**A18.** Because the requested answer is one total *per chunk*, not one total for the whole input. The shape of the answer chooses the ending.

---

**Q19.** We already own a destination buffer and want doubled values in it. Which ending says that?

**A19.**

```align
fn scale_into(src: slice<i64>, out dst: slice<i64>) {
    src.map(fn x { x * 2 }).map_into(dst)
}
```

`to_array` creates a new array. `map_into` writes into storage supplied by the caller as `out dst` (A11). The transformation is the same; the destination differs.

---

**Q20.** The chain is getting hard to read. Should we break it into half-pipelines?

**A20.** No. Name callables, or lay the chain across lines, but keep the data sentence whole:

```align
answer := xs
    .where(is_wanted)
    .map(normalize)
    .map(score)
    .sum()
```

Put the transformations in named functions while keeping the pipeline connected to its ending.

---

For a diagram comparing a fused reduction with `to_array()` followed by another pass, see the guide's [pipeline chapter](../guide/06-pipelines.md).

> **The Fifth Commandment**
>
> *Read a chain left to right. Filter early when it preserves the answer, and choose the ending the result needs.*
