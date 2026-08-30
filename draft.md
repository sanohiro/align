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

A `mut` binding may hold arena-owned storage on one path and individually heap-owned storage on
another, provided every assigned value outlives the binding's scope. The compiler tracks the
shortest possible region for escape safety and a separate path-local ownership bit for cleanup.
Reassignment drops an old heap value exactly once, never individually frees arena storage, and
transfers the selected ownership bit when the value moves.

A name binds **once** per scope chain: re-declaring a name already visible — in the same scope,
or shadowing an outer binding or a parameter — is a compile error. Rebinding hides a state change
from the reader; use `mut` for a value that changes, a new name for a new thing. Two *disjoint*
sibling blocks may each declare the same name (no point in the program sees two bindings).

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

An initializer may also be an **array literal** — an *aggregate constant*:

```align
PRIMES := [2, 3, 5, 7, 11]              // slice<i64>
SCALE: slice<f64> := [0.5, 1.0, 2.0]    // annotated element type
DAYS := ["Mon", "Tue", "Wed"]           // slice<str>
```

Its type is **`slice<T>`, not `array<T>`** — ownership is a property of the type, and a top-level
constant owns nothing: exactly as `GREETING := "hello"` is a `str` view of static bytes (not an
owned `string`), `PRIMES` is a `slice<i64>` view of a compile-time table. The elements live in a
per-unit read-only data section and the constant is a borrowed `{ptr, len}` view of them, so it is
shared (never copied) at every use and lives for the whole program. Because it is a `slice<T>`,
indexing, `.len()`, slicing, and pipelines (`map`/`where`/`reduce`/…) all flow through the normal
borrowed-view paths with no allocation. An `array<T>` annotation is rejected — write `slice<T>` or
omit the annotation.

A constant table is **read-only**: because it views the read-only data section, writing through it —
`TABLE[i] = v`, or passing it to an `out slice<T>` parameter (which the callee writes) — is a compile
error, even through a `mut` binding or a sub-slice (a `mut` binding rebinds the *view*, it does not
make the underlying storage writable). Copy it into an owned array first to modify. The same rule
covers a string literal's byte view (`"…".bytes()`). When a constant is `pub`, its value is part of
the exported interface (the initializer is shipped and re-folded in importing modules), so a `pub`
constant's initializer may reference only `pub` constants.

The element type is inferred from the elements (an unannotated `[1, 2, 3]` is `slice<i64>`) or
taken from a `slice<T>` annotation; every element must share it. Elements are scalars or `str`,
each folded by the same compile-time evaluation as a scalar constant (literals, unary/binary
operators, and references to *scalar* constants). A **constant index** folds to the element itself
(`PRIMES[2]` is `5`, with no load); a dynamic index reads the table. What is *not* yet allowed in an
element position is a function call, an `as` cast, a nested array, or a reference to another
aggregate constant; struct constants and struct elements are likewise deferred.

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

Function completion follows the declared return exactly. A Unit function may
fall through an empty/reachable block tail or use bare `return`. A non-Unit
function must return a value on every reachable path, either through a typed
tail expression or `return value`; bare `return` and reachable fallthrough are
compile errors. A path proven non-fallthrough (`return value`, the propagated
Err edge of `?`, or a diverging loop) needs no synthetic value.

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

An owned `array<string>` is a valid ordinary struct field. Its Drop walks the initialized strings
before freeing the array buffer. The shipped borrowed JSON schema still excludes it; the shipped
direct-owned JSON route in §18.1 admits it in a closed flat record. The accepted recursive owned
extension also admits it at any reachable field and inside an accepted record array. A finite Move
struct containing that field may be used in the existing `Option`, `Result`, and user-sum payload
positions; Drop still follows only the
active tag. Use `array<str>` when the strings are borrowed.

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

When the scrutinee is a stable place whose complete root/path pair has a direct shared or exclusive
borrow fact — either the borrowed parameter itself or a checked struct-field path below it — `match`
performs a read-only projection. A descendant field's borrow fact does not promote an owning parent
or a mixed-provenance local. The selected payload must be a Copy scalar/view, `string`, an ordinary
dynamic scalar/AoS-record array, or a finite acyclic struct, `Option`, `Result`, or user sum whose
reachable leaves recursively have those forms. The array element must satisfy the same closed
grammar. Fixed and specialized arrays, tuples, other collections, resources, opaque handles, and
other unsupported Move shapes keep the ordinary borrowed-place diagnostic. The match reads the tag
in place and binds an admitted active payload as a caller-owned borrow projection. The binding retains the payload's static type for field
and method checking, but it receives no independent `Drop` or cleanup bit, does not move or null the
source, and does not make a hidden aggregate copy. Copy fields can be read, and an owned `string`
field can be passed to a `str` consumer without moving it; that owned string leaf may use the existing
`.clone()` operation to create an owned copy. Aggregate and nested-sum clone operations are not part
of this capability. A borrowed arm binding is not a stable place for a nested borrowed `match`; that
use is rejected and never falls back to consuming extraction from the outer source. Returning,
storing, capturing, sending, or consuming the whole non-Copy/Move payload is rejected by the ordinary
borrowed-place diagnostics; existing Copy/view matching retains its
current result behavior. Views derived from the payload follow the existing inferred owner-generation
and region rules. A free-standing or otherwise owning scrutinee keeps the existing consuming match
behavior.

Ordinary indexing of an admitted `array<str>` or AoS array of Copy records with any admitted
region-bearing Copy field, including direct or nested `str` and `slice<T>` fields, keeps
those same owner-generation and input/arena roots through direct, field, and borrowed-projection
bases. Returned views and views retained into a `borrow mut` destination cannot outlive those roots.
If the index expression terminates, no bounds action or result is formed.

An explicit shared-`borrow` call may take one indexed Move element from an ordinary dynamic scalar
or AoS record array: `inspect(values[i])` is valid only when the selected direct, imported, or
function-value target's corresponding parameter is `borrow` and `values` is a stable local,
borrowed/projection binding, or struct-field path. The complete array root is reserved from the
once-only index evaluation through every later call argument and the call action: an operation that
might move, drop, replace, transfer, or mutably borrow an overlapping root is rejected, while
unrelated mutation remains valid. MIR performs the existing bounds check at the indexed argument's
source position only after the index falls through, retains no pointer while later arguments
evaluate, and forms the element pointer only after every later argument falls through and the root
is revalidated. A terminating index forms no guard, descriptor, later argument, pointer, or call; a
terminating later argument forms no pointer or call. The callee receives a pointer to caller-owned
element storage.
No element value, cleanup bit, allocation, or ownership transfer is created. A returned view or one
retained through an existing `borrow mut` destination summary remains rooted in the array generation
and contained region roots. By-value Move-element indexing, temporary-array bases, nested indexed
bases, and element `borrow mut` remain rejected.

### Loop

