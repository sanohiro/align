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

The data-oriented core is built so the *shape* of ordinary code avoids the usual stalls: arrays and
slices are contiguous, so pipelines (`map` / `where` / `reduce`) walk memory **sequentially** (no
pointer-chasing, no random jumps), and a `where` / conditional reduction lowers **branchless** (a
mask + `select`, not a per-element `if`) so hot loops stay vectorizable and don't fight the branch
predictor.

**Build targets and portability.** The default build targets a **safe, portable, per-architecture
baseline** — `x86-64-v2` (SSE4.2) for amd64, `armv8-a` (NEON included) for arm64 — so one binary
runs across a varied, unknown fleet (cloud VMs, containers, feature-masked or migrated hosts).
Anything more is **opt-in, never the default**: `--target-cpu native` (fastest on the build host,
non-portable) and higher baselines (`x86-64-v3`/AVX2, …) for a fleet you control. Because
AOT-generated code is fixed at build time, **wide SIMD for a varied fleet comes from runtime
CPU-feature dispatch in the library layer** (one binary detects AVX2 / NEON at runtime and falls
back safely) — not from a fixed high baseline. Heavy SIMD work (JSON / UTF-8 / string scan, bulk
copy) lives in the library, written once with portable mechanisms (no per-architecture intrinsics),
covering x86-64 and aarch64. One good portable default; visible opt-in for more.

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

### Constants

A `:=` binding at the top level (outside any function) is a **named constant**: the same
keyword-less form as a local binding, but evaluated at compile time and substituted as a literal
wherever it is used. It is immutable — `mut` is not allowed at the top level.

```align
WIDTH: i32 := 6
HEIGHT: i32 := 7
AREA := WIDTH * HEIGHT      // folded at compile time
MAX_USERS := 1000
GREETING := "hello"
```

A constant's value is a scalar or string, computed from literals, unary/binary operators, and
other constants. Its type is **fixed at the definition** (a constant is stable across modules, so
it does not infer from a use site the way a local does): an unannotated integer defaults to `i64`
and a float to `f64`, so annotate when another width is wanted. `pub` exports a constant to
importing modules, where it is named qualified — `mod.NAME` — exactly like a `pub` function or
type. Division by zero, a cyclic definition, or a type mismatch is a compile-time error.

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

### Integer Literals

Integers are written in decimal, or with a base prefix: `0x` (hex), `0o` (octal), `0b` (binary). A
literal is the same value whatever the base, with its width inferred from context like any literal.
A `_` may separate digits for readability (in any base). When no context constrains it, an integer
literal defaults to `i64` and a float literal to `f64` — the default is deliberate and visible here
because it affects observable behavior (overflow width, float precision); annotate to pick another
width.

A *value* literal whose value provably does not fit the type it is given by context (`x: u8 := 300`,
an argument, a field initializer, an array element, a return value) is a **compile error**, not a
silent wrap: when both the value and the type are known at compile time, wrapping it would be hidden
data corruption ("nothing hidden"). This is a static check on literals only — runtime arithmetic
overflow still wraps by the rule below — and it is symmetric with rejecting a negative literal given
an unsigned type. Write the value in range, or convert the bit pattern explicitly with `as`
(`0xFFFFFFFF as i32`). A negated literal is checked at its effective value, so `-128` is a valid `i8`
though the positive `128` is not. (A too-wide literal in a `match` *pattern* is a separate case: it
truncates to the scrutinee's type by the defined wrap rule, since a pattern is a comparison, not a
stored value.)

```align
mask := 0xFF_FF            // hex, = 65535
flags := 0b1010            // binary
perm := 0o755              // octal
big := 1_000_000           // decimal with separators
```

### Integer Overflow

Integer arithmetic does **not** produce undefined behavior on overflow. The default is two's-complement wrap (identical across all builds, no branching).

```text
default      wrap (defined, zero-cost, does not block SIMD)
explicit op  checked_*(-> Option) / saturating_* / wrapping_*
development  overflow-checked build + lint for bug detection (semantics unchanged)
```

