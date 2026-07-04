# Language basics

> 🌐 **English** · [Japanese](./ja/02-language-basics.md)

One sitting, the whole expression-oriented core: bindings, types, functions, control flow. After this chapter you can read any Align function.

## Bindings

```align
fn main() -> i32 {
    x := 10             // introduce a binding (immutable)
    y: i64 := 20        // with an explicit type annotation
    mut total := 0      // mutable — only `mut` bindings may be reassigned
    total = x + y       // reassign with `=`, not `:=`
    print(total)        // 30
    return 0
}
```

`:=` introduces, `=` reassigns. Reassigning a non-`mut` binding is a compile error — mutability is visible at the declaration, always. Types are inferred; annotate when you want a different type than inference would pick, or for documentation.

## Statements and lines

Align is Go-style: a newline ends a statement, and `;` exists only to cram two statements onto one line. Braces `{}` delimit blocks, so indentation carries no meaning. A line that *starts* with `.` or a binary operator continues the previous line — that is how long pipelines wrap:

```align
fn main() -> i32 {
    total := [1, 2, 3]
        .map(fn x { x * 2 })
        .sum()
    print(total)        // 12
    return 0
}
```

## The numeric types

Signed `i8 i16 i32 i64`, unsigned `u8 u16 u32 u64`, floats `f32 f64`, plus `bool` and `char` (a Unicode scalar, `'A'`, `'あ'`). An unconstrained integer literal defaults to `i64`; an unconstrained float literal to `f64`. There is **no implicit numeric conversion** — mixing widths is a type error, and you convert explicitly with `as`:

```align
fn main() -> i32 {
    x: i8 := 127
    y := x + 1          // i8 arithmetic
    print(y)            // -128 — overflow wraps, defined two's-complement
    big := 300
    b := big as i8      // explicit narrowing
    print(b)            // 44 (300 mod 256) — and the compiler warns: lossy conversion
    return 0
}
```

Two deliberate decisions live in that example:

- **Integer overflow wraps.** It is defined two's-complement behavior — never undefined behavior, never a hidden trap. When you *want* a checked/saturating operation, the spec provides explicit `checked_*` / `saturating_*` / `wrapping_*` forms so the intent is visible in source.
- **Narrowing is explicit and audited.** `as` truncation is defined behavior, and the compiler flags every lossy `as` with a warning so silent truncation can't hide.

Division by zero (and `%` by zero) is a hard runtime error — the program aborts; it is never a silent wrong answer. Out-of-range literals (`x: i8 := 200`) are a compile error.

## Everything is an expression

`if`, `match`, blocks — they all produce values. A block's value is its trailing expression:

```align
fn main() -> i32 {
    limit := 100
    fee := if limit > 50 { 10 } else { 25 }   // if is an expression
    x := {
        a := 3
        a * 2                                  // trailing expression = block's value
    }
    print(fee + x)                             // 16
    return 0
}
```

This is why Align needs no ternary operator and no separate statement/expression forms of `if`: there is one `if`, and it has a value when you use its value.

## Functions

```align
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

// single-expression form: `=` instead of a block
fn square(x: i64) -> i64 = x * x

fn main() -> i32 {
    print(add(square(3), 1))    // 10
    return 0
}
```

Two body forms, nothing else: a block with `return`, or `= expr` for single-expression functions. Parameters are immutable values. Small values are copied; what happens with owning types is chapter [05](05-memory.md).

## There is no loop keyword

Align has no `for`, no `while`. This is not an omission — it is the language's center of gravity. Iteration over data is a **pipeline** (`xs.map(f).where(p).sum()`, chapter [06](06-pipelines.md)), which the compiler fuses into a single vectorizable loop. For the rare genuinely sequential process, use **recursion**:

```align
fn sum_to(n: i64, acc: i64) -> i64 {
    if n == 0 { return acc }
    return sum_to(n - 1, acc + n)   // tail call — compiles to a jump, not a stack frame
}

fn main() -> i32 {
    print(sum_to(10, 0))    // 55
    return 0
}
```

When you feel the urge to write a loop, first ask what the *transformation* is — nine times out of ten it is a pipeline. The tenth time, write the recursion with an accumulator, as above.

## Named constants

```align
WIDTH: i32 := 6
HEIGHT: i32 := 7
AREA: i32 := WIDTH * HEIGHT     // folded at compile time

fn main() -> i32 = AREA         // exits 42
```

Top-level `NAME := expr` is a compile-time constant: keyword-less like everything in Align, immutable, folded and substituted at each use. Definition order doesn't matter (a constant may reference one defined later). An unannotated integer constant is `i64`.

## `print` and template strings

`print(x)` writes any primitive value and a newline: integers, floats (`1.0` prints as `1.0`, shortest round-trip), `bool` (`true`/`false`), `char`, and strings. For composing text there are **template strings**:

```align
fn main() -> i32 {
    name := "align"
    score := 40
    print(template "Hello {name}, score={score + 2}")   // Hello align, score=42
    return 0
}
```

The holes take full expressions. More on strings — including why there is a `builder` and when `+` is allowed — in chapter [07](07-strings-and-text.md).

---

That is the whole scalar core. Next: shaping data — structs, sum types, and `match`.
