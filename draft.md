# Align Language Specification Draft v0.1

## 1. Vision

Align is an AOT-compiled language where humans, AI, the compiler, and hardware can all face the same direction.

Its goal is:

> Write less. Predictably fast.

Align does not aim to be "a language for writing optimization tricks"; it aims to be "a language where ordinary code naturally takes a form the compiler and hardware can optimize."

---

## 2. Target Use Cases

Primary targets.

```text
CLI tools
batch processing
API server foundation
data processing
JSON / HTTP processing
compiler / parser / tooling
systems-adjacent applications
```

Non-primary targets.

```text
OS kernel
browser frontend
large OOP application
dynamic scripting
heavy GUI framework
```

---

## 3. Core Philosophy

### 3.1 One Way

The same thing is, in principle, written one way.

```text
error      = Result + ?
optional   = Option
memory     = value / arena / explicit heap
parallel   = map / reduce / chunks
string     = str / string / buffer / builder
```

### 3.2 Less Code, Fewer Bugs

Less code means fewer bugs.

However, the following are never hidden.

```text
allocation
error
side effect
parallelism
unsafe
```

### 3.3 Compiler Friendly

The design makes the following easy for the compiler to infer.

```text
contiguous memory
non-null
no-alias
arena lifetime
cold error path
pure-ish function
loop independence
alignment
```

### 3.4 Hardware Friendly

Make it easy to generate code that maps cleanly to modern CPU / GPU / Cache / SIMD / Branch Predictor.

---

## 4. Basic Syntax

### Variables

```align
x := 10
name := "sano"

mut count := 0
count = count + 1
```

The default is immutable.

### Type Annotation

```align
x: i64 := 10
```

### Function

```align
fn add(a: i32, b: i32) -> i32 {
  return a + b
}
```

A single-expression function is written in `= expr` form.

```align
fn add(a: i32, b: i32) -> i32 = a + b
```

### Statement Terminator (Go style)

A statement is terminated by a newline (Go style). Normally `;` is not written. Indentation is insignificant (blocks use `{}`); there is no Python-like layout enforcement.

```align
fn classify(u: User) -> str {
  s := score(u)
  if s > 80 { "high" } else { "low" }
}
```

`;` is an **optional separator**, used only to cram multiple statements onto one line. Because of `{}`, any block can be inlined onto one line (freedom of one-liners).

```align
fn classify(u: User) -> str { s := score(u); if s > 80 { "high" } else { "low" } }

point := { mut a := x; a = a * 2; a }
```

If a line begins with `.` or a binary operator, it continues the previous line. A chain can be written across multiple lines without `;`.

```align
total := users
  .where(.active)
  .score
  .sum()
```

### Block Value

An expression placed at the end of a block (with no statement following it) becomes that block's value. Expression statements not intended to be the value are simply listed as-is.

```align
fn abs(x: i32) -> i32 = if x < 0 { -x } else { x }

user := find_user(id) else return Error.NotFound
```

`if` / `else`-unwrap / `match` are expressions, and a single expression naturally fits on one line.

### Style and Convergence

The official formatter (§16) normalizes **only meaningless variation**.

```text
normalized       spacing / placement of ; / trailing comma / alignment
not normalized   one-line ↔ multi-line choice (the author's freedom is kept; no Python-like enforcement)
```

"One way to write" does not mean "one allowed layout" but "**one correct formatting for a given layout**" (same as gofmt / rustfmt). The per-form canonical shape — `= expr` for a single-expression body, a `{}` block for a multi-statement body — is preserved, while line packing is not enforced.

### Struct

```align
User {
  id: i64,
  name: str,
  active: bool,
  score: i32,
}
```

There is no class / inheritance.

### Sum Type

A type whose body is **variants** (not `field: Type`) is a sum type — the keyword-less companion of
the struct, disambiguated by content. This is how variation is modeled (there is no inheritance);
adding a variant turns every incomplete `match` into a compile error.

```align
Color { Red, Green, Blue }                  // tag-only
Shape { Circle(f32), Rect(f32, f32) }       // positional payloads
```

Construct a value by the qualified `Type.Variant`:

```align
c := Color.Red
s := Shape.Circle(3.0)
```

`Option<T>` and `Result<T, E>` are themselves sum types (`{ Some(T), None }` / `{ Ok(T), Err(E) }`).

### Match

`match` is an expression; every arm yields the match's value (or all arms diverge). Patterns are the
(unqualified) variants of the scrutinee, binding any payload positionally. **`match` must be
exhaustive** — cover every variant, or end with a `_` wildcard; a missing variant is a compile
error. Use `match` for variants and `if` for value conditions (one way each).

```align
area := match s {
  Circle(r)  => 3.14159 * r * r,
  Rect(w, h) => w * h,
}
```

Several variants may share one arm with an **or-pattern** — `A | B | ...`, matching if the
scrutinee is any of them. An or-pattern lists bare variant names and **binds nothing** (a
payload variant may appear; its payload is simply not bound). Use it for "match the shape, not
the data"; when you need a binding, write separate arms. Or-patterns count toward exhaustiveness
like any other arm, so they can partition all the variants with no `_`:

```align
warm := match signal {
  Red | Yellow => true,
  Green | Off  => false,
}
```

`match` also works on `Option` / `Result`; `else`-unwrap and `?` are the ergonomic shorthands for the
common unwrap / propagate cases.

---

## 5. Types

### Primitive Types

```text
bool

i8 i16 i32 i64
u8 u16 u32 u64

f32 f64

char
```

### Integer Overflow

Integer arithmetic does **not** produce undefined behavior on overflow. The default is two's-complement wrap (identical across all builds, no branching).

```text
default      wrap (defined, zero-cost, does not block SIMD)
explicit op  checked_*(-> Option) / saturating_* / wrapping_*
development  overflow-checked build + lint for bug detection (semantics unchanged)
```

The reason is to commit fully to "predictably fast." Behavior is the same across all builds and does not break vectorization of hot loops. Where safety is needed, use the explicit ops.

Arithmetic errors other than overflow, such as division by zero, are handled separately; they are never silent and always produce an error.

### Optional

```align
Option<User>
```

There is no null.

```align
user := find_user(id) else {
  return Error.NotFound
}
```

### Result

```align
Result<T, E>
```

```align
data := fs.read_file(path)?
user: User := json.decode(data)?
```

`?` is Result-only. The error type `E` is any sum type (or scalar) — a domain may define its own
error enum and use `Result<T, MyError>`. `?` requires the *same* `E` (there is no implicit error
conversion — that would be hidden); to change the error type, convert it explicitly with
`result.map_err(f)` (`f: fn(E) -> E'`), then `?`:

```align
v := inner().map_err(to_error)?
```

`Error` is the canonical builtin error sum type — universal categories plus a generic code:

```align
Error { NotFound, Invalid, Denied, Code(i32) }
```

Construct it with `Error.NotFound` / `Error.Code(c)` (`error(c)` is sugar for `Error.Code(c)`),
discriminate it with `match`, and at `main` it becomes the process exit code (`Code(c)` → `c`, a
category → a small distinct code). The standard fallible operations (`fs.read_file`, `json.decode`,
…) return `Result<T, Error>`.

**Context is structured, not free-form.** To attach context to an error — where it occurred, what
failed — give the error variant a payload that *carries that data*: a position, a code, a name.
There is no free-form string-chaining (`anyhow`-style `.with_context("…")`); the structured payload
is the context, which keeps errors data-oriented and machine-inspectable.

```align
Pos        { line: i32, col: i32 }
ParseError { BadToken(Pos), Eof }      // a parse error carries its position

fn parse(src: str) -> Result<Ast, ParseError> = …
```

### Tuple

An anonymous, positional product type `(T, U, ...)` — the companion of the keyword-less
named struct (a named struct for a domain type, a tuple for an ad-hoc "several things"
result). A function returns multiple values by returning a tuple; there is no separate
multiple-return mechanism.

```align
fn divmod(a: i32, b: i32) -> (i32, i32) = (a / b, a % b)

t := divmod(17, 5)        // a tuple value
(q, r) := t               // destructure; `_` ignores an element
first := t.0              // positional access: `t.0`, `t.1`, ...
```