The reason is to commit fully to "predictably fast." Behavior is the same across all builds and does not break vectorization of hot loops. Where safety is needed, use the explicit ops.

Arithmetic errors other than overflow, such as division by zero, are handled separately; they are never silent and always produce an error — a runtime division/remainder by zero aborts (and a constant one is a compile error). The one signed division that could overflow, `INT_MIN / -1`, is not an exception to this: it **wraps** to `INT_MIN` (and `INT_MIN % -1` yields `0`), consistent with the defined two's-complement overflow above; only division by zero aborts.

Unary negation `-x` is a **signed** operation: applying it to an unsigned type (e.g. a `-5` literal given an unsigned type by context, `x: u32 := -5`) is a **compile error**, not a silent wrap — a negative value cannot have an unsigned type. Convert explicitly (`(-5) as u32`) if the wrapped bit pattern is actually wanted. (This is distinct from unsigned *subtraction* `a - b`, which is ordinary defined two's-complement wrap.)

### Numeric Conversion

There is **no implicit numeric coercion** — not even widening. A value changes type only through the explicit `as` operator, so every conversion is visible in source ("nothing hidden"):

```align
a: i32 := 300
b: i64 := a as i64        // widen — explicit, never implicit
n := b as i32             // narrow — defined two's-complement truncation
x := 3.9 as i32           // float → int — truncates toward zero
f := a as f32             // int → float
code := 'A' as i32        // char → its code point (a u32); int as char goes the other way
```

`as` is the language's **only** conversion, and applies only between the numeric primitives (`i8..u64`, `f32`/`f64`) and `char`. It is **zero-UB by design**, matching the overflow model above:

```text
int → int      truncate / sign- or zero-extend (defined wrap; sign from the source)
int → float    exact where representable, else nearest
float → float  widen / narrow
float → int    truncate toward zero, SATURATING (out-of-range clamps to MIN/MAX, NaN → 0)
char ↔ int     the Unicode code point (a u32); char never converts directly to/from a float
```

`bool` does not participate (use `if`); there is no conversion to or from struct, string, or other composite types.

### Bitwise and Shift Operators

Integers support the bitwise operators `&` `|` `^` and unary `~` (complement), and the shifts `<<` /
`>>`. They are **integer-only** — `bool` uses the logical `&&` / `||` / `!`, and there is no implicit
coercion, so the shift amount shares the value's type (`x << (n as i32)` to mix widths).

```align
flags := 0x1 | 0x4         // set bits
masked := flags & 0x6      // test bits
toggled := flags ^ 0x2     // flip a bit
high := value >> 8         // arithmetic shift on a signed value, logical on unsigned
inv := ~mask               // complement
```

Precedence follows the Go model: `<<` `>>` `&` bind like `*`, and `|` `^` like `+` — so **every
bitwise/shift operator binds tighter than a comparison** (`a & b == c` is `(a & b) == c`, avoiding
the classic C footgun). A shift amount is taken **modulo the value's bit width** (defined and
zero-cost — the same "no UB, predictable" stance as the overflow-wrap rule above; a constant shift
≥ the width is a lint), and `>>` is arithmetic (sign-extending) on a signed value, logical
(zero-filling) on an unsigned one. The higher-level `bitset` type (large bit sets, SIMD-friendly) is
a separate `core` type built on these.

The logical `&&` / `||` **short-circuit**: the right operand is evaluated only when the left does not
already decide the result (`a && b` skips `b` when `a` is false; `a || b` skips `b` when `a` is
true). This is what makes a guard like `i < xs.len() && xs[i] > 0` safe — the bounds-checked index
`xs[i]` is never evaluated when `i` is out of range. (The bitwise `&` / `|` are unconditional — they
always evaluate both integer operands; only the logical operators are lazy.)

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

A fallible entry point — `fn main() -> Result<(), E>` — requires `E` to be the builtin `Error`; the
exit-code mapping above is defined only for it. A user-defined error enum at that position is a
compile error today (it will be allowed once the full `Error` design settles the general
enum→exit-code mapping); propagate it with `map_err(to_error)?` to convert to `Error` at the
boundary.

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

