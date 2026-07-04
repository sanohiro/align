# 11. There Is No Loop

> 🌐 **English** · [Japanese](./ja/11-there-is-no-loop.md)

**Q1.** Where is Align's `for`?

**A1.** There isn't one. No `while` either. You have lived ten chapters without noticing — the pipelines were the loops.

---

**Q2.** But some processes are truly step-by-step. Count down from 3 to 1?

**A2.** A function that calls itself:

```align
fn countdown(n: i64) -> i64 {
    if n == 0 { return 0 }
    print(n)
    return countdown(n - 1)
}
```

`3, 2, 1`. Recursion is the sequential tool.

---

**Q3.** What are the two parts every such function has?

**A3.** The **base case** (`n == 0` — stop) and the **step** (do a little, recurse on something smaller). Base first. Always base first.

---

**Q4.** Sum 1 to n, recursively?

**A4.**

```align
fn sum_to(n: i64, acc: i64) -> i64 {
    if n == 0 { return acc }
    return sum_to(n - 1, acc + n)
}
```

`sum_to(10, 0)` is `55`.

---

**Q5.** What is `acc` doing there?

**A5.** Carrying the answer-so-far *down* the calls, so the final call already holds the result. It is `reduce`'s accumulator, hand-rolled — chapter 4 in the mirror.

---

**Q6.** A thousand steps — a thousand stack frames?

**A6.** No. `return sum_to(...)` is a **tail call** — returning the recursion directly, nothing left to do after — and it compiles to a jump. Same frame, reused. Keep the recursion in tail position and depth is free.

---

**Q7.** And this one — tail call or not?

```align
fn sum_to(n: i64) -> i64 {
    if n == 0 { return 0 }
    return n + sum_to(n - 1)
}
```

**A7.** Not — the `n +` still waits after the call returns, so the frame must live on. Correct, but it stacks. The accumulator version does the `+` *before* the call; that is the whole trick of Q4.

---

**Q8.** Read a file in 4096-byte gulps until empty. Loop shape — so, recursion shape?

**A8.**

```align
fn pump(r: reader, w: writer, buf: buffer) -> Result<(), Error> {
    n := r.read(buf)?
    if n == 0 { return Ok(()) }
    w.write(buf.bytes())?
    return pump(r, w, buf)
}
```

Base case: EOF. Step: one gulp. Tail call: same buffer, next round. This is the canonical I/O loop of the standard library.

---

**Q9.** May `?` and recursion share a function, as in Q8?

**A9.** They just did. Any failure returns `Err` out of the *whole* descent at once — an early exit from the "loop", no `break` machinery required.

---

**Q10.** When do we recurse, and when do we pipeline? The final sorting hat:

**A10.**

- Same act on many elements → **pipeline** (chapters 2–5).
- Grouped folding → **`group_by`** (chapter 10).
- *Each step needs what the last step made* — parsers, state machines, gulp-until-empty → **recursion**, accumulator in hand, call in tail.

---

**Q11.** You are recursing over `i` from `0` to `len`, doing the same thing to `xs[i]` each time. What have you done?

**A11.** Written a `for` loop in a funny hat. Take it off — that one was `xs.map(...)` all along.

---

> **The Eleventh Commandment**
>
> *Base case first, accumulator in hand, call in tail. And before you recurse at all — ask if it wasn't a pipeline.*

---

**Q12.** Is that the end?

**A12.** Of the drills, yes. Of Align — [the guide](../guide/README.md) has the rest: strings, JSON, parallelism, SIMD, the standard library. But you no longer read Align like a foreign language. You read it like a menu.

*Go cook something.*
