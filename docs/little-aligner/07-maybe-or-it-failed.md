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

**A9.** The compiler says no — *unhandled Result*, a hard error. Handle it (`?`, `match`, `else`, or bind it and decide). In Align you may fail, but you may not fail *silently*.

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

**A12.** Yes: `value := result else fallback`. It visibly discards the `Err` payload and uses the fallback. Do that only when the reason truly does not matter; use `match` when it does, or pass the failure on with `?`.

---

**Q13.** Three calls to `safe_head`:

```align
a := safe_head([7, 8]) else 0
b := safe_head([]) else 0
c := safe_head([]) else -1
```

What are they?

**A13.** `a` is `7`, `b` is `0`, `c` is `-1`. The producer reported only absence; each caller supplied its own meaning.

---

**Q14.** A missing optional nickname and a missing required input file: same type?

**A14.** No. Nickname → `Option<str>`; no nickname may be ordinary. Required file → `Result<string, Error>`; the reason may matter and the operation failed.

---

**Q15.** Trace the happy path:

```align
fn load_score(path: str) -> Result<i64, Error> {
    text := fs.read_file(path)?
    score := parse_score(text).map_err(to_error)?
    return Ok(score)
}
```

**A15.** `read_file` yields `Ok(text)`, `parse_score` yields `Ok(score)`, and the function wraps that score in `Ok` for its caller.

---

**Q16.** The file is missing. Does `parse_score` run?

**A16.** No. The first `?` returns the file's `Err` from `load_score` immediately. Later work is not half-done; it is not begun.

---

**Q17.** The file exists but contains bad text. Which error leaves the function?

**A17.** `parse_score`'s error, after `map_err(to_error)` visibly converts it to the function's `Error` type. The second `?` then passes that converted error upward.

---

**Q18.** We decide a malformed score should count as zero, but a missing file should still fail. Where does `else 0` go?

**A18.**

```align
text := fs.read_file(path)?
score := parse_score(text) else 0
return Ok(score)
```

Policy sits at the boundary it changes. File errors keep their story; parse errors are deliberately discarded.

---

**Q19.** Why not write `fs.read_file(path) else ""` too?

**A19.** You may only if “unreadable file means empty input” is truly the application's policy. `else` is not a shorter `?`; it answers a different question. Choose by meaning, never by punctuation count.

---

> **The Seventh Commandment**
>
> *Absence is `Option`; failure is `Result`. Pass failures up with `?`, and let no `Result` fall on the floor.*
