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

vecN<T>
maskN<T>
bitset
```

### Integer literals

Decimal, or base-prefixed `0x` (hex) / `0o` (octal) / `0b` (binary); `_` may separate digits in any
base. A literal's width is inferred from context like any literal; with no constraining context an
integer defaults to `i64` and a float to `f64` (a visible default — it affects overflow width /
precision). A *value* literal that provably does not fit its context type (`x: u8 := 300`, an
argument / field / array element / return value) is a **compile error**, not a silent wrap — cast
explicitly (`0xFFFFFFFF as i32`) if the wrapped bit pattern is wanted; `-128` is checked at its
effective value so it is a valid `i8`. Runtime arithmetic overflow still wraps, and an over-wide
`match` *pattern* literal still truncates to the scrutinee's type by the defined wrap rule. (`draft.md` §5.)

### Numeric conversion

No implicit coercion — not even widening. The explicit `as` operator is the **only** conversion,
between the numeric primitives (`i8..u64`, `f32`/`f64`) and `char`:

```align
b: i64 := a as i64        // widen (explicit); int→int truncates/extends with defined wrap
n := x as i32             // float → int truncates toward zero, saturating (no UB; NaN → 0)
code := 'A' as i32        // char ↔ int = the Unicode code point; char never pairs with a float
```

`bool` and composite types do not participate. Integer overflow is defined two's-complement wrap;
explicit `checked_*` / `saturating_*` / `wrapping_*` ops cover the rest. Division by zero is never
silent — a runtime `/`/`%` by zero aborts, a constant one is a compile error — but `INT_MIN / -1`
wraps to `INT_MIN` (`INT_MIN % -1` yields `0`), consistent with the two's-complement overflow rule;
only zero divisors abort. Unary `-` is signed, so negating an unsigned type (`x: u32 := -5`) is a
compile error, not a silent wrap — cast explicitly if the wrapped pattern is wanted. (`draft.md` §3.)

### Bitwise & shift

Integers have `&` `|` `^` `~` and the shifts `<<` / `>>` (integer-only — `bool` uses `&&`/`||`/`!`;
no implicit coercion, so the shift amount shares the value's type). Precedence is Go's: `<< >> &`
bind like `*`, `| ^` like `+`, so all of them bind tighter than comparison (`a & b == c` is
`(a & b) == c`). A shift amount is masked mod the bit width (defined, zero-cost); `>>` is arithmetic
on a signed value, logical on an unsigned one. The `bitset` type is built on these. The logical
`&&` / `||` **short-circuit** (the right operand runs only when the left doesn't decide the result),
so a guard like `i < xs.len() && xs[i] > 0` never indexes out of range; the bitwise `& | ` always
evaluate both operands. (`draft.md` §5.)

### Constants

A top-level `:=` (outside any function) is a **named constant**: the same keyword-less binding form,
evaluated at compile time and substituted as a literal at each use. It is immutable (no `mut`); its
value is a scalar / string built from literals, unary/binary operators, and other constants.

```align
WIDTH: i32 := 6
AREA := WIDTH * 7        // folded at compile time
GREETING := "hello"
```

A constant's type is fixed at the definition (it does not infer from a use site — it is stable
across modules), so an unannotated integer defaults to `i64` / a float to `f64`; annotate otherwise.
`pub` exports it; an importer names it qualified (`mod.NAME`), like a `pub` function/type. (`draft.md` §3/§4.)

An initializer may be an **array literal** — an aggregate constant, typed **`slice<T>` not `array<T>`**
(ownership is a property of the type, so a top-level constant owns nothing; like a `str` literal, it
is a `{ptr,len}` view of a per-unit read-only table, shared and never copied):

```align
PRIMES := [2, 3, 5, 7]          // slice<i64> (inferred)
SCALE: slice<f64> := [0.5, 1.0] // annotated element type
DAYS := ["Mon", "Tue"]          // slice<str>
```

Elements are scalars / `str` sharing one type (inferred or from the `slice<T>` annotation), each
folded like a scalar constant. A constant index folds to the element (`PRIMES[1]` is `3`, no load); a
dynamic index / `.len()` / slice / pipeline reads the table with no allocation. An `array<T>`
annotation is rejected. Deferred in an element: function calls, `as` casts, nested arrays, references
to other aggregate constants, and struct constants / elements.

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

### Loop

`loop { ... }` is the one sequential-control construct, and an expression: `break expr` ends the
loop with that value (bare `break` = `()`); all breaks in one loop unify like `match` arms; a
`loop` with no `break` diverges. There is no `for`, no `while`, no `continue`, no labeled break.
`?`/`return` exit the function; `break` is the only loop exit and cannot cross a lambda boundary.
The pipeline owns the data path; `loop` owns the control path (EOF pumps, retry, convergence) —
walking an array by index inside a `loop` is a lint. Recursion stays legal for recursive problems
(parsers, trees) but is not iteration: no tail-call optimization is guaranteed. See `draft.md` §4
"Loop".

### Display, literals, equality, shadowing, floats (2026-07-09)

`print(x)` takes primitives only, with a stable per-type display contract (floats = shortest
round-trip; `bool` = `true`/`false`; strings verbatim); printing an aggregate is a compile error.
String literals are single-line; escapes are `\n \t \r \0 \\ \" \' \u{...}` and an unknown escape
is a compile error; a `char` literal holds one Unicode scalar. `==` is scalars + strings only —
no structural equality (explicit fields / `match` / pipeline instead). No shadowing: a name binds
once per scope chain. Floats are IEEE 754 and never abort (`x/0.0` → `±inf`, NaN ≠ NaN); only
integer division aborts. `str`/`string` are `Ord` (byte-lexicographic; locale collation is a
library concern), so strings sort and compare. `else` works on `Result` as well as `Option` —
the intent triangle is `?` propagates / `else` falls back / `match` inspects. Details:
`draft.md` §4 / §5 / §12.