`loop` is the one sequential-control construct: it repeats its block until a `break` executes.
Like `if` / `match` / `arena`, it is an expression — `break expr` ends the loop and `expr` becomes
the loop's value (a bare `break` yields `()`). Every `break` in one loop must carry the same type,
exactly like `match` arms. A `loop` containing no `break` diverges (a server's accept loop): it
never yields, and like a `match` whose arms all diverge it satisfies any expected type.

```align
mut total := 0
mut buf := buffer(4096)
n_read := loop {
  n := r.read(buf)?
  if n == 0 { break total }
  total = total + n
}
```

The division of labor is strict: **the pipeline owns the data path; `loop` owns the control
path.** Traversing a collection is `map` / `where` / `reduce` — there is no `for`, no `while`, no
counting loop, and `loop` does not change that (walking an array by index inside a `loop` is a
lint, §16). `loop` exists for what a pipeline cannot express: iteration whose trip count is
decided by the iteration itself — reading until EOF, retrying with backoff, driving a protocol,
pumping a state machine to convergence.

Rules:

- `break` binds to the innermost enclosing `loop`, and must appear lexically inside one in the
  same function. A lambda body is its own function — `break` cannot cross it.
- A `break` lexically inside an `arena {}` or `task_group {}` **nested in the loop** is rejected
  today (unwinding the scoped region on the break edge is a separate slice). Restructure so the
  region ends before the `break`.
- There is no `continue` and no labeled break. Skip-to-next-iteration is an `if` around the rest
  of the body; a nested loop needing a two-level exit is a function waiting to be extracted.
- `?` and `return` inside a loop behave as everywhere else: they exit the **function**, not the
  loop. `break` is the only loop exit.
- Loop-carried state lives in `mut` locals declared before the loop. Locals declared inside the
  body are per-iteration — they drop at the end of each pass. A `break` value therefore must not
  borrow from a per-iteration local (the block-value escape rule applies); moving an owned
  per-iteration value out through `break` is fine.
- **Recursion is not iteration.** Functions may recurse (a parser, a tree walk), but Align
  guarantees no tail-call optimization — scope-end drops and `?` make tail position fragile — so
  a recursive "loop" costs stack proportional to the trip count. Iteration is the pipeline for
  data and `loop` for control, never recursion. (Rationale: `design-notes.md`.)

### print and Value Display

`print(x)` is the builtin output primitive: it writes one value and a newline to stdout. It
accepts **primitive values only** — integers, floats, `bool`, `char`, `str`/`string`. Printing an
aggregate (struct / tuple / array / sum value) is a compile error: compose text explicitly with a
template string (§13) or a `builder` (§12) — a magic deep-formatter would be a hidden recursive
walk.

The display of each printable type is a language contract — identical in every build and on every
target, and shared by `print` and template-string interpolation:

```text
integers     decimal, - sign for negatives
floats       shortest round-trip decimal (1.0 prints as 1.0; reparsing yields the same bits)
bool         true / false
char         the character itself (UTF-8)
str/string   the bytes verbatim (no quotes, no escaping)
```

`print` is Impure (it is I/O — effect inference, §11).

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

### Float Semantics

Floats are **IEEE 754** (`f32`/`f64`) and float arithmetic is total: `x / 0.0` is `±inf`,
`0.0 / 0.0` is NaN, overflow rounds to `±inf`, and NaN propagates through arithmetic. No float
operation aborts — the mirror image of the integer rules above, for the same reason: defined,
zero-cost semantics that never block vectorization (a per-lane guard would). Scalar `%` on floats
is IEEE remainder (`frem`), unguarded, like its vector form (§9).

Comparison follows IEEE: NaN compares unequal to everything **including itself** (`x == x` is
`false` iff `x` is NaN), and `<` / `<=` / `>` / `>=` are all `false` when either side is NaN. The
`min`/`max` reducers' NaN policy is specified with the reducers (§8). Only conversion leaves pure
IEEE behavior: `as` saturates and maps NaN to `0` (next section).

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

### Equality and Ordering

`==` / `!=` are defined for **scalars and strings only**: numbers, `bool`, `char`, and
`str`/`string` (byte equality; a `str` and a `string` with the same bytes are equal). There is
**no structural equality** — comparing structs, tuples, arrays/slices, or sum values with `==` is
a compile error. A struct comparison is written out field by field (the cost stays visible); a
sum value is tested with `match` (one way: `match` = variants, `if` = conditions); arrays are
compared element-wise as a pipeline. So `==` is always a flat comparison — string bytes are the
one visible-length case — never a hidden recursive walk, and generic `Eq` (§ Generics) stays
exactly the scalar hierarchy it declares.

Ordering (`<` `<=` `>` `>=`) follows the same shape: defined for numbers, `char`, and — the same
one visible-length case — `str`/`string`, whose order is **byte-lexicographic** (for valid UTF-8
this equals Unicode scalar order). It is deterministic and locale-free; dictionary/locale
collation is a library concern (`pkg`), never the operator. A `sort_by_key` key is a **Copy** `Ord`
value: a number, a `char`, or a borrowed `str`. An owned `string` key is **not** supported today —
it satisfies `Ord`, so the checker accepts it, but the program is then rejected at the MIR boundary
as an internal error rather than with a user diagnostic (the fused sort path has no per-key Drop;
the deferral is recorded in `docs/impl/19-hir-validation-ledger.md`). Project the key to a `str`.
Aggregates have no order, exactly as they have no `==`.

The operator and builtin-bound paths compare owned `string` values through a non-consuming,
zero-cost `str` borrow. Mixed `string`/`str` operands use the same path; comparison neither moves
nor allocates.

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

`else` — the unwrap-with-fallback form (§ Optional) — works on a `Result` too: `v := f() else
fallback` yields the `Ok` value, or evaluates the fallback and **discards the error value**. The
discard is deliberate and visible — this form says "the reason does not matter here" — and it
still counts as handling (the unhandled-`Result` error never fires on it). `else` binds no error
variable; needing the error *is* the signal to `match`. So each intent has exactly one form, for
`Option` and `Result` alike: **`?` propagates, `else` falls back, `match` inspects.**

`Error` is the canonical builtin error sum type — universal categories plus a generic code:

```align
Error { NotFound, Invalid, Denied, Timeout, Code(i32) }
```

Construct it with `Error.NotFound` / `Error.Code(c)`, discriminate it with `match`, and at `main` it
becomes the process exit code (`Code(c)` → `c`, a category → a small distinct code). `error(c)` is
syntax sugar for `core.Error.Code(c)` and therefore always constructs the builtin even in a module
that declares a local `Error`. `Timeout` is the category for a run/transport deadline (a
`std.process` `run` that overran its `timeout_ns`, or — once shipped — a `std.http`/`std.net` I/O
timeout); it is produced only by an explicit timeout site, never inferred from an errno. The standard fallible operations (`fs.read_file`, `json.decode`,
…) return `Result<T, Error>`.

The compiler-provided nominal aliases form this closed table:

| Bare alias | Explicit builtin spelling | Availability of the explicit spelling |
|---|---|---|
| `Error` | `core.Error` | language-syntactic core; always in scope and requires no import |
| `argon2_params` | `crypto.argon2_params` | requires `import std.crypto` |
| `rs256_private_key` | `crypto.rs256_private_key` | requires `import std.crypto` |
| `rs256_public_key` | `crypto.rs256_public_key` | requires `import std.crypto` |
| `es256_private_key` | `crypto.es256_private_key` | requires `import std.crypto` |
| `es256_public_key` | `crypto.es256_public_key` | requires `import std.crypto` |
| `ed25519_private_key` | `crypto.ed25519_private_key` | requires `import std.crypto` |
| `ed25519_public_key` | `crypto.ed25519_public_key` | requires `import std.crypto` |
| `regex_match` | `regex.regex_match` | requires `import std.regex` |

A non-entry module may declare a local type with any bare alias in this table. Inside that module
the bare name denotes the local type; in a module without such a declaration the bare alias still
denotes the builtin. The explicit spelling always denotes the builtin. A `std` explicit spelling
counts as a use of its required import for the unused-import lint; `core.Error` is part of the
always-in-scope syntactic core, and `import core` is neither required nor valid. Importers name the
local type by its ordinary qualified module path, for example `pkg.db.Error`. The entry module
cannot declare any table member because its unmangled type namespace is also the builtin namespace.
This is type namespacing, not value shadowing.

A fallible entry point — `fn main() -> Result<(), E>` — requires `E` to be the builtin `Error`; the
exit-code mapping above is defined only for it. A user-defined error enum at that position is a
compile error; propagate it with `map_err(to_error)?` to convert to `Error` at the boundary.

The complete entry signature is closed. A no-argument `main` returns only `()`, exact `i32`, or
`Result<(), Error>`. `main(args: array<str>)` returns exactly `Result<(), Error>`. Unit and Result
forms use a generated C `main` wrapper returning `i32`; Unit maps to exit 0 and Result uses the
mapping above. The exact-i32 no-argument form is the C entry directly. Every other return or
parameter shape is a compile error rather than a non-C-ABI external `main`.

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
ordering, and equality (the numeric types); `Ord` grants ordering and equality (numbers, `char`,
and `str`/`string` — byte-lexicographic, § Equality and Ordering); `Eq` grants equality (numbers,
`char`, `bool`, `str`/`string`). A type argument that does not
satisfy a parameter's bound is a compile error at the call. There are **no user-defined
trait-style bounds** — deliberately, for AI-friendliness and *one way*.

`RegionPlain` and `SoaPlain` are the two separate closed structural bounds. Neither is part of the
numeric hierarchy or grants arithmetic/equality operations. `RegionPlain` admits recursively plain
values for region-backed construction. `SoaPlain` is narrower: it admits exactly a nonempty struct
whose fields are primitive integer/float/`bool`/`char` scalars or `str`, and grants only symbolic
`soa<T>` formation at a generic boundary.

Concrete `string` and `str` both satisfy `Eq` and `Ord`; an owned value is borrowed, not moved, by
the comparison inside a monomorphized function.

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

Type parameters compose through the ordinary package types that use them. Inside a generic
function, `T` may appear under `Option`, `Result`, `array`, `slice`, or an application of a
top-level generic struct, sum, or resource definition:

```align
fn collect<P, R: RegionPlain>(
  q: Query<P, R>,
  p: P,
  out: region,
) -> Result<array<R>, Error>
```

`RegionPlain` is a closed builtin structural bound, not a user-defined trait. It admits recursively
plain scalars/views/structs for region-backed construction and rejects resources, independently
owned fields, raw/function values, and builders. Nested applications remain symbolic while the
template is checked, then are fully substituted and monomorphized before Move/escape analysis and
MIR. This adds no runtime dictionary, reflection, turbofish, or definition nested inside a
function, and does not make a concrete container element legal when that container otherwise
rejects it.

`soa<T>` may likewise remain symbolic only as `soa<R>` inside a template that declares
`R: SoaPlain`. A public template interface preserves that canonical symbolic application and bound
for separate compilation. Concrete substitution must satisfy the ordinary `soa<T>` field rule and
happens before Move/escape analysis and emitted HIR/MIR; no abstract SoA or runtime type test
survives monomorphization.

After substitution, a struct field may be Copy or recursively Move when its finite, non-recursive
Drop plan is known. Move fields participate in the aggregate's one-owner cleanup bit and allocation
mode; an unsupported container element remains illegal merely because it appears behind a generic
field. A no-payload variant (`Opt.None`) has nothing to infer its type from, so it needs a
payload-bearing sibling to fix the type at construction.

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

Value-carrying control flow preserves two independent facts. The inferred **region** is a
conservative lifetime bound used to reject escapes. An owned value also carries an internal
path-local **cleanup bit**: set for individually owned storage, clear for arena-owned, moved, or
uninitialized storage. A control-flow join selects that bit on the same edge that selects the value;
it is not recomputed from the joined region.

| Value form | Result region | Owned move / drop behavior |
|---|---|---|
| `{ ...; value }` | The trailing value's region. | Moves the trailing owned value and forwards its cleanup bit; the moved source is cleared. |
| `if c { a } else { b }` | The shorter of the continuing arms' regions. | Each arm stores its value and cleanup bit into the join; only the selected pair reaches the consumer. |
| `match x { ... }` | The shortest region among the continuing arm values. | An owning scrutinee transfers a selected Move payload and clears its source; a borrowed-place scrutinee binds a read-only projection with no independent cleanup bit. Each selected arm forwards its result bit under the existing join rules. |
| `opt else fallback` | The shorter of the `Some`/`Ok` payload and fallback regions. | `Some`/`Ok` moves the payload and clears the container; the fallback moves normally. Their bits join with the value. |
| `result?` | The `Ok` payload's region. | `Ok` moves the payload and its bit, clearing the input; `Err` drops live individually owned locals, closes regions, and returns early. |
| `loop { ... break v }` | `Static` — every accepted `break` value obeys the return-escape rule, so the loop value never borrows a per-iteration (or enclosing-arena) local. | Each `break` moves its value and forwards its cleanup bit; per-iteration owned locals are dropped at the back edge and at every `break`, with the moved-out break value nulled first. |

The table is exhaustive for value-carrying control syntax except `task_group`, whose trailing-value
transfer is specified in the ownership-bit paragraph below and in §11. Adding another form requires
choosing both columns and adding the corresponding regression cells.

**One restriction applies to the `if` row today.** A value-carrying `if`/`else` **expression** cannot
move an already-bound owned local out of an arm:

```align
a := "hello".clone()
c := if n > 2 { a } else { "hi".clone() }   // compile error
```

The rejection belongs to the `if`-expression arm, not to the consumer: the binding above, an
argument position (`f(if n > 2 { a } else { … })`), and `return if n > 2 { a } else { … }` are all
rejected identically, with `cannot move owned value '<name>' out through a conditional expression
yet`. Every sibling form moves a bound owned local normally — a `match` arm, an `else`-unwrap
fallback, a block tail, and a statement-form `if` + `return` — and an `if` expression whose arms
build fresh temporaries is fine. Use one of those, or bind the arms' shared value before the `if`.

A by-value function argument is an ownership transfer for a Move type. Only a free-standing owned
value may cross that boundary: the callee owns and drops it. An arena-owned value remains tied to
the caller's arena and cannot be passed by value; pass a non-owning slice/view instead. This rule
applies recursively to tuples, structs, and sum-type payloads. A call evaluates all arguments before
transferring them, so an early exit while evaluating a later argument leaves every earlier owned
member under caller cleanup. Synthesized calls obey the same boundary: `Result.map_err` cannot pass
an arena-owned Move error to its mapper. Pipeline functions cannot receive a Move source element
by value or produce a Move element until the pipeline has explicit per-iteration cleanup; project
or produce a Copy or borrowed value instead. `reduce` and materializing `scan` require a Copy
accumulator until per-iteration transfer and every error cleanup path are explicit. `map_err`
forwards the selected `Ok`/`Err` allocation bit with its rebuilt result, retains a fresh receiver
while the mapper expression is evaluated, and includes mapper-capture borrows in the result region.
A Move value leaving a value-carrying scope such as `task_group` transfers the trailing local's
ownership bit and clears that inner source before the outer return or call takes ownership.

A function may explicitly borrow a value instead of taking ownership:

```align
fn inspect(borrow c: Conn) -> i64
fn advance(borrow mut rows: Rows) -> Option<Row>
```

Shared `borrow` applies to a stable bound Copy or Move place. Copy already passes without consuming
ownership, but an explicit shared borrow also prevents a potentially large aggregate copy and is
the typed ABI for producer-generated callbacks. It does not make a temporary addressable. `borrow`
leaves the argument owned by the caller. The callee cannot move, replace, or drop it; a view returned
from the call is inferred to borrow the exact caller-side owner generation. `borrow mut`
accepts a writable Move or Copy place, has exclusive access for the call, and ends the previous
storage generation. This is required for a Copy state aggregate whose field mutation must update the
caller rather than a discarded copy. Older views become invalid; a returned view belongs to the
fresh generation. Parameter modes are part of named-function and function-value signatures, while
lifetime roots remain inferred — there are no written lifetime parameters. A concrete function
value retains inferred return-borrow and return-region parameter/capture sets. Target-relative
capture roots travel with the selected closure environment; assignment/control-flow joins
conservatively union parameter roots without reinterpreting another target's capture slots. An
unresolved higher-order parameter conservatively names every compatible
input, including embedded provenance in a by-value Move aggregate/dependent resource. The call
captures that provenance before moving/nulling the source and transfers it to the result; this does
not permit a callee to return a bare view of an owner it destroys. For every `borrow mut` input,
every other argument mode (`ByValue`, `Borrow`, `BorrowMut`, and `Out`) is checked. Direct place
overlap or recursively carried provenance rooted in the owner's old generation rejects the call,
including a Copy view, `resource_ref`, dependent resource, or either of two distinct holder
aggregates;
invalidation may not deliver a dangling peer argument to the callee. This is a structural
provenance check, never a package/type-name exception.

A borrowed pointee remains caller-owned and receives no callee function-exit cleanup. If
`borrow mut` replaces an owned value, however, the old value's ordinary Drop plan runs before the
store and the caller's path-local cleanup bit is updated to the replacement's bit. The unchanged
pointee is never dropped by the callee.

`borrow`, `out`, and `resource` are contextual rather than reserved words. Parameter lookahead
recognizes `borrow name: T`, `borrow mut name: T`, and `out name: T` as modes, while `borrow: T` and
`out: region` use those words as parameter names. At item position, only
`resource Name = path` starts a resource declaration; dotted intrinsic paths such as
`resource.borrow` parse normally.

Mutation whose root is an explicit `borrow mut` parameter remains Pure if the function performs no
other Impure operation. It is an exclusive input effect, like mutating an owned local/builder, not
mutation of hidden captured state. Alias checking makes the root unique at the call. Unsafe, FFI,
I/O, and database work remain Impure regardless of parameter mode.

One aggregate value uses one path-local cleanup bit. Its owned members must therefore use one
allocation mode: all free-standing or all arena-owned. Mixing the two in one tuple, struct, sum
value, or owned array is a compile error; keep them separate or construct every owned member in the
same mode. Replacing an owned field or element must preserve the aggregate's mode. Field replacement
is additionally limited today: the assigned leaf must be a `string` or an `Option<string>` (the two
leaves with typed drop-old lowering). Replacing any other owned leaf — a nested Move struct, an
owned array — is a compile error naming the unsupported type; replace the whole aggregate instead.
Borrowed fields do not affect this rule.
A one-owner aggregate may forward a heap/arena path-dependent runtime bit,
but its owned fields or elements cannot be mutated until the mode is definite. Direct Move-struct
and fixed-array bindings retain every completed field owner until initialization succeeds.

Tagged values use the same ownership rule recursively. `Option<T>`, `Result<T,E>`, and user sum
variants may carry any finite, non-recursive type whose Drop plan is known. The enclosing value is
Move when any possible live payload is Move. Drop tests the active tag and drops only that payload;
construction moves the payload and clears its source; an owning-place `match`, `else`, and `?` move a
selected payload out and clear the container, while an admitted borrowed-place `match` reads it in
place without clearing the source. An `Ok` success path does not allocate merely because its
error type owns strings. This supports structured owned errors and
`Result<Option<MoveOutput>, MoveError>` without a second error model. It does not imply arbitrary
arrays of Move elements; collection element layout/drop remains a separate explicit capability.

Every recursively Move function return carries its selected path-local cleanup bit as part of the
internal direct/indirect/module ABI. The caller stores that bit beside the returned value. Return
region/borrow summaries describe lifetime provenance only: they cannot reconstruct whether, for
example, the selected `Result` path returned arena-owned `Ok` data or an individually owned `Err`.

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

An arena may bind its allocation capability when ordinary functions need to construct into that
exact region:

```align
fn collect(input: slice<User>, out: region) -> array<User>

arena out {
  value := collect(input, out)
  use(value)
}
```

`region` is a Copy, non-owning, scope-bound capability created only by `arena name {}`. It cannot be
returned, stored in an aggregate, sent to a task, or passed through FFI. A value allocated through it
has that exact inferred arena region and cannot outlive the lexical arena block. Anonymous
`arena {}` and named `arena out {}` are the same ownership mechanism; the latter only makes the
allocation destination passable. There is no allocator trait or ambient caller-arena lookup.

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

A function-local array literal is fixed storage and normally infers its element type from its
elements. The empty literal `[]` is accepted only when the surrounding expression supplies one
exact element type, such as a `slice<Option>` parameter or a `slice<T>` binding annotation. It
materializes a zero-length fixed array and borrows it through the ordinary array-to-slice path;
there is no allocation and no special untyped-empty-array value. Without an expected element type,
`[]` is a compile error.

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
counting loops; collection iteration is the array pipeline, and sequential control is `loop`, §4).

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

