# 3. Keep Some

> 🌐 **English** · [Japanese](./ja/03-keep-some.md)

**Q1.** Of `[1, 2, 3, 4, 5]`, which are greater than 2?

**A1.** `3, 4, 5`. You just did a `where`.

---

**Q2.** In Align?

**A2.** `[1, 2, 3, 4, 5].where(fn x { x > 2 }).sum()` — which is `12`.

---

**Q3.** What is `fn x { x > 2 }` called, when it answers only `true` or `false`?

**A3.** A predicate. `where` keeps each element for which the predicate returns `true`.

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

**Q8.** Two new pieces of syntax in Q7. What is `.active` doing inside `where`?

**A8.** Field shorthand: `where(.active)` keeps the rows whose `active` field is `true`. Nothing more to write when the field already is the predicate.

---

**Q9.** And what is the bare `.price` stage doing?

**A9.** Projecting. From each surviving `Item`, take the `price` — a stream of structs becomes a stream of numbers, ready to sum.

---

**Q10.** What is `items.price.where(fn p { p > 60 }).sum()`?

**A10.** Also `300` (`100 + 200`). This time we filter by price and do not check `active`. The two conditions happen to give the same answer for these items; they ask different questions.

---

**Q11.** Does Q7 copy any `Item` into intermediate storage?

**A11.** No. `where` skips unwanted rows, `.price` reads each remaining row's price, and `sum` adds it. One loop, no intermediate array: the fusion we saw in chapter 2.

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

**A13.** `[5, 12]`. Keep the rows whose `valid` field is `true`, then take their `value` fields.

---

**Q14.** What is `readings.value.where(fn x { x > 10 }).to_array()`?

**A14.** `[40, 12]`. This expression does not check `valid`. After `.value`, the pipeline carries numbers, so the other fields are no longer available to later stages.

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

**A18.** Zero. Of the three readings, only `12` passes both predicates. It is doubled and added to the sum.

---

**Q19.** Say Q17 without syntax.

**A19.** “Take the values from valid readings, keep those greater than ten, double them, and add.” Check which part of the expression performs each operation.

---

> **The Third Commandment**
>
> *Use `where` to keep elements that satisfy a condition, and `.field` to take a field from each element.*