Implementation status: comparison operators and `Eq`/`Ord` bounds currently accept borrowed
`str`; direct owned-`string` comparison is still pending. To compare owned strings today, pass
them through a `str`-typed helper so the normal `string` → `str` borrow coercion is explicit in
the program's type flow.

### Generics

A function may declare type parameters — `fn f<T>(...)` — and is **monomorphized** per distinct
concrete instantiation (zero run-time cost; a Move `T` moves, a Copy `T` copies). Type arguments
are **inferred** (from arguments or the expected type via the binding annotation) — no turbofish.
A bare type parameter is **opaque**: passed / returned / stored by value, with no operations of its
own (`x + x` on a bare `T` is rejected). A **builtin bound** grants capabilities — `fn f<T: Bound>`
— in a fixed `Num ⊃ Ord ⊃ Eq` hierarchy: `Num` = arithmetic+ordering+equality (numbers), `Ord` =
ordering+equality (numbers, `char`, `str`), `Eq` = equality (numbers, `char`, `bool`, `str`). A type
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

Escape lifetime and cleanup provenance are separate inferred facts. A value-carrying block keeps
its trailing value's region; `if` and `match` take the shortest continuing-arm region;
`else`-unwrap takes the shorter of the payload and fallback; `?` keeps the `Ok` payload region.
For an owned value, the same selected edge forwards a path-local bit that distinguishes individual
heap ownership from arena ownership. Moves clear the source; scope exit drops only a live
individually owned value. A `mut` binding may therefore change allocation region when every assigned
value outlives its scope, without leaking heap storage or individually freeing arena storage.

### Error handling

```text
Result<T,E>
?
Error { NotFound, Invalid, Denied, Code(i32) }   // canonical builtin error sum type
```