`()` is unit and `(e)` is just grouping, so a tuple has arity ≥ 2. A tuple's ownership is
derived from its elements (Move if any element is Move; a tuple of views is region-tied to
their sources) — the same rule as a struct, no new ownership concept.

### Generics

A function may take **type parameters** in `<...>` — `fn f<T>(...)`. Each distinct use is compiled
to a concrete copy (**monomorphization**): there is one specialized instance per set of concrete
type arguments, so generic code costs nothing at run time and a type parameter behaves exactly like
the type it is instantiated with (a Move `T` moves; a Copy `T` copies).

```align
fn id<T>(x: T) -> T = x
fn pick<T>(a: T, b: T) -> T = a

n := id(5)                 // T = i32
p := pick(point, origin)   // T = Point
```

Type arguments are **inferred** — from the arguments, or from the expected type propagated to the
call (the binding annotation). There is no turbofish (`f<T>(x)` at a call); when a type cannot be
inferred, annotate the binding. A bare type parameter is **opaque**: it is passed, returned, and
stored by value, but it has no operations of its own — `x + x` on a bare `T` is a compile error,
because nothing says `T` is a number.

To use a type parameter in operations, give it a **builtin bound** — `fn f<T: Bound>`:

```align
fn add<T: Num>(a: T, b: T) -> T = a + b               // Num → arithmetic
fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }   // Ord → comparison
fn same<T: Eq>(a: T, b: T) -> bool = a == b           // Eq  → equality
```

The bounds are a small fixed hierarchy — **`Num` ⊃ `Ord` ⊃ `Eq`**: `Num` grants arithmetic,
ordering, and equality (the numeric types); `Ord` grants ordering and equality (numbers and
`char`); `Eq` grants equality (numbers, `char`, `bool`, `str`). A type argument that does not
satisfy a parameter's bound is a compile error at the call. There are **no user-defined
trait-style bounds** — deliberately, for AI-friendliness and *one way*.

---

## 6. Memory Model

### 6.1 Default

```text
no GC
value-type centric
heap is explicit
arena is standard
unsafe is isolated
```

### 6.2 Value

```align
p := Point{x: 1, y: 2}
```

Small structs are treated as values.

Copying large values is a lint target.

### 6.3 Move

Owning types are move by default.

```align
data := fs.read_file(path)?
other := data

print(data) // compile error
```

Explicit clone.

```align
other := data.clone()
```

### 6.4 Arena

```align
arena {
  data := fs.read_file(path)?
  users: array<User> := json.decode(data)?
  process(users)?
}
```

Allocations inside an arena are freed all at once when the block ends.

A view inside an arena cannot escape the arena.

### 6.5 Heap

```align
p := heap.new(User{id: 1})
```

In ordinary code there is no manual free.

Raw allocation is only in unsafe.

```align
unsafe {
  p := raw.alloc(size)
  raw.free(p)
}
```

---

## 7. Array and Slice

### Array

```align
users: array<User>
```

`array<T>` is owned contiguous memory.

### Slice

```align
items: slice<User>
```

`slice<T>` is a view.

### Index

```align
x := xs[i]
```

`xs[i]` reads element `i` of an array / slice / owned `array<T>`. The index is bounds-checked;
an out-of-range index is a hard runtime error (abort), never a silent out-of-bounds read.

A field of a struct-array element is read directly:

```align
name := users[i].name
```

### Out Parameter

```align
fn add(out dst: slice<f32>, a: slice<f32>, b: slice<f32>) {
  dst = a + b
}
```

`out` is a no-alias optimization hint and also a safety constraint.

---

## 8. Data Processing Core

The core of Align is array processing.

### Basic Operations

```align
scores := users.map(calc_score)
active := users.where(.active)
total := active.score.sum()
```

### Function Arguments

Every stage and reducer (`map` / `where` / `reduce` / `par_map` / `scan` / `partition` /
`any` / `all`) takes a function — either a named function, or an inline **lambda**
`fn params { body }`. Parameter types are inferred from the element type; the body is a block
whose trailing expression is its value.

