# 11. Do It Until

> 🌐 **English** · [Japanese](./ja/11-do-it-until.md)

**Q1.** Where is Align's `for`?

**A1.** There isn't one. No `while` either. You have lived ten chapters without noticing — the pipelines were the loops.

---

**Q2.** If pipelines are so perfect, why give us a `loop` at all?

**A2.** Pipelines are for when data flows in a straight line, from a known start to a known end. But some problems—like finding a path in a graph, or polling a socket until it answers—are shaped like circles. `loop` exists for when the algorithm itself is a circle. But notice: even a `loop` in Align is an expression that yields a value. It is just a pipeline stage that had to tie its own shoes.

---

**Q3.** Show me this circle. Read a file in 4096-byte gulps until empty?

**A3.** The one loop the language has:

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

**Q4.** What did `break Ok(())` do there?

**A4.** Two things at once: ended the loop, and handed it a value. `loop` is an **expression** — like `if`, like `match`. The loop is the function's last expression, so the loop's value is the function's answer.

---

**Q5.** Sum everything a reader gives you, then?

**A5.**

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

**Q6.** May `?` and `loop` share a function, as in Q3?

**A6.** They just did. `?` exits the **function** — any failure returns `Err` out of the whole affair at once. `break` exits the **loop**. Two doors, clearly labeled, never the same door.

---

**Q7.** Where is `continue`?

**A7.** There isn't one. To skip to the next round, wrap the rest of the body in an `if`. To exit two loops at once — that inner loop wants to be a function. One door out: `break`.

---

**Q8.** You are counting `i` from `0` to `len` inside a `loop`, doing the same thing to `xs[i]` each round. What have you done?

**A8.** Written a `for` loop in a funny hat. Take it off — that one was `xs.map(...)` all along. The pipeline owns the *data*; `loop` owns the *control*. The compiler will say so too (it is a lint).

---

**Q9.** And recursion? The old books say functions calling themselves are the loops.

**A9.** Not here. A recursive "loop" spends a stack frame per round — Align promises no tail-call magic, and scope-end drops and `?` would quietly break such magic anyway. A thousand gulps must not cost a thousand frames. That is what `loop` is for.

---

**Q10.** Then what is recursion for?

**A10.** Problems *shaped* like recursion — the ones that nest. A parser inside a parenthesis inside a parser; a tree whose branches hold trees. When the **data** recurses, the function may too. Recursion is for the shape, never for the count.

---

**Q11.** When do we pipeline, when do we `loop`, when do we recurse? The final sorting hat:

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

**Q16.** The inner pipeline fails on one byte and uses `?`. Which boundary does it leave?

**A16.** The function, not merely the pipeline or loop. `?` keeps its one meaning everywhere. If the caller should receive the failure, that is exactly the right door.

---

**Q17.** A directory contains subdirectories, each containing more directories. Should a `loop` hold a manual stack?

**A17.** It can, but first recognize the recursive data shape. A recursive helper often says the structure more directly. Choose an explicit stack only when depth, memory bounds, or traversal order require that machinery.

---

> **The Eleventh Commandment**
>
> *Pipelines for the many, `loop` for the until, recursion for the nested. And before you loop at all — ask if it wasn't a pipeline.*

---

**Q18.** Is that the end?

**A18.** Of the control-path drills. Next we give independent work to many hands, then look down at four hardware lanes and ten gigabytes as one flat flow. After that, one last chapter will make us read the whole story at once.