A type parameter may also appear **nested** in an `Option<T>` / `Result<T, E>`, in a parameter or
return position — generic combinators like `fn unwrap_or<T>(o: Option<T>, d: T) -> T` or
`fn ok<T>(x: T) -> Result<T, Error>`:

```align
fn unwrap_or<T>(o: Option<T>, fallback: T) -> T = o else fallback
```

**Structs and sum types may be generic too** — `Pair<T> { a: T, b: T }`, `Opt<T> { Some(T), None }`
— and are monomorphized per concrete instantiation like generic functions. The type arguments are
inferred (no turbofish): from a struct literal's field values, or from a variant's payload; the
concrete form is also written as a type — `Pair<i32>` — for a parameter or annotation.

```align
Pair<T> { a: T, b: T }
Opt<T>  { Some(T), None }

p := Pair { a: 1, b: 2 }          // Pair<i32>, inferred from the fields
o := Opt.Some(7)                  // Opt<i32>, inferred from the payload
fn sum(q: Pair<i32>) -> i32 = q.a + q.b
```

A struct field must be a Copy type after substitution. A no-payload variant (`Opt.None`) has nothing
to infer its type from, so it needs a payload-bearing sibling to fix the type at construction.

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

A half-open **range** `start..end` slices instead of indexing: `xs[start..end]` borrows a
sub-view of a `str` (→ `str`) or an array / slice (→ `slice<T>`) — the same backing storage, no
allocation, region-tied to the source so it cannot outlive it. Either bound may be omitted —
`xs[start..]` runs to the length, `xs[..end]` from `0`, `xs[..]` is the whole thing. The bounds
`0 <= start <= end <= len` are checked at runtime; a violation aborts like an out-of-range index.
`..` is a slicing construct only — there is no first-class range value (the language has no
counting loops; iteration is the array pipeline).

For a `str` the bounds are **byte** offsets, and because a `str` is always valid UTF-8 (§12), each
bound must also land on a UTF-8 scalar boundary — a `start`/`end` that would split a multi-byte
scalar aborts at runtime, in the same style as an out-of-range index, so the resulting `str` is never
invalid. Arbitrary byte-range work belongs on the byte view instead (`s.bytes()[a..b]`, → `bytes`),
which carries no UTF-8 obligation.

```align
head := s[0..5]        // a borrowed sub-str
rest := xs[1..]        // a slice<T> view of the tail
```

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

SIMD comes in two layers. The **fixed vector types** `vecN<T>` / `maskN<T>` below are an escape hatch
for hand-written register kernels — they are **always a fixed size** (a `Copy` register value with a
constant width). Bulk vectorization — including future scalable ISAs (SVE/RVV) — instead belongs to
the **array pipeline** (`map` / `where` / `reduce`, §8), which **never names a width**: the same
source lowers to whatever the target supports (a fixed-width loop, or scalable predicated code), so a
vector length stays a hardware detail rather than something written in source.

### Fixed Vector Types

```align
vec2<f32>
vec4<f32>
vec8<i32>
vec16<u8>
```

A vector is built from an array literal under the annotation (the annotation picks the SIMD
representation — no separate constructor), elementwise `+` `-` `*` `/` `%` map to one lane-wise
hardware instruction each, and `v[i]` reads lane `i` (a constant index). Integer `/` and `%` carry
the same defined semantics as their scalar forms, applied per lane: a **zero divisor lane aborts**
(never a silent poison lane), and a signed `INT_MIN / -1` lane wraps to the two's-complement result.
Float `%` is the IEEE remainder (`frem`), with no guard.

```align
a: vec4<f32> := [1.0, 2.0, 3.0, 4.0]
b: vec4<f32> := [10.0, 20.0, 30.0, 40.0]
c := a + b                 // elementwise, one instruction
x := c[0]                  // lane 0
d := dot(a, b)             // reduction to a scalar
r := a.sqrt()              // elementwise float math: one vector instruction
f := fma(a, b, c)          // fused a*b + c, one rounding (one vfmadd/fmla)
```

