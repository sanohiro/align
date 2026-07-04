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

**A8.** No. Captured **by value** — a copy. There is no shared environment to mutate. Remember this in chapter 10's parallel world; it is why nothing races.

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

**Q13.** What does `print` do inside a map?

```align
[1, 2].map(fn x { print(x) ... })
```

**A13.** Stop. A pipeline stage is for *computing*, and later — chapter 10 — you will meet stages that demand purity outright. Do your printing after the pipeline hands you the answer.

---

> **The Second Commandment**
>
> *When you would visit each element, `map` it. When you would keep the result, say `to_array()` out loud.*
