# 5. Chains

> 🌐 **English** · [Japanese](./ja/05-chains.md)

**Q1.** Read this aloud, left to right:

```align
scores.map(fn s { s + 5 }).where(fn s { s >= 60 }).count()
```

**A1.** "Take the scores; add five to each; keep those at least sixty; count them." If you can read it, you can write it. That is the point of chains.

---

**Q2.** With `scores := [55, 40, 90]`, what does it answer?

**A2.** `2`. The curve lifts `55` to `60` (in) and `40` to `45` (out); `90` was never in doubt.

---

**Q3.** Does the order of stages matter? Compare:

```align
xs.where(fn x { x > 0 }).map(fn x { x * 2 }).sum()
xs.map(fn x { x * 2 }).where(fn x { x > 0 }).sum()
```

**A3.** For doubling, the answers agree — doubling doesn't change sign. But the *work* differs: the first filters early and doubles fewer. Filter early when you can; the chain does exactly what you wrote, in the order you wrote it.

---

**Q4.** And these two?

```align
xs.map(fn x { x - 10 }).where(fn x { x > 0 }).count()
xs.where(fn x { x > 0 }).map(fn x { x - 10 }).count()
```

**A4.** Different answers! With `xs := [5, 15]`: the first subtracts then keeps positives — count `1`. The second keeps positives then subtracts — count `2` (a `where` after which nothing filters again). A chain is a sentence; word order is meaning.

---

**Q5.** How many loops does a five-stage chain compile to?

**A5.** One. Always one. `map`–`where`–`map`–`scan`–`sum` — one counted loop, intermediates in registers, ready for the vectorizer. You may check: `alignc emit-llvm yourfile.align`.

---

**Q6.** So when I split a long chain across lines, does it cost anything?

```align
total := items
    .where(.active)
    .price
    .map(with_tax)
    .sum()
```

**A6.** Nothing. A line starting with `.` continues the chain. Layout is for the reader; the compiler sees one pipeline either way.

---

**Q7.** May I stop a chain in the middle and hold the half-done work?

```align
halfway := items.where(.active).price
```

**A7.** No — a chain must end in a collapse (`sum`, `count`, …) or a materialization (`to_array`, `sort`, `map_into`). A held middle would be a secret unfinished loop. End it, or don't start it.

---

**Q8.** Then how do I *reuse* a filtered set for two questions?

**A8.** Materialize once, ask twice:

```align
active := items.where(.active).price.to_array()
print(active.sum())
print(active.max())
```

One visible allocation, two cheap reductions. (One day you'll want *many* aggregates over *groups* — that is chapter 10's `agg`.)

---

**Q9.** What does `chunks` do here?

```align
[1, 2, 3, 4, 5].chunks(2).map(fn c { c.sum() }).to_array()
```

**A9.** `[3, 7, 5]` — sums of `[1,2]`, `[3,4]`, `[5]`. `chunks(n)` deals the array into hands of `n` (last hand short), each a slice, each pipelined like anything else.

---

**Q10.** A chain that writes into memory *you* own — what is `map_into`?

```align
src.map(dbl).map_into(dst)
```

**A10.** The zero-allocation ending: results go into the slice `dst`, which must be the same length, and which the compiler proves doesn't overlap `src`. For the hot path that recycles buffers.

---

**Q11.** The habit, then. You are about to write a loop. What do you ask first?

**A11.** *"What is the transformation?"* Then you write it as stages: change-each → keep-some → collapse. If you cannot name the stages, chapter 11 (`loop`) is waiting — but ask the question first, every time.

---

> **The Fifth Commandment**
>
> *A chain reads left to right, filters early, and ends. One sentence, one loop, one answer.*
