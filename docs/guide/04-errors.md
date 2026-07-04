# Errors: Option, Result, and `?`

> 🌐 **English** · [Japanese](./ja/04-errors.md)

Align has one error model. A computation that might not produce a value returns `Option<T>`; a computation that can *fail* returns `Result<T, E>`. There is no null, and there are no exceptions — an error is an ordinary value that travels back through `?`. This chapter is the whole model.

## `Option<T>` — maybe absent

```align
fn find_even(xs: slice<i64>) -> Option<i64> {
    if xs.any(fn x { x % 2 == 0 }) {
        return Some(xs.where(fn x { x % 2 == 0 }).min())
    }
    return None
}

fn main() -> i32 {
    a := find_even([3, 8, 5, 4][0..4]) else 0    // Some(4) → 4
    b := find_even([3, 7, 5][0..3]) else 0       // None    → the default
    print(a + b)                                  // 4
    return 0
}
```

`Some(x)` and `None` construct; the **`else`-unwrap** consumes: `expr else default` gives you the payload or the default. The `else` arm can also diverge (`return`, or a call that aborts), which is how "unwrap or bail" looks. For anything richer, `match` on it — `Some(v) =>` / `None =>`, exhaustive like every match.

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

`?` is the whole story of propagation: unwrap an `Ok`, or return the `Err` to the caller immediately. The happy path reads top to bottom; the error path is the cold edge, and the compiler literally lays it out as the cold branch. At the point where you finally *consume* a `Result`, `match` on it. (`else` is Option-only — a `Result` carries an error you must look at or pass on, not paper over with a default.)

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
    data := fs.read_file(args[1])?      // ENOENT becomes Err(NotFound)
    print(data.len())
    return Ok(())
}
```

If an `Err` propagates out of `main`, the process exits non-zero — each `Error` category maps to a small fixed code (`NotFound` → 1, `Invalid` → 2, `Denied` → 3), and `Error.Code(c)` exits with `c`. `error(c)` is shorthand for constructing that carrier: `return Err(error(7))` exits with 7. No handler boilerplate at the top of `main`; the mapping is part of the language.

(That signature also shows how programs receive arguments: `main(args: array<str>)` is the only argv there is — `args[1]` is the first user argument. No global, no `env.args`.)

## Your own error types

Any sum type can be an error. But `?` never converts error types implicitly — a `Result<T, MyErr>` doesn't propagate through a function returning `Result<T, Error>`. Convert visibly with `map_err`:

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

Design functions so the caller can't misuse them: return `Option` when absence is normal, `Result` when failure is exceptional-but-real, and a plain `T` when it truly cannot fail. Then call everything with `?` and let `main`'s signature do the exit-code plumbing.
