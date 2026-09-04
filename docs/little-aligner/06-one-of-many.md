# 6. One of Many

> 🌐 **English** · [Japanese](./ja/06-one-of-many.md)

**Q1.** A traffic light is red, yellow, or green. Is it ever two of them?

**A1.** No. One of many — that is a sum type:

```align
Light { Red, Yellow, Green }
```

---

**Q2.** How do we make the green one?

**A2.** `Light.Green`. Qualified by its type, always — a bare `Green` could be anyone's.

---

**Q3.** How do we ask which one we hold?

**A3.** We don't ask — we `match`:

```align
fn go(l: Light) -> i64 = match l {
    Red    => 0,
    Yellow => 0,
    Green  => 1,
}
```

---

**Q4.** In the arms — why `Red` and not `Light.Red`?

**A4.** Inside a `match`, the scrutinee's type is known; the arms speak in bare variant names. Outside, construction stays qualified.

---

**Q5.** Delete the `Green` arm. What happens?

**A5.** A compile error: the match no longer covers every variant. Add a variant next year, and a `match` that lists the old variants without a wildcard will need another arm.

---

**Q6.** `Red` and `Yellow` share an answer. Must we write two arms?

**A6.** One arm may hold both: `Red | Yellow => 0`. Or sweep the leftovers: `_ => 0` — but sweep knowingly; `_` also swallows variants not yet invented.

---

**Q7.** May a variant carry something?

```align
Shape { Circle(i64), Rect(i64, i64), Dot }
```

**A7.** Yes — a payload. `Shape.Circle(10)` is a circle *with its radius inside it*. `Shape.Dot` carries nothing.

---

**Q8.** In other languages, I would use class inheritance and a virtual `draw()` method to handle different shapes. Why does Align use sum types and `match`?

**A8.** A sum type lists the possible shapes. A `match` lists what this operation does for each shape, and the compiler checks that it covers them all. The variants and their payloads also give the compiler a known representation to work with.

---

**Q9.** How does the payload come back out?

**A9.** The match arm names it:

```align
fn area(s: Shape) -> i64 = match s {
    Circle(r)  => 3 * r * r,
    Rect(w, h) => w * h,
    Dot        => 0,
}
```

`area(Shape.Rect(3, 4))` is `12`. The names bound to payload values are available within that arm. We cannot directly read a circle's radius from a value that might be a rectangle.

---

**Q10.** What is `match` — a statement or an expression?

**A10.** An expression. Q9's whole function body *is* one. Bind it, return it, pass it: `verdict := match s { ... }`.

---

**Q11.** May we match on a number? `match n { 0 => ..., _ => ... }`

**A11.** No. Align's `match` examines variants of a sum type. Use `if` for numeric conditions.

---

**Q12.** What may a payload be?

**A12.** Scalars and structs: `Wrap(Point)` works, and `Wrap(p) => p.x + p.y` reads its payload. Owning payloads such as `string` work too. They make the sum type Move, and cleanup drops the payload of its active variant.

---

**Q13.** Model this: a fetched page is *loading*, *ready with a size*, or *failed with a code*.

**A13.**

```align
Page { Loading, Ready(i64), Failed(i64) }

fn describe(p: Page) -> i64 = match p {
    Loading   => 0,
    Ready(n)  => n,
    Failed(c) => -c,
}
```

The impossible states — ready *and* failed, a size with no page — cannot be written. That is what sum types are *for*.

---

**Q14.** Another one-of-many:

```align
Reading { Good(i64), Missing, Bad(i64) }
```

Write “use the good value; use zero for everything else.”

**A14.**

```align
fn value_or_zero(r: Reading) -> i64 = match r {
    Good(n) => n,
    Missing => 0,
    Bad(_)  => 0,
}
```

The two zeroes have different meanings even when they share an answer.

---

**Q15.** May we shorten the last two arms to `_ => 0`?

**A15.** Yes, and today the result is the same. But the explicit arms will force a decision if `Reading` later gains `Stale(i64)`. A wildcard buys brevity by giving up that future question.

---

**Q16.** Sum every good value in an array of readings.

**A16.**

```align
readings.map(value_or_zero).sum()
```

A sum type handles one element; a pipeline handles the many. The tools compose because each keeps its own job.

---

**Q17.** Count the bad readings instead.

**A17.**

```align
readings.where(fn r {
    match r {
        Good(_) => false,
        Missing => false,
        Bad(_)  => true,
    }
}).count()
```

The `match` produces the predicate's `bool`.

---

**Q18.** Could `match` in Q17 return `1` or `0`, followed by `sum`?

**A18.** Yes:

```align
readings.map(fn r {
    match r {
        Good(_) => 0,
        Missing => 0,
        Bad(_)  => 1,
    }
}).sum()
```

Same answer. Prefer `where(...).count()` when the thought is “which elements?”; prefer `map(...).sum()` when each variant contributes a quantity.

---

**Q19.** Add `Stale(i64)` to `Reading`. Which earlier code raises its hand?

**A19.** The matches that list the variants: `value_or_zero`, the predicate in Q17, and the function in Q18. Each needs a decision about the new case.

---

**Q20.** What question should we ask before inventing a sum type?

**A20.** “What impossible combination am I trying to make unwriteable?” If the answer is “a reading cannot be Good and Missing at once,” the variants are doing real modeling work. If the states can coexist, they may be fields instead.

---

> **The Sixth Commandment**
>
> *List mutually exclusive possibilities in a sum type. Handle them with `match`, and let the compiler check coverage.*
