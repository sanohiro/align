# Errors: Option, Result, and `?`

> 🌐 **English** · [Japanese](./ja/04-errors.md)

Align has one error model. A computation that might not produce a value returns `Option<T>`; a computation that can *fail* returns `Result<T, E>`. There is no null, and there are no exceptions — an error is an ordinary value that travels back through `?`. This chapter is the whole model.

## `Option<T>` — maybe absent

```align
fn half_if_even(n: i64) -> Option<i64> {
    if n % 2 == 0 {
        return Some(n / 2)
    }
    return None
}

fn main() -> i32 {
    a := half_if_even(8) else 0    // Some(4) → 4
    b := half_if_even(7) else 0    // None    → the default
    print(a + b)                  // 4
    return 0
}
```

`Some(x)` and `None` construct; the **`else`-unwrap** consumes: `expr else default` gives you the payload or the default. The `else` arm can also diverge (`return`, or a call that aborts), which is how "unwrap or bail" looks. For anything richer, `match` on it — `Some(v) =>` / `None =>`, exhaustive like every match.

`half_if_even(0)` returns `Some(0)`, while `half_if_even(7)` returns `None`. After `else 0`, both give the same number. Use `match` if the caller must distinguish an absent value from a present zero. The `?` operator described below applies only to `Result`; it does not unwrap `Option`.

There is no null in the language, so there is no "forgot to check" — the type system won't give you a `T` until you've said what happens when there isn't one.

## `Result<T, E>` — can fail

```align
fn parse_positive(n: i64) -> Result<i64, Error> {
    if n <= 0 { return Err(Error.Invalid) }
    return Ok(n)
}

fn run(n: i64) -> Result<i64, Error> {
    v := parse_positive(n)?     // Ok(v) unwraps; Err returns early
    return Ok(v * 10)
}

fn report(r: Result<i64, Error>) -> i64 = match r {
    Ok(v)  => v,
    Err(_) => -1,
}

fn main() -> i32 {
    print(report(run(4)))       // 40
    print(report(run(-4)))      // -1
    return 0
}
```

`?` is the whole story of propagation: unwrap an `Ok`, or return the `Err` to the caller immediately. The happy path reads top to bottom; the error path is the cold edge, and the compiler literally lays it out as the cold branch. At the point where you finally *consume* a `Result`, you have two forms: `match` when the error's *content* matters, and `else` when it doesn't — `v := f() else fallback` takes the `Ok` value or deliberately discards the error for the fallback. Each intent has exactly one form: `?` propagates, `else` falls back, `match` inspects.

## Errors cannot be silently dropped

Discarding a `Result` is a **compile error**, not a lint:

```align
import std.fs

fn main() -> Result<(), Error> {
    fs.write_file("out.txt", "hi")     // error: unhandled Result
    return Ok(())
}
```

You have three moves, all visible in source:

```align
fs.write_file("out.txt", "hi")?                  // propagate
ok := fs.write_file("out.txt", "hi")             // bind it (and deal with it)
match fs.write_file("out.txt", "hi") {           // decide per case
    Ok(_)  => print(1),
    Err(_) => print(0),
}
```

## `main` returns `Result` — and the exit code follows

A program that can fail gives `main` the type `Result<(), Error>`:

```align
import std.fs

pub fn main(args: array<str>) -> Result<(), Error> {
    if args.len() != 2 { return Err(Error.Invalid) }
    data := fs.read_file(args[1])?      // ENOENT becomes Err(NotFound)
    print(data.len())
    return Ok(())
}
```

If an `Err` propagates out of `main`, the process exits non-zero — each `Error` category maps to a small fixed code (`NotFound` → 1, `Invalid` → 2, `Denied` → 3, `Timeout` → 4), and `Error.Code(c)` exits with `c`. `error(c)` is shorthand for constructing that carrier: `return Err(error(7))` exits with 7. No handler boilerplate at the top of `main`; the mapping is part of the language.

Programs receive command-line arguments through `main(args: array<str>)`; `args[1]` is the first user argument. This example requires one path argument and returns `Error.Invalid` for any other count. Checking before indexing prevents an out-of-bounds abort when no path was supplied. There is no global argument list or `env.args`.

## Your own error types

Any sum type can be an error. When the called function and its caller use the same error type, `?` propagates it directly; that type need not be the built-in `Error`. If they use different error types, convert explicitly with `map_err`. A `Result<T, ParseErr>` cannot propagate unchanged through a function returning `Result<T, Error>`:

```align
ParseErr { Empty, BadChar }

fn to_error(e: ParseErr) -> Error = match e {
    Empty   => Error.Invalid,
    BadChar => Error.Invalid,
}

fn inner(n: i64) -> Result<i64, ParseErr> {
    if n == 0 { return Err(ParseErr.Empty) }
    return Ok(n)
}

fn outer(n: i64) -> Result<i64, Error> {
    v := inner(n).map_err(to_error)?    // the conversion is visible at the call
    return Ok(v + 1)
}

fn show(r: Result<i64, Error>) -> i64 = match r {
    Ok(v)  => v,
    Err(_) => -1,
}

fn main() -> i32 {
    print(show(outer(9)))       // 10
    return 0
}
```

One rule keeps the model honest end to end: everything that can fail says so in its type, every failure is handled or visibly propagated, and nothing converts behind your back.

## The habit

Return `Option` when a value may be absent, `Result` when an operation can fail, and a plain `T` when no failure needs to be reported. At the call site, propagate a `Result` with `?`, supply a fallback with `else`, or inspect it with `match`. When `main` returns `Result<(), Error>`, an `Err` returned from `main` determines the process exit code.
