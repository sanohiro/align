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

**Q6.** Why must mutability be announced at birth? Why can't I just change my mind later?

**A6.** Because mutability is not a feature of a variable; it is a declaration of intent. When you read a block of code, you must instantly know what stands still and what dances. If anything could dance at any time, you could never trust the floor.

---

**Q7.** What is the value of this block?

```align
{
    a := 3
    a * 2
}
```

**A7.** `6`. A block's value is its trailing expression. No `return` needed inside a block — the last expression *is* the answer.

---

**Q8.** What is `if 2 > 1 { "yes" } else { "no" }`?

**A8.** `"yes"`. `if` is an expression. It has a value, so you may bind it: `ans := if 2 > 1 { "yes" } else { "no" }`.

---

**Q9.** What does this function return, given `3`?

```align
fn square(x: i64) -> i64 = x * x
```

**A9.** `9`. The `= expr` form is a whole function body in one expression.

---

**Q10.** And this one, given `3`?

```align
fn cube(x: i64) -> i64 {
    return x * x * x
}
```

**A10.** `27`. A block body says `return`. Two body forms; there is no third.

---

**Q11.** What is `square(square(2))`?

**A11.** `16`. Functions compose. You knew that. We are warming up.

---

**Q12.** What does `print(7 / 2)` print?

**A12.** `3`. Integer division truncates toward zero.

---

**Q13.** What does `print(7 / 0)` do?

**A13.** It stops the program with an error — loudly, at that line. Never a quiet wrong number.

---

**Q14.** `x: i8 := 127`. What is `x + 1`?

**A14.** `-128`. Integer overflow wraps around, two's-complement. Defined, always, on purpose.

---

**Q15.** Is `[1, 2, 3]` a value?

**A15.** Yes — an array of three `i64`s, sitting contiguously in memory. Contiguous matters. It is the whole next four chapters.

---

**Q16.** What is `[1, 2, 3][0]`?

**A16.** `1`. Indexing starts at zero, and it is bounds-checked.

---

**Q17.** What is `[1, 2, 3][3]`?

**A17.** A runtime abort. Out of bounds is an error, not an adventure.

---

**Q18.** One more toy. What does this whole program print, and what does it exit with?

```align
fn main() -> i32 {
    print("ready")
    return 0
}
```

**A18.** It prints `ready` and exits with `0`. `main -> i32` is the C entry point; the return value is the exit code.

---

**Q19.** May a smaller block introduce a second `x` over an existing `x`?

```align
x := 1
{
    x := 2
    print(x)
}
```

**A19.** No. Align does not shadow names. Call the second thing what it is — `double`, `next`, `inner` — so that every `x` in sight means the same `x`.

---

**Q20.** Why forbid a convenience that many languages allow?

**A20.** Because reading should not require a search for the nearest declaration. A new binding gets a new name; a changing binding says `mut`. The source then answers both questions — *which value?* and *may it change?* — at the place you read it.

---

**Q21.** Do this one without running it:

```align
x := 4
y := if x % 2 == 0 { x / 2 } else { x * 3 + 1 }
print(y)
```

**A21.** `2`. The condition chooses an expression; the chosen expression becomes `y`.

---

**Q22.** Change only `x := 4` to `x := 5`. Now?

**A22.** `16`. Same program shape, other arm. Tiny changes are where reading becomes a habit.

---

**Q23.** Give that choice a name we can use twice.

**A23.**

```align
fn step(x: i64) -> i64 =
    if x % 2 == 0 { x / 2 } else { x * 3 + 1 }
```

Then `step(4)` is `2`, and `step(5)` is `16`. A function is a named transformation, not a place to hide state.

---

**Q24.** What is `step(step(5))`?

**A24.** `8`: first `16`, then half of `16`. Read from the inside out when calls nest.

---

**Q25.** Which line is the first error?

```align
x := 3
x = 4
y := [10, 20][2]
```

**A25.** `x = 4`. The missing `mut` is caught before the out-of-bounds access can ever run. Read compile-time promises before runtime behavior.

---

**Q26.** Repair it so that it compiles and prints `20`, changing as little as possible.

**A26.**

```align
mut x := 3
x = 4
y := [10, 20][1]
print(y)
```

Announce the changeable name; ask for an index that exists.

---

> **The First Commandment**
>
> *Bind with `:=`. Reassign with `=`, and only what is `mut`.*

Now we can play.
