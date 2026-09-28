# Modeling data: structs, sum types, match

> 🌐 **English** · [Japanese](./ja/03-modeling-data.md)

Align has exactly two ways to compose types — a struct ("all of these fields") and a sum type ("one of these variants") — plus tuples for the anonymous case. Type declarations are **keyword-less**: what's inside the braces decides which you wrote.

## Structs

```align
Point { x: i64, y: i64 }

fn main() -> i32 {
    mut p := Point { x: 3, y: 4 }
    p.y = 10                        // field write needs a `mut` binding
    print(p.x + p.y)                // 13
    return 0
}
```

`Name { field: Type, ... }` declares; `Name { field: value, ... }` constructs; `.field` reads. Plain-data structs are **Copy** values — assigning or passing one duplicates it, like an integer. Structs nest, and field paths go as deep as the data does:

```align
Point { x: i64, y: i64 }
Line  { a: Point, b: Point }

fn main() -> i32 {
    mut l := Line { a: Point{x: 1, y: 2}, b: Point{x: 3, y: 4} }
    l.a.x = 100                     // deep write
    l.b = Point { x: 30, y: 40 }    // replace a whole nested struct
    print(l.a.x + l.b.y)            // 140
    return 0
}
```

Structs pass and return by value:

```align
Point { x: i64, y: i64 }

fn sum(p: Point) -> i64 = p.x + p.y
fn flip(p: Point) -> Point = Point { x: p.y, y: p.x }

fn main() -> i32 {
    p := Point { x: 1, y: 9 }
    print(sum(flip(p)))     // 10
    return 0
}
```

Recursive structs (`Node { next: Node }`) are rejected — there is no null to terminate them with. A struct with an owning field (say `name: string`) is legal and turns the whole struct into a Move type; that story is chapter [05](05-memory.md).

## Fixed arrays inside records

Use `[T; N]` when the count is part of the data's shape:

```align
Table { weights: [i64; 4] }

fn main() -> i32 {
    mut table := Table { weights: [2, 3, 5, 7] }
    table.weights[1] = 11
    print(table.weights.sum())    // 25
    return 0
}
```

The four elements live inline in `Table`: no array header or heap allocation. `N` must be a decimal integer literal, not a variable or a const parameter, and the initializer must have exactly that many elements. A Copy-element array is Copy, so passing a large table by value copies its full contents; use `borrow` or a slice when the function only reads it (chapter [05](05-memory.md)). A slice such as `table.weights[1..3]` borrows the table's storage. Bind a returned table before indexing its array field so the storage has a named owner.

`[T; N]` has fixed length. Use `array<T>` for a dynamically sized owned collection. Nested fixed arrays and independently owned scalar elements such as `[string; 4]` are not supported; the complete element rules are in the [specification](../../draft.md#array).

## Sum types

A sum type lists variants; a variant may carry a payload:

```align
Shape { Circle(i64), Rect(i64, i64), Dot }

fn area(s: Shape) -> i64 = match s {
    Circle(r)  => 3 * r * r,
    Rect(w, h) => w * h,
    Dot        => 0,
}

fn main() -> i32 {
    print(area(Shape.Rect(3, 4)))   // 12
    print(area(Shape.Dot))          // 0
    return 0
}
```

Construction is qualified — `Shape.Rect(3, 4)`, `Shape.Dot` — so a reader always knows which type a variant belongs to. Payloads are positional. They may be scalars, borrowed `str` views, structs, sums, owned strings, or supported owned arrays. Owned payloads make the whole sum value a Move value; only the live arm is recursively dropped. Multiple owned payloads in one variant must share one allocation mode. Nested tagged payloads such as `Result<Option<T>, E>` work too, reusing the same one error model and one ownership model rather than a second mechanism.

## `match`

`match` is how you take a sum type apart, and it is an expression:

- Arms use the **bare** variant name: `Circle(r)`, not `Shape.Circle(r)`.
- Payloads bind positionally: `Rect(w, h) => w * h`.
- `A | B => ...` covers several variants with one arm (binding nothing).
- `_ => ...` covers the rest.
- **Exhaustiveness is mandatory.** Forget a variant and the program does not compile. This is the point: add a variant next month, and the compiler lists every `match` that needs a decision.

```align
Signal { Red, Yellow, Green, Off }

fn go(s: Signal) -> i64 = match s {
    Red | Yellow => 0,
    Green        => 1,
    _            => 0,      // Off
}

fn main() -> i32 {
    print(go(Signal.Green))     // 1
    return 0
}
```

### Integer, character, and string patterns

`match` also handles integer and `char` literals, inclusive ranges, and exact string literals:

```align
fn digit(c: char) -> bool = match c {
    '0'..='9' => true,
    _         => false,
}

fn bucket(n: i64) -> i64 = match n {
    -1 | 0 => 0,
    1..=9  => 1,
    _      => 2,
}

fn command(name: str) -> i64 = match name {
    "build" | "check" => 1,
    "run"             => 2,
    _                 => 0,
}
```

An integer match must cover the entire type's domain or include `_`. Character and string matches always require `_`. String matches compare decoded bytes exactly, including embedded NUL; they borrow `str` or `string` without a clone or allocation. Overlapping literal/range patterns are rejected. Guards (`Circle(r) if r > 10`), floating-point patterns, and string ranges are not supported. Use `if` for conditions that compute a predicate.

## Tuples

For "a pair of values" that doesn't deserve a named type:

```align
fn divmod(a: i64, b: i64) -> (i64, i64) = (a / b, a % b)

fn main() -> i32 {
    (q, r) := divmod(17, 5)     // destructure; use _ to skip a slot
    print(q * 10 + r)           // 32
    return 0
}
```

Construct with `(a, b)`, destructure with `(q, r) :=`, or index positionally: `t.0`, `t.1`. If you find yourself passing a tuple through more than one function, give it a name — a struct costs one line.

## The built-in `Error`

One sum type ships with the language: `Error`, the standard error payload of `Result<T, Error>`. Its variants are the categories the OS boundary needs — `NotFound`, `Invalid`, `Denied`, `Timeout` (a run/transport deadline), and `Code(i32)` for everything else. You cannot redeclare `Error`; you can match on it like any sum type:

```align
fn describe(e: Error) -> i64 = match e {
    NotFound => 1,
    Invalid  => 2,
    _        => 99,
}
```

How `Error` flows through programs — `?`, `main`'s exit code, your own error types — is the next chapter.