```align
doubled := xs.map(fn x { x * 2 })
big     := xs.where(fn x { x > limit })   // captures `limit`
```

A lambda may **capture** enclosing variables (`limit` above). Capture is by value, and there is
**no hidden closure environment**: a lambda compiles exactly like a named function — captured
values are passed as ordinary arguments — so it fuses into the same loop and adds no allocation.
(Consistent with *Nothing hidden*: a lambda that escaped and outlived its captures would need a
visible heap environment; the in-loop case never does.)

A field selector is shorthand for a one-field lambda — `where(.active)` is `where(fn u { u.active })`,
and `.score` projects a field out of each element.

A function is also a **first-class value**: it can be bound (`f := fn x: i32 { x * 2 }`), reassigned,
and passed to another function through a `fn(T, U) -> R`-typed parameter — a **higher-order function**.
A function value's parameter types must be written when there is no use site to infer them (a bound
or passed lambda: `fn x: i32 { … }`); a stage lambda still infers them from the element type.

```align
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)
apply(fn n: i64 { n + base }, 5)   // a (capturing) closure passed as an argument
```

A passed closure's captured environment lives in the caller's frame for the duration of the call, so
no heap allocation is needed. A function value that *escapes* — returned from a function, stored
beyond the frame, or handed to `spawn` (§11) — needs a region-owned environment, owned by the
enclosing `arena {}` / `task_group {}` scope (never a hidden `malloc`); escape analysis chooses the
representation.

### Core Array Functions

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

### Reductions

```text
sum
min
max
count
any
all
dot
```

### Chunk Processing

```align
users
  .chunks(1024)
  .par_map(process_chunk)
```

The unit of parallelism is basically the chunk.

---

## 9. SIMD and Vector

### Fixed Vector Types

```align
vec2<f32>
vec4<f32>
vec8<i32>
vec16<u8>
```

Example.

```align
a: vec4<f32>
b: vec4<f32>
c := a + b
d := dot(a, b)
```

### Array Expressions

```align
a = b + c
a = (b + c) * d - e
```

Loop-fused without creating temporary arrays.

### Mask

```align
m := scores > 80
total := scores.sum_where(m)
```

A mask is a first-class concept for SIMD / branchless / GPU.

---

## 10. Branch and Hot Path

`if` exists.

But in bulk data processing, branches are not the center.

```align
active := users.where(.active)
total := active.score.sum()
```

The failure path of Result is made easy to treat as a cold path.

```align
data := fs.read_file(path)?
json := json.parse(data)?
```

---

## 11. Parallelism

### Philosophy

thread / mutex are not the normal way.

The basis is data parallelism.

```align
scores := users.par_map(calc_score)
```

### Side Effect Rule

A function passed to `par_map` cannot modify external mutable state.

Forbidden example.

```align
mut total := 0

users.par_map(fn u {
  total = total + u.score
})
```

Use reduce instead.

```align
total := users.reduce(0, fn acc, u {
  acc + u.score
})
```

### Task Group

I/O concurrency uses `task_group` — a **structured** scope (like `arena {}`): the tasks it
spawns cannot outlive it, and the scope joins them at its end.

```align
task_group {
  a := spawn(fn { fs.read_file("a.txt") })   // a deferred task; the `fn { }` makes the
  b := spawn(fn { fs.read_file("b.txt") })   // "runs as a separate task" visible
  wait()?                                     // join all; propagate the first error
  process(a.get(), b.get())                   // extract each result (after the join)
}
```

`spawn` takes a **lambda** (the deferred work), not a bare call — the deferral is then visible
in the source (*Nothing hidden*), and it is the same lambda mechanism as `map`/`reduce`/`par_map`
rather than a second, special-cased one (*One way*). It returns a `Task<R>` handle; `wait()?` is
the single error boundary (it joins every task and propagates the first `Err`), and `a.get()`
reads a task's result after the join.

Unlike a `par_map` lambda (which must be Pure), a spawned task **may** be impure — that is the
point: it performs I/O. Safety comes from capture being by value (a task shares no mutable state
with another) rather than from purity.

