# Align Language Specification v0.1 (Summary)

A summary of `draft.md` (the authoritative detailed spec). For detail and the latest version, always refer to `draft.md`.

## Purpose

Align is an AOT-compiled language designed to align the following.

* Human intent
* AI-generated code
* Compiler optimization
* Modern hardware

What Align prioritizes:

```text
Less code
Predictable performance
Compiler-friendly design
Data-oriented programming
```

## Core principles

* One way to do the same thing
* No hidden allocation
* No hidden error
* No hidden parallelism
* Data-oriented by default
* Cache-friendly by default
* SIMD-friendly by default
* AI-friendly by default

## What it includes

### Types

```text
bool

i8 i16 i32 i64
u8 u16 u32 u64

f32 f64

char

str
string
bytes
buffer
builder

Option<T>
Result<T,E>

(T, U, ...)   // anonymous tuple; multi-value return = returning a tuple

array<T>
slice<T>

vec<N,T>
mask<T>
bitset
```

### Numeric conversion

No implicit coercion — not even widening. The explicit `as` operator is the **only** conversion,
between the numeric primitives (`i8..u64`, `f32`/`f64`) and `char`:

```align
b: i64 := a as i64        // widen (explicit); int→int truncates/extends with defined wrap
n := x as i32             // float → int truncates toward zero, saturating (no UB; NaN → 0)
code := 'A' as i32        // char ↔ int = the Unicode code point; char never pairs with a float
```

`bool` and composite types do not participate. Integer overflow is defined two's-complement wrap;
explicit `checked_*` / `saturating_*` / `wrapping_*` ops cover the rest. (`draft.md` §3.)

### Type declarations (keyword-less)

```text
User  { id: i64, name: str }              // struct (field: Type bodies)
Shape { Circle(f32), Rect(f32, f32) }     // sum type (variant bodies)
```

A sum type models variation (there is no class / inheritance). Construct with `Type.Variant`
(`Shape.Circle(3.0)`); branch with an exhaustive `match` expression (every variant covered or a
`_` wildcard — a missing variant is a compile error). Several variants share one arm with an
**or-pattern** `A | B` (bare variant names, binds nothing). `Option<T>` / `Result<T,E>` are sum
types; `match` works on them, with `else`-unwrap and `?` as the common-case shorthands.

```align
area := match s {
  Circle(r)  => 3.14159 * r * r,
  Rect(w, h) => w * h,
}
```

### Generics

A function may declare type parameters — `fn f<T>(...)` — and is **monomorphized** per distinct
concrete instantiation (zero run-time cost; a Move `T` moves, a Copy `T` copies). Type arguments
are **inferred** (from arguments or the expected type via the binding annotation) — no turbofish.
A bare type parameter is **opaque**: passed / returned / stored by value, with no operations of its
own (`x + x` on a bare `T` is rejected). A **builtin bound** grants capabilities — `fn f<T: Bound>`
— in a fixed `Num ⊃ Ord ⊃ Eq` hierarchy: `Num` = arithmetic+ordering+equality (numbers), `Ord` =
ordering+equality (numbers, `char`), `Eq` = equality (numbers, `char`, `bool`, `str`). A type
argument that does not satisfy the bound is a compile error. No user-defined trait bounds.

A type parameter may also appear nested in an `Option<T>` / `Result<T, E>` (parameter or return
position) — generic combinators like `fn unwrap_or<T>(o: Option<T>, d: T) -> T`. **Structs and sum types may
be generic** — `Pair<T> { a: T, b: T }`, `Opt<T> { Some(T), None }` — monomorphized per
instantiation, type arguments inferred from a struct literal's fields / a variant's payload
(`Pair { a: 1, b: 2 }`, `Opt.Some(7)`) or written as a type (`Pair<i32>`). (Nested in
`array<T>` / `box<T>` / a tuple, and using a generic def inside a generic function, are later slices.)

```align
fn id<T>(x: T) -> T = x                  // unconstrained: pass/return only
fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }
fn unwrap_or<T>(o: Option<T>, d: T) -> T = o else d
n := id(5)        // T = i32, monomorphized to id$i32
```

### Memory

```text
value types
arena
explicit heap
unsafe
```

### Error handling

```text
Result<T,E>
?
Error { NotFound, Invalid, Denied, Code(i32) }   // canonical builtin error sum type
```

No exceptions. `E` is any sum type (a domain may use its own error enum). `Error` is the builtin
error type — construct `Error.NotFound` / `Error.Code(c)` (`error(c)` is sugar), `match` it, and at
`main` it maps to the process exit code. Fallible builtins (`fs.read_file`, `json.decode`, …)
return `Result<T, Error>`. `?` requires the same `E` (no implicit conversion — convert explicitly
with `result.map_err(f)`). Error **context is structured, not free-form**: a variant carries the
relevant data (a position, a code), e.g. `ParseError { BadToken(Pos), Eof }` — there is no
`.with_context("…")` string-chaining.

### Data processing

```text
map
par_map
where
reduce
scan
partition
group_by
sort
sort_by_key
chunks
```

### Reduction

```text
sum
min
max
count
any
all
dot
```

Stages and reducers take a named function or an inline lambda `fn x { ... }` (parameter types
inferred). A lambda may capture enclosing variables by value — with no hidden closure
environment (it compiles like a named function, captures passed as arguments). `where(.active)`
is shorthand for a one-field lambda.

### Strings

```text
str
string
bytes
buffer
builder
```

### JSON

```text
json.scan
json.decode
json.encode
json.validate<T>
```

`decode`/`encode` take no written type argument — the target type comes from
context (`u: User := json.decode(d)?`) or the value argument; Align has no
expression-position type-argument syntax (no turbofish). `validate<T>` is the
residual schema-selector case (type in neither args nor result), still open.

### Templates

```text
template
html
json
raw
```

### Parallelism

```text
par_map
reduce
chunks
task_group
```

No async/await in v1.

### Safety

Normal code:

```text
safe
```

Dangerous operations:

```text
unsafe
```

Only inside an unsafe block.

## Core library

```text
core.option
core.result

core.array
core.slice

core.vec
core.mask
core.bitset

core.builder

core.json
core.template

core.hash
core.math

core.arena
```

## Standard library

```text
std.io
std.fs
std.path
std.process
std.env
std.time
std.net
std.cli
std.encoding
std.compress
std.rand
std.crypto
std.http
```

## Packages

```text
pkg.db.*
pkg.web.*
pkg.rpc.*
pkg.cloud.*
pkg.ai.*
```

Not part of the language core.
