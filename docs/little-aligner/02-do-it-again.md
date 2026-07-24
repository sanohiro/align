# 2. Do It Again

> 🌐 **English** · [Japanese](./ja/02-do-it-again.md)

**Q1.** Here is an array: `[1, 2, 3]`. Say each element, doubled.

**A1.** `2, 4, 6`. You just did a `map` in your head.

---

**Q2.** Now in Align. What is `[1, 2, 3].map(fn x { x * 2 }).sum()`?

**A2.** `12`. `map` doubled each element — `[2, 4, 6]` — and `sum` added them.

---

**Q3.** What is `fn x { x * 2 }`?

**A3.** A lambda: a nameless function. `fn`, a parameter, a block. Its parameter type is inferred from the elements flowing in.

---

**Q4.** Could we have used a named function instead?

**A4.** Yes:

```align
fn double(x: i64) -> i64 = x * 2

[1, 2, 3].map(double).sum()
```

Same answer, `12`. A stage takes any function of the right shape.

---

**Q5.** What is `[1, 2, 3].map(fn x { x * x }).sum()`?

**A5.** `14`. Squares: `1 + 4 + 9`.

---

**Q6.** What is `[].map(fn x { x * x }).sum()` — mapping over nothing?

**A6.** Trick question — an empty array literal has no element type to infer. But map over an empty *slice* of `i64` and the sum is `0`. Nothing, squared, is still nothing.

---

**Q7.** May a lambda use a name from outside itself?

```align
factor := 3
[1, 2, 3].map(fn x { x * factor }).sum()
```

**A7.** Yes — `18`. The lambda **captures** `factor` by value: it takes a copy the moment it is made.

---

**Q8.** If `factor` were `mut` and changed *after* the `map`, would the map see it?

**A8.** No. Captured **by value** — a copy. There is no shared environment to mutate. This becomes especially important when work is made explicitly parallel: the lambda does not share a changing `factor`.

---

**Q9.** What is `[1, 2, 3].map(fn x { x * 2 })` — with no `.sum()`?

**A9.** It does not compile. A pipeline must end — in a reduction, or a materialization. A floating half-pipeline would be a hidden cost, and Align does not do hidden.

---

**Q10.** So how do we *keep* the doubled array, if we truly want it?

**A10.** Say so: `[1, 2, 3].map(fn x { x * 2 }).to_array()`. Now you hold `[2, 4, 6]`, and the allocation is written where everyone can see it.

---

**Q11.** What is `[1, 2, 3].map(fn x { x * 2 }).map(fn x { x + 1 }).sum()`?

**A11.** `15`. Two maps in a row: `[3, 5, 7]`. And here is the secret: the compiler makes **one loop**, not two. The stages fuse.

---

**Q12.** How many arrays did Q11 build along the way?

**A12.** Zero. `3`, `5`, `7` lived in a register for one instant each, already being summed. That is why we end pipelines instead of keeping middles.

---

**Q13.** May a sequential `map` print?

```align
[1, 2].map(fn x {
    print(x)
    x * 2
}).sum()
```

**A13.** Yes. It prints `1`, then `2`, and answers `6`. Sequential stages may have side effects; they run in input order and stage order, exactly once for each element that reaches them.

---

**Q14.** Then why is printing inside a pipeline usually a poor habit?

**A14.** Because it mixes the answer with observation, restricts optimization, and stops the same function from being used by `par_map`. Keep a stage Pure when it naturally can be — but know the rule, not a myth: sequential `map` permits effects; explicit parallel work does not.

---

**Q15.** What changes when `map` becomes `par_map`?

**A15.** The order of workers is no longer a story you may observe, so its callable must be Pure. No printing, no changing outside state. Parallelism is written in the source, and so is the discipline that makes it safe.

---

**Q16.** Keep your finger on each element:

```align
[2, 4, 6]
    .map(fn x { x + 1 })
    .map(fn x { x * x })
    .to_array()
```

**A16.** `[9, 25, 49]`. Each element finishes the whole chain: `2 → 3 → 9`, then `4 → 5 → 25`, then `6 → 7 → 49`.

---

**Q17.** Swap the two maps. Same answer?

**A17.** No: `[5, 17, 37]`. Now each path is `2 → 4 → 5`, and so on. Function composition has an order even when both stages visit every element.

---

**Q18.** Add ten to every score and keep the new scores. What ending?

**A18.** `.to_array()`:

```align
scores.map(fn s { s + 10 }).to_array()
```

The request asked for many scores back, so materialize many.

---

**Q19.** Add ten to every score and ask for their total. What ending?

**A19.** `.sum()`:

```align
scores.map(fn s { s + 10 }).sum()
```

The request asked for one answer, so collapse directly. Do not build many on the way to one.

---

**Q20.** `offset` is `7`. Write the transformation “move every point seven places right.”

**A20.**

```align
offset := 7
moved := points.map(fn x { x + offset }).to_array()
```

The lambda carries the one outside fact it needs.

---

**Q21.** Now the same transformation will be used in five places. What should change?

**A21.** Give it a name:

```align
fn move_right(x: i64) -> i64 = x + 7
```

Then use `.map(move_right)`. Inline lambdas are for local thoughts; named functions are for repeated thoughts. The pipeline does not care which spelling supplied the callable.

---

> **The Second Commandment**
>
> *When you would visit each element, `map` it. When you would keep the result, say `to_array()` out loud.*