A spawned lambda *escapes* (it outlives the `spawn` call, running later on the task), so it is
represented as a first-class closure with an environment holding its captured values — distinct
from a pipeline lambda, which never escapes and is inlined. The compiler's escape analysis chooses
the representation, so pipelines stay allocation-free and SIMD/GPU-friendly while spawned tasks get
the environment they need. That environment is **owned by the `task_group` scope** (like an
`arena {}` allocation) and freed when the scope ends — not a hidden allocation, but a region one,
with the visible scope as its boundary.

async/await is not in the initial specification; structured `task_group` is the one concurrency
model for I/O.

---

## 12. String

### Types

```text
str      // read-only view
string   // owned string
bytes    // read-only byte view
buffer   // mutable byte buffer
builder  // append-oriented writer
```

### No Implicit Concatenation Allocation

```align
msg := a + b // string allocation is forbidden or linted
```

Recommended.

```align
b := builder()
b.write("hello ")
b.write(name)
msg := b.to_string()
```

### Static String Meta

String literals carry meta at compile time.

```text
len
hash
ascii
utf8_valid
json_escape_needed
html_escape_needed
```

On the surface they are treated as `str`.

### Const String Pool

The following can be placed in a const string pool.

```text
literal strings
JSON field names
template static parts
HTTP header names
```

### Scan Once

Do not scan the same byte sequence repeatedly.

The standard parser reuses scan results.

---

## 13. Template

Templates are analyzed at compile time, not parsed at runtime format time.

```align
msg := template "Hello {name}, score={score}"
```

Internally it expands to:

```text
write_static("Hello ")
write_value(name)
write_static(", score=")
write_value(score)
```

### Escaping Context

```align
html "<p>{name}</p>"
json "{name}"
```

Raw output is explicit.

```align
raw(name)
```

---

## 14. JSON

JSON is treated as a near-core feature of Align.

The reason is that all of Align's strengths come into play.

```text
SIMD scan
scan once
zero-copy
arena
typed decode
field table
builder encode
```

### Typed Decode

```align
user: User := json.decode(data)?
```

### Zero Copy

A decoded `str` / `array` / nested field is a view into the input buffer (no allocation),
region-tied to that input (see the memory model, §6, and `docs/impl/08-memory-model-v2.md`).

To make a decoded value outlive its input, the user clones it explicitly:

```align
first_name := arena {
  data := fs.read_file(path)?
  users: array<User> := json.decode(data)?   // views into `data`
  process(users)?                             // zero copy, cache-local
  users[0].name.clone()                       // explicit copy to escape the arena
}
// `first_name` outlives the arena; the views in `users` do not.
```

The compiler never silently inserts a copy on escape — allocation stays visible in source
("Nothing hidden") and the cost class stays predictable. A `.clone()` inside an arena is a
bump allocation (bulk-freed), so escaping is not a sudden heap cost.

### Struct as Schema

```align
User {
  id: i64,
  name: str,
  active: bool,
}
```

From a struct definition, the following can be generated.

```text
decode
encode
validate
field table
```

### Field Table

Field information is held at compile time.

```text
name
len
hash
first byte
offset
escape info
```

### SIMD Scan

The JSON scanner finds structural chars with SIMD.

```text
{ } [ ] : , " \ whitespace
```

---

## 15. Safety

In ordinary code the following are forbidden or restricted.

```text
use-after-free
uninitialized read
data race
manual free
raw pointer
unchecked cast
```

Dangerous operations are only in an `unsafe` block.

```align
unsafe {
  p := raw.ptr_cast<T>(x)
}
```

Rust-style lifetimes are not exposed on the surface.
However, an obvious lifetime violation, such as a view escaping an arena, is a compile error.

---

## 16. AI Friendly Rules

### Formatter

The official formatter is mandatory. It normalizes **only meaningless variation** such as spacing / placement of `;` / trailing comma / alignment, and does not enforce the one-line ↔ multi-line choice (§4 Style and Convergence).

### Lint

The standard lint detects:

```text
allocation in loop
huge struct copy
unnecessary clone
unnecessary heap
unhandled Result
branch in hot loop
string re-scan
implicit copy
```

### Convergence Over Expression

Convergence is valued over expressiveness.