An `out` parameter is a writable output buffer the callee fills. `out` is both a **safety
constraint** and a **no-alias optimization**: an `out` argument must not alias any other argument
(the compiler rejects a call that passes the same backing buffer twice — compared by root buffer,
so a slice and the array it borrows, or two sub-slices of one array, are all caught), which lets the
compiler emit `noalias` on the output and vectorize the fill without a runtime overlap check.

The pipeline terminal `map_into` writes a pipeline's results straight into an `out` (or `mut`)
slice, the caller-storage counterpart of `to_array` (which allocates a fresh owned array):

```align
fn scale(src: slice<i64>, out dst: slice<i64>) {
  src.map(fn x { x * 2 }).map_into(dst)
}
```

`map_into` is a **length-preserving** materializing terminal: it stores `dst[i] = f(src[i])` in one
fused loop for `map` / field-projection stages, requires `dst.len() == src.len()` (a mismatch aborts
at runtime, like an out-of-bounds index), and yields `()`. A filtering `where` before `map_into`
(which would write a variable-length prefix) is not part of this form — filtering materializes with
`to_array`. The compiler proves `dst` is a distinct buffer from the source (the same root-buffer
no-alias rule as an `out` argument) and emits the disjoint-buffer `noalias`, so the fused loop
vectorizes with no runtime overlap guard.

For a same-index transform over multiple inputs, `zip` is the lazy pipeline source:

```align
zip(a, b, c).map(fn v { v.0 + v.1 * v.2 }).map_into(dst)
```

It accepts two or more arrays/slices of Copy primitive scalars. Every runtime length is checked
equal before the loop; fixed unequal lengths are a compile error. The per-index tuple exists only
as an SSA value, never as an allocated tuple array. `map`/`where`/reducers retain their ordinary
increasing-index effect and trap rules. For `map_into`, `dst` must be disjoint from every source,
while the sources may alias each other (the compiler does not emit source-vs-source `noalias`).

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

Pipeline construction follows written order. The receiver/source is evaluated first. Each stage
then snapshots its captured Copy values once at that stage's written position. A terminal evaluates
its explicit arguments next; therefore `reduce(init, fn ...)` and `scan(init, fn ...)` snapshot
stage captures before `init`, then evaluate `init`, then snapshot reducer captures. The fused loop
reuses those snapshots; it does not reload enclosing locals per element. A capture containing a
view remains tied to its owner, so an intervening terminal argument cannot move, replace, or drop
that owner before the callback action. If any earlier operand does not continue, no later snapshot
or pipeline callback executes.

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

Function types preserve parameter modes: `fn(borrow Conn) -> i64`,
`fn(borrow mut State, Row) -> Result<(), Error>`, and `fn(out slice<u8>, str) -> ()`. Binding,
passing, joining, or indirectly calling a named function retains the exact mode and direct-call
ABI; there is no mode-erasing adapter. It also preserves inferred return-borrow/region summaries
for parameters and captured environment slots, so an indirect result cannot outlive any possible
target input, closure environment, or captured owner. Target-relative capture roots move with the
function value. The inferred effect and provenance summaries remain unwritten.

A passed closure's captured environment lives in the caller's frame for the duration of the call, so
no heap allocation is needed. A function value that *escapes* — returned from a function, stored
beyond the frame, or handed to `spawn` (§11) — needs a region-owned environment, owned by the
enclosing `arena {}` / `task_group {}` scope (never a hidden `malloc`); escape analysis chooses the
representation.

A trusted compile-time library producer may instead require one exact named or non-capturing lifted
function and compile it into a nominal static descriptor plus a producer-owned C-ABI trampoline.
This is available only to producer identities whose package contract fixes the source signature,
effect and provenance facts, native ABI, lifetime, cleanup, validation order, and malformed-input
behavior. It forms no closure environment or ordinary function value, and application source cannot
construct the descriptor, obtain its pointer, select a native signature, or use the mechanism as
general reverse FFI. A callback invocation may expose only the package's declared invocation-scoped
views; those views cannot escape the generated trampoline or cross a `spawn`/`par_map` worker
boundary. The compiler carries this non-Send provenance through direct and indirect helpers and
fails closed when an imported or indirect transfer fact is unavailable.

### Sequential Effects and Evaluation Order

Ordinary sequential `map` / `where` / `reduce` / `scan` / `partition` / `any` / `all` callables may
be Impure. They execute in input-index order and stage order. A callable runs exactly once for each
element that reaches it; when a `where` predicate is false, no later stage or reducer runs for that
element. `any` and `all` evaluate their predicate for every surviving element rather than
short-circuiting, so observable call counts stay deterministic.

Fusion may preserve this order, but effect inference restricts transformations: a call may be
reordered, erased, duplicated, speculated, or parallelized only when its inferred effect and the
specific operation make that transformation legal. Pure alone does not mean non-trapping or total.
`par_map` remains different: every callable moved into its parallel range must be Pure (§11).
`sort` and `sort_by_key` are stable. A `sort_by_key` key callable may be Impure; it runs exactly once
for each surviving element, in input-index order, before any reordering. Sorting compares the
recorded keys and never calls the key callable again.

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

### Broadcast and lane access

A scalar operand of a vector op **broadcasts** across the lanes, on either side (the operand order is
preserved for the non-commutative ops, so `20 - a` is `[20 - a[0], …]`). Broadcast is implicit in the
operand types — a lossless splat, not a hidden allocation. The scalar's type must unify with the
element: `vec4<i32> + 2.0` is rejected.

A `slice<T>` is the bridge between array memory and a vector register. `s.load(i) -> vecN<T>` reads
`N` consecutive elements from runtime index `i` (the width and element come from the target
annotation, exactly as for a vector literal); `s.store(i, v)` writes a vector's lanes into a
**writable** (`mut` / `out`) slice at `i..i+N`. Both are bounds-checked (`0 <= i && i + N <= len`,
the ordinary range-fail path), and a fixed array is loaded/stored by passing it where a slice is
expected (the array→slice borrow — nothing hidden). Both emit the `<N x T>` access at the *element*
alignment; the `align(N)` binding form below is what promotes a provably aligned offset to an
aligned load.

`v[i]` **reads** lane `i` and `v[i] = x` **writes** it, in both cases at a compile-time-constant
index in `0..N` (a dynamic lane is rejected). The write requires a `mut` vector local; a vector is a
register value, so it lowers to an `insertelement` that re-stores the whole vector.

```align
fn kernel(data: slice<i64>, out dst: slice<i64>) -> i64 {
  v: vec4<i64> := data.load(0)   // slice → vector
  w := v + 5                     // scalar broadcast on the right
  u := 5 + v                     // ... and on the left
  m: mask4<i64> := v > 2         // a written mask annotation
  hi := select(m, w, u)
  dst.store(0, hi)               // vector → writable slice
  mut z: vec4<i64> := [1, 2, 3, 4]
  z[0] = 9                       // lane assignment
  return z[0] + hi[1]
}
```

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
form when its suffix is safe on rejected lanes: `xs.where(p).sum()` lowers **branchless** (mask +
`select`, a masked reduction), not a per-element `if`. A general callable after `where` is guarded
instead; rejected elements never execute it.

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

An owning package resource may also expose a borrowed `soa<T>` view over its own exact-length
column buffer. The resource, not an implicit arena, is then the view's lifetime root. In
particular, `pkg.db.batch<T>` uses this form only through `batch_soa<T: SoaPlain>`; moving or dropping
the batch invalidates the view. This does not add an owned `soa<T>` language value or permit
Move-fielded columns.

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
spawns cannot outlive it, and the scope joins them at its end. An early exit also joins first,
before releasing any frame-owned or enclosing-arena storage captured by a task.

The similarity to `arena` is the visible lifetime boundary, not general allocation permission.
The task-group region stores only spawned closure environments and task result slots. Ordinary
owned values created in the block keep their normal individual ownership; operations that require
arena allocation, including `heap.new`, still require an explicit nested `arena {}`.

```align
import std.fs

fn main() {
  task_group {
    a := spawn(fn { fs.exists("a.txt") })   // a deferred task; the `fn { }` makes the
    b := spawn(fn { fs.exists("b.txt") })   // "runs as a separate task" visible
    wait()                                   // join all primitive-result tasks
    print(a.get())                           // extract a result after its group joined
    print(a.get())                           // primitive results may be read again
    print(b.get())
  }
}
```

`spawn` takes a **lambda** (the deferred work), not a bare call — the deferral is then visible
in the source (*Nothing hidden*), and it is the same lambda mechanism as `map`/`reduce`/`par_map`
rather than a second, special-cased one (*One way*). It returns a `Task<R>` handle; `wait()?` is
the single error boundary (it joins every task and propagates the lowest-spawn-index `Err`), and `a.get()`
reads a task's result after the join.

For a fallible group, `get()` requires control to have passed through a successful `wait()` that
covers the current task generation. The Result may be handled immediately or first kept in a bare
local, copied/reassigned, passed through a block tail, `map_err`, or value-producing
`if`/`match`/`else`/`loop`; the success proof survives only when every reachable value predecessor
has the same group, proof epoch, Wait id, and covered task generation. `?`, an exhaustive Result `match`, or Result `else`
then resolves that exact Wait. Every earlier Wait Result for the same drained task generation must
also be proved successful: ignoring or delaying a failed first Wait and handling a later empty
Wait cannot authorize an uninitialized result slot. An Err invalidates every Task and Wait proof
it covered. Every `spawn` advances the current task generation and stales old Wait proofs; when an
earlier Wait remains unresolved it also invalidates the covered Tasks. After success, one later
successful Wait can reauthorize both old and new Task handles. An unrelated overwrite clears the
local proof. Calls, returns, closure captures, imported values, and aggregate
reconstruction do not transport the proof. Passing a Copy Result does not erase the caller's
original local, but no callee result acquires its provenance.
A later `wait()` with no pending task cannot undo a completion already established for that
generation, so leaving that later Result unhandled does not make initialized slots unreadable.
Loop analysis carries fallthrough state through a fixed point before joining reachable `break`
states; an unresolved or failed Wait from an earlier iteration therefore cannot disappear at a
later break.

Each `Task<R>` is a Move handle whose compiler-only origin names the `task_group` and task
generation that spawned it.
Local moves, reassignment, block tails, and value-producing control flow preserve that origin;
calls, returns, captures, imports, and aggregate reconstruction do not transport it. `get()` checks
the originating group, not merely the innermost active group. Entering a nested group preserves
outer group facts: an inner `wait()` cannot authorize an outer Task, while handling an outer
fallible Wait Result inside the inner group updates the outer group's success fact. Current task
results are primitive Copy values, so `get()` does not consume the Task handle and may be repeated.
Leaving a nested group removes proofs that name that group, including proof on its block result;
handling that Result outside cannot authorize an enclosing Task. Proofs for still-active outer
groups remain. Owned task results remain a future extension.

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

A `str` **literal** is a `{ptr, len}` view of bytes placed in the program's read-only data section
(rodata) — shared, never copied, process-lifetime (`Static`). The same mechanism backs an aggregate
`slice<T>` constant (§3 Constants): its elements are one rodata table and the constant is a borrowed
view of it, so a top-level array constant is a `slice<T>`, exactly as a string literal is a `str`.

### Binary decode and encode

Binary formats (GGUF headers, `alignpack`/`alignidx`, any packed record) are read from a `bytes`
view and written into a growable `buffer` with **bounds-checked, endian-explicit** scalar
operations. Endianness is **never implicit**: every multi-byte read/write names its byte order with
a `_le` / `_be` suffix, so a format's byte order is visible in the source (Nothing hidden). Single
bytes (`u8` / `i8`) carry no suffix.

```align
// decode — read a scalar from a `bytes` (slice<u8>) view at a byte offset
h := fs.read_bytes_view("model.gguf")?     // bytes
magic  := h.u32_le(0)                       // u32, little-endian
count  := h.u64_le(8)                       // u64
tag    := h.u8(4)                           // u8 (no endian suffix)
weight := h.f32_le(off)                     // f32 (reads its bits, reinterprets)

// encode — append a scalar to a growable `buffer` (grows in place; needs a `mut` binding)
mut out := buffer(0)
out.put_u32_le(magic)
out.put_u64_be(count)
out.put_f32_le(weight)
out.append(payload)                         // copy a raw bytes/str blob in
data := out.bytes()                         // view the accumulated bytes
```

