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

**A9.** It does not compile. A pipeline must end in a reduction or a materialization. `map` describes the transformation; the ending says what to do with its results.

---

**Q10.** So how do we *keep* the doubled array, if we truly want it?

**A10.** Say so: `[1, 2, 3].map(fn x { x * 2 }).to_array()`. Now you hold `[2, 4, 6]`, and the allocation is written where everyone can see it.

---

**Q11.** What is `[1, 2, 3].map(fn x { x * 2 }).map(fn x { x + 1 }).sum()`?

**A11.** `15`. Two maps in a row produce `3, 5, 7`, which `sum` adds. The compiler combines the stages into **one loop**. This is called fusion.

---

**Q12.** How many arrays did Q11 build along the way?

**A12.** Zero intermediate arrays. Each transformed value goes straight into the sum. We described the values `3, 5, 7` without asking the program to store them as an array.

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

**A14.** It mixes calculation with output, limits optimization, and prevents us from using the same function with `par_map`. Keep a stage Pure when its job allows it. Remember the distinction: sequential `map` permits side effects; explicit parallel work does not.

---

**Q15.** What changes when `map` becomes `par_map`?

**A15.** Workers may run in any order, so the callable must be Pure: no printing and no changing outside state. Chapter 12 shows how to use it.

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

`.to_array()`. We need to keep the transformed scores as an array.

---

**Q19.** Add ten to every score and ask for their total. What ending?

**A19.** `.sum()`:

```align
scores.map(fn s { s + 10 }).sum()
```

`.sum()`. We need only the total, so there is no need to store the transformed scores in an array.

---

**Q20.** `offset` is `7`. Write the transformation “move every point seven places right.”

**A20.**

```align
offset := 7
moved := points.map(fn x { x + offset }).to_array()
```

The lambda captures `offset` from the surrounding scope.

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
> *Use `map` to transform each element. If you need an array of the results, write `to_array()`.*