Degrees of freedom that make AI hesitate are reduced.

---

## 17. Modules

```align
module main

import core.json
import std.fs
```

Exports are explicit.

```align
pub fn main(args: array<str>) -> Result<(), Error> {
}
```

---

# 18. Library Layout

## 18.1 core

`core` is the foundation, close to the language philosophy itself.

```text
core.option
core.result

core.array
core.slice
core.chunks

core.vec
core.mask
core.bitset

core.map
core.reduce
core.scan
core.partition
core.sort

core.str
core.string
core.bytes
core.buffer
core.builder

core.arena

core.json
core.template

core.hash
core.math
```

### core.array / core.slice

```text
array<T>
slice<T>
chunks
map
where
reduce
scan
partition
sort
group_by
```

### core.vec / core.mask

```text
vec<N,T>
mask<T>
bitset
dot
sum_where
select
```

### core.string

```text
str
string
bytes
buffer
builder
find
find_any
split
trim
contains
starts_with
ends_with
```

Has a SIMD fast path in the standard implementation.

### core.json

```text
json.scan
json.decode
json.encode
json.validate<T>
json.token
json.field_table<T>
```

`decode` and `encode` carry no written type argument: `decode`'s target is the
expected type from context (`u: User := json.decode(d)?`) and `encode`'s is the
type of its value argument — inference recovers both, so Align has no
expression-position type-argument syntax (no turbofish). `validate<T>` and
`field_table<T>` are the residual schema-selector case where `T` appears in
neither arguments nor result; their explicit-type surface is still open (they may
fold into `decode`).

### core.template

```text
template
html
json template
raw
```

### core.hash

Hash for non-cryptographic use.

```text
hash64
hash128
```

Cryptographic hashes are in std.crypto.

---

## 18.2 std

`std` is the boundary with the OS.

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

### std.io

```text
reader
writer
stream
stdin
stdout
stderr
```

### std.fs

```text
read_file
write_file
open
create
remove
exists
read_dir
```

### std.path

```text
join
base
dir
ext
normalize
```

### std.process

```text
spawn
exec
exit
```

### std.env

```text
args
get
set
```

### std.time

```text
now
instant
duration
sleep
```

### std.net

Low-level focused.

```text
tcp
udp
dns
socket
```

### std.cli

```text
args
flags
command
usage
```

### std.encoding

```text
base64
base64url
hex
utf8
```

### std.compress

```text
gzip
zstd
```

### std.rand

Non-cryptographic use.

```text
seed
range
shuffle
sample
```

### std.crypto

Cryptographic use.

```text
crypto.random
sha256
sha512
blake3
hmac
hkdf
argon2id
aes_gcm
chacha20_poly1305
constant_time_equal
```

### std.http

A primitive, not a framework.

```text
request
response
header
method
status
client
server primitive
```

---

## 18.3 pkg

`pkg` is the area for external packages.

```text
pkg.web
pkg.router
pkg.db.postgres
pkg.db.mysql
pkg.db.sqlite
pkg.orm
pkg.rpc
pkg.aws
pkg.openai
```

DB drivers and Web frameworks are not in core/std.

However, the building blocks that make them easy to build are placed in core/std.

```text
bytes
buffer
builder
arena
json
reader/writer
http primitive
crypto
encoding
```

---

# 19. Example

```align
module main

import core.json
import std.fs
import std.io

User {
  id: i64,
  name: str,
  active: bool,
  score: i32,
}

pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    data := fs.read_file(args[1])?
    users: array<User> := json.decode(data)?

    total := users
      .where(.active)
      .score
      .sum()

    out := builder()
    out.write("active score: ")
    out.write_int(total)
    out.write("\n")

    io.stdout.write(out)?
  }

  return ok
}
```

---

# 20. Positioning

```text
Allocation and errors are more visible than in Go
The normal path is safer than in Zig
Lifetimes are not written, unlike Rust
Alias and lifetime are clearer than in C
Faster than Python, and performance is less likely to degrade even when written by AI
```

---

# 21. One Sentence

Align is a data-oriented AOT language designed to align human intent, AI generation, compiler optimization, and modern hardware.