The read/write scalar set is `u8`, `i8`, and — with an explicit `_le` / `_be` — `u16`/`i16`,
`u32`/`i32`, `u64`/`i64`, `f32`, `f64`. A read is `bytes.<scalar>(off)` and its encode dual is
`buffer.put_<scalar>(v)`; `buffer.append(data)` writes a raw `bytes` / `str` blob. The value handed
to `put_*` must match the writer's scalar type exactly (no silent coercion). An **out-of-range**
read (`off < 0`, or `off + width > len`) **aborts** — the same fail-closed policy as `slice[i]`, so
a parser checks `.len()` before reading, exactly as it checks a slice's length before indexing. A
read returns a Copy scalar (it never carries the view's region), so it composes freely; the
`bytes`/`buffer` themselves stay borrowed (never consumed).

### Literals and Escapes

A string literal is double-quoted and **single-line** — a raw newline inside a literal is a
compile error (multi-line text is composed with `\n` or a `builder`). A `char` literal is
single-quoted and holds exactly **one Unicode scalar value** (`'A'`, `'あ'`; surrogates are not
scalar values and are rejected).

The escape set — the same in both forms, `\"` in strings, `\'` in chars:

```text
\n \t \r \0    newline / tab / carriage return / NUL
\\             backslash
\" or \'       the delimiter
\u{...}        1-6 hex digits, any Unicode scalar value
```

An unknown escape is a **compile error**, never passed through silently.

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
builder encode
```

### Typed Decode

```align
user: User := json.decode(data)?
```

### Zero Copy

A decoded clean `str` or string-bearing `array` / nested field remains a view into the input buffer
(no allocation), region-tied to that input. Numeric scalar arrays are copied into their owned
buffer. JSON string tokens accept the RFC 8259 escapes `\"`, `\\`, `\/`, `\b`,
`\f`, `\n`, `\r`, `\t`, and `\uXXXX`; raw C0 bytes, malformed escapes, and invalid
surrogate sequences are errors.
A selected escaped `str` is materialized exactly once in the caller's enclosing arena and the
decoded value is region-bounded by both input and arena. Outside an arena, a selected escaped string
is a decode error; clean strings retain the input-view path. Ignored escaped keys and values are
validated without proportional scratch allocation.

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

### Direct owned records

A direct declared record containing `string`, `Option<string>`, or `array<string>` selects the
owned JSON route. The route is closed and flat: every
other field is a required signed/unsigned 8/16/32/64-bit integer or `bool`. Borrowed text, floats,
nested records/arrays/enums, other optional forms, and explicit record layout/alignment are rejected
before allocation. A record without an owned text leaf keeps the existing borrowed route.

Owned decode materializes every text value into free-standing storage. `array<string>` owns its
spine and every element, and `Option<string>` owns only `Some`. The result has no input or arena
dependency and remains free-standing when the call occurs inside `arena {}`. This is not a hidden
copy: the declared owned field types plus `json.decode` select the materializing boundary. A mixed
owned/borrowed graph is rejected instead of converted.

`json.decode`, `json.encode`, and `json.encode_bounded` accept the same owned graph. Encode preserves
declaration order and the existing JSON string grammar; bounded encode retains its inclusive byte
ceiling and owned `Result<string, Error>`. Decode range-checks the exact integer width and supports
the full unsigned 64-bit range. Missing and `null` map to `None`, encode omits `None`, and
`Some("")` remains distinct. Recoverable failure drops every initialized direct owner exactly once;
overflow and allocation failure retain the terminal-abort policy.

The exact descriptor, error precedence, ownership matrix, and golden vectors are specified by
`docs/impl/24-owned-json-plan.md`. AoS, SoA, union, scalar-array, `json.doc`, and `json.scan` routes
are unchanged.

### Recursive owned records

The accepted Request 13 extension replaces that flat implementation boundary with one recursive,
view-free graph. A direct natural-layout record selects the owned route when any reachable field is
an owned `string`. The complete graph may contain fixed-width signed/unsigned integers, bool,
`string`, nonempty natural-layout records, `Option<T>` payloads, and dynamic `array<T>`. An option
payload may not itself be an option because missing and `null` are one absence state. Array elements
are integers, bool, `string`, or an accepted record; arrays of options/arrays remain outside the
language's element representation. The graph must be acyclic and at most 128
record/option/dynamic-array constructor levels.
`str`, floats, char, enums, fixed arrays, explicit layout/alignment, and every other constructor
reject before descriptor construction or allocation.

All decoded owners are free-standing, including nested option payloads, array spines, strings, and
record elements. Partial construction and recoverable failure recursively drop live fields in
declaration order and array elements in ascending order exactly once. `json.decode`, `json.encode`,
and `json.encode_bounded` share the same graph and declaration-order canonical bytes. The V2
structural graph descriptor replaces V1 for flat and recursive roots, travels only in a target-bound
V2 envelope, and atomically advances the compiler interface format from 7 to 8; there is no
flat/recursive compatibility path. The exact accepted design and implementation closure matrix are
`docs/impl/25-recursive-owned-json-plan.md`; implementation is pending.

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
encode_bounded
```

That is the whole generated typed-schema surface: there is no `validate<T>` (decoding and
discarding *is* validation — one way) and no public `field_table<T>`. `encode_bounded` is the
accepted, not-yet-implemented fallible destination for the same generated encode plan. The compiler
does hold per-field information (name, length, hash, first byte, offset, escape info) at compile
time to drive the scanner, but it is an internal artifact with no source-level name. See §18.1
`core.json`.

### SIMD Scan

The JSON scanner finds structural chars with SIMD.

```text
{ } [ ] : , " \ whitespace
```

### Dynamic (schema unknown): `json.doc`

When the shape is not known at compile time, `json.doc` is the lazy document view — parsed **once**
into an arena-backed tape (no serde-style per-node heap tree, no `map` type). It is a **Copy** view
handle region-tied to `min(input, arena)`, so it — and any `str` / sub-doc read of it — cannot escape
the arena that owns the tape.

```align
arena {
  d := json.doc(body)?                       // Result<json.doc, Error> — malformed input is Err
  match d.kind() { Object => …, Array => …, _ => … }   // json.kind: Object/Array/Str/Number/Bool/Null/Missing
  id := d.get("id").as_i64() else 0          // navigate then read a leaf
  first := d.get("tags").at(0).as_str() else "?"
}
```

Navigation is **total and Missing-propagating**: `d.get(key)` (object member) and `d.at(index)`
(array element) always return a `json.doc`; a missing member / out-of-range index / navigation on a
non-container yields a `Missing` doc, which propagates through further navigation so absence surfaces
**once** — as `None` from a leaf accessor. The leaf accessors `as_str` / `as_i64` / `as_f64` /
`as_bool` return `Option` (`None` when the value is not that kind); `d.kind()` returns the builtin
`json.kind` sum type and distinguishes JSON `null` from absence (`Missing`). `as_str` is a zero-copy
view into the input, except an escaped string, which is unescaped into the arena (the one allocating
accessor). `d.len()` is the member / element count (0 on a non-container); `d.key(i) -> Option<str>` is
the i-th object key in document order — so an object is read as ordered data (keys + `at(i)` values)
without a `map` type. `d.elems()` materializes one level (each array element, or each object member
value) as a `slice<json.doc>` — arena-backed, built once (so iterating with `elems()`+`[i]` is O(n),
vs `at(i)`'s O(i) per call), indexable and `len`-able like any slice, and nameable as a
`fn f(xs: slice<json.doc>)` parameter (a level walked by recursion). The types `json.doc` and
`json.kind` are nameable, so a `fn f(d: json.doc)` helper factors doc code out. `json.doc` (and
`elems()`) need an enclosing `arena {}`, like the direct-to-`soa` decode.

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
  none := raw.null()        // explicit null pointer for a native ABI argument or sentinel
  empty := none.is_null()   // the only null test — `bool`, valid on any `raw`
  p := raw.alloc(16)        // 16 bytes → a `raw` pointer
  raw.store(p, 0, 42)       // write a flat value at a byte offset (type from the value)
  x: i64 := raw.load(p, 0)  // read it back (type from the annotation — no turbofish, like decode)
  q := raw.offset(p, 8)     // advance by 8 bytes → a new `raw` pointer
  raw.free(p)               // manual free; a `raw` is Copy and never auto-dropped
}
```

`raw.offset(p, n)` is byte-granular pointer arithmetic: it returns `p` advanced by `n` bytes as a
new `raw`, for stepping through a buffer or handing an interior pointer to C. It is distinct from
the inline byte offset of `load`/`store` — that one addresses a value, this one produces the pointer
itself. The result's validity is the enclosing `unsafe` block's obligation, exactly like the
pointer it derives from.

`raw.null()` is the only null-pointer constructor and is explicit at the unsafe boundary; a raw
pointer is tested with `p.is_null()`. There is no general null value in ordinary Align types.
The stored/loaded type is inferred (from the value
for `store`, from the expected type for `load`) —
Align has **no turbofish**, so an explicit `raw.op<T>(...)` is not the surface. (An unchecked pointer
cast / reinterpret is a later `raw.*` op.) The admitted flat values are primitive scalars, `raw`
pointers, and eligible non-empty `layout(C)` structs. Storing a `raw` pointer makes package-owned
native handle tables explicit without converting an address to an integer; loading it requires an
expected `raw` type just as a scalar load requires its scalar type. A function
containing `unsafe` is inferred impure, so it can never be a `par_map` callee — the danger stays
visible and traceable.

### Foreign functions (FFI)

A C function is declared `extern "C"` and called like any other function — but only inside `unsafe`,
because foreign code is outside the safe core and can violate every invariant (ownership, no-alias,
non-null). A direct call or a non-escaping pipeline/reducer/sort callback is valid only when that
invocation expression is lexically inside `unsafe`. An extern declaration cannot be formed into a
first-class function value: the current function-value type has no unsafe-call permission that
could remain visible after escape.

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
supported. Parameters admit integer and float scalars, `raw`, `str`, numeric `slice<T>` (including
`bytes`), and an eligible non-empty `layout(C)` struct. Returns admit `()`, integer and float
scalars, `raw`, and an eligible non-empty `layout(C)` struct; a view never returns. The later
by-value-struct section gives the target-specific eligibility gate. A function that calls an extern is inferred impure
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

### Opaque native resources

A package may turn a persistent non-null native pointer into an opaque Move resource with a
type-owned Drop hook:

```align
import pkg.db.internal.resource

pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt<P, R> = pkg.db.internal.resource.drop_stmt
```

The hook is a `pub` function in the package's allowed `internal` subtree, accepts only `raw`, returns
unit, and performs native destruction inside an `unsafe {}` body; Align does not add `unsafe fn`
syntax. Resolving the resource declaration synthesizes a non-user-callable hidden support thunk in
the declaring producer. Resource type metadata carries that thunk's symbol and ABI fingerprint, so
an importing unit can emit Drop and link the producer without importing the internal source module.
A resource is one opaque pointer, always Move, and calls the thunk exactly once on ordinary
cleanup. Only the declaring module and its descendant module subtree may use
`resource.from_raw`, `resource.from_raw_borrowed`, `resource.view_from_raw`, `resource.raw`, and
`resource.into_raw`; `unsafe` does not bypass this representation privilege. The Drop-hook module
need not import the declaring module, so a root resource may name an internal raw Drop hook while
driver descendants construct the root type without a module cycle. The `internal` import rule keeps
the `pub` source hook outside consumer APIs.
`resource.borrow(r)` returns a Copy `resource_ref<R>` tied to the owner's inferred generation.
Owner move/Drop or a `borrow mut` call invalidates it. Resources and references are non-Send by
default. This is the one mechanism used by native `std` handles and FFI-backed `pkg` libraries; safe
APIs never expose `raw` or manual destroy.

In v1, `resource.into_raw` consumes only a standalone initialized resource local or by-value
resource parameter owned by the current function. A field, element, projection, borrowed/out
parameter, or temporary is rejected. This keeps one aggregate cleanup bit exact rather than adding
hidden per-field ownership state.

`resource.from_raw_borrowed(ptr, parent_ref)` creates an owned child resource tied to one parent
resource generation; the compiler keeps the parent alive until the child drops.
`resource.view_from_raw(owner_ref, ptr, len)` is the native zero-copy view constructor: inside
`unsafe`, and only in the resource's declaring/internal code, it returns an expected `Option<str>`
or `Option<slice<FFIScalar>>` tied to that owner generation. `None` reports a negative or
unrepresentable length, a non-empty null pointer, misalignment, or invalid UTF-8 for `str`; an empty
null pointer is a valid empty view. Foreign range validity remains the unsafe wrapper's proof
obligation. There is no owner-free safe conversion from `raw` to a view.

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

Among structs, only a `layout(C)` struct may be moved through a `raw` pointer (`raw.store`/`raw.load`
of a whole struct), because only it promises a fixed representation — this is the pointer-based FFI
pattern (hand C a buffer, read/write structs in it). Primitive scalars and `raw` pointers are the
other admitted flat values. Its fields must be FFI-mappable scalars (integers, floats).

### By-value structs (SysV AMD64 only)

A `layout(C)` struct also crosses the boundary **by value**, using the System V AMD64 register
convention — but *only* on x86-64 Linux. Each eightbyte of the struct is classified INTEGER (a
general-purpose register) or SSE (an XMM register); a struct ≤ 16 bytes is passed/returned in
registers, and the compiler emits exactly the coerced form a C compiler does (`i64`/`double`
argument slots, an `{T0,T1}` aggregate return), so a call is binary-compatible with a real C callee.
This is the one FFI corner where a *wrong* per-target rule silently miscompiles, so it is deliberately
scoped: on any non-SysV target the compiler **refuses** with a clear diagnostic (pass the struct by
pointer instead) rather than guessing, and a struct larger than 16 bytes (MEMORY class, needing a
`byval`/`sret` pointer) is likewise rejected — that shape is already served by struct-by-pointer, so
a redundant second mechanism is not added. The same MEMORY boundary is enforced under register
pressure: SysV passes a struct in registers only if *all* its eightbytes fit in the class registers
left after the preceding arguments, else the whole struct goes to memory — so a signature where a
by-value struct argument would fall to memory (e.g. a two-eightbyte struct after five integer
arguments) is also rejected (reorder it earlier, or pass it by pointer). (AAPCS64 and the
MEMORY-class `byval`/`sret` path are future work, added only when a concrete wrapper needs them.)

### Not in FFI v1 (deliberate boundaries)

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
index-walk in loop     (walking an array by index inside `loop` — write it as a pipeline, §4 Loop)
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

`import geom` resolves by **filename convention** to `geom.align` in the entry file's directory (its `module` declaration must match the filename). A nested path follows the directory tree: `import util.math` → `util/math.align` declaring `module util.math`, called `util.math.fn(...)`. A cross-module reference is written qualified — `geom.area(...)` for a function, `geom.Point` for a type — and reaches only `pub` members; a bare name resolves within the calling module (so an imported type *must* be qualified). Each module has its own function and type namespace, so two modules may define a function or type with the same name. That includes compiler-provided bare type aliases in non-entry modules: a same-module declaration wins locally, the builtin remains available by its provider-qualified spelling such as `core.Error`, and importers use the ordinary module-qualified user type. Entry-module declarations that would collide with an unmangled builtin canonical name remain errors.

A `pub` item's signature may name only `pub` types: a `pub` function's parameter and return types, a `pub` struct's field types, and a `pub` sum type's payload types (a `pub` constant's type is a scalar, `str`, or a `slice<T>` of one — so its element type is transitively `pub` by construction) must all be `pub` — a private type cannot leak through a public interface. The rule holds transitively (a type nested under `Option`, `array`, a tuple, or a fn-type is checked too), so a module's public interface is fully self-contained: everything it exposes is itself exported and usable by an importer. A **generic** `pub` function's *body* is part of its interface too — its template is instantiated in importing modules, where the defining module's private items do not exist — so a generic `pub` function's body may reference only `pub` same-module items (its params, locals, and type parameters aside): a private same-module function, type, or constant in a generic `pub` body is rejected at the defining module.

The module import graph must be a DAG: a cycle of `import`s — direct (`a` imports `b`, `b` imports `a`), transitive, or a module importing itself — is a compile error. Mutual dependency means the two modules are one unit of meaning: merge them, or extract the shared part into a third module both import. The restriction keeps every module's interface computable bottom-up (each module is checked against the already-checked interfaces of its imports), which is what makes per-module compilation and caching possible.

An imported `pub` sum type's variant is constructed qualified, the same way its type is named:
`pal.Color.Green` (tag-only) and `pal.Color.Code(40)` (with a payload). Together with holding,
returning, and `match`ing it, an exported sum type is fully usable across modules.

### Packages (the pkg layer)

A **package** is a *distribution-layer* unit — the module subtree a tool (or a human) vendors under
`pkg/` — and the compiler never learns what one is: resolution, visibility, effects, escape, and
capabilities all carry over unchanged from the module system above. The package graph, like the
unit graph, is discovered from `import`s + the filesystem, so a build is hermetic on reachable source
inputs (no manifest, no search paths, no registry lookup at compile time). Besides `.align` units, a
compiler-known static constructor may register an exact file as a content-hashed source input; it
cannot scan, execute code, read environment state, or access the network. A package is its root
module file `pkg/<name>.align` plus, optionally, its submodule tree `pkg/<name>/…`; a package's own
sibling imports are written absolute (`import pkg.web.internal.util`), the same path a consumer
writes, so vendoring is literally copying the subtree into `pkg/` — there is no develop-layout vs
installed-layout split. One version of a package exists per tree by construction (a `pkg/<name>/` can
exist once), so there is no version solver; an incompatible major version is a new name (`pkg.web2`).

The first import segment is a **trust tier**: `core` (language) / `std` (OS boundary) / `pkg`
(third-party) / anything else (this project). `core`/`std` are compiler builtins (never files); `pkg`
is the third-party area. A file's import header thus shows not just *what* it reaches but *whose*
code it trusts. Two path rules govern package edges (both pure path checks, no new syntax, no
package-boundary metadata):

- **The `internal` path rule.** An import whose path contains a segment `internal` is legal only from
  within the subtree rooted at that `internal` segment's parent: `pkg.web.internal.router` is
  importable from `pkg.web` and `pkg.web.*` only. This is what lets a package keep implementation
  modules private — without it, every module would be permanent public API. (A project-root
  `internal` with no parent prefix is visible project-wide; the rule is uniform.)
- **Layering.** A module under `pkg/` may import only `core` / `std` / `pkg` modules — never the
  consuming project's own modules. A vendored package that reached back into the project would
  compile in exactly one tree and invert the dependency arrow. This keeps the library layering
  `core → std → pkg → project` a compiler-checked fact, not a convention.

Visibility stays one model — module-level `pub` plus the `internal` path rule; there is no
`pub(pkg)` / export-list / re-export machinery (hide a module by path, hide an item by omitting
`pub`). Import aliases are not offered (they would hide provenance at the call site), so a call stays
fully qualified — `pkg.web.get(...)` — and the trust tier is visible at every use. Manual vendoring
(copy the subtree) is a complete dependency mechanism; a fetch tool and a lockfile that records
source provenance are a tooling concern that ends before the compiler starts (deferred).

---

# 18. Library Layout

## 18.1 core

`core` is the foundation, close to the language philosophy itself.

> This section is the design-intent catalog. The **exact shipped surface** per core area —
> verified signatures, ownership/effect classification, error/abort policy, pitfalls, test
> anchors, and which of the names below are not implemented yet — is maintained at
> std-design depth in `docs/impl/core-design/`.

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
core.array_builder

core.arena

core.json
core.template

core.hash
core.math
```

Every name above is an importable module except `core.array_builder`: `array_builder<T>()` is a
language-intrinsic global (like `builder()`), listed here as a core area rather than as an `import`
target.

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
zip
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
trim_start
trim_end
contains
starts_with
ends_with
eq_ignore_ascii_case
```

Has a SIMD fast path in the standard implementation.

### core.array_builder

```text
array_builder<T>()      // open an empty growable typed builder
array_builder<T>(out)   // grow in the explicitly supplied region
b.push(v)               // append one element (mut receiver)
b.append(xs: slice<T>)  // bulk-append Copy-scalar elements (mut receiver)
b.build() -> array<T>   // freeze into an owned array<T> (consumes the builder)
```

`array_builder<T>` is the **typed** member of the grow-then-freeze family — `builder`
grows a `string`, `buffer` grows bytes, `array_builder<T>` grows an `array<T>`. It is
the answer to "accumulate an unknown number of elements, then hand back one owned
array": the `loop`-and-collect output that a fixed array literal and the
pipeline's `.to_array()` (which needs its length known up front) cannot express.

```align
mut b: array_builder<i64> := array_builder()
mut i := 0
loop {
  b.push(i * i)
  i = i + 1
  if i >= n { break }
}
squares := b.build()          // owned array<i64>, fed to the pipeline
total := squares.sum()
```

It is an owned **Move** handle bound to one `mut` local (like `buffer`/`builder`).
An ordinary helper may temporarily receive that same bound owner through a `borrow mut`
parameter, mutate it, and return without consuming it; the builder still never becomes an
aggregate field:
it never rides an `Option`/`Result`/array/tuple, is not `print`/`==`-able, and
cannot be captured into a `par_map`/`spawn` closure. `push`/`append` grow it in
place (amortized doubling) and are **Pure** (in-memory growth, no I/O); `build`
**consumes** it (using it afterward is a moved-value error). Freeze is **zero-copy**:
the builder's storage is heap memory grown in place, and `build` is a pointer+length
retype into the `array<T>` — no element copy, and the array's own `Drop` frees the
whole buffer (a deep free for `array<string>`).

Crucially, the individually owned heap `array_builder` holds **no views** — nothing a growth reallocation
could invalidate — which is exactly why a directly growable `array<T>` was rejected:
a live element/slice borrow would dangle across a `push`. The builder confines
growth to a phase with no outstanding borrows, then freezes to the immutable,
borrowable `array<T>`.

The individually owned heap form accepts **Copy scalars** (int/float/bool/char), **`string`**, and a
closed recursively view-free declared-record class. An admitted record is nonempty, naturally
aligned to at most 8 bytes, has no explicit layout/alignment, and contains Copy scalars, `string`,
another admitted record, recursively admitted `Option<T>`, or an owned `array<E>` whose element is
a Copy scalar, `string`, or an admitted record. Options may nest. Arrays whose elements are options
or dynamic arrays remain outside the type representation; fixed arrays, results, sums, tuples,
boxes, resources/refs, raw/function/builder values, empty or cyclic records, explicit
layout/alignment, and every direct or nested view are rejected before construction. A concrete
generic record is tested only after monomorphization; there is no new generic bound or runtime
dictionary.

A `string` or Move record element is **moved** into the builder by `push`: its complete source is
nulled, and every reachable string or dynamic-array owner must be free-standing before the growth
side effect.
Bound locals, fresh literals, by-value function results, transparent block tails, value-carrying
`if`/`match`/`else`, and successful `?` unwraps including `map_err(...)?` are complete source forms. Borrowed or
partially projected records, already-consumed arms, type-divergent joins, and arena-owned, mixed, or
path-dependent nested owners reject before push. Reallocation relocates initialized record bytes
without dropping the old byte positions. An unfinished builder recursively drops its initialized
prefix in both stack-local and boxed-header modes. Option Drop visits only `Some`; string arrays
drop every element before their buffer; arrays of Move records recurse through every element.
`build` transfers the same buffer from either mode to the ordinary deeply dropped `array<T>`.
`append` — which bulk-copies a borrowed `slice<T>` — remains Copy-scalar-only. The builder retains
no runtime reflection or self-describing record wire: its nominal element type, interface graph,
and compiler `DropPlan` are the single type/cleanup identity, and two same-shape declarations remain
distinct nominal types.

The region form is the required generalization for ordinary libraries. `out: region` comes from
`arena out {}`. It accepts recursively plain scalars, options, fixed arrays, structs, and
region-valid `str`/bytes views; resources, raw values, functions, and independently owned fields are
rejected. It grows in arena chunks with no hidden heap allocation, then `build()` performs one
documented compacting pass into the exact contiguous arena result. A short-lived view must be copied
first with `clone_in(out)`. It remains one mutable local, though a `borrow mut` helper may push
through it; it is not storable in a shaping-state struct. The heap form above retains zero-copy
freeze.

### core.json

```text
json.decode
json.encode
json.encode_bounded
json.doc
json.scan
```

`decode` and `encode` carry no written type argument: `decode`'s target is the
expected type from context (`u: User := json.decode(d)?`) and `encode`'s is the
type of its value argument — inference recovers both, so Align has no
expression-position type-argument syntax (no turbofish). `scan`'s row type comes
from the binding annotation the same way (`rows: json.scanner<Row> :=
json.scan(view)`). This is the complete surface: there is no `validate<T>`
(decoding and discarding IS validation — one way), no SAX `token` tier (`doc` +
`scan` cover it), and no public `field_table<T>` (a compiler-internal artifact).
`doc` is the schema-unknown tier — see §14.

`json.encode_bounded(value, max_bytes: i64) -> Result<string, Error>` is the owned, fallible sibling
of `json.encode`. It accepts exactly the same typed values, including the shipped flat direct-owned
graph and the accepted recursive owned graph once implemented, and emits exactly the same
declaration-order bytes. `max_bytes` is an inclusive UTF-8
byte ceiling: exact fit succeeds; a
negative ceiling or the first byte beyond it is `Error.Invalid`; no partial string or allocation
beyond the ceiling is exposed. “Canonical” names this one typed encoder, not RFC 8785 sorting or a
schema-unknown dynamic value surface.

`json.scan` accepts only a recursively Copy row: the complete reachable row definition graph must
require no `Drop` under Align's canonical ownership classification. Among rows admitted by the
existing JSON decode schema, a direct or transitive owned `array<T>`, `array<Struct>`, owning option
payload, or owning union payload is rejected at the scanner call with the exact diagnostic:

```text
`json.scan` row type '<row-type-source-spelling>' must be Copy; Move rows need per-row Drop before the scanner can reuse its row slot
```

An unsupported JSON field shape, such as an owned `string` or `array<string>` that fails the existing
schema whitelist, retains that schema diagnostic instead. This is a scanner-only restriction; the
same declaration remains an ordinary valid type and keeps its existing non-scanner JSON behavior.
The placeholder is the declared public local/imported spelling with concrete generic arguments;
internal `$`-mangled and monomorph-interner names never appear. The check is semantic and precedes
input validation, MIR, descriptor construction, and runtime side effects.

The scanner generic boundary is concrete-row-only. Concrete generic monomorphs
such as `Wrap<i64>` remain eligible after row resolution, and ordinary generic
calls use the expected-return propagation rule owned by
`align_sema::Checker::check_generic_call`; numeric `IntVar`/`FloatVar` retain
their deterministic `i64`/`f64` defaults. An unresolved `Wrap<T>` /
`json.scanner<Wrap<T>>` type argument inside a generic function retains the exact
resolver diagnostic `instantiating a generic struct with a type parameter
('Row<…>' inside a generic function) is not supported yet`. That unresolved-row
capability is a separate compiler prerequisite, not an implied scanner surface.

A struct field may itself be a `Struct`: `decode` recurses into the nested object
and `encode` renders it back, so a nested record round-trips in declaration order
(unknown keys are still skipped at every level, and nested `str` fields stay
zero-copy views into the input). A field may also be an `Option<T>` (payload
scalar / `str` / nested struct): decode maps a missing key or JSON `null` to
`None`, a type mismatch to `Err`, and a present value to `Some`; encode **omits**
a `None` field entirely (never `"k": null`), so `decode(encode(x))` round-trips.
A non-`Option` field still errors when its key is missing — optionality is
declared in the type, never inferred. The Decode schema admits scalar, borrowed-
`str`, and nested-struct Option payloads, including the currently admitted
`Option<Move-struct>` shape. Ordinary decode, encode, and scope Drop preserve
that admitted shape; a known partial-error cleanup defect after a later required
sibling fails is a separate ownership request. The scanner-only Copy restriction
does not narrow ordinary JSON behavior.
A field may also be an owned
`array<Struct>` (the `messages: array<Message>` shape): decode parses the JSON
array into an owned array-of-structs in the field (freed by the struct's drop),
and encode renders it back — so a full nested/array/optional record round-trips.
The array element struct may itself be Move; its elements are deep-dropped. The shipped direct-owned
route admits a flat `array<string>` field. The accepted Request 13 route admits owned text through
its separately reviewed recursive record graph; top-level owned AoS selection remains separate. A
`soa<Struct>` stays primitive/`str` columns.

### Union (Sum-Type) Mapping

A JSON `oneOf` maps to an Align sum type, discriminated by the value's **shape
class** — `Str` / `Number` / `Bool` / `Object` / `Array` — an O(1) dispatch on the
first structural byte, no backtracking:

```align
Content { Text(str), Parts(array<Part>) }
Message { role: str, content: Content }    // "content": "hi"  OR  "content": [ ... ]
```

A union-decodable sum type must have every variant carry exactly one payload, and
the payload shape classes must be **pairwise distinct** — two object-payload
variants (or `i64 | f64`, both `Number`) are a compile error. `null` is not a
class: absence belongs to `Option` (`Option<Content>` composes). Encode writes the
live variant's payload **bare** (no wrapper key), so the mapping round-trips by
construction. Distinguishing object-vs-object by a tag field (`{"type": …}`) is
expressed as a single struct with `Option` fields, not a second discrimination
rule.

### Document View (schema unknown)

When the shape is not known at compile time, `json.doc` parses the input once into
an arena-backed structural tape and navigates it by zero-copy views — the
schema-unknown tier that complements typed decode (never a competing way to read
typed data). Parsing is fallible (`Result` — malformed input is an `Err`);
navigation after it is **total**: `get`/`at` always return a `json.doc`, and a
missing member / out-of-range index yields a doc whose `kind()` is `Missing`,
which propagates through further navigation — absence surfaces once, as `None`
from the leaf `as_*` accessor, never as per-step unwrapping (`?` is Result-only):

```align
arena {
  d := json.doc(body)?                    // Result<json.doc, Error>
  model := d.get("model").as_str() else ""
  text := d.get("choices").at(0).get("message").get("content").as_str()
  n := d.get("choices").len()             // 0 on a non-array/object
  k := d.key(0)                           // Option<str> — objects as data (ordered)
  parts := d.get("items").elems()         // slice<json.doc>: one materialized level
}
```

`kind() -> json.kind { Object, Array, Str, Number, Bool, Null, Missing }`
distinguishes JSON `null` from absence when the caller cares; both make every
`as_*` return `None`. Everything is a borrowed view region-tied to the input and
the arena. There is no heap value tree: no per-node allocation, no map type —
keys-as-data is `key(i)` + `at(i)` over an object's ordered members. The one
allocating accessor is `as_str()` on an escape-bearing string (unescapes into the
arena, bulk-freed).

### Streaming (larger than memory)

`json.scan` streams NDJSON or a top-level array as typed rows without
materializing the whole input; the row type comes from the binding annotation and
the scanner is a pipeline source (row views borrow the current chunk and die with
the stage). The row must be recursively Copy because the scanner reuses one row
slot without per-row `Drop`:

```align
rows: json.scanner<Event> := json.scan(view)
total := rows.where(.active).pay.sum()?
```

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

`std` is the boundary for OS services and reusable application mechanisms that do not belong in
language-intrinsic `core` (for example encoding and regex). An import makes that dependency explicit;
only the individual module's contract determines whether it performs OS I/O.

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
std.regex
std.compress
std.rand
std.crypto
std.http
```

### I/O design principles

`std.io` / `std.fs` / `std.path` / `std.env` / `std.time` (M9) are built on four rules:

- **One concrete type, not a trait.** Align has no traits/comptime polymorphism, so `reader` and
  `writer` are concrete, builtin **Move** types — each owns exactly one fd, and `Drop` closes it.
  The polymorphism lives in how one is *constructed* (`fs.open`, `io.stdin`, `io.stdout.buffered()`,
  …), not in the type: one type, many constructors — "one way."
- **Read = view or explicit owned; write = sink.** A read fills a caller-owned `mut buffer` (no
  hidden allocation) or, for a whole file, returns an explicit owned value. A write always appends
  into a sink (`writer` / `builder`) — never a hidden allocate-and-return.
- **Mechanism may be hidden; cost class may not.** Which syscall serves a read/write/copy is an
  implementation choice, not a language contract — hiding the *mechanism* doesn't violate "nothing
  hidden" (that principle governs allocation / errors / effects / parallelism / `unsafe`, not
  syscall selection). What must stay visible **in the spec** is the **cost class**: `io.copy` is
  documented `O(buffer)` memory, never `O(file size)`.
- **Implementation follows the `core.json` precedent.** Each of these modules is Rust runtime
  (`align_rt_*`) + sema builtin dispatch + a required `import`, exactly like `core.json` — not yet
  Align-over-FFI library code. "`std` as a real library" stays a Future item (`docs/open-questions.md`
  "Transparent zero-copy I/O"), not this milestone's job.

### Error mapping (all of std)

Recoverably fallible `std` functions return `Result<T, Error>` (the builtin `Error` sum type,
§4 "Result"/`open-questions.md` "Error type design"). An absence-only query may return `Option<T>`, and an
operation specified as total returns its value directly. Programmer errors abort rather than
returning `Error`. A failing syscall in a `Result`-returning operation maps its `errno` through
**one fixed table**, the same everywhere — not a per-module ad hoc mapping ("one way"):

```text
ENOENT           -> Error.NotFound
EACCES, EPERM    -> Error.Denied
EINVAL           -> Error.Invalid
(anything else)  -> Error.Code(errno)
```

`Error.Timeout` is **not** part of this errno table — an `ETIMEDOUT` from an unrelated syscall still
maps to `Error.Code`. `Timeout` is produced only where a deadline is enforced explicitly (a
`std.process` run's `timeout_ns`; the `std.http`/`std.net` I/O timeouts), so a caller can tell "the
operation timed out" apart from any other failure.

### std.io

```text
reader
writer
file      // random-access block read+write handle (offset-addressed; no cursor / no seek)
stream    // surface not yet specified
```

```text
io.stdin                    -> reader
io.stdout                   -> writer
io.stderr                   -> writer
io.stdout.buffered()        -> writer   // buffering is a writer, not a separate type — "one way"
r.read(b: mut buffer)       -> Result<i64, Error>   // fills b up to its capacity, overwrites b's len;
                                                     // returns bytes read, 0 = EOF
r.buffered()                -> reader   // upgrade a reader to carry a lookahead (the read dual of the
                                        // buffered writer); consumes r, yields a buffered reader over
                                        // the same fd. Required before read_line.
r.read_line(b: mut buffer)  -> Result<i64, Error>   // (buffered reader only) fills b with the next line
                                                     // BODY, terminator already stripped (b.len() = body
                                                     // length); returns bytes consumed incl. terminator,
                                                     // 0 = EOF. b GROWS as needed (up to a 64 MiB line
                                                     // cap -> Error.Invalid).
bytes.as_str()              -> Result<str, Error>   // validate UTF-8, yield a zero-copy str VIEW of the
                                                     // same bytes (region-bound); Error.Invalid on bad
                                                     // UTF-8. The one bytes->text path.
w.write(x: str | bytes | builder) -> Result<(), Error>
w.flush()                   -> Result<(), Error>
io.copy(r: reader, w: writer) -> Result<i64, Error>   // returns bytes transferred; memory is always
                                                       // O(buffer), never O(file size)
f.pread(b: mut buffer, off: i64)  -> Result<i64, Error>   // one positionless read at off into b's
                                                          // window; returns the actual count, 0 = EOF
f.pwrite(data: bytes, off: i64)   -> Result<i64, Error>   // writes ALL of data at off (loops to full);
                                                          // returns the full len; past-EOF extends
f.len()                           -> Result<i64, Error>   // live fstat (not cached)
```

Every native operation that fills a `buffer` (`read`, `read_line`, `pread`, `recv_from`, and
`crypto.random`) requires a bare source local declared with `mut`. Bind a temporary first; passing
an immutable local or an expression directly is rejected before the operation is formed. This keeps
the mutation, address stability, and ownership boundary explicit.

`file` is the offset-addressed block read+write handle (`fs.create_rw` / `fs.open_rw`, below). Every
access carries an explicit `off` — there is **no cursor and no `seek`** (a settable cursor is hidden
mutable state), and there is **no read-only constructor** (pure random reads stay `reader` or the
`fs.read_bytes_view` mmap view — a third read path would break "one way"). A **negative** offset is a
programmer bug and **aborts**. `file` is Move (owns its fd, `Drop` closes it) and structurally
single-threaded (no `par_map`/`spawn` capture); it never rides an aggregate other than its
constructor's `Result<file, Error>`.

`reader`/`writer` are the concrete Move types from "I/O design principles" above. `io.copy`
dispatches on fd kind internally (a portable fixed-buffer loop is the v1 / reference
implementation; a `sendfile`/`splice`/mmap fast path may follow, validated against that loop,
without changing this signature — `docs/open-questions.md` "Transparent zero-copy I/O").

**v1 restriction (temporary implementation limit, not a design choice):** an **owned** handle —
`reader` / `writer` (from `fs.open` / `fs.create`) or a `buffer` — must be bound to a local before a
method is called on it: `w := fs.create(p)?` then `w.write(d)?`, not `fs.create(p)?.write(d)?`; `b
:= buffer(n)` then `b.bytes()`, not `buffer(n).bytes()`. An unbound owned handle never runs its
`Drop`, which silently loses a buffered `writer`'s output, leaks a `reader`'s fd, and leaves a
`buffer`'s `.bytes()` slice dangling. The **unbuffered** borrowed standard streams (`io.stdin` /
`io.stdout` / `io.stderr`) own no fd and hold no buffer, so they may be used inline (chained method
call, or passed to `io.copy`) directly. A **buffered** std writer (`io.stdout.buffered()`) is
subject to the same rule as an owned handle: it accumulates bytes that reach the OS only on
`flush`/`Drop`, so it must be bound to a local (`w := io.stdout.buffered()` then `w.write(d)?` /
`io.copy(r, w)?`) — an unbound temporary would silently drop its tail chunk. This restriction lifts
once Move *temporaries* get a `Drop`.

**Streaming line reads.** Line reading needs *lookahead* (bytes past a `\n` must survive to the next
call), so it is built explicitly rather than hidden on the raw handle: `r.buffered()` is the read dual
of the buffered `writer` (one type, many constructors), and `read_line` is a **buffered-reader-only**
method — a sema error on an unbuffered reader. On a buffered reader every `read`/`read_line` drains the
lookahead before touching the fd (a `read` after a `read_line` sees the retained surplus, never
fd-fresh bytes). `read_line` fills `b` with the line **body**, its terminator already stripped: exactly
one `\r?\n` is removed (a lone `\r` is *not* a terminator; a BOM is *never* stripped — no hidden
transformation, so `json.decode` fails on line 1 if a BOM is present and stripping it is the consumer's
call), so `b.len()` is the body length and the return is the bytes consumed including the terminator (`0`
= EOF, an empty line returns `1` with body length `0`, a final unterminated line returns its bare
length). **Growth asymmetry:** unlike `r.read`, which caps at the buffer's capacity, `read_line`
**grows** `b` as needed — a line has no caller-chosen bound — up to a 64 MiB line cap (`Error.Invalid`
beyond it, so a terminator-free/binary input can't grow the buffer without bound). The canonical loop
starts with `mut buf := buffer(4096)` and is `loop { n := r.read_line(buf)?; if n == 0 { break };
line := buf.bytes().as_str()?; … }`. The output must be that bare `mut` local. **Warning:** the
per-iteration line view (`buf.bytes()` / its `as_str`) must **not** be hoisted across iterations — the
next `read_line` overwrites `buf`, so a view kept from a previous line reads stale/overwritten bytes
(a borrow-liveness gap not yet caught by the compiler; `.clone()` a line you need to keep).

`bytes.as_str()` is the one validating bytes→text boundary op (the zero-copy **view** counterpart of a
copying `bytes`→`string` conversion): it checks the bytes are UTF-8 and returns a `str` view of the same
storage, region-bound through the receiver — a view of `buf.bytes()` stays pinned to the buffer, so it
cannot escape past the buffer's `Drop`. It works on any `bytes` (`slice<u8>`) value, not just a buffer's.

### std.fs

```text
fs.read_file(path: str)      -> Result<string, Error>
fs.read_file_view(path: str) -> Result<str, Error>
  // an mmap view: requires an enclosing arena — the region is bound to the arena, munmap runs at
  // arena end (the same shape as M3's heap.new requiring an arena). Escapes the region via .clone().
fs.read_bytes_view(path: str) -> Result<bytes, Error>
  // the binary sibling of read_file_view: the same arena mmap (regular-file fast path, owned-copy
  // fallback for special / zero-length files, munmap at arena end) minus the UTF-8 validation, so a
  // binary asset (a GGUF model, a packed index) maps zero-copy as a `bytes` (slice<u8>) view that a
  // `str` view would reject. Same arena region rule — the view cannot escape the arena.
fs.write_file(path: str, data: str | bytes | builder) -> Result<(), Error>
fs.open(path: str)   -> Result<reader, Error>
fs.create(path: str) -> Result<writer, Error>
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
fs.create_rw(path: str) -> Result<file, Error>   // O_RDWR|O_CREAT|O_TRUNC — a fresh random-access file
fs.open_rw(path: str)   -> Result<file, Error>   // O_RDWR, must exist — in-place update (see std.io `file`)
fs.exists(path: str) -> bool
fs.remove(path: str) -> Result<(), Error>
fs.read_dir(path: str) -> Result<array<string>, Error>   // v1: owned strings
```

`fs.create_exclusive` creates one new regular file entry with the native
exclusive-create operation (`O_CREAT|O_EXCL`, plus close-on-exec and final
component no-following on the accepted Unix targets). An occupied final entry
—including a regular file, directory, symlink, FIFO, or device—returns
`Error.Code(native EEXIST)` and is never opened, truncated, replaced, or
removed. The returned value is the existing owned `writer`; its `Drop` closes
the descriptor and does not remove the file. A failed write or flush may leave
a partial file for the caller to remove explicitly. The path is borrowed only
for the call: it must be non-empty, valid UTF-8, and contain no NUL, and the
runtime uses an ephemeral bounded NUL-terminated copy without lexical
normalization or hidden parent checks.

`fs.rename_no_replace` performs one native no-replace directory-entry rename
(`renameat2(..., RENAME_NOREPLACE)` on Linux or
`renameatx_np(..., RENAME_EXCL)` on macOS). It succeeds only when the
destination entry is absent; an occupied destination returns
`Error.Code(native EEXIST)` and is not followed, removed, or replaced. The
source is moved as an entry, so a source symlink or special file is not opened
or inspected first. Missing source, cross-device, unsupported-volume, parent,
permission, and other failures use the fixed errno table above. The operation
does not emulate across filesystems, promise crash durability, or roll back a
previous operation. Both borrowed paths are validated and marshalled in
source-before-destination order before the native call; a checked `len + 1`
capacity overflow is `Error.Invalid`, while actual allocation failure follows
the language's terminal OOM policy.

These operations are independent primitives, not a pair transaction. A
consumer that publishes two artifacts must close both writers, recheck both
final paths, rename result then evidence, and explicitly clean only its own
temporary or already-published paths. The consumer owns any single-writer and
trusted-path precondition; `std.fs` does not classify filesystems or provide a
hidden cleanup lock.

`fs.open_beneath` and `fs.create_exclusive_beneath` are separate retained-root constructors. The
root may be absolute, relative, exactly `.`, or exactly `/`; the second argument is a non-empty
strict relative path. Both reject empty, `.` or `..` components, validate both complete inputs
before filesystem access, retain and walk directory descriptors without following root or
intermediate symlinks, and never change process cwd or install a global root. `open_beneath`
observes and opens the final entry without following it, requires the same regular-file identity
before and after open, reads no byte in the constructor, and returns the existing owned `reader`.
`create_exclusive_beneath` performs one exclusive no-follow create from the retained final parent
and returns the existing owned `writer`; every occupied final entry remains unchanged. Missing,
denied, invalid/symlink/type, and other native failures use the fixed `Error` mapping. Both paths
are borrowed only for the call; checked copy overflow is `Error.Invalid`, actual OOM is terminal,
and all temporary path storage and directory descriptors are released on recoverable exits. The
operations add no public directory handle, metadata API, canonical path, sandbox, parent creation,
rollback, durability, or multi-file transaction. A same-final open/create pair has no implicit
exclusion or snapshot: open returns `NotFound` if it observes absence, or may acquire the newly
created regular inode while the writer is still live. An immutable-input consumer must reject that
overlap itself.

Any read that yields a `str`/`string` (`read_file`, `read_file_view`, and a decoded `str` from `json.decode`) validates the bytes as UTF-8 — `str` is always valid UTF-8 (§7, §12), so non-UTF-8 content fails with `Error.Invalid`; read binary zero-copy with `read_bytes_view` (a `bytes` mmap view, no validation) or into an owned buffer with `reader.read(buffer)` — `bytes`/`buffer` carry no UTF-8 invariant. `read_bytes_view` shares `read_file_view`'s v1 limitations: special / zero-length files fall back to an owned arena copy (not zero-copy), and concurrent truncation of a mapped file can raise `SIGBUS` (no handler is installed — a process-global signal handler is the hidden side effect Align forbids). For the same reason `read_dir` **excludes** any directory entry whose name is not valid UTF-8 (it cannot be a `string`, and is unreachable through a `str` path regardless).

### std.path

```text
path.join(a: str, b: str) -> string
path.base(p: str) -> str   // zero-copy substring view of p — the existing str-view region rule
path.dir(p: str)  -> str   // ditto
path.ext(p: str)  -> str   // ditto
path.normalize(p: str) -> string
```

### std.process

```text
process.spawn(cmd: str, args: array<str>) -> Result<child, Error>  // fork+exec; the child owns its pid
ch.wait() -> Result<i64, Error>          // reap; returns the exit code
ch.kill(sig: i64) -> Result<(), Error>
process.exec(cmd: str, args: array<str>) -> Result<(), Error>      // replace this image (returns only on error)
process.exit(code: i64)                  // run cleanup (flush buffered output), then exit
process.abort()                          // immediate `_exit(1)`, NO cleanup
process.cpu_count() -> i64               // parallelism available to THIS process (affinity/quota aware, >= 1)
```

`child` is a **Move** handle owning a pid. Explicit `wait()` returns the exit code; Drop without a
`wait()` still reaps the child with a blocking `waitpid` (discarding the code) so it can never become
a zombie. `exit` and `abort` are the deliberate pair: `exit` runs ordinary cleanup so a buffered
`print` is flushed, `abort` is the no-cleanup escape.

`cpu_count` is the number a `task_group` worker count is sized against — the runtime schedules a
group's tasks on a pool sized from exactly this source, so a set of never-returning tasks larger
than it would leave the extra ones unstarted.

**Captured output.** `spawn` inherits the terminal, so reading a child's output goes through the
`command` builder instead:

```text
c := process.command(cmd: str, args: array<str>) -> command   // Move handle
c.cwd(dir: str)                    // set the child's working directory  -> ()
c.env(name: str, value: str)       // add/override one variable          -> ()
c.env_clear()                      // start the child environment empty  -> ()
c.timeout_ns(ns: i64)              // kill + Err(Timeout) past ns; 0 = no timeout  -> ()
c.max_capture_bytes(limit: i64)    // per-stream stdout/stderr bound; explicit 0 = empty only  -> ()
out := c.run() -> Result<run_output, Error>   // fork + capture; borrows c, so it is re-runnable
raw := c.run_bytes() -> Result<run_bytes, Error> // same capture as bytes; borrows c

out.code()   -> i64
out.stdout() -> str    // captured stdout, a zero-copy view region-tied to `out`
out.stderr() -> str    // ditto
raw.code()   -> i64
raw.stdout() -> slice<u8> // arbitrary bytes, zero-copy and region-tied to `raw`
raw.stderr() -> slice<u8>
```

The config methods are in-place setters on a bound local and yield `()`. A negative `timeout_ns`
or capture limit is a programmer error and aborts at the call. An unset capture limit preserves the
existing unbounded behavior; a selected non-negative limit applies independently to stdout and
stderr, exact-limit output succeeds, and the first byte beyond either limit signals the owned process
group when present, kills and reaps the direct child, discards both partial streams, and returns
`Error.Invalid`. A selected `0` therefore accepts
only two empty streams. `run` is where `Error.Timeout` (§4 "Result") is produced: the drain checks
the deadline before each poll/read checkpoint and through direct-child wait after pipe EOF, so an
already-observable timeout wins over cap overflow; either error is returned without a half-answer.
Hard pipe/wait failures signal the owned group when present, kill and reap the direct child, and
return their fixed `Error.Code` without partial output. `run_output` and `run_bytes` are Move
handles, not by-value structs, because each owns both captured buffers. The text handle validates
UTF-8 and returns `Error.Invalid` for invalid bytes; the byte handle preserves arbitrary bytes and
embedded NUL. All four output views are region-bound to their handle.

```align
c := process.command("git", ["git", "status", "--porcelain"])
c.cwd(repo_dir)
c.timeout_ns(30_000_000_000)          // 30 s
out := c.run()?
```

### std.env

```text
env.get(name: str) -> Option<string>
env.set(name: str, value: str) -> Result<(), Error>
```

`args` is deliberately **not** here: `main(args: array<str>)` (§17/§19) is the one way to reach
argv — there is no `env.args`.

### std.time

```text
time.now()      -> i64   // wall clock, UNIX epoch nanoseconds
time.instant()  -> i64   // monotonic nanoseconds
time.sleep(ns: i64)
```

One duration representation, an `i64` nanosecond count — there is no `Duration` type ("one way").

### std.net

Low-level focused.

```text
tcp
udp
dns
socket
```

### std.cli

A parser over `main(args: array<str>)`'s `array<str>` (§17) — the one way to reach argv (`std.env`
above: "there is no `env.args`"); `std.cli` is not a second argv source.

```text
c := cli.command(name: str)                    // builder
c.flag_bool(name: str)                          // register a bool flag (default false)
c.flag_str(name: str, default: str)             // register a str flag with a default
c.flag_i64(name: str, default: i64)             // register an i64 flag with a default
c.parse(args: array<str>) -> Result<parsed, Error>
p.get_bool(name: str) -> bool                   // total after a successful parse (see below)
p.get_str(name: str) -> str
p.get_i64(name: str) -> i64
c.usage() -> string
```

**Lookups are total, not fallible — validation happens once, at `parse`.** Every registered flag
has a value after `c.parse(args)?` succeeds: the one given on the command line, or the default from
its `flag_*` registration (`bool` defaults to `false`). So `p.get_bool`/`get_str`/`get_i64` never
fail and return the value directly — the same shape as `json.decode`, where decoding validates the
whole input and field access is then a plain read. But the lookup itself is only **checked at
runtime**: `p.get_bool(name)`/`get_str`/`get_i64` return the parsed value or default for a
registered flag, and a `get_*` call for a name that was never registered, or against the wrong
flag's type, **aborts at runtime** — the same "programmer error aborts, never silently misbehaves"
rule as out-of-bounds indexing or div-by-zero. Align has no comptime evaluator, so there is no
compile-time way to check a `get_*` call against the flag set the builder happened to register at
runtime — a static check here would need flow-dependent typing (the value registered by a prior
`flag_*` call determining what a later `get_*` call may accept), which is a second, ad hoc type
mechanism and so contradicts One way. All *input* errors (unknown flag, missing value, wrong kind,
bad `i64` literal) are surfaced by `parse` as `Error.Invalid` — the same fixed mapping as every
other `std` fn — with `c.usage()` available to render help. Once a derive/declarative flag-spec
mechanism exists (see below), `get_*` calls against it can move to compile-time validation; this
runtime-checked lookup is the v1 shape for the explicit builder.

**`parse` borrows the command, it does not consume it.** `c.parse(args)` reads the registered-flag
table by borrow, so `c.usage()` stays callable *after* `parse` — including on the `Err` path, which
is exactly when help is wanted. (If `parse` moved `c`, a parse failure would leave you unable to
render usage.)

**v1 argv grammar.** `parse` treats `args[0]` as the program name (the `main(args)` convention) and
reads flags from `args[1..]` in three forms: `--name` (a bool flag, set to `true`), `--name value`
(the value is the next token), and `--name=value` (str/i64 flags). A bool flag takes no value; a
str/i64 flag with no following value, an unknown flag, a bare positional token, or a malformed `i64`
is an *input* error (`Error.Invalid`). Richer conventions (`-x` short flags, `--` end-of-flags,
clustered bools, positionals) are deferred behind this same signature.

**v1 is an explicit flag-registration builder API.** Align has no derive/attributes yet, so
decoding straight into a struct (the `json.decode`-shaped ideal) waits for that mechanism. This API
shape is a v1 provisional: builder vs. a declarative spec is revisited once derive lands.

### std.encoding

```text
encoding.base64_encode(data: bytes) -> string          // owned; standard alphabet + padding
encoding.base64_decode(s: str) -> Result<buffer, Error> // invalid input -> Error.Invalid
encoding.base64url_encode(data: bytes) -> string        // URL-safe alphabet, no padding
encoding.base64url_decode(s: str) -> Result<buffer, Error>
encoding.hex_encode(data: bytes) -> string
encoding.hex_decode(s: str) -> Result<buffer, Error>
encoding.percent_encode(data: bytes) -> string          // RFC 3986 URI component; %XX, upper-case
encoding.percent_decode(s: str) -> Result<buffer, Error> // `%` not followed by 2 hex -> Error.Invalid
encoding.form_encode(data: bytes) -> string             // x-www-form-urlencoded; space -> `+`
encoding.form_decode(s: str) -> Result<buffer, Error>    // `+` -> space, %XX -> byte
encoding.html_escape(data: bytes) -> string             // & < > " ' -> entities (text + attribute safe)
encoding.utf8_valid(b: bytes) -> bool                   // check before turning bytes into str
```

Decode returns an owned `buffer` — `bytes` carries no UTF-8 invariant, so a decoded blob is not a
`str` — consistent with the sink/owned-return convention above. SIMD (Lemire's
Base64-at-memcpy-speed) is an internal optimization; it does not change these signatures. Encode
returns `string`; a builder-sink variant is a later addition once bulk-output demand appears.

### std.regex

Regular expressions are a standard-library facility, not language syntax. Patterns are ordinary
`str` values compiled explicitly into an owned Move handle:

```text
regex.compile(pattern: str) -> Result<regex, Error>
re.is_match(text: str) -> bool
re.find(text: str) -> Option<regex_match>
re.find_at(text: str, start: i64) -> Option<regex_match>

regex_match { start: i64, end: i64 }
```

`regex` owns a compiled automaton and is `Drop`-freed; search methods borrow it, so callers compile
once and reuse it explicitly. `regex_match` is a builtin Copy struct. Its half-open `start..end`
positions are UTF-8 **byte offsets** and always character boundaries. `find_at` takes the same kind
of byte offset; a negative, past-end, or non-character-boundary start is a programmer error and
aborts like an invalid slice boundary. Invalid pattern syntax, a pattern over 64 KiB, or a compiled
program over the 10 MiB engine limit yields `Error.Invalid`.

The v1 engine is the Rust `regex` crate: automata-based, worst-case linear in searched input for a
fixed compiled pattern, with Unicode enabled by default. Look-around and backreferences are not
supported; this is intentional to retain predictable search complexity. There is no implicit
global cache, no `rx"..."` literal, and no compiler-time regex execution. Captures, replacement,
iteration, and split remain additive library slices after a concrete application consumer appears.

### std.compress

```text
gzip
zstd
```

### std.rand

Non-cryptographic use (`std.crypto` is the cryptographic counterpart).

```text
rand.seed() -> rng             // OS getrandom-seeded
rand.seed_with(s: i64) -> rng  // deterministic (tests / reproducibility)
r.next() -> i64                    // rng is a mut receiver; state advances each call — Xoshiro256++ class
r.range(lo: i64, hi: i64) -> i64   // uniform [lo, hi), bias-free (Lemire nearly-divisionless)
r.shuffle(out xs: slice<T>)        // Fisher-Yates
r.sample(xs: slice<T>, k: i64) -> array<T>   // k items, without replacement
```

`rng` is a **Copy** value — a small state-only struct, no fd/ownership — deliberately unlike the
Move `reader`/`writer` handles: it holds no external resource, so Copy is the right default, not a
special case. Methods take a `mut` receiver to advance the state. The OS seed comes from
`getrandom`/`urandom` — never raw `RDRAND`/`RNDR` (outside the x86-64-v2/armv8-a baseline, `SIGILL`
on older silicon; `docs/open-questions.md` #342). `rand.seed`'s OS call is assumed not to fail in
practice; a real failure **aborts** (it is not surfaced as a `Result` — seeding is not a fallible
user-facing operation), like the other runtime traps.
`lo >= hi` on `r.range` is a programmer error and aborts at runtime (like out-of-bounds indexing /
div-by-zero) — there is no non-empty range to draw from. Likewise `r.sample`'s `k < 0` or
`k > xs.len()` aborts at runtime: sampling more distinct items than exist, without replacement, is
impossible.

### std.crypto

Cryptographic use. EVP-backed operations use OpenSSL libcrypto, which the compiler links only when
a used capability requires it. Most operations work with OpenSSL ≥ 3.0; `argon2id` requires the
`ARGON2ID` provider added in OpenSSL 3.2 and returns `Error.Code` when that provider is unavailable.
See `docs/impl/std-design/crypto.md` for the full design. `constant_time_equal` is the one
self-hosted primitive and its branchless (constant-time) property is verified against the compiled
machine code, not just the source.

```text
crypto.random(out: mut buffer)                                  // OS CSPRNG fill
crypto.sha256(data: bytes) -> array<u8>                         // 32-byte digest
crypto.sha512(data: bytes) -> array<u8>                         // 64-byte digest
crypto.blake3                                                   // deferred: no system engine provides BLAKE3
crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>        // 32-byte tag
crypto.hkdf_sha256(salt: bytes, ikm: bytes, info: bytes, len: i64) -> Result<buffer, Error>
crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>
crypto.aes_gcm_seal(key: bytes, nonce: bytes, plaintext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.aes_gcm_open(key: bytes, nonce: bytes, ciphertext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.chacha20_poly1305_seal(...) / _open(...)                 // same shape as aes_gcm
crypto.constant_time_equal(a: bytes, b: bytes) -> bool          // CT — self-hosted
```

The asymmetric signature extension adds six algorithm-and-class-specific Move key types and the
following exact surface. It is designed; implementation is pending.

```text
crypto.rs256_private_key_from_pem(pem: str) -> Result<rs256_private_key, Error>
crypto.es256_private_key_from_pem(pem: str) -> Result<es256_private_key, Error>
crypto.ed25519_private_key_from_pem(pem: str) -> Result<ed25519_private_key, Error>
crypto.rs256_public_key_from_pem(pem: str) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_pem(pem: str) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_pem(pem: str) -> Result<ed25519_public_key, Error>
crypto.rs256_public_key_from_jwk(n: bytes, e: bytes) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_jwk(x: bytes, y: bytes) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_jwk(x: bytes) -> Result<ed25519_public_key, Error>
crypto.rs256_sign(borrow key: rs256_private_key, message: bytes) -> Result<buffer, Error>
crypto.es256_sign(borrow key: es256_private_key, message: bytes) -> Result<buffer, Error>
crypto.ed25519_sign(borrow key: ed25519_private_key, message: bytes) -> Result<buffer, Error>
crypto.rs256_verify(borrow key: rs256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.es256_verify(borrow key: es256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.ed25519_verify(borrow key: ed25519_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
```

RS256 is RSASSA-PKCS1-v1_5 with SHA-256. ES256 is P-256 ECDSA with SHA-256 and uses the
JOSE 64-byte `r || s` signature rather than DER. Ed25519 is pure Ed25519 with no caller-selected
digest, context, or prehash. The PEM constructors accept one bounded unencrypted PKCS#8
`PRIVATE KEY` or SPKI `PUBLIC KEY`; encrypted/traditional/certificate/OpenSSH forms reject without
a password or ambient I/O. The JWK constructors take already-base64url-decoded public fields and
perform complete algorithm/size/point validation. Each key is an independent Move owner of a shell
that pins a private OpenSSL context and its built-in default provider; every fetch uses exact
`provider=default`, and global provider configuration cannot substitute it. Ed25519 point admission
independently checks canonical RFC 8032 recovery and rejects small-order public points rather than
delegating that invariant to `EVP_PKEY_public_check`. Sign/verify borrow the key and complete message.
Constructor/key-format or malformed internal ABI
input is `Error.Invalid`, a provider/allocation failure is `Error.Code(0)`, and every signature
length/encoding/mathematical mismatch after valid byte views is `Ok(false)`. Key construction is a
trusted setup operation with no timing promise. Signing an admitted key is constant-time with
respect to private-key/message contents at fixed public lengths under the pointer-verified built-in
OpenSSL default-provider dependency. The exact PEM grammar, RSA bounds, validation order,
ownership/allocation rules, ABI,
cache identity, timing boundary, and closure matrix are authoritative in
`docs/impl/std-design/crypto.md`.

`argon2_params { m_cost: i64, t_cost: i64, parallelism: i64, len: i64 }` is a builtin struct
(reserved name, ordinary struct literal): memory cost in KiB, iterations, lanes, and output
length in bytes. AEAD uses 32-byte keys and 12-byte nonces; seal output is
`ciphertext || 16-byte tag` (one buffer), and `open` is all-or-nothing — any failure returns a
single opaque `Error.Invalid` with zero plaintext bytes released. Nonce reuse under the same key
is catastrophic; v1 does not auto-generate nonces (pair with `crypto.random`).

### std.http

A primitive, not a framework.

The whole-body client has explicit receive limits at both reusable-client and request scope:

```text
cl.max_response_body_bytes(limit: i64) -> ()
r.max_response_body_bytes(limit: i64) -> ()
```

Zero clears the client limit or makes a request inherit its client. A positive request limit can
only narrow the positive client limit or the fixed 1 GiB default. Invalid values abort before state
changes. The cap is enforced while Content-Length, chunked, or close-delimited payload bytes are
received; exact fit succeeds, while the first recognizable excess returns the reserved
`Error.Code(-1)` with no partial response and a closed connection. Bodyless `HEAD`/`204`/`304`
responses validate their framing metadata but do not compare a valid declared magnitude with the
cap. A configured 262,144-byte cap bounds aggregate live Align-owned response storage to 557,056
bytes: body cap + 262,144-byte cumulative head allowance + one 32,768-byte read scratch. The full
framing, allocation, cleanup, batch, TLS, and error-precedence contract is the Request 5 ledger in
`docs/impl/std-design/http.md`.

The implemented post-`pkg.db` streaming receive surface keeps that whole-body terminal unchanged:

```text
cl.request_stream(req: request) -> Result<http_read_stream, Error>
stream.status() -> i64
stream.header(name: str) -> Option<str>
stream.read(out: mut buffer) -> Result<i64, Error>  // de-framed bytes; 0 = complete
events := stream.sse()                              // consumes stream
events.status() -> i64
events.header(name: str) -> Option<str>
events.last_event_id() -> str
events.retry_ms() -> Option<i64>
events.next(out: mut buffer) -> Result<Option<http_sse_event>, Error>

http_sse_event { event: str, data: str, last_event_id: str, retry_ms: Option<i64> }
```

Both streams are dependent Move resources: they own the checked-out connection and decoder state,
retain a shared borrow of the creating client, and return an exactly completed self-delimited
connection to that client's pool. Mid-body Drop closes without draining. Raw reads overwrite a
caller-owned fixed-capacity buffer and expose no HTTP chunk/trailer framing. A positive selected
`max_response_body_bytes` remains a cumulative decoded-byte cap; when both scopes are unset, a
stream has no cumulative total cap because it never materializes the whole body. The per-operation
timeout snapshot remains active on every later receive. Each body-facing call gets a fresh 262,144
byte chunk-framing work allowance, so indefinite chunked streams do not inherit the whole-message
framing counter while one call remains bounded. Its complete finite storage grammar is
`C ::= stream | Option<C> | Result<C,N> | Result<N,C> | Result<C,C>`, where `N` contains no stream.
Only builtin-tag edges carry the creating-client dependency. One cycle-safe exhaustive classifier
over every type discriminator rejects any other storage edge by default, including user
structs/sums, anonymous tuples, collections, boxes, builders, and tasks; a future type constructor
must be classified before the compiler builds. `C` may be a local, by-value/borrow/borrow-mut
parameter, or function result. An out parameter, global/constant, user native/extern signature, or
borrowed-place owning projection rejects; ordinary return and consuming match are the only transfer
paths. Closure/task captures and parallel elements/results reject separately. `request_stream`,
`read`, and `next` are Impure; the ownership-only `sse` transition and head/state getters are Pure.

`sse()` consumes the raw reader at its current logical body position and prevents later raw/SSE
mixing. `next` applies the WHATWG UTF-8, BOM, CRLF/LF/CR, comment, field, blank-line dispatch, data
join, persistent last-event-id, and retry rules. It interprets only the event stream: HTTP status,
media-type validation, redirects, reconnects, waits, and `Last-Event-ID` request construction remain
explicit caller policy. Stream accessors expose the latest id/retry state even when a control-only
block precedes completion. Event bytes are written into the supplied buffer in
`event || data || last_event_id` order; the three `str` fields are zero-copy views tied to that fresh
buffer generation, while `retry_ms` is inline Copy data. Exact capacity succeeds. ID/retry changes
in a control-only block commit at its blank line. A data-bearing block commits them atomically with
successful event publication; an output/body/framing/work/timeout/transport failure or incomplete
EOF rolls back only the pending block, preserving earlier control-only commits so reconnect state
cannot skip an undelivered event. A cumulative body-cap or event-output-cap excess is the stable
explicit HTTP receive-bound result `Error.Code(-1)`, with no partial publication and a closed
connection. Independently, one `next` scans at most the output capacity plus 262,144 de-framed
source bytes; comments, unknown fields, invalid retry lines, and control-only blocks consume that
allowance, and excess is `Error.Invalid`. The exact ownership, parsing, ABI, allocation,
validation-order, and acceptance ledger is `docs/impl/std-design/http.md` “Client streaming
receive.” This surface was implemented on 2026-08-30.

```text
request
response
header
method
status
client
server primitive
```

The server primitive streams a response via `http_stream` (SSE / chunked). `respond_stream` consumes
a **header-only** response builder (a bodied builder aborts — the body is streamed, not preset) and
**borrows** the request context: the connection is lifted into the stream and the context stays with
the caller, *spent* (its request views remain valid while streaming; a later `respond` on it is
`Err`). The head is **lazy** — validated immediately, written by the first `send` (or `finish`); until
then `reject` may answer with a complete normal response instead (the pre-stream 4xx window). A 1.1
client gets `Transfer-Encoding: chunked`, a 1.0 client close-delimited raw. `finish` is the sole
clean terminator (Drop closes without a terminal chunk — abrupt close is chunked's own truncation
signal).

```text
ctx.respond_stream(rb: response_builder) -> Result<http_stream, Error>  // consumes rb; borrows ctx (spent)
s.send(chunk: bytes) -> Result<(), Error>       // one chunk frame, one write; send("") = no-op
s.finish() -> Result<(), Error>                 // consumes s; writes 0\r\n\r\n (framed) + closes
s.reject(rb: response_builder) -> Result<(), Error> // consumes s + rb; pre-first-send only:
                                                    //   discard the head, answer normally, close
```

---

## 18.3 pkg

`pkg` is the third-party / framework layer — DB drivers, web frameworks, and ecosystem libraries
that are deliberately **not** in `core`/`std`. The building blocks that make them easy to build *are*
in core/std (`bytes`, `buffer`, `builder`, `arena`, `json`, `reader`/`writer`, the `http` primitive,
`crypto`, `encoding`), so a `pkg` library is ordinary Align that needs no privileged surface.

The first-party packages developed in this repository are exactly two subtrees:

```text
pkg.web            // the zero-copy REST framework (routing included; no separate pkg.router)
pkg.db             // the common driver surface: db.value, db.row, db.Driver, db.Error
pkg.db.sqlite      // driver submodule
pkg.db.postgres    // driver submodule
pkg.db.pool        // explicit fixed-capacity connection pool
```

`pkg/db` is **one vendorable subtree with four public module boundaries**, not four independently
versioned packages; the root owns the semantic contracts and the closed internal resource dispatch,
and the driver submodules own connection construction and native options. Further drivers
(`pkg.db.mysql`, `pkg.db.odbc`, `pkg.db.duckdb`) and any ecosystem package (an RPC layer, a cloud
SDK, a model client) are ordinary third-party `pkg` subtrees on the same two path rules — the
language reserves no names for them.

A package is a **distribution-layer** concept, defined entirely by §17 "Packages": it is the module
subtree under `pkg/<name>/` (root `pkg/<name>.align` + optional submodules), discovered from imports
+ the filesystem with no manifest. Its edges obey the two package path rules from §17 — the
**`internal`** rule (a `pkg.web.internal.*` module is importable only from within `pkg.web`) and
**layering** (a `pkg/` module imports only `core`/`std`/`pkg`, never the consuming project). Calls
stay fully qualified (`pkg.web.get(...)`); there are no import aliases. Vendoring a package is copying
its subtree into `pkg/`; one version exists per tree by construction. Compiled-library distribution
(shipping an interface + objects instead of source) is enabled by the per-unit interface summaries
but stays a future packaging exercise — source-first, fully greppable dependencies are the default.

**First-party packages** (developed in this repo, distributed with the system as vendorable subtrees)
live at the same depth as any other `pkg` — `pkg.web` is the flagship. They are ordinary pkg-layer
code, never ambiently resolvable: a consumer copies `pkg/web/` into their project exactly as they
would a third-party dependency.

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

  return Ok(())
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
