# 11. Do It Until

> 🌐 **English** · [Japanese](./ja/11-do-it-until.md)

**Q1.** Where is Align's `for`?

**A1.** There isn't one. No `while` either. You have lived ten chapters without noticing — the pipelines were the loops.

---

**Q2.** When do we need a `loop` instead of a pipeline?

**A2.** Use a pipeline to transform or reduce elements from a source. Use `loop` when the next step depends on the current state: read until EOF, retry an operation, or improve an estimate until it converges. A `loop` is also an expression: `break` can give it a value.

---

**Q3.** Read a file in 4096-byte blocks until EOF?

**A3.** The one loop the language has:

```align
fn pump(r: reader, w: writer) -> Result<(), Error> {
    mut buf := buffer(4096)
    loop {
        n := r.read(buf)?
        if n == 0 { break Ok(()) }
        w.write(buf.bytes())?
    }
}
```

`loop` repeats its block until a `break` runs. Until empty — then out.

---

**Q4.** What did `break Ok(())` do there?

**A4.** Two things at once: ended the loop, and handed it a value. `loop` is an **expression** — like `if`, like `match`. The loop is the function's last expression, so the loop's value is the function's answer.

---

**Q5.** Then how do we total the number of bytes read?

**A5.**

```align
mut buf := buffer(4096)
mut total := 0
sum := loop {
    n := r.read(buf)?
    if n == 0 { break total }
    total = total + n
}
```

The answer-so-far lives in a `mut` local declared *before* the loop; `break total` carries it out when the rounds are over.

---

**Q6.** May `?` and `loop` share a function, as in Q3?

**A6.** They just did. `?` exits the **function** — any failure returns `Err` out of the whole affair at once. `break` exits the **loop**. Two doors, clearly labeled, never the same door.

---

**Q7.** Where is `continue`?

**A7.** There isn't one. To skip to the next round, wrap the rest of the body in an `if`. To exit two loops at once — that inner loop wants to be a function. One door out: `break`.

---

**Q8.** You are counting `i` from `0` to `len` inside a `loop`, doing the same thing to `xs[i]` each round. What have you done?

**A8.** Written a `for` loop in a funny hat. Take it off — that one was `xs.map(...)` all along. The pipeline owns the *data*; `loop` owns the *control*. The specification calls for a lint that says so too — `alignc` does not raise it yet, so for now the habit is yours to keep.

---

**Q9.** And recursion? The old books say functions calling themselves are the loops.

**A9.** Align does not guarantee tail-call optimization, so each recursive call may need another stack frame. Scope-end cleanup and `?` also affect whether that optimization is possible. We do not need a thousand call frames just to repeat an operation a thousand times. Use `loop` for that.

---

**Q10.** Then what is recursion for?

**A10.** Problems *shaped* like recursion — the ones that nest. A parser inside a parenthesis inside a parser; a tree whose branches hold trees. When the **data** recurses, the function may too. Recursion is for the shape, never for the count.

---

**Q11.** When do we pipeline, when do we `loop`, when do we recurse?

**A11.**

- Same act on many elements → **pipeline** (chapters 2–5).
- Grouped folding → **`group_by`** (chapter 10).
- No knowing how many rounds until you are in them — gulp until empty, retry until it works, converge → **`loop`**, `break value` in hand.
- The data itself nests → **recursion**, for the shape.

---

**Q12.** Sort these without syntax:

- double every element of an array
- read blocks until EOF
- visit every branch of a tree
- total sales per region

**A12.** Pipeline; `loop`; recursion; `group_by`. Name the shape before choosing the tool.

---

**Q13.** This code counts from zero to `xs.len()` and adds `xs[i]`. Rewrite the thought.

**A13.** `xs.sum()`. If the loop's state is only an index and an accumulator, a reducer was probably waiting underneath it.

---

**Q14.** Now we repeatedly call `step(state)` until `state.done` becomes true. Pipeline?

**A14.** No known collection is flowing. The next state decides whether another round exists. Use `loop`, keep `state` in a `mut` binding, and `break` with the final value.

---

**Q15.** A loop reads blocks. For every block, every byte must be transformed. One tool or two?

**A15.** Two shapes, nested honestly: `loop` owns “until EOF”; a pipeline over the block owns “for each byte.” Control on the outside, data flow on the inside.

---

**Q16.** The function applies `?` to a failing pipeline's `Result`. Which boundary does that `?` leave?

**A16.** The enclosing function returns the error. Here `?` is applied to the pipeline's result in that function. A `?` inside a lambda would instead return from the lambda; it does not jump out of the function that called it.

---

**Q17.** A directory contains subdirectories, each containing more directories. Should a `loop` hold a manual stack?

**A17.** It can, but first recognize the recursive data shape. A recursive helper often says the structure more directly. Choose an explicit stack only when depth, memory bounds, or traversal order require that machinery.

---

> **The Eleventh Commandment**
>
> *Pipelines for the many, `loop` for the until, recursion for the nested. And before you loop at all — ask if it wasn't a pipeline.*

---

**Q18.** Is that the end?

**A18.** Of the control-path drills. Next come parallel work, SIMD, and a large-file example. The final chapter brings together the ways we have learned to read a program.
