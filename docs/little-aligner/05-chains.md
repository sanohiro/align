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

**Q5.** Why don't we just write `for` loops like in C or Go?

**A5.** A `for` loop is a set of instructions on *how* to move the CPU's feet. A pipeline is a statement of *what* the data becomes. When you declare *what* the data is doing, the compiler is free to optimize the *how*—vectorizing, fusing, and unrolling—far better than hand-written loops. 

---

**Q6.** In other languages, chaining `map` and `where` creates temporary intermediate arrays for every step, chewing up memory. Does Align?

**A6.** No. Align pipelines are lazy until the final collapse. However many stages you chain—`map`, `where`, another `map`, a `scan`, a `sum`, ten more if you like—the compiler fuses all of them into exactly one loop, intermediates in registers. It is as if you wrote a painstakingly hand-optimized C loop, but you didn't have to. You may check any chain you write: `alignc emit-llvm yourfile.align`.

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

**A8.** No — a chain must end in a collapse (`sum`, `count`, …) or a materialization (`to_array`, `sort`, `map_into`). A held middle would be a secret unfinished loop. End it, or don't start it.

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

**A10.** `[3, 7, 5]` — sums of `[1,2]`, `[3,4]`, `[5]`. `chunks(n)` deals the array into hands of `n` (last hand short), each a slice, each pipelined like anything else.

---

**Q11.** A chain that writes into memory *you* own — what is `map_into`?

```align
src.map(dbl).map_into(dst)
```

**A11.** The zero-allocation ending: results go into the slice `dst`, which must be the same length, and which the compiler proves doesn't overlap `src`. For the hot path that recycles buffers.

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

**A14.** Exactly. One physical loop carries one logical sentence. The compiler may change the machinery only while preserving the sentence.

---

**Q15.** Write this request: “From the temperatures, keep positive ones, convert each from Celsius to doubled half-degrees, and find the maximum.”

**A15.**

```align
temps.where(fn t { t > 0 }).map(fn t { t * 2 }).max()
```

Nouns give the source and answer; verbs give the stages.

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

**Q17.** Split `[1, 2, 3, 4, 5, 6, 7]` into hands of three and total each hand.

**A17.**

```align
[1, 2, 3, 4, 5, 6, 7]
    .chunks(3)
    .map(fn hand { hand.sum() })
    .to_array()
```

`[6, 15, 7]`. A chunk is a view, so the last short hand costs no padding and no copy.

---

**Q18.** Why does Q17 end in `to_array` rather than `sum`?

**A18.** Because the requested answer is one total *per hand*, not one total for the whole input. The shape of the answer chooses the ending.

---

**Q19.** We already own a destination buffer and want doubled values in it. Which ending says that?

**A19.**

```align
src.map(fn x { x * 2 }).map_into(dst)
```

`to_array` asks for new storage; `map_into` names existing storage. Same transformation, different ownership story.

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

Break thoughts into named functions, not unfinished streams.

---

> **The Fifth Commandment**
>
> *A chain reads left to right, filters early, and ends. One sentence, one loop, one answer.*
