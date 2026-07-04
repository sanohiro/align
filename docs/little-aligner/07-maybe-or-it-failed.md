# 7. Maybe, or It Failed

> 🌐 **English** · [Japanese](./ja/07-maybe-or-it-failed.md)

**Q1.** The list of even numbers in `[1, 3, 5]` — what is its first element?

**A1.** There isn't one. And "there isn't one" needs a type: `Option<i64>` — either `Some(n)` or `None`.

---

**Q2.** Is `None` the same as `null`?

**A2.** No, and the difference is everything. A `null` hides inside any reference, waiting. A `None` lives only inside an `Option`, and the type system will not hand you the `i64` until you have said what happens when it isn't there.

---

**Q3.** So how do we get the number out of `Some(5)`?

**A3.** Two doors. The quick one: `x := maybe else 0` — the payload, or your fallback. The thorough one: `match`, with `Some(n) =>` and `None =>`, exhaustive as ever.

---

**Q4.** What is `safe_head([1, 2, 3]) else -1`, if `safe_head` returns `Option<i64>`?

**A4.** `1`. And over the empty slice, `-1`. The caller chose the meaning of absence — the function didn't have to guess.

---

**Q5.** Now failure. What type does a function return when it tries to read a file?

**A5.** `Result<string, Error>` — either `Ok(contents)` or `Err(why)`. Failure is a *value*, with the reason inside it.

---

**Q6.** What is the difference between `Option` and `Result`?

**A6.** `None` is a normal answer ("no first even number" — fine). `Err` is a failure with a story (`NotFound`, `Denied`…). If absence is ordinary, `Option`. If someone may need to know *why*, `Result`.

---

**Q7.** Here is a function that can fail, calling another that can fail:

```align
fn load(path: str) -> Result<i64, Error> {
    data := fs.read_file(path)?
    return Ok(data.len())
}
```

What is the `?` doing?

**A7.** The whole error model, in one character: if `read_file` came back `Ok(s)`, then `data` is `s` and we continue; if `Err(e)`, we **return** `Err(e)` right now, to our caller. Unwrap or bail.

---

**Q8.** So who eventually *handles* the error?

**A8.** Whoever can. Each layer either passes it up (`?`) or sits down with a `match`. At the very top, `main() -> Result<(), Error>` turns an escaped `Err` into a non-zero exit code. The error travels as a value the whole way — no invisible unwinding, no catch-at-a-distance.

---

**Q9.** May we simply ignore a `Result` we don't care about?

```align
fs.write_file("log.txt", "hi")
```

**A9.** The compiler says no — *unhandled Result*, a hard error. Handle it (`?`, `match`, or bind it and decide). In Align you may fail, but you may not fail *silently*.

---

**Q10.** What is in the built-in `Error`?

**A10.** The categories the OS speaks — `NotFound`, `Invalid`, `Denied` — and `Code(n)` for the rest. `match` on it like any sum type (chapter 6 taught you how; `Error` is just a sum type with a badge).

---

**Q11.** My own error type, then — and `?` across the seam?

**A11.** Declare one (`ParseErr { Empty, BadChar }`) and convert **visibly** at the boundary:

```align
v := inner(n).map_err(to_error)?
```

`?` never converts types on its own; `map_err` shows the reader exactly where `ParseErr` became `Error`.

---

**Q12.** `else` on a `Result` — may we?

**A12.** No; `else` is Option's. A `Result` carries a reason, and reasons are for reading — `match` it, or pass it on with `?`. (Want a fallback anyway? Say it in two honest lines with a `match`.)

---

> **The Seventh Commandment**
>
> *Absence is `Option`; failure is `Result`. Pass failures up with `?`, and let no `Result` fall on the floor.*