The unary float math functions — `sqrt`, `abs`, `floor`, `ceil`, `round`, `trunc` — apply lane-wise
to a float vector (the same names as on a scalar float), each one lane-wise hardware instruction. The
element-wise `a.min(b)` / `a.max(b)` of two vectors, and `abs`, also work on integer vectors (`a.min()`
with no argument is the reduction instead). Each maps to one SIMD instruction; `pow` (a libcall) stays
scalar-only. `fma(a, b, c)` is the fused multiply-add `a*b + c` with a single rounding (a free builtin,
float scalar or vector) — the kernel of dot products, FIR filters, and Horner-method polynomials.

### Array Expressions

```align
a = b + c
a = (b + c) * d - e
```

Loop-fused without creating temporary arrays.

### Mask

Comparing two vectors elementwise yields a **mask** (one bool lane per vector lane), and
`select(mask, a, b)` blends two vectors lane-wise — lane `i` is `a[i]` where the mask is set, else
`b[i]`:

```align
m: mask4<i32> := a > b     // mask: one bool lane per comparison
hi := select(m, a, b)      // elementwise max (a where a > b, else b)
total := scores.sum_where(m)   // masked reduction
```

A mask has the type `maskN<T>` — spelled like `vecN<T>`, with the same width and element as the
vectors it compares (the type is usually inferred; name it to thread a mask through a function). A
mask is a first-class concept for SIMD / branchless / GPU. The pipeline's `where` is the implicit
form: `xs.where(p).sum()` lowers **branchless** (mask + `select`, a masked reduction), not a
per-element `if` — so a filtered hot loop stays vectorizable and does not fight the branch predictor.

### Memory Layout (`soa<T>`)

By default a collection is row-major (array-of-structs): `array<User>` stores each `User`
contiguously. For a large table processed field-wise, declare it column-major
(struct-of-arrays) with `soa<T>`:

```align
users: soa<User>
total := users.where(.active).pay.sum()
```

`soa<User>` stores one contiguous column per field, so a pipeline that touches only some fields
streams only those columns — better cache use and clean vectorization. The layout is **chosen
explicitly by the type** (not inferred behind your back): the choice is visible and performance is
predictable, while the field-wise lowering under the type is automatic. Crossing a byte-layout
boundary (FFI, `json`, by-value) materializes to AoS explicitly. Use `array<T>` by default; reach
for `soa<T>` on large, hot, field-wise-processed data.

**Field order within a struct is unspecified.** For a normal (non-`layout(C)`) struct the compiler
chooses the field order — it lays fields out in descending alignment to eliminate padding (so
`{ a: i8, b: i64, c: i8 }` occupies 16 bytes, not 24). Access is by name, so the reordering is
invisible in source and costs nothing; it packs hot structs tighter, which is a direct cache-density
win — the language's center of gravity. When you need a fixed, C-compatible byte layout (declaration
order, no reordering) — for FFI, `raw` memory, or any external byte-layout contract — mark the struct
`layout(C)` (see §15). That marker is the one escape hatch; everything else is the compiler's business.

Build one with `.to_soa()` (transpose a row-major struct array) or decode JSON into one:

```align
arena {
  users := rows.to_soa()                       // rows: array<User> (AoS) → soa<User> (columns)
  hot: soa<User> := json.decode(body)?         // JSON → a column-major soa<User> result
  total := hot.where(.active).pay.sum()        // streams only the active + pay columns
}
```

The column buffer is arena-allocated (the `soa` view borrows it), so both forms live inside an
`arena {}` — building once and then running several column scans amortizes the transpose. The JSON
form is the analytics win: idiomatic Rust decodes to a `Vec<User>` (AoS) and deserializes every
field, while Align produces a column-major `soa<User>` and a scan reads only the fields it touches.
(The guarantee is the **result** — a `soa<T>` laid out column-major; the decoder parses straight
into the columns, with no AoS intermediate or transpose pass.)

