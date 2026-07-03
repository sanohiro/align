# Your first program

The smallest Align program:

```align
fn main() -> i32 {
    return 0
}
```

`main` returning `i32` is the C entry point; the return value is the exit code.

## Printing

```align
fn main() -> i32 {
    print(42)
    return 0
}
```

`print` is a builtin. (In the current implementation it prints integers; richer output arrives with the string/IO work.)

## Values and inference

```align
fn main() -> i32 {
    x := 10
    y := x + 5
    return y
}
```

`:=` binds a new value. Types are inferred: `x` is an `i32` here because it flows into an `i32` return. An unconstrained integer literal defaults to `i64`. Bindings are immutable by default; add `mut` to reassign:

```align
fn main() -> i32 {
    mut total := 0
    total = total + 1
    return total
}
```

Note `:=` to introduce, `=` to reassign. Reassigning without `mut` is a compile error — visible mutability, One way.

## Errors as values

A program that can fail returns `Result`:

```align
fn main() -> Result<(), Error> {
    n := parse_count()?
    print(n)
    return Ok(())
}
```

The `?` operator unwraps an `Ok`, or returns the `Err` early — the cold path. There are no exceptions; an error is an ordinary value that travels back through `?`. When `main` returns `Result`, a non-zero exit code is produced from an `Err` automatically.

`Option<T>` is the same idea for "maybe absent," with no null:

```align
first := items.first()      // Option<T>
n := first else 0           // unwrap with a default
```

## Functions

```align
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

// single-expression form
fn square(x: i64) -> i64 = x * x
```

That is the whole surface you need to start. Next: stop writing loops.
