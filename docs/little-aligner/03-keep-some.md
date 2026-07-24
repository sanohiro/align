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

**Q13.** A smaller table:

```align
Reading { value: i64, valid: bool }

readings := [
    Reading { value: 5,  valid: true },
    Reading { value: 40, valid: false },
    Reading { value: 12, valid: true },
]
```

What is `readings.where(.valid).value.to_array()`?

**A13.** `[5, 12]`. Keep rows first; project the surviving field second.

---

**Q14.** What is `readings.value.where(fn x { x > 10 }).to_array()`?

**A14.** `[40, 12]`. This question never looked at `valid`. A field projection forgets the other fields; after `.value`, only numbers flow.

---

**Q15.** Keep readings that are both valid and greater than ten.

**A15.**

```align
readings
    .where(.valid)
    .where(fn r { r.value > 10 })
    .value
    .to_array()
```

The answer is `[12]`. The second predicate still receives a `Reading`, because projection has not happened yet.

---

**Q16.** Could we project `.value` before the second `where`?

**A16.** Yes, after valid rows have been selected:

```align
readings.where(.valid).value.where(fn x { x > 10 }).to_array()
```

Same answer. Now the second predicate receives an `i64`. Choose the order that says the thought most plainly.

---

**Q17.** Double those surviving values and sum them.

**A17.**

```align
readings
    .where(.valid)
    .value
    .where(fn x { x > 10 })
    .map(fn x { x * 2 })
    .sum()
```

`24`.

---

**Q18.** How many temporary arrays did Q17 make?

**A18.** None. Three readings entered; one reached the multiply; one number reached the accumulator. Trace elements, not imaginary collections.

---

**Q19.** Say Q17 without syntax.

**A19.** “Take the readings; keep valid ones; take their values; keep values over ten; double them; add them.” If the sentence and the chain disagree, one of them is wrong.

---

> **The Third Commandment**
>
> *Filter with `where`, point with `.field`, and let the data flow to its answer.*