`json.decode` only parses the fields you declare — any other key in the input is skipped
structurally (no value conversion, no copy). To read just a few columns of a wide record, declare a
struct with only those: `soa<{ active: bool, pay: i32 }>` over a JSON object with twenty keys parses
two columns and skips the rest.

The field contract is **strict and exactly-once**: every declared field must appear exactly once in
the object — a missing declared field and a duplicate of a declared field are both `decode` errors
(`Result` `Err`, not a silent last-wins). Undeclared keys are the only thing skipped. This is the one
error model again: a malformed or unexpected shape surfaces as a value, never a silent partial decode.
Both decode paths enforce this — the strict fallback and the Mison speculative fast path alike (a
duplicate landing at a colon the learned pattern treats as unqueried is re-checked against the declared
set and rejected; see `open-questions.md`).

Grouped aggregation reads as a column pipeline too:

```align
totals := orders.group_by(.customer).sum(.amount)   // (array<Customer>, array<Amount>)
```

`group_by(.key).sum(.value)` yields two parallel arrays — the distinct keys and their per-key sums —
not a hash map; idiomatic Rust reaches for a generic `HashMap<K, Acc>`, while Align reads the two
columns sequentially into a primitive-key aggregate. An **integer** key over a `soa` runs as a
primitive-key open-addressing aggregate, and when the keys fall in a tight range it skips hashing
entirely (direct-index accumulation — ~5× a `std::HashMap`, beating even `ahash`). A **string** key is
**interned** to a dense id once, then aggregated by id — yielding `(array<str>, array<Acc>)` whose key
views borrow the source. This works both over an AoS `array<Struct>` (the key + value read from one
strided record) and over a `soa<Struct>` **str key column** (the key column and value column read as
two separate contiguous columns). The surface is identical either way (`group_by(.key).sum(.value)`);
the layout picks the runtime path. (First cut: an `i64` or `str` key over a `soa`, or a `str` key over
an `array<Struct>`, with an `i64` value and `sum`/`min`/`max`/`count`.)

This is the layout lever that lets Align *beat* an array-of-structs (what a hand-written `Vec<User>`
gives by default): a one-field scan over `soa<User>` reads only that column, where an AoS scan drags
whole structs through cache. Measured ≈7× faster than an idiomatic-Rust `Vec<Struct>` field sum on a
memory-bound workload (`bench/`, `col_sum`). *(Status: a borrowed `soa<T>` of a
primitive-scalar struct is implemented — field-column projection `ps.field`, mixed-width columns
(each padded to its alignment), a column-spanning `rs.where(.active).pay.sum()` (branchless, ≈3× an
AoS filtered aggregate), `.to_soa()` construction (transpose an `array<Struct>`, arena-allocated),
and decode-direct-to-`soa` (`s: soa<User> := json.decode(d)?`, parsed straight into columns) all
work, feeding the normal pipeline. A projected column is an ordinary `slice<FieldTy>`, so it
**windows** like any slice — `s.pay[a..b].sum()` scans rows `a..b` of one column. A field of one
element is **writable in place** through a `mut` view — `s[i].pay = v` stores one column (the write
counterpart of the `s[i].pay` read, the soa analogue of AoS `arr[i].pay = v`); a **whole element** is
replaceable too — `s[i] = value` scatters a struct value's fields into their columns (the write
counterpart of the `s[i]` gather / AoS `arr[i] = value`), for plain-data structs. A struct may also
carry **`str` columns** — a `str` field lands in a 16-byte `{ptr,len}` view column, whether decoded
(`json.decode`, the views borrowing the JSON input) or transposed (`.to_soa()`, the views borrowing
the source array), so a str-bearing `soa` is region-tied to that borrow (it cannot escape it) while
primitive columns still scan and reduce as usual; str columns are read-only (no `s[i].name = v`).
The plain column scan beats AoS ≈7×. The remaining slices are known-schema field-skip decode (parse
only the used columns), owned/nested columns, and a multi-column `soa_slice<T>` sub-view (`s[a..b]`
over every column).)*

