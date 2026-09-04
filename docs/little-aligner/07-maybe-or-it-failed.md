# 7. Maybe, or It Failed

> 🌐 **English** · [Japanese](./ja/07-maybe-or-it-failed.md)

**Q1.** What do we find when we look for the first even number in `[1, 3, 5]`?

**A1.** There isn't one. And "there isn't one" needs a type: `Option<i64>` — either `Some(n)` or `None`.

---

**Q2.** Is `None` the same as `null`?

**A2.** `None` is a variant of `Option`, so the type tells you that a value may be absent. To get the `i64`, you must handle both possibilities: `Some` and `None`.

---

**Q3.** So how do we get the number out of `Some(5)`?

**A3.** To use a fallback, write `x := maybe else 0`: `x` gets the payload of `Some`, or `0` for `None`. To perform different work for each case, use `match` with `Some(n)` and `None` arms.

---

**Q4.** Suppose `safe_head` returns the first element as `Some`, or `None` for an empty slice. What is `safe_head([1, 2, 3]) else -1`?

**A4.** `1`. And over the empty slice, `-1`. The caller chose the meaning of absence — the function didn't have to guess.

We will write `safe_head` ourselves in Q20.

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

**A7.** If `read_file` returns `Ok(s)`, then `data` is `s` and we continue. If it returns `Err(e)`, we **return** `Err(e)` immediately to our caller.

---

**Q8.** So who eventually *handles* the error?

**A8.** Whoever can. Each layer either passes it up (`?`) or sits down with a `match`. At the very top, `main() -> Result<(), Error>` turns an escaped `Err` into a non-zero exit code. The error travels as a value the whole way — no invisible unwinding, no catch-at-a-distance.

---

**Q9.** May we simply ignore a `Result` we don't care about?

```align
fs.write_file("log.txt", "hi")
```

**A9.** The compiler says no — *unhandled Result*, a hard error. Use `?`, `match`, or `else`, or bind the result for later handling. Discarding the error with `else` is an explicit choice, as we will see in Q12.

---

**Q10.** What is in the built-in `Error`?

**A10.** The categories the OS speaks — `NotFound`, `Invalid`, `Denied`, `Timeout` — and `Code(n)` for the rest, where `n` is an `i32`. Five arms, so `match` on it like any sum type and name all five (chapter 6 taught you how; `Error` is just a sum type with a badge). Let one escape `main` and it becomes the exit status: `1`, `2`, `3`, `4` in that order, and `Code(c)` exits `c`.

---

**Q11.** How do we use our own error type with `?`?

**A11.** Declare one, such as `ParseErr { Empty, BadChar }`. If `inner` returns `Result<i64, ParseErr>` and its caller also uses `ParseErr` as its error type, use `inner(n)?` directly. Convert only when the caller needs a different error type, such as the built-in `Error`:

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

Only the parse result uses `else 0`. A file-reading error still returns to the caller.

---

**Q19.** Why not write `fs.read_file(path) else ""` too?

**A19.** You may only if “unreadable file means empty input” is truly the application's policy. `else` is not a shorter `?`; it answers a different question. Choose by meaning, never by punctuation count.

---

**Q20.** Now write `safe_head(xs: slice<i64>) -> Option<i64>`. What must we check before reading `xs[0]`?

**A20.** Whether there is an element to read:

```align
fn safe_head(xs: slice<i64>) -> Option<i64> {
    if xs.len() == 0 { return None }
    return Some(xs[0])
}
```

The empty case returns before indexing. A nonempty slice produces `Some` even when its first element is zero or negative.

---

**Q21.** Do these two `-1` results mean the same thing?

```align
found := safe_head([-1, 7])
missing := safe_head([])
print(found else -1)
print(missing else -1)
```

**A21.** No. `found` is `Some(-1)`; `missing` is `None`. The fallback makes them look alike. To keep the distinction, inspect the variant:

```align
print(match found { Some(_) => true, None => false })     // true
print(match missing { Some(_) => true, None => false })   // false
```

Use a fallback when this loss of information is acceptable to the caller.

---

**Q22.** We want to add one to the first element, or return `None` if the slice is empty. Can we unwrap `safe_head(xs)` with `?`?

**A22.** No. `?` applies only to `Result`. For `Option`, the `else` arm can return from the function:

```align
fn head_plus_one(xs: slice<i64>) -> Option<i64> {
    head := safe_head(xs) else { return None }
    return Some(head + 1)
}
```

`head_plus_one([4, 9])` returns `Some(5)`. An empty input returns `None` before the addition runs.

---

> **The Seventh Commandment**
>
> *Absence is `Option`; failure is `Result`. Pass failures up with `?`, and let no `Result` fall on the floor.*
