# 3. Keep Some

> 🌐 **English** · [Japanese](./ja/03-keep-some.md)

**Q1.** Of `[1, 2, 3, 4, 5]`, which are greater than 2?

**A1.** `3, 4, 5`. You just did a `where`.

---

**Q2.** In Align?

**A2.** `[1, 2, 3, 4, 5].where(fn x { x > 2 }).sum()` — which is `12`.

---

**Q3.** What is `fn x { x > 2 }` called, when it answers only `true` or `false`?

**A3.** A predicate. `where` keeps the elements its predicate blesses.

---

**Q4.** What is `[1, 2, 3, 4, 5].where(fn x { x > 2 }).count()`?

**A4.** `3`. Count what survived; don't sum it.

---

**Q5.** What is `[1, 2, 3].where(fn x { x > 10 }).sum()`?

**A5.** `0`. Nothing survived; the sum of nothing is zero. No error — an empty result is an answer, not a failure.

---

**Q6.** Now data with names on it:

```align
Item { price: i64, active: bool }

items := [
    Item { price: 100, active: true },
    Item { price: 50,  active: false },
    Item { price: 200, active: true },
]
```

Which prices are active?

**A6.** `100` and `200`.

---

**Q7.** Say that in Align.

**A7.** `items.where(.active).price.sum()` — which is `300`.

---

**Q8.** Two new spells in Q7. What is `.active` doing inside `where`?

**A8.** Field shorthand: `where(.active)` keeps the rows whose `active` field is `true`. Nothing more to write when the field already is the predicate.

---

**Q9.** And what is the bare `.price` stage doing?

**A9.** Projecting. From each surviving `Item`, take the `price` — a stream of structs becomes a stream of numbers, ready to sum.

---

**Q10.** What is `items.price.where(fn p { p > 60 }).sum()`?

**A10.** `300` again — `100 + 200`, this time by price, ignoring `active` entirely. Project first, filter after: also legal. Stages snap together in the order *you* mean.

---

**Q11.** How many of those three structs did Q7 copy anywhere?

**A11.** None. `where` skips, `.price` is a field load from the row where it lies, `sum` accumulates. One pass, no temporaries — same fusion as chapter 2.

---

**Q12.** May `where` and `map` share a pipeline?

**A12.** They were made for it:

```align
items.where(.active).price.map(fn p { p * 108 / 100 }).sum()
```

`324` — the active prices, taxed, summed. Read it left to right and it says what it does.

---

> **The Third Commandment**
>
> *Filter with `where`, point with `.field`, and let the data flow to its answer.*
