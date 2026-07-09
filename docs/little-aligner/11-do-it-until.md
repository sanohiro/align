# 11. Do It Until

> 🌐 **English** · [Japanese](./ja/11-do-it-until.md)

> *The `loop` expression in this chapter is designed and specified, but the compiler is still
> catching up — **implementation in progress**. Be the compiler yourself for a few more weeks.*

**Q1.** Where is Align's `for`?

**A1.** There isn't one. No `while` either. You have lived ten chapters without noticing — the pipelines were the loops.

---

**Q2.** But some processes are truly step-by-step. Read a file in 4096-byte gulps until empty?

**A2.** The one loop the language has:

```align
fn pump(r: reader, w: writer, buf: buffer) -> Result<(), Error> {
    loop {
        n := r.read(buf)?
        if n == 0 { break Ok(()) }
        w.write(buf.bytes())?
    }
}
```

`loop` repeats its block until a `break` runs. Until empty — then out.

---

**Q3.** What did `break Ok(())` do there?

**A3.** Two things at once: ended the loop, and handed it a value. `loop` is an **expression** — like `if`, like `match`. The loop is the function's last expression, so the loop's value is the function's answer.

---

**Q4.** Sum everything a reader gives you, then?

**A4.**

```align
mut total := 0
sum := loop {
    n := r.read(buf)?
    if n == 0 { break total }
    total = total + n
}
```

The answer-so-far lives in a `mut` local declared *before* the loop; `break total` carries it out when the rounds are over.

---

**Q5.** May `?` and `loop` share a function, as in Q2?

**A5.** They just did. `?` exits the **function** — any failure returns `Err` out of the whole affair at once. `break` exits the **loop**. Two doors, clearly labeled, never the same door.

---

**Q6.** Where is `continue`?

**A6.** There isn't one. To skip to the next round, wrap the rest of the body in an `if`. To exit two loops at once — that inner loop wants to be a function. One door out: `break`.

---

**Q7.** You are counting `i` from `0` to `len` inside a `loop`, doing the same thing to `xs[i]` each round. What have you done?

**A7.** Written a `for` loop in a funny hat. Take it off — that one was `xs.map(...)` all along. The pipeline owns the *data*; `loop` owns the *control*. The compiler will say so too (it is a lint).

---

**Q8.** And recursion? The old books say functions calling themselves are the loops.

**A8.** Not here. A recursive "loop" spends a stack frame per round — Align promises no tail-call magic, and scope-end drops and `?` would quietly break such magic anyway. A thousand gulps must not cost a thousand frames. That is what `loop` is for.

---

**Q9.** Then what is recursion for?

**A9.** Problems *shaped* like recursion — the ones that nest. A parser inside a parenthesis inside a parser; a tree whose branches hold trees. When the **data** recurses, the function may too. Recursion is for the shape, never for the count.

---

**Q10.** When do we pipeline, when do we `loop`, when do we recurse? The final sorting hat:

**A10.**

- Same act on many elements → **pipeline** (chapters 2–5).
- Grouped folding → **`group_by`** (chapter 10).
- No knowing how many rounds until you are in them — gulp until empty, retry until it works, converge → **`loop`**, `break value` in hand.
- The data itself nests → **recursion**, for the shape.

---

> **The Eleventh Commandment**
>
> *Pipelines for the many, `loop` for the until, recursion for the nested. And before you loop at all — ask if it wasn't a pipeline.*

---

**Q11.** Is that the end?

**A11.** Of the drills, yes. Of Align — [the guide](../guide/README.md) has the rest: strings, JSON, parallelism, SIMD, the standard library. But you no longer read Align like a foreign language. You read it like a menu.

*Go cook something.*
