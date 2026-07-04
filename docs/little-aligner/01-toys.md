# 1. Toys

> 🌐 **English** · [Japanese](./ja/01-toys.md)

**Q1.** Is `42` a value?

**A1.** Yes. An `i64`, unless something nearby asks for a different width.

---

**Q2.** Is `42.0` the same value?

**A2.** No. That one is an `f64`. Align never mixes them behind your back.

---

**Q3.** What does `x := 42` do?

**A3.** It introduces `x`, bound to `42`. `:=` means *a new name is born*.

---

**Q4.** Then what does `x = 43` do, right after?

**A4.** It does not compile. `x` was not declared `mut`.

---

**Q5.** How do we say that `x` may change?

**A5.** `mut x := 42`. Then `x = 43` is welcome. Mutability is announced at birth, never discovered later.

---

**Q6.** What is the value of this block?

```align
{
    a := 3
    a * 2
}
```

**A6.** `6`. A block's value is its trailing expression. No `return` needed inside a block — the last expression *is* the answer.

---

**Q7.** What is `if 2 > 1 { "yes" } else { "no" }`?

**A7.** `"yes"`. `if` is an expression. It has a value, so you may bind it: `ans := if 2 > 1 { "yes" } else { "no" }`.

---

**Q8.** What does this function return, given `3`?

```align
fn square(x: i64) -> i64 = x * x
```

**A8.** `9`. The `= expr` form is a whole function body in one expression.

---

**Q9.** And this one, given `3`?

```align
fn cube(x: i64) -> i64 {
    return x * x * x
}
```

**A9.** `27`. A block body says `return`. Two body forms; there is no third.

---

**Q10.** What is `square(square(2))`?

**A10.** `16`. Functions compose. You knew that. We are warming up.

---

**Q11.** What does `print(7 / 2)` print?

**A11.** `3`. Integer division truncates toward zero.

---

**Q12.** What does `print(7 / 0)` do?

**A12.** It stops the program with an error — loudly, at that line. Never a quiet wrong number.

---

**Q13.** `x: i8 := 127`. What is `x + 1`?

**A13.** `-128`. Integer overflow wraps around, two's-complement. Defined, always, on purpose.

---

**Q14.** Is `[1, 2, 3]` a value?

**A14.** Yes — an array of three `i64`s, sitting contiguously in memory. Contiguous matters. It is the whole next four chapters.

---

**Q15.** What is `[1, 2, 3][0]`?

**A15.** `1`. Indexing starts at zero, and it is bounds-checked.

---

**Q16.** What is `[1, 2, 3][3]`?

**A16.** A runtime abort. Out of bounds is an error, not an adventure.

---

**Q17.** One more toy. What does this whole program print, and what does it exit with?

```align
fn main() -> i32 {
    print("ready")
    return 0
}
```

**A17.** It prints `ready` and exits with `0`. `main -> i32` is the C entry point; the return value is the exit code.

---

> **The First Commandment**
>
> *Bind with `:=`. Reassign with `=`, and only what is `mut`.*

Now we can play.