No exceptions. `E` is any sum type (a domain may use its own error enum). `Error` is the builtin
error type — construct `Error.NotFound` / `Error.Code(c)` (`error(c)` is sugar), `match` it, and at
`main` it maps to the process exit code. Fallible builtins (`fs.read_file`, `json.decode`, …)
return `Result<T, Error>`. A fallible `main` (`fn main() -> Result<(), E>`) restricts `E` to the
builtin `Error` (the only type with a defined exit-code mapping; a user error enum there is a
compile error until the full `Error` design lands — convert with `map_err(to_error)?`). `?` requires the same `E` (no implicit conversion — convert explicitly
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

Sequential `map` / `where` / `reduce` / `scan` / `partition` / `any` / `all` callables may be
Impure. They run in input-index and stage order, exactly once for each element that reaches them; a
false `where` suppresses every later stage and reducer for that element. `any` / `all` do not
short-circuit. Effects restrict optimization legality, while explicit `par_map` still requires
Pure callables. Pure alone does not make a trapping or nonterminating call safe to speculate.
`sort` and `sort_by_key` are stable. A `sort_by_key` key callable may be Impure; it runs exactly once
for each surviving element, in input-index order, before any reordering, and sorting never calls it
again.

A pipeline **materializes** either into a fresh owned `array<T>` (`.to_array()`) or into a
caller-provided `out`/`mut` slice (`.map_into(dst)` — the caller-storage counterpart). `map_into` is
length-preserving (`map` / field-projection stages; `dst.len() == src.len()`, a mismatch aborts) and
yields `()`; because the compiler proves `dst` is a distinct buffer from the source (the `out`
no-alias rule), it emits the disjoint-buffer `noalias` so the fused write vectorizes with no runtime
overlap check. An `out` parameter (`fn scale(src: slice<T>, out dst: slice<T>)`) is a writable output
buffer that must not alias any other argument — both a safety constraint and the no-alias hint.

SIMD is two layers: the pipeline (`map`/`where`/`reduce`) is the width-agnostic main road — it never
names a width, so bulk vectorization (including future scalable ISAs, SVE/RVV) is chosen in the
backend and stays a hardware detail. `vecN<T>` / `maskN<T>` (below) are the fixed-size escape hatch
for hand-written register kernels. (`draft.md` §9.)

`array<T>` is row-major (array-of-structs); `soa<T>` is the explicit column-major (struct-of-arrays)
layout, so a field-wise pipeline streams only the columns it touches (the cache lever that beats an
AoS `Vec<Struct>`). Build one with `.to_soa()` (transpose an `array<Struct>`) or decode JSON into one
(`s: soa<User> := json.decode(d)?` counts the rows, allocates the columns, and fills them directly
without an AoS intermediate or transpose), both arena-allocated. `json.decode`'s field contract is strict and
exactly-once (a missing or duplicated declared field is an `Err`; undeclared keys are skipped),
enforced on both the strict fallback and the Mison speculative fast path (a duplicate at an unqueried
position is re-checked against the declared set and rejected). A struct field may itself be a
`Struct` — `decode` recurses into the nested object and `encode` renders it back (a nested record
round-trips; the strict contract recurses; nested `str` fields stay zero-copy views into the input).
A field may also be an `Option<T>` (payload scalar/`str`/nested struct): missing key or JSON `null`
→ `None`, type mismatch → `Err`, present → `Some`; `encode` omits a `None` field entirely, so
`decode(encode(x))` round-trips (a non-`Option` field still errors when missing). A field may also
be an owned `array<Struct>` (the `messages: array<Message>` shape) — decode fills an owned
array-of-structs in the field (freed by the struct's drop) and encode renders it back, so a full
OpenAI request/response round-trips. The element struct may itself be Move and is deep-dropped;
bare `array<string>` fields remain deferred. `soa<T>` columns stay primitive/`str`. The settled
completeness design (draft §14): a JSON `oneOf` maps to a sum
type discriminated by pairwise-distinct **shape classes** (compile-checked; O(1) dispatch, encode
writes the live payload bare); schema-unknown JSON is read through the zero-copy arena-backed
`json.doc` view (no serde-style value tree, no map type) — `d := json.doc(s)?` in an `arena {}`, then
total Missing-propagating navigation `d.get(k)` / `d.at(i)` (always a `json.doc`), `d.kind()` → the
builtin `json.kind` sum type, leaf accessors `as_str` / `as_i64` / `as_f64` / `as_bool` → `Option`,
`d.len()` / `d.key(i)` (objects-as-ordered-data), and `d.elems() -> slice<json.doc>` (materialize a
level once, then index/`len`/recurse — reuses the slice machinery, no new array type); `json.scan`
streams typed rows as a pipeline source. The
core.json surface is exactly `decode`/`encode`/`doc`/`scan` — `validate<T>`, `token`, and
`field_table<T>` are deleted. See `draft.md` §9, §14, §18.1.

`xs[i]` reads a bounds-checked element. A half-open range `xs[start..end]` slices instead: a
borrowed sub-view of a `str` (→ `str`) or an array / slice (→ `slice<T>`) — same storage, no
allocation, region-tied to the source. Bounds may be omitted (`xs[a..]`, `xs[..b]`, `xs[..]`);
`0 <= start <= end <= len` is checked at runtime (a violation aborts). `..` is slicing-only — not a
first-class value (the language has no counting loops; sequential control is `loop`). See
`draft.md` §7.

### Strings

```text
str
string
bytes
buffer
builder
```

`str` carries `.len()` (byte length), `==`/`!=` (byte equality), the byte-oriented
predicates `.contains(n)` / `.starts_with(p)` / `.ends_with(s)` (all `bool`),
`.find(n)` / `.rfind(n)` → `Option<i64>` (the first / last byte index, the index
siblings of `contains`; pair with range slicing — `i := s.find("=") else …; s[..i]`),
`.eq_ignore_ascii_case(o)` → `bool` (ASCII-case-insensitive byte equality, for
headers/protocols), and the
ASCII-whitespace trims `.trim()` / `.trim_start()` / `.trim_end()` (each yields
a **borrowed sub-`str`**, no allocation). All take a `str` (an owned `string` is
auto-borrowed) and work on bytes — UTF-8 is the representation, but the scan is
byte-level (the SIMD-friendly default the spec mandates over a `chars()` walk); the
predicates are backed by `memchr`-class scans. The trim set is the WHATWG ASCII
whitespace (space, `\t`, `\n`, `\x0c`, `\r`; not vertical tab); Unicode-whitespace
trimming is deliberately package-level, out of core. A `str`/`string` is **always valid
UTF-8** (a type invariant): a range slice `s[a..b]` uses byte offsets and aborts if a bound
splits a scalar, so arbitrary-byte work goes through `s.bytes()` (→ `bytes`, no UTF-8 obligation).
`str + str` is a **hard error** — `+` never concatenates (a hidden allocation, and a second way to
build a string); the one way is a `builder`. (`draft.md` §7/§12.)

### JSON

```text
json.decode
json.encode
json.doc
json.scan
```

`decode`/`encode` take no written type argument — the target type comes from
context (`u: User := json.decode(d)?`) or the value argument; Align has no
expression-position type-argument syntax (no turbofish); `scan`'s row type comes
from the binding annotation the same way. This is the complete surface —
`validate<T>`, `token`, and `field_table<T>` are settled out (draft §18.1).

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

Only inside an unsafe block: the `raw.*` flat-memory ops (`alloc`/`free`/`load`/`store`/`offset`) and
a foreign call. A C function is declared `extern "C" fn name(params) -> ret` (or a braced group) and
called like any other function, but only inside `unsafe` — foreign code is outside the safe core. The
declaration is bodyless and bound to the C symbol; FFI-safe signature types are primitive scalars and
`raw`, plus a `()` return. A foreign call is a direct native `call` (no marshaling — Align is
AOT-via-LLVM with no GC), which is the keystone of the library strategy: `std`/`pkg` own the memory
wrappers and borrow C engines via FFI.

A normal (non-`layout(C)`) struct has an **unspecified field order**: the compiler reorders fields by
descending alignment to eliminate padding (`{ a: i8, b: i64, c: i8 }` → 16 bytes, not 24), a
by-name-invisible cache-density win. A `layout(C)` attribute (`layout(C) Point { … }`, composes with
`align(N)`) is the escape hatch — it pins a struct to a stable, C-compatible flat layout (declaration
order, natural alignment, no reordering). Only such a struct may be written to / read from `raw`
memory (`raw.store`/`raw.load` of a whole struct) — the pointer-based FFI pattern. Its fields must be
FFI-mappable scalars. On x86-64 SysV, a `layout(C)` struct in the ABI's register classes and no
larger than 16 bytes may also cross by value; MEMORY-class/larger structs and other platform ABIs
remain pointer-only.

An `align(N)` attribute (`align(N) S { … }`, a power of two, composes with `layout(C)`) over-aligns a
struct's storage — the max of `N` and the natural alignment, so it never under-aligns — for SIMD /
GPU / DMA / page-aligned interop. It also rounds the type's **size** up to `N` (as C does), so a fixed
array `[align(64) S]` has a tight, over-aligned element stride (every element stays `align(N)`). The
same prefix on a **numeric scalar-array binding** (`align(64) data := [...]`, int/float elements)
over-aligns that array's storage — the aligned-vector-load enabler: a `vecN<T>` load of a whole borrow of the binding at a provably
`N`-aligned offset (e.g. `data[..].load(0)`) is emitted as an aligned load; any other offset stays a
plain element-aligned load (the alignment is never over-stated).

A `str`/`slice`/`bytes` view is FFI-safe as a **parameter**: it lowers to its data pointer (C
`char*`/`void*`), the length passed separately by the caller (`s.len()`) — the C `(ptr, len)` idiom.
A view is not a valid return type (a bare pointer has no length), and it is not NUL-terminated (only
hand it to length-based C functions).

An `extern "C" link("name")` clause names an external library to link (`-lname`), beyond the
always-linked libc/libm — the visible dependency the `std`/`pkg` C-engine wrappers ride on. A block
names one library; a repeated name links once.

Deliberately out of FFI v1 (draft §15): MEMORY-class or larger-than-16-byte structs by value, and
all by-value struct ABIs other than x86-64 SysV (struct-by-pointer covers the portable case);
`bool`/`char` as FFI types (use the integer types — a C `char` is `i8`/`u8`, a `char32_t` is `u32`;
Align `char` is a Unicode scalar, not a C `char`), and a typed pointer cast `raw.ptr_cast<T>` (waits
on typed pointers).

### Modules / imports

A prefix-accessed library namespace must be `import`ed before use — a file's header lists the
capabilities it reaches ("nothing hidden"):

```align
import core.json
import std.fs
```

Using `json.*` / `fs.*` / `io.stdout.write` without its `import`, or importing a non-existent
module, is a compile error. The language-syntactic core (`Option`/`Result`/`?`/`else`, `arena`, the
array pipeline, numeric methods, `template`) is always in scope and needs no import. `core` is
language-intrinsic and `std` the OS boundary; both are compiler builtins today.

A program also spans **user modules**: a non-entry file declares `module geom` and exports
functions and types with `pub`; `import geom` resolves by filename to `geom.align` in the entry's
directory (nested `import util.math` → `util/math.align`). A cross-module reference is qualified —
`geom.area(...)` for a function, `geom.Point` for a type — reaching only `pub` members; a bare name
resolves within the calling module (an imported type must be qualified). A qualified `pub` function
may also be passed to a pipeline/reducer (`xs.map(geom.area)`) or bound as a function value
(`f := geom.area`) under the same import and visibility rules. Each module has its own
function and type namespace, so two modules may reuse a name. A `pub` item's signature may name only
`pub` types (a `pub` fn's params/return, a `pub` struct's fields, a `pub` sum type's payloads;
transitively, through arrays/tuples/generics) — a private type cannot leak through a public interface,
so a module's public interface is self-contained. A **generic** `pub` fn's *body* is part of its
interface (its template is instantiated in importers), so it may reference only `pub` same-module
items — a private same-module fn/type/const in a generic `pub` body is rejected. The import graph must be a DAG —
cyclic imports are a compile error. An imported sum type's variant is constructed with the fully
qualified type receiver: `geom.Color.Red` or `geom.Color.Code(40)`. (`draft.md` §17.)

**Packages (the `pkg` layer).** A *package* is a distribution-layer subtree under `pkg/<name>/` (root
`pkg/<name>.align` + optional submodules), discovered from imports + the filesystem with no manifest —
the compiler adds no new concept, only two pure path rules on import edges: (1) the **`internal`**
rule — a module path containing an `internal` segment is importable only from within the subtree
rooted at that segment's parent (`pkg.web.internal.router` reaches out to `pkg.web.*` only); (2)
**layering** — a `pkg/` module may import only `core`/`std`/`pkg`, never the consuming project. The
first import segment is a trust tier (`core`/`std`/`pkg`/project); calls stay fully qualified
(`pkg.web.get(...)`) with no aliases. Vendoring is copying the subtree; one version per tree by
construction. (`draft.md` §17 "Packages" / §18.3.)

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

`core.hash`: one canonical non-crypto mixer (`wyhash`) over a byte view — `hash64(str|slice<u8>) ->
u64`, `hash128(...) -> (u64, u64)`. No `Hash` trait; deterministic within a build; not crypto/DoS-
resistant (crypto → `std.crypto`). `core.bitset` is the M6 SIMD layer (`vec`/`mask`), not built yet.

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

`std.io`: concrete builtin **Move** `reader`/`writer` types (own an fd, `Drop` closes it) — one
type, many constructors (`fs.open`, `io.stdin`, `io.stdout.buffered()`), not a trait. `io.stdout`/
`io.stderr` are `writer`; `io.stdout.buffered()` unifies buffering into `writer` rather than a
separate type. `r.read(b: mut buffer) -> Result<i64, Error>` fills `b` up to its capacity (0 =
EOF); `w.write(str|bytes|builder)` / `w.flush()`; `io.copy(r, w) -> Result<i64, Error>` is always
`O(buffer)` memory (a portable fixed-buffer loop is the v1/reference; a `sendfile`/`splice`/mmap
fast path may follow without an API change). `std.fs`: `read_file`/`write_file`/`open`/`create`/
`exists`/`remove`/`read_dir`, plus `read_file_view` (a `str` mmap view — requires an enclosing
arena, escapes via `.clone()`) and `read_bytes_view` (its binary sibling — the same arena mmap
without UTF-8 validation, returning a `bytes` view so a GGUF/binary asset maps zero-copy). `std.path`: `join`/`normalize` (owned), `base`/`dir`/`ext` (zero-copy
substring views). `std.env`: `get`/`set` only — `args` comes solely from `main(args: array<str>)`,
there is no `env.args`. `std.time`: one `i64`-nanosecond timeline, no `Duration` type — `now()`
(wall), `instant()` (monotonic), `sleep(ns)`. Recoverably fallible `std` functions return
`Result<T, Error>`; absence-only queries may return `Option<T>`, total operations return their value
directly, and programmer errors abort. A failing syscall in a `Result`-returning operation maps
through one fixed errno table (`ENOENT`→`NotFound`, `EACCES`/`EPERM`→`Denied`, `EINVAL`→`Invalid`,
else `Code(errno)`). (`draft.md` §18.2, M9.)

`std.encoding`: `base64`/`base64url`/`hex`/`percent` (RFC 3986 URI components — everything outside
the unreserved set becomes `%XX`) / `form` (`application/x-www-form-urlencoded` — the same rule but
space is `+`; encode one key or value at a time, the `=`/`&` joining them are structure) encode+decode,
plus `html_escape` (encode-only: `& < > " '` become entities, making one output safe in both element
text and a quoted attribute; reversing HTML needs a parser's full entity table, not a codec)
(decode returns an owned `buffer` — no
UTF-8 invariant on `bytes`; invalid input is `Error.Invalid`) plus `utf8_valid`. `std.rand`
(non-cryptographic): `rand.seed()`/`seed_with(s)` produce a **Copy** `rng` value (state-only, no
fd — unlike `reader`/`writer`); `r.next()`/`r.range(lo, hi)`/`r.shuffle(out xs)`/`r.sample(xs, k)`
take a `mut` receiver, OS-seeded via `getrandom`/`urandom`; `lo >= hi` (`range`) and `k < 0` or
`k > xs.len()` (`sample`) are programmer errors and abort at runtime, like out-of-bounds indexing.
`std.crypto`: EVP-backed operations use OpenSSL libcrypto, linked only when a used capability
requires it. Most work with OpenSSL 3.0; `argon2id` requires the `ARGON2ID` provider added in OpenSSL 3.2
and returns `Error.Code` when it is unavailable.
`std.cli`: an explicit flag-registration builder (`cli.command`/`c.flag_bool`/`flag_str`/`flag_i64`/
`c.parse -> Result<parsed, Error>`/`p.get_*`/`c.usage`) parsing `main(args: array<str>)`'s
`array<str>` — not a second argv source. Lookups are **total** after a successful `parse` (every
flag has a value or its default, like `json.decode`), but the lookup itself is checked at
**runtime**, not compile time: a `get_*` call for an unregistered name or the wrong type aborts at
runtime (Align has no comptime evaluator to statically validate against the builder's registered
flags); input errors surface from `parse` as `Error.Invalid`. A v1 provisional pending derive — a
future declarative flag-spec can move `get_*` validation to compile time. (`draft.md` §18.2, M10.)

## Packages

```text
pkg.db.*
pkg.web.*
pkg.rpc.*
pkg.cloud.*
pkg.ai.*
```

Not part of the language core.
