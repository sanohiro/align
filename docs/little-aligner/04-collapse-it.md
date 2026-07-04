# 4. Collapse It

> 🌐 **English** · [Japanese](./ja/04-collapse-it.md)

**Q1.** We have met `sum` and `count`. What is `[5, 3, 8, 1].min()`?

**A1.** `1`.

---

**Q2.** And `[5, 3, 8, 1].max()`?

**A2.** `8`. The reducers so far: `sum`, `count`, `min`, `max`. Each collapses many to one.

---

**Q3.** What is `[1, 2, 3, 4].any(fn x { x > 3 })`?

**A3.** `true` — at least one element passes. Its sibling `all(fn x { x > 0 })` is also `true` here. `any` and `all` collapse to a `bool`.

---

**Q4.** What is `[2, 4, 6].all(fn x { x % 2 == 0 })`?

**A4.** `true`. And `[2, 4, 7]`? — `false`. The `7` ruins it, and `all` can say so the moment it meets it.

---

**Q5.** Suppose no reducer fits: we want the *product*. What now?

**A5.** The general one: `reduce`.

```align
[1, 2, 3, 4].reduce(1, fn acc, x { acc * x })
```

`24`.

---

**Q6.** Why the `1` in front?

**A6.** The starting value — first the seed, then the folding function. `acc` begins at `1`; each element multiplies in. For a sum you would seed `0`. The seed is what an empty array answers.

---

**Q7.** In `fn acc, x { acc * x }`, which is which?

**A7.** `acc` is the answer so far; `x` is the next element. The lambda says how the next element joins the answer.

---

**Q8.** Write `sum` yourself, with `reduce`.

**A8.** `xs.reduce(0, fn acc, x { acc + x })`. Now you know `sum`, `count`, `min`, `max` are courtesies — `reduce` is the general engine, and the named ones say the intent quicker.

---

**Q9.** What is `[1, 2, 3, 4].scan(0, fn acc, x { acc + x })`?

**A9.** The *running* sums: `1, 3, 6, 10`. `scan` is `reduce` that shows its work — and so it is a **stage**, not an ending: something must still consume it.

---

**Q10.** Then what is `[1, 2, 3, 4].scan(0, fn acc, x { acc + x }).max()`?

**A10.** `10` — the largest the running sum ever got. (With negatives in the array, that question becomes interesting: the high-water mark.)

---

**Q11.** What is `[3, 1, 2].map(fn x { x }).sort()` — may we ask for order?

**A11.** `[1, 2, 3]`, materialized. `sort` must build the array to sort it — so like `to_array`, it hands you a real array, allocation in plain sight.

---

**Q12.** And `[10, 21, 32, 3].sort_by_key(fn x { -x })`?

**A12.** `[32, 21, 10, 3]`. Sort by the negated value: descending, without a `reverse` in sight.

---

**Q13.** What is `[1, 2, 3, 4, 5].partition(fn x { x % 2 == 0 })`?

**A13.** Two arrays at once: `([2, 4], [1, 3, 5])` — the blessed, then the rest. Catch them together: `(evens, odds) := ...`.

---

> **The Fourth Commandment**
>
> *Seed first, then fold. And when a named reducer says it plainer — `sum`, `count`, `min`, `max`, `any`, `all` — say it plainly.*
