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

**A4.** `true`. And for `[2, 4, 7]`? `false`, because `7` fails the predicate. Align's `any` and `all` **do not short-circuit**: they scan the full input even after the answer is known. This approach suits vectorization (chapter 13).

---

**Q5.** Suppose no reducer fits: we want the *product*. What now?

**A5.** Use `reduce` to multiply the elements.

```align
[1, 2, 3, 4].reduce(1, fn acc, x { acc * x })
```

`24`.

---

**Q6.** Why the `1` in front?

**A6.** It is the initial value of `acc`. Start at `1` and multiply by each element. For a sum, start at `0`. If there are no elements, the initial value is the answer.

---

**Q7.** In `fn acc, x { acc * x }`, which is which?

**A7.** `acc` is the answer so far; `x` is the next element. The lambda says how the next element joins the answer.

---

**Q8.** Write `sum` yourself, with `reduce`.

**A8.** `xs.reduce(0, fn acc, x { acc + x })`. With `reduce`, we specify how to combine the values. When we just want their sum, `sum` says that more directly.

---

**Q9.** What is `[1, 2, 3, 4].scan(0, fn acc, x { acc + x })`?

**A9.** The array of running sums: `[1, 3, 6, 10]`. Where `reduce` returns only the final value, `scan` stores each intermediate value in an array. It is a materializing **ending**, so it needs no `to_array()`. We may apply another reduction to the array it returns.

---

**Q10.** Then what is `[1, 2, 3, 4].scan(0, fn acc, x { acc + x }).max()`?

**A10.** `10`, the largest running sum. Try an array containing negative numbers too.

---

**Q11.** What does `[3, 1, 2].map(fn x { x }).sort()` return?

**A11.** The array `[1, 2, 3]`. `sort` is another ending that materializes an array.

---

**Q12.** And `[10, 21, 32, 3].sort_by_key(fn x { -x })`?

**A12.** `[32, 21, 10, 3]`. Sort by the negated value: descending, without a `reverse` in sight.

---

**Q13.** What is `[1, 2, 3, 4, 5].partition(fn x { x % 2 == 0 })`?

**A13.** Two arrays at once: `([2, 4], [1, 3, 5])` — elements that pass the predicate, then those that fail it. Bind them together: `(evens, odds) := ...`.

---

**Q14.** Over an empty slice, what do `any` and `all` answer?

**A14.** `any` returns `false`; `all` returns `true`. There is no element that passes the predicate, and none that fails it.

---

**Q15.** Why is `all` of nothing `true`? It sounds strange.

**A15.** Ask instead: “Is there an element that fails the predicate?” In an empty slice, there is none. Just as `sum` returns `0` for empty input, `all` returns `true`.

---

**Q16.** Sum the squares of `[1, 2, 3, 4]`.

**A16.**

```align
[1, 2, 3, 4].map(fn x { x * x }).sum()
```

`30`. First say what each becomes; then say how the many become one.

---

**Q17.** Count the even squares. Must we square first?

**A17.** No. A number and its square have the same evenness:

```align
[1, 2, 3, 4].where(fn x { x % 2 == 0 }).count()
```

`2`. Do not compute a value the final question does not need.

---

**Q18.** Find both the sum and count in one fold.

**A18.**

```align
pair := [2, 4, 6].reduce((0, 0), fn acc, x {
    (acc.0 + x, acc.1 + 1)
})
```

`pair` is `(12, 3)`. A tuple accumulator can keep the sum and the count together.

---

**Q19.** Then the integer average?

**A19.** `pair.0 / pair.1`, or `4`: divide the sum by the count. If the input might be empty, decide how to handle that case before dividing.

---

**Q20.** Running totals of `[3, -5, 4, 2]`?

**A20.** `[3, -2, 2, 4]` from:

```align
totals := [3, -5, 4, 2].scan(0, fn acc, x { acc + x })
```

No `to_array` in sight: `scan` already built the array (A9). Asking for one again would allocate the same answer twice.

---

**Q21.** What was the highest running total?

**A21.**

```align
[3, -5, 4, 2].scan(0, fn acc, x { acc + x }).max()
```

`4`. `max` finds the largest of the running sums stored by `scan`.

---

> **The Fourth Commandment**
>
> *Give `reduce` an initial value and a way to combine elements. When `sum` or `count` says what you need, use that name.*