### Over-alignment (`align(N)`)

A struct may declare an over-alignment with `align(N)` (a power of two), for SIMD / GPU / DMA /
page-aligned zero-copy interop:

```align
align(64) CacheLine {       // 64-byte aligned storage
  a: i64, b: i64, c: i64, d: i64
}

align(4096) Page { data: i64 }   // page-aligned
```

The attribute only ever *over*-aligns — it is the max of `N` and the type's natural alignment, so it
can never under-align. It changes the value's **storage alignment**, not its bytes or semantics.

The type's **size** is rounded up to `N` as well (`round_up(size, align)` — exactly as C does), so a
fixed array `[align(64) S]` has a tight, over-aligned element **stride**: from a `64`-aligned base,
each element stays on a `64`-byte boundary. (This is the general rule — an array element's stride is
always `round_up(element_size, element_align)` — made visible: `align(N)` is the only case that
raises it above the natural size.)

The same prefix over-aligns a **scalar array binding** — the aligned-vector-load enabler:

```align
align(64) data := [1, 2, 3, 4, 5, 6, 7, 8]   // storage on a 64-byte boundary
v: vec4<i64> := data[..].load(0)             // an aligned vector load
```

Here `align(N)` is a property of the *binding's storage*, not of a type (a scalar has no room for a
declared alignment), so it is written on the `:=`. It applies to a fixed array of a **numeric**
scalar (int/float) — the element a vector load can target (`int` covers every byte-buffer / DMA
case, `u8..u64`). A vector load of a whole borrow of such a binding is emitted as an *aligned* load
whenever the address is provably an `N`-multiple (from the `N`-aligned base, e.g. `load(0)`); any
other offset stays a plain element-aligned load — the alignment is never over-stated (that would be
undefined behavior).

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

