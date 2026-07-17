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

**A5.** A compile error: the match no longer covers every variant. **Exhaustiveness is the contract.** Add a variant next year, and every `match` in the program raises its hand.

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

**Q8.** In other languages, I would use class inheritance and a virtual `draw()` method to handle different shapes. Why does Align use `enum` and `match`?

**A8.** Inheritance scatters the logic across many files, and dynamic dispatch (virtual methods) hides what code actually runs from the CPU's branch predictor, destroying performance. `enum` and `match` gather the logic into one place and guarantee at compile-time that every case is handled. The CPU loves a clear path, and the reader loves the whole truth in one place.

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

`area(Shape.Rect(3, 4))` is `12`. The payload exists only where a pattern has caught it — there is no `.radius` to call on a maybe-Rect.

---

**Q10.** What is `match` — a statement or an expression?

**A10.** An expression. Q9's whole function body *is* one. Bind it, return it, pass it: `verdict := match s { ... }`.

---

**Q11.** May we match on a number? `match n { 0 => ..., _ => ... }`

**A11.** No. `match` is for one-of-many types; numbers take `if`. Two tools, each whole; no half-overlap to memorize.

---

**Q12.** What may a payload be?

**A12.** Scalars and plain structs — `Wrap(Point)` is fine, and the arm `Wrap(p) => p.x + p.y` reaches inside. (An owning payload like `string` is not accepted today.)

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

> **The Sixth Commandment**
>
> *When a thing is one of many, say the many. Then `match`, and let the compiler keep the list.*