A `str` (and an owned `string`) is **always valid UTF-8** — that is a type invariant, so every
operation that produces a `str` preserves it (a range slice that would split a scalar aborts, §7).
The byte types `bytes` / `buffer` carry **no** UTF-8 obligation and are where arbitrary-byte work
lives (`s.bytes()` views a `str`'s bytes without the invariant).

### No Implicit Concatenation Allocation

`str + str` is a **hard compile error** — `+` never concatenates strings. A concatenation is a heap
allocation, and an allocation must always be visible in source ("nothing hidden"); allowing `a + b`
to allocate silently would also be a second way to build a string when there is already one. So there
is exactly one way, and it is explicit:

```align
msg := a + b // compile error: `+` does not concatenate strings — use a builder
```

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

Dangerous operations are only in an `unsafe` block. The `raw.*` surface manages flat memory with a
`raw` byte pointer:

```align
unsafe {
  p := raw.alloc(16)        // 16 bytes → a `raw` pointer
  raw.store(p, 0, 42)       // write a primitive scalar at a byte offset (type from the value)
  x: i64 := raw.load(p, 0)  // read it back (type from the annotation — no turbofish, like decode)
  raw.free(p)               // manual free; a `raw` is Copy and never auto-dropped
}
```

The stored/loaded type is inferred (from the value for `store`, from the expected type for `load`) —
Align has **no turbofish**, so an explicit `raw.op<T>(...)` is not the surface. (An unchecked pointer
cast / reinterpret is a later `raw.*` op; the flat load/store above is the first cut.) A function
containing `unsafe` is inferred impure, so it can never be a `par_map` callee — the danger stays
visible and traceable.

### Foreign functions (FFI)

A C function is declared `extern "C"` and called like any other function — but only inside `unsafe`,
because foreign code is outside the safe core and can violate every invariant (ownership, no-alias,
non-null):

```align
extern "C" fn abs(x: i32) -> i32       // one declaration
extern "C" {                           // or a braced group
  fn sqrt(x: f64) -> f64
  fn memset(p: raw, c: i32, n: i64) -> raw
}

fn main() -> i32 {
  unsafe {
    p := raw.alloc(4)
    memset(p, 0, 4)        // hand the `raw` pointer straight to C
    raw.free(p)
    return abs(-7)         // 7 — a direct native call into libc
  }
}
```

The declaration is **bodyless** and bound to the C symbol `name` (never mangled). Because Align is
AOT-compiled via LLVM with no GC, a foreign call is a direct native `call` — no marshaling, pinning,
or stack-switch — and a `slice`/`str`/`raw` hands its pointer straight to C. Only `extern "C"` is
supported; the FFI-safe signature types are the primitive scalars (integers, floats) and `raw` (an
opaque byte pointer), plus a `()` (void) return. A function that calls an extern is inferred impure
(it contains `unsafe`), so it can never be a `par_map` callee. This is the keystone of the library
strategy: `std`/`pkg` **own the memory wrappers and borrow the mathematical engines** (wrapping
`libzstd`, `sqlite`, … via FFI) rather than reimplementing assembly-tuned algorithms in Align.

### Passing a view (`str` / `slice` / `bytes`)

An Align view is a `{ptr, len}` pair; as an extern **parameter** it lowers to just its **data
pointer** (a C `char*` / `void*`). The length is passed separately by the caller when the C function
needs it — matching the C `(ptr, len)` idiom, one Align argument becoming one C pointer argument:

```align
extern "C" fn write(fd: i32, buf: str, count: i64) -> i64

unsafe {
  msg := "hello\n"
  write(1, msg, msg.len())   // buf → char*, length passed explicitly
}
```

A view is **not** a valid return type (a bare C pointer carries no length — a C function returning a
pointer maps to `raw`). And an Align `str`/`slice` is **not NUL-terminated** (it is a length-bounded
view), so only hand it to length-based C functions (`memcmp`/`memcpy`/`write`/…); a NUL-terminated
API (`strlen`, `printf "%s"`) would read past the end — the programmer's `unsafe` responsibility.

### Linking an external library (`link("name")`)

libc and libm symbols resolve with no extra flag (they are always linked). Any *other* C library is
named by a `link("name")` clause on the extern block — the driver links `-lname`, and the dependency
is visible right at the declaration ("nothing hidden"):

```align
extern "C" link("z") {                 // links libz
  fn compress(dst: raw, dstlen: raw, src: str, srclen: i64) -> i32
}
```

A block names one library; several blocks may share or differ, and a repeated name links once. This
is the mechanism the library-wrapper strategy rides on — `std`/`pkg` own thin Align wrappers and
`link(...)` the mature C engines rather than reimplementing them.

### `layout(C)` — a C-compatible struct layout

A struct's memory layout is normally the compiler's own business — a non-`layout(C)` struct has an
**unspecified field order**, and the compiler reorders fields by descending alignment to eliminate
padding (§9). A `layout(C)` attribute pins it to a **stable, C-compatible flat layout** —
declaration order, natural alignment, no reordering — so the struct can cross the FFI boundary:

```align
layout(C) Point { x: i32, y: i32 }   // composes with align(N), in any order

unsafe {
  p := raw.alloc(8)
  raw.store(p, 0, Point { x: 30, y: 12 })   // write a struct into raw memory
  a: Point := raw.load(p, 0)                // …and read it back (or read one C wrote)
  raw.free(p)
}
```

Only a `layout(C)` struct may be moved through a `raw` pointer (`raw.store`/`raw.load` of a whole
struct), because only it promises a fixed representation — this is the pointer-based FFI pattern
(hand C a buffer, read/write structs in it). Its fields must be FFI-mappable scalars (integers,
floats).

### Not in FFI v1 (deliberate boundaries)

- **A struct by value.** A `layout(C)` struct crosses the boundary by **pointer** (above). Passing or
  returning one *by value* needs per-target register-classification (SysV eightbytes, AAPCS64, `byval`
  / `sret`) — the one FFI corner where a wrong rule silently miscompiles. It is deferred rather than
  shipped half-right; struct-by-pointer already covers the dominant C-API shape.
- **`bool` / `char` as FFI types.** Use the integer types, which map to C unambiguously: a C `_Bool`
  is a `u8` (0/1), a C `char` is an `i8`/`u8`, a `char32_t` is a `u32` (a `wchar_t` is platform-sized
  — pick the matching integer width). Align's
  `char` is a 32-bit Unicode scalar (**not** a C `char`), so admitting it as an FFI type would invite
  the wrong mapping; `bool` is kept out for the same "one unambiguous way" reason (and to avoid the
  `i1`-`zeroext` ABI subtlety).
- **A typed pointer cast (`raw.ptr_cast<T>`).** With one opaque pointer type (`raw`) a *typed*
  reinterpret has nothing to reinterpret to; it earns meaning only once FFI grows typed/external
  pointers, so it is deferred until then.

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
lossy conversion       (narrowing / float->int / wide-int->float / char narrowing `as`)
wasteful default type  (large literal array left to the i64/f64 default)
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

### Imports are a visible capability surface

A prefix-accessed library namespace must be `import`ed before use, so a file's header lists every capability it reaches ("nothing hidden"):

```align
import core.json
import std.fs

fn main() -> Result<(), Error> {
  data := fs.read_file("u.json")?     // needs `import std.fs`
  users := json.decode(data)?         // needs `import core.json`
}
```

Using `json.*` / `fs.*` / `io.stdout.write` without the matching `import` is a compile error; an `import` naming a module that does not exist is a compile error. The **language-syntactic** core — `Option` / `Result` / `?` / `else`, `arena`, the array pipeline (`map` / `where` / `reduce` / `sum` / …), the numeric methods (`x.abs()`, `a.min(b)`), `template "…"` — is always in scope and needs no import (requiring one would be requiring an import for syntax).

`core` is language-intrinsic and `std` is the OS boundary; both are compiler builtins today (std becomes real Align-over-FFI library code once FFI lands).

### User modules (multi-file)

A program spans multiple files. A non-entry file declares its module name and exports functions and types with `pub`:

```align
// geom.align
module geom

pub Point { x: i64, y: i64 }      // exported type
pub fn area(w: i64, h: i64) -> i64 = w * h
fn helper() -> i64 = 1            // private — not visible to importers
```

```align
// main.align
module main
import geom

fn main() -> i32 {
  p := geom.Point { x: 2, y: 3 }  // an exported type is named qualified
  return geom.area(p.x, p.y) as i32
}
```

`import geom` resolves by **filename convention** to `geom.align` in the entry file's directory (its `module` declaration must match the filename). A nested path follows the directory tree: `import util.math` → `util/math.align` declaring `module util.math`, called `util.math.fn(...)`. A cross-module reference is written qualified — `geom.area(...)` for a function, `geom.Point` for a type — and reaches only `pub` members; a bare name resolves within the calling module (so an imported type *must* be qualified). Each module has its own function and type namespace, so two modules may define a function or type with the same name.

An imported `pub` sum type's variant is constructed qualified, the same way its type is named:
`pal.Color.Green` (tag-only) and `pal.Color.Code(40)` (with a payload). Together with holding,
returning, and `match`ing it, an exported sum type is fully usable across modules.

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
vecN<T>      // vec2/vec4/vec8/vec16 (the width is in the name)
maskN<T>     // a comparison mask over a vecN<T>
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
rfind
find_any
split
trim
contains
starts_with
ends_with
eq_ignore_ascii_case
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

Hash for non-cryptographic use. One canonical mixer (`wyhash`) over a **byte view** — `str` or
`slice<u8>` (`bytes`). There is no `Hash` trait; you hash bytes, not arbitrary values.

```text
hash64(data)  -> u64           // data: str | slice<u8>
hash128(data) -> (u64, u64)    // 128-bit result as a tuple (no u128 type)
```

Deterministic for a given input within a build (fixed seed). **Non-cryptographic**: not
DoS-resistant, not a stable on-disk/wire format, not for security. Cryptographic hashes are in
std.crypto.

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
