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
* No hidden side effect
* No hidden parallelism
* No hidden `unsafe`
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
bitset        // designed, not built yet (its layout is the SIMD vec/mask model)
```

`()` is unit and `(e)` is grouping, so a **tuple has arity ≥ 2**. Its ownership derives from its
elements (Move if any element is Move), exactly like a struct.

### Integer literals

Decimal, or base-prefixed `0x` (hex) / `0o` (octal) / `0b` (binary); `_` may separate digits in any
base. A literal's width is inferred from context like any literal; with no constraining context an
integer defaults to `i64` and a float to `f64` (a visible default — it affects overflow width /
precision). A *value* literal that provably does not fit its context type (`x: u8 := 300`, an
argument / field / array element / return value) is a **compile error**, not a silent wrap — cast
explicitly (`0xFFFFFFFF as i32`) if the wrapped bit pattern is wanted; `-128` is checked at its
effective value so it is a valid `i8`. Runtime arithmetic overflow still wraps, and an over-wide
`match` *pattern* literal still truncates to the scrutinee's type by the defined wrap rule.
(`draft.md` §4 "Integer Literals".)

### Statement termination and line continuation

A newline terminates a statement; braces delimit blocks and indentation is insignificant. `;` is an
**optional separator** used only to put several statements on one line, so any block can be inlined.
A line that **begins** with `.` (the pipeline form) or a binary operator continues the previous line,
which is how a multi-line chain is written without any continuation marker.
(`draft.md` §4 "Statement Terminator".)

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
compile error, not a silent wrap — cast explicitly if the wrapped pattern is wanted.
(`draft.md` §4 "Numeric Conversion".)

### Bitwise & shift

Integers have `&` `|` `^` `~` and the shifts `<<` / `>>` (integer-only — `bool` uses `&&`/`||`/`!`;
no implicit coercion, so the shift amount shares the value's type). Precedence is Go's: `<< >> &`
bind like `*`, `| ^` like `+`, so all of them bind tighter than comparison (`a & b == c` is
`(a & b) == c`). A shift amount is masked mod the bit width (defined, zero-cost); `>>` is arithmetic
on a signed value, logical on an unsigned one. The `bitset` type is built on these. The logical
`&&` / `||` **short-circuit** (the right operand runs only when the left doesn't decide the result),
so a guard like `i < xs.len() && xs[i] > 0` never indexes out of range; the bitwise `& | ` always
evaluate both operands. (`draft.md` §4 "Bitwise and Shift Operators".)

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
`pub` exports it; an importer names it qualified (`mod.NAME`), like a `pub` function/type.
(`draft.md` §4 "Constants".)

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

A constant table is **read-only**. It views the per-unit read-only data section, so writing through
it — `TABLE[i] = v`, or passing it to an `out slice<T>` parameter — is a compile error *even through
a `mut` binding or a sub-slice* (a `mut` binding rebinds the view, it does not make the storage
writable). The same rule covers a string literal's byte view (`"…".bytes()`); copy into an owned
array to modify. A `pub` constant's value is part of the exported interface (the initializer ships
and re-folds in importers), so a `pub` constant's initializer may reference only `pub` constants.
Division by zero, a cyclic definition, and a type mismatch in a constant are compile errors.

Function completion matches the declared return. Unit functions may use bare `return` or reachable
fallthrough. A non-Unit function must produce its value on every reachable path through a typed
tail expression or `return value`; bare `return` and reachable fallthrough are compile errors.
A proven non-fallthrough path needs no value.

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

When the scrutinee is a stable place whose complete root/path pair has a direct shared or exclusive
borrow fact — the borrowed parameter itself or a checked struct-field path below it — `match` reads
the tag and active payload in place. A descendant field fact does not promote an owning parent or a
mixed-provenance local. Borrowed mode admits Copy scalars/views, `string`, ordinary dynamic scalar
and AoS record arrays, and finite acyclic structs and tagged values built recursively from those
forms; array elements obey the same closed grammar. Fixed and specialized arrays, tuples, other
collections, resources, opaque handles, and other unsupported Move shapes retain the borrowed-place
diagnostic. An admitted non-Copy payload binding is a caller-owned read-only projection: it keeps the payload's static type
for field and method checking, but has no independent `Drop` or cleanup bit, does not move or null
the source, and does not trigger a hidden aggregate copy. Copy fields remain readable, owned
`string` fields can use the existing non-consuming conversion to `str`, and that owned string leaf
may use the existing `.clone()` operation for an explicit owned copy. A borrowed arm binding is not
a stable place for a nested borrowed `match`; that use is rejected and never falls back to consuming
extraction from the outer source. Returning, storing, capturing, sending, or consuming the whole
non-Copy/Move
payload is rejected by the ordinary borrowed-place diagnostics; existing Copy/view matching retains
its current result behavior. Views derived from an admitted payload follow the existing inferred
owner-generation and region rules. A free-standing or otherwise owning scrutinee retains the
existing consuming match behavior.

Ordinary indexing of an admitted `array<str>` or AoS array of Copy records with any admitted
region-bearing Copy field, including direct or nested `str` and `slice<T>` fields,
preserves the complete owner generation and input/arena roots for direct, field, and
borrowed-projection bases. Return and `borrow mut` retention cannot outlive those roots. A
terminating index forms no bounds action or result.

Indexing an `array<string>` yields a non-consuming `str` view. The array remains the sole owner;
the view carries its complete source generation and contained region roots and creates no clone,
allocation, cleanup bit, nulling, or transfer. Receiver/index order, termination, and hard bounds
behavior match every ordinary dynamic-array index. Other whole Move elements are not ordinary
values.

An indexed Move element of an admitted ordinary dynamic array may be passed only to an explicit
shared-`borrow` parameter selected by a direct, imported, or function-value call. The array base
must be a stable local, borrowed/projection binding, or struct-field path. Its complete root is
reserved from once-only index evaluation through every later argument and the call action; any
possibly overlapping move, Drop, replacement, transfer, or mutable borrow is rejected. MIR emits
the existing bounds check at the indexed argument's source position only after the index falls
through, retains no pointer while later arguments evaluate, and forms the pointer only after every
later argument falls through and the root is revalidated. A terminating index forms no guard,
descriptor, later argument, pointer, or call; a terminating later argument forms no pointer or
call. The element stays
caller-owned, and a returned or mutably retained view remains rooted in the array generation and
contained region roots. By-value Move-element indexing, temporary or nested-index bases, and
element `borrow mut` remain rejected.

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
`?`/`return` exit the function; `break` is the only loop exit and cannot cross a lambda boundary. A
`break` value obeys the return-escape rule, so it may not borrow a per-iteration local (`.clone()`
to carry one out); a `break` lexically inside an `arena {}` / `task_group {}` **nested in the loop**
is rejected today — restructure so the region ends before the `break`.
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
once per scope chain — with one exception, two **disjoint sibling blocks** may each bind the same
name, because neither is in the other's chain. Floats are IEEE 754 and never abort (`x/0.0` → `±inf`, NaN ≠ NaN); only
integer division aborts. `str`/`string` are `Ord` (byte-lexicographic; locale collation is a
library concern), so strings sort and compare. A `sort_by_key` key is a **Copy** `Ord` value — a
number, a `char`, or a borrowed `str`; an owned `string` key type-checks but is then rejected at the
MIR boundary as an internal error (no per-key Drop in the fused sort path — see
`docs/impl/19-hir-validation-ledger.md`), so project the key to a `str`. `else` works on `Result` as
well as `Option` — the intent triangle is `?` propagates / `else` falls back / `match` inspects.
Details: `draft.md` §4 (display, equality, ordering, floats) and §12 (literals and escapes).

Comparison operators and `Eq`/`Ord` bounds accept both `str` and owned `string`. An owned operand
is compared through a non-consuming, zero-cost `str` borrow, including in mixed `string`/`str`
comparisons and monomorphized generic functions.

### Generics

A function may declare type parameters — `fn f<T>(...)` — and is **monomorphized** per distinct
concrete instantiation (zero run-time cost; a Move `T` moves, a Copy `T` copies). Type arguments
are **inferred** (from arguments or the expected type via the binding annotation) — no turbofish.
A bare type parameter is **opaque**: passed / returned / stored by value, with no operations of its
own (`x + x` on a bare `T` is rejected). A **builtin bound** grants capabilities — `fn f<T: Bound>`
— in a fixed `Num ⊃ Ord ⊃ Eq` hierarchy: `Num` = arithmetic+ordering+equality (numbers), `Ord` =
ordering+equality (numbers, `char`, `str`), `Eq` = equality (numbers, `char`, `bool`, `str`). A type
argument that does not satisfy the bound is a compile error. No user-defined trait bounds.
`RegionPlain` and `SoaPlain` are separate closed structural bounds outside the numeric hierarchy.
`RegionPlain` grants region-backed plain construction. `SoaPlain` accepts exactly a nonempty struct
of integer/float/`bool`/`char`/`str` fields and grants only symbolic `soa<T>` formation at a generic
boundary. Neither grants arithmetic/equality operations.

A type parameter may also appear nested in an `Option<T>` / `Result<T, E>` (parameter or return
position) — generic combinators like `fn unwrap_or<T>(o: Option<T>, d: T) -> T`. **Structs and sum types may
be generic** — `Pair<T> { a: T, b: T }`, `Opt<T> { Some(T), None }` — monomorphized per
instantiation, type arguments inferred from a struct literal's fields / a variant's payload
(`Pair { a: 1, b: 2 }`, `Opt.Some(7)`) or written as a type (`Pair<i32>`). In a generic function,
parameters may also occur under `array`, `slice`, and applications of top-level generic
struct/sum/resource definitions. These symbolic applications are fully substituted before
Move/escape analysis and MIR. `RegionPlain` is a closed builtin structural bound for region-backed
plain construction; it is not a user trait. Definitions nested inside functions, call-site
turbofish, runtime dictionaries/reflection, and new concrete container element capabilities remain
absent.

`soa<R>` is an additional symbolic template form only when `R: SoaPlain`. A public template
interface preserves that canonical symbolic application and bound for separate compilation. Each
instantiation substitutes the ordinary concrete `soa<Struct>` before Move/escape analysis and
emitted HIR/MIR. No abstract SoA or runtime type test survives monomorphization.

```align
fn id<T>(x: T) -> T = x                  // unconstrained: pass/return only
fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }
fn unwrap_or<T>(o: Option<T>, d: T) -> T = o else d
n := id(5)        // T inferred from the argument; one specialized instance per type argument
```

A **no-payload generic variant has nothing to infer its type argument from** — `Opt.None` needs a
payload-bearing sibling to fix the type at construction.

A function-local `[]` has no element type of its own. It is accepted only when an enclosing
`slice<T>` context supplies the exact `T` (for example, a typed parameter or binding), and then
uses the normal zero-length fixed-array-to-slice borrow with no allocation. An uncontextualized
empty literal is a compile error.

### Memory

```text
value types
arena
explicit heap
unsafe
```

Escape lifetime and cleanup provenance are separate inferred facts. A value-carrying block keeps
its trailing value's region; `if` and `match` take the shortest continuing-arm region;
`else`-unwrap takes the shorter of the payload and fallback; `?` keeps the `Ok` payload region; a
`loop`'s value must satisfy the return-escape rule on every accepted `break`.
For an owned value, the same selected edge forwards a path-local bit that distinguishes individual
heap ownership from arena ownership. Moves clear the source; scope exit drops only a live
individually owned value. A `mut` binding may therefore change allocation region when every assigned
value outlives its scope, without leaking heap storage or individually freeing arena storage.

For a borrowed-place `match`, the exact stable root/path pair must have a direct shared or exclusive
borrow fact; a descendant field fact does not promote an owning parent or mixed-provenance local.
An admitted non-Copy/Move payload is a read-only projection with the source's owner generation and
no independent cleanup bit; the source is neither copied nor nulled. Admitted payloads are Copy
scalars/views, `string`, ordinary dynamic scalar/AoS-record arrays, and finite acyclic structs and
tagged values built recursively from those forms; array elements follow the same closed grammar.
Fixed and specialized arrays, tuples, other collections, resources, opaque handles, and other
unsupported Move shapes retain the borrowed-place diagnostic. Derived views follow the ordinary
borrow summary and region checks, while existing Copy/view matching retains its current result behavior. An owning-place
`match` keeps the consuming extraction and source-clearing rule.

An admitted projected `array<str>` element or view-bearing field of an AoS Copy record preserves
the source generation and every contained region through ordinary indexing, return, and mutable
destination retention. A terminating index forms no bounds action or result.

An indexed Move element is a stable call place only for an explicit shared-`borrow` parameter on a
direct, imported, or function-value target and a stable ordinary dynamic-array base. The base root
cannot be invalidated during once-only index evaluation, any later argument, or the call action.
MIR checks bounds at the indexed argument position after index fallthrough, revalidates the root
after later arguments, and forms the pointer only at the call. A terminating index forms none of
those actions. The element remains caller-owned, and returned or mutably
retained views retain the array's generation and contained region roots. By-value and mutable
element forms remain unsupported.

One restriction applies to `if` today: a value-carrying `if`/`else` **expression** cannot move an
already-bound owned local out of an arm (`c := if n > 2 { a } else { … }` is rejected, and so are
the argument and `return` positions). `match`, `else`-unwrap, a block tail, and a statement-form
`if` + `return` all move a bound owned local normally, and an `if` expression whose arms produce
fresh temporaries is fine.

A Move argument passed by value transfers ownership to the callee, which becomes responsible for
its drop. Only free-standing owned values may cross a call boundary. Arena-owned values must stay
in the caller's arena and be passed through a non-owning slice/view. The restriction is recursive
for owned aggregates. Argument evaluation remains caller-owned until the call is reached, so an
early exit while evaluating a later argument cleans up every earlier owned component. Synthesized
calls follow the same rule: `Result.map_err` cannot pass an arena-owned Move error to its mapper,
and a pipeline function cannot receive or produce a Move element without per-iteration cleanup.
Project or produce a Copy or borrowed value instead. `reduce` and materializing `scan` require a
Copy accumulator until per-iteration transfer and error cleanup are explicit. A mapped result
preserves the selected `Ok`/`Err` allocation bit and cannot outlive either its source or mapper
captures. A Move value leaving a `task_group` carries the trailing local's ownership bit and clears
that inner source before an outer return or call takes ownership.

An aggregate has one path-local cleanup bit, so its owned members must all be free-standing or all
be arena-owned. Mixing allocation modes in one tuple, struct, sum value, or owned array is rejected.
Replacing an owned field or element must preserve the existing mode, and the replaced leaf must be a
`string` or `Option<string>` today — replacing any other owned leaf (a nested Move struct, an owned
array) is a compile error naming the type; replace the whole aggregate instead. A bare
`array<string>` is a valid ordinary struct field with element-wise Drop, but remains outside the
shipped borrowed JSON schema. The accepted direct-owned JSON route below admits it only in that
route's closed flat record. A finite Move struct containing that field retains the existing
Option/Result/user-sum payload behavior and active-tag Drop. Use `array<str>` when the strings are
borrowed. Borrowed members do not
participate in this allocation-mode check. A path-dependent one-owner aggregate forwards its
runtime mode, but mutation requires a definite mode. After generic substitution, a struct field may
be Copy or recursively Move when its finite, non-recursive Drop plan is known; this does not make an
otherwise unsupported container element legal.

`Option`, `Result`, and user sum payloads recursively accept finite non-recursive types with a
known Drop plan. A tagged value is Move when any possible live payload is Move; Drop follows the
active tag, while construction and owning-place extraction move the payload and clear its old owner.
An admitted borrowed-place `match` reads the active payload in place and leaves the source owner
unchanged. Structured
owned errors and `Result<Option<MoveOutput>, MoveError>` therefore use the one existing error and
ownership models. Arbitrary collections of Move elements remain a separate container capability.

Function parameters may instead be `borrow x: T`; a shared borrow accepts a stable bound Copy or
Move place, does not consume its owner, avoids a by-value aggregate copy, and may return an inferred
view of the current generation. It does not make a temporary addressable. `borrow mut x: T` accepts a
writable Move or Copy place, is exclusive for the call, ends the previous generation, and may
return a view of the fresh generation. Copy mutable borrow is the in-place state-update form.
Parameter modes and inferred return-borrow summaries cross module interfaces; function-value types
also retain every mode, both return-borrow/region summaries, and the Move-return cleanup ABI, so
indirect and direct calls use the same ABI and result lifetime. Named summaries record parameter
roots; concrete closure targets additionally record capture slots resolved through the selected
environment. Function-value joins preserve target-relative capture roots and union compatible
parameter roots; an
unresolved higher-order parameter uses every compatible input, including embedded borrow/region
provenance in a by-value Move value. That provenance transfers with a returned Move result; a bare
view of an owner destroyed inside the callee remains illegal. Lifetimes are never written.
Mutation rooted only in an explicit `borrow mut` parameter remains Pure when the body has no other
Impure operation; alias checking proves the input exclusive. Captured mutation, unsafe/FFI, I/O,
and database work remain Impure.

Call-site exclusivity checks every peer mode beside `borrow mut owner`: `ByValue`, `Borrow`,
`BorrowMut`, and `Out`. Direct overlap or a recursively carried view, resource reference,
dependent-resource parent, or aggregate provenance rooted in the owner's previous generation is
rejected, including distinct holder aggregates. Generation invalidation therefore never delivers a
dangling peer argument to the callee. The rule is structural, not package-named. Replacing an owned
pointee through `borrow mut` drops the old value before the store and updates the caller's cleanup
bit; an unchanged pointee receives no callee function-exit Drop.

Every recursively Move return carries one dynamic path-selected cleanup bit through direct,
indirect, and imported ABIs. The caller stores it beside the result. Return borrow/region summaries
describe provenance and never reconstruct this ownership bit.

`borrow`, `out`, and `resource` are contextual words. `borrow name: T`, `borrow mut name: T`, and
`out name: T` select parameter modes; `borrow: T` and `out: region` use ordinary parameter names.
`resource Name = path` is recognized only at item position, leaving `resource.from_raw` and
`resource.borrow` parseable as dotted intrinsic calls.

`arena name {}` binds a scope-local `region` capability. Ordinary functions may accept that value to
allocate into the exact caller-selected arena; returned arena values remain tied to the lexical
block. The capability is Copy but cannot escape, enter aggregates/tasks/FFI, or be constructed by
users. Anonymous `arena {}` is the same mechanism without a bound capability.

### Error handling

```text
Result<T,E>
?
Error { NotFound, Invalid, Denied, Timeout, Code(i32) }   // canonical builtin error sum type
```

No exceptions. `E` is any sum type **or scalar** (a domain may use its own error enum). `Error` is the builtin
error type — construct `Error.NotFound` / `Error.Code(c)`, use `error(c)` as syntax sugar for the
explicit builtin `core.Error.Code(c)`, `match` it, and at
`main` it maps to the process exit code. Fallible builtins (`fs.read_file`, `json.decode`, …)
return `Result<T, Error>`. A fallible `main` (`fn main() -> Result<(), E>`) restricts `E` to the
builtin `Error` (the only type with a defined exit-code mapping; a user error enum there is a
compile error — convert with `map_err(to_error)?`). `?` requires the same `E` (no implicit conversion — convert explicitly
with `result.map_err(f)`). Error **context is structured, not free-form**: a variant carries the
relevant data (a position, a code), e.g. `ParseError { BadToken(Pos), Eof }` — there is no
`.with_context("…")` string-chaining.

The compiler-provided nominal aliases are exactly `Error` → `core.Error`; `argon2_params`,
`rs256_private_key`, `rs256_public_key`, `es256_private_key`, `es256_public_key`,
`ed25519_private_key`, and `ed25519_public_key` → the same name prefixed by `crypto.`; and
`regex_match` → `regex.regex_match`. `core.Error` is language-syntactic core and is always available
without an import. The crypto and regex explicit spellings require respectively `import std.crypto`
and `import std.regex`, and their type references count as uses for the unused-import lint. A
non-entry module may declare a local type with any of those bare names: bare lookup resolves locally,
the explicit spelling still names the builtin, and importers use the ordinary qualified local name
such as `pkg.db.Error`. Without a same-module declaration the bare alias retains its builtin meaning.
The entry module cannot declare a type whose unmangled canonical name collides with one of these
builtins.

The entry signature is exact. No-argument `main` returns only `()`, exact `i32`, or
`Result<(), Error>`; `main(args: array<str>)` returns exactly `Result<(), Error>`. Unit and Result
forms use an i32-returning C wrapper; exact i32 is the C entry directly. Every other parameter or
return shape is a compile error.

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
zip
```

`zip(a, b, ...)` is the same-index multi-source pipeline source over two or more arrays/slices of
Copy primitive scalars (`zip(a, b).map(fn v { v.0 * v.1 }).sum()`). Every runtime length is checked
equal before the loop and fixed unequal lengths are a compile error; the per-index tuple is an SSA
value, never an allocated tuple array. For `map_into`, `dst` must be disjoint from every source,
while the sources may alias each other.

`group_by(.key).sum(.value)` yields **two parallel arrays** — the distinct keys and their per-key
aggregate — not a hash map. First cut: an `i64` or `str` key over a `soa`, or a `str` key over an
`array<Struct>`, with an `i64` value and `sum`/`min`/`max`/`count`.

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

Pipeline operands are evaluated in written order: receiver/source, each stage and its one-time
Copy-capture snapshot, then terminal arguments and their captures. In `reduce(init, fn ...)` and
`scan(init, fn ...)`, stage captures are snapshotted before `init`, while reducer captures are
snapshotted after `init`. The loop reuses those snapshots rather than reloading enclosing locals.
An intervening argument may not invalidate the owner of a captured view. A non-continuing operand
suppresses every later snapshot and callback.

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
for hand-written register kernels.

The register layer's surface: a vector is built from an array literal under a `vecN<T>` annotation;
elementwise `+ - * / %` and the unary float math map one-to-one to lane-wise instructions; a
comparison yields a `maskN<T>`, which is a **nameable** type (annotation, parameter, return) with the
same element and width as the compared vectors; `select(m, a, b)` blends. A **scalar broadcasts on
either side** of a vector op (`v + 5`, `5 + v`), preserving operand order for the non-commutative
ops, and its type must unify with the element. A `slice<T>` bridges memory and registers:
`s.load(i) -> vecN<T>` reads `N` consecutive elements from runtime index `i` (width/element from the
target annotation) and `s.store(i, v)` writes them back into a **writable** (`mut`/`out`) slice; both
are bounds-checked and emitted at the *element* alignment, so only an `align(N)` binding promotes a
provably aligned offset to an aligned load. `v[i]` reads lane `i` and `v[i] = x` writes it, both at a
compile-time-constant index in `0..N`, the write requiring a `mut` vector local. (`draft.md` §9.)

`array<T>` is row-major (array-of-structs); `soa<T>` is the explicit column-major (struct-of-arrays)
layout, so a field-wise pipeline streams only the columns it touches (the cache lever that beats an
AoS `Vec<Struct>`). Build one with `.to_soa()` (transpose an `array<Struct>`) or decode JSON into one
(`s: soa<User> := json.decode(d)?` counts the rows, allocates the columns, and fills them directly
without an AoS intermediate or transpose), both arena-allocated. `json.decode`'s field contract is strict and
exactly-once (a missing or duplicated declared field is an `Err`; undeclared keys are skipped),
enforced on both the strict fallback and the Mison speculative fast path (a duplicate at an unqueried
position is re-checked against the declared set and rejected). A struct field may itself be a
`Struct` — `decode` recurses into the nested object and `encode` renders it back (a nested record
round-trips; the strict contract recurses; clean nested `str` fields stay zero-copy views into the input; selected escaped strings materialize in the enclosing arena).
Every JSON string token, including ignored keys and values, follows the RFC 8259 escape grammar:
`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, and `\uXXXX`. Raw C0 bytes,
malformed escapes, lone or reversed surrogates, and invalid UTF-8 are decode errors. A clean selected
string borrows the input; an escaped selected string is decoded once into the caller arena. A
selected escaped string outside an arena is an error, while ignored escaped tokens are validated
without a proportional scratch buffer.
A direct record with at least one `string`, direct `Option<string>`, or direct `array<string>` field
selects the shipped owned JSON route. That route is flat and
closed: every other field is a required signed/unsigned 8/16/32/64-bit integer or `bool`; borrowed
text, float, nested aggregate, other optional, and explicit-layout forms reject before allocation.
Every decoded text value is free-standing, so the result has no input or arena dependency even when
decoded inside `arena {}`. Mixed owned/borrowed graphs reject rather than clone. `json.decode`,
`json.encode`, and `json.encode_bounded` share this graph; recoverable failures clean every live
direct owner exactly once, and full-range `u64` never passes through a signed intermediate. The
exact contract is `docs/impl/24-owned-json-plan.md`.
The accepted recursive extension selects the same route when any reachable field is owned `string`.
Its acyclic, view-free, natural-layout graph admits fixed-width integers, bool, `string`, records,
`Option<T>` payloads, and dynamic arrays whose elements are integers, bool, `string`, or accepted
records. An option payload cannot itself be an option because missing and `null` are one absence
state. Record/option/dynamic-array constructor depth is at most 128. Arrays of options/arrays and every borrowed, float,
char, enum, fixed-array, explicit-layout, or other constructor reject before allocation. Decode
materializes the complete graph free-standing; the three JSON operations share one canonical graph,
recursive cleanup, and target-bound V2 descriptor. The V2 design replaces V1 and interface format 7
rather than adding a parallel path; `docs/impl/25-recursive-owned-json-plan.md` is authoritative and
implementation is pending.
A field may also be an `Option<T>` (payload scalar/`str`/nested struct): missing key or JSON `null`
→ `None`, type mismatch → `Err`, present → `Some`; `encode` omits a `None` field entirely, so
`decode(encode(x))` round-trips (a non-`Option` field still errors when missing). The same JSON field
contract also admits the current Decode schema's `Option<Move-struct>` nested-record shape. Ordinary
decode, encode, and scope Drop preserve that admitted shape; a known partial-error cleanup defect
after a later required sibling fails is a separate ownership request. The scanner-only Copy
restriction does not narrow ordinary JSON behavior. A field may also
be an owned `array<Struct>` (the `messages: array<Message>` shape) — decode fills an owned
array-of-structs in the field (freed by the struct's drop) and encode renders it back, so a full
OpenAI request/response round-trips. The element struct may itself be Move and is deep-dropped.
The shipped direct-owned route above adds a flat `array<string>` field. The accepted Request 13
extension adds nested owned record/option/array graphs under its closed V2 predicate; top-level AoS
selection remains on the existing route. `soa<T>` columns stay primitive/`str`.

An owning package resource may expose a borrowed `soa<T>` view over its exact-length column buffer;
that resource generation is then the lifetime root. `pkg.db.batch_soa<T: SoaPlain>` uses this form.
It does not create an owned `soa<T>` value or admit Move-fielded columns.

The settled completeness design (`draft.md` §18.1 "Union (Sum-Type) Mapping"): a JSON `oneOf` maps to a sum
type discriminated by pairwise-distinct **shape classes** (compile-checked; O(1) dispatch, encode
writes the live payload bare); schema-unknown JSON is read through the zero-copy arena-backed
`json.doc` view (no serde-style value tree, no map type) — `d := json.doc(s)?` in an `arena {}`, then
total Missing-propagating navigation `d.get(k)` / `d.at(i)` (always a `json.doc`), `d.kind()` → the
builtin `json.kind` sum type, leaf accessors `as_str` / `as_i64` / `as_f64` / `as_bool` → `Option`,
`d.len()` / `d.key(i)` (objects-as-ordered-data), and `d.elems() -> slice<json.doc>` (materialize a
level once, then index/`len`/recurse — reuses the slice machinery, no new array type); `json.scan`
streams typed rows as a pipeline source. The
core.json surface is exactly `decode`/`encode`/`encode_bounded`/`doc`/`scan` — `validate<T>`,
`token`, and `field_table<T>` are deleted. `encode_bounded` is shipped with the same typed encode
plan and an inclusive emitted-byte ceiling. See `draft.md` §9, §14, §18.1.

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

**Binary decode / encode.** Packed binary is read from a `bytes` view and written into a growable
`buffer`, bounds-checked and **endian-explicit**: every multi-byte read/write names its byte order
with a `_le` / `_be` suffix (`h.u32_le(0)`, `out.put_u64_be(n)`), and only `u8` / `i8` carry no
suffix. The scalar set is `u8`, `i8`, `u16`/`i16`, `u32`/`i32`, `u64`/`i64`, `f32`, `f64`; a read is
`bytes.<scalar>(off)`, its dual `buffer.put_<scalar>(v)`, and `buffer.append(data)` copies a raw
`bytes`/`str` blob in. The value handed to `put_*` must match the writer's scalar type **exactly**
(no silent coercion), and an out-of-range read (`off < 0`, or `off + width > len`) **aborts**, the
same fail-closed policy as `slice[i]` — check `.len()` first. A read returns a Copy scalar carrying
no region; the `bytes`/`buffer` stay borrowed. (`draft.md` §12.)

### JSON

```text
json.decode
json.encode
json.encode_bounded
json.doc
json.scan
```

`decode`/`encode` take no written type argument — the target type comes from
context (`u: User := json.decode(d)?`) or the value argument; Align has no
expression-position type-argument syntax (no turbofish); `scan`'s row type comes
from the binding annotation the same way. A scan row must be recursively Copy:
its complete reachable definition graph must require no `Drop`; among rows admitted
by the existing JSON decode schema, a direct or transitive owned `array<T>`,
`array<Struct>`, owning option payload, or owning union payload is rejected before
MIR or runtime construction with the exact diagnostic:

```text
`json.scan` row type '<row-type-source-spelling>' must be Copy; Move rows need per-row Drop before the scanner can reuse its row slot
```

An unsupported JSON field shape, such as an owned `string` or `array<string>` that
fails the existing schema whitelist, retains that schema diagnostic instead. The
diagnostic placeholder uses the declared public local/imported spelling with concrete
generic arguments; internal `$`-mangled and monomorph-interner names never appear.
This restriction is scanner-only; the declaration remains a valid ordinary type. This is the complete surface —
`validate<T>`, `token`, and `field_table<T>` are settled out (draft §18.1).

`json.encode_bounded(value, max_bytes: i64) -> Result<string, Error>` accepts exactly the same
typed values as `json.encode`, including the accepted direct-owned graph. Its nonnegative inclusive
ceiling counts emitted UTF-8 bytes; exact fit succeeds, while a negative limit or the first byte beyond the limit yields
`Error.Invalid` without a partial value or allocation beyond the ceiling. Success is byte-identical
to `json.encode` and owns its `string`. This typed declaration-order encoding is Align's canonical
artifact byte form; it is not RFC 8785 key sorting or a dynamic JSON canonicalizer.

The scanner generic boundary is concrete-row-only. Concrete generic monomorphs
such as `Wrap<i64>` remain eligible after row resolution, and ordinary generic
calls use expected-return propagation owned by `align_sema::Checker::check_generic_call`;
numeric `IntVar`/`FloatVar` retain deterministic `i64`/`f64` defaults. An
unresolved `Wrap<T>` / `json.scanner<Wrap<T>>` type argument inside a generic
function retains the exact resolver diagnostic `instantiating a generic struct
with a type parameter ('Row<…>' inside a generic function) is not supported yet`.
That unresolved-row capability is a separate compiler prerequisite, not an
implicit extension of the scanner surface.

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
Leaving a `task_group`, including by early return or error propagation, joins its tasks before
captured frame-owned locals or enclosing-arena storage are released.
Its private runtime region stores spawned environments and result slots only. It is not a general
allocation arena: ordinary owned values keep individual ownership, and arena-only operations still
require an explicit `arena {}`.

`spawn` takes a lambda, not a bare call, and returns a `Task<R>`. `wait()?` is the single error
boundary: it joins every task and propagates the **lowest-spawn-index** `Err`.

`Task.get()` is valid only after a successful `wait()` on every reachable path for its task
generation. In a fallible group, the Wait Result may be handled directly or through a bare local,
copy/reassignment, block tail, `map_err`, or value-producing `if`/`match`/`else`/`loop`; only an
exact proof present on every result predecessor survives. `?`, Result `match`, and Result `else`
establish success only on their Ok continuation. Every earlier Wait Result for that drained
generation must also be proved successful; a later empty Wait cannot hide an unresolved or failed
one. Err invalidates every Task/Wait proof it covered. Every Spawn advances the current generation
and stales old Wait proofs. If a Wait was unresolved it also invalidates the covered Tasks;
otherwise the next successful Wait reauthorizes old and new Task handles. An unrelated
overwrite clears the local proof. Calls, returns, closure captures, imported values, and aggregate reconstruction do not
transport it; passing a Copy Result leaves only the caller's original local proof intact.
A later no-task Wait does not revoke completion already established for that generation, even when
its Result is left unhandled. Loop fallthrough reaches a fixed point before accepted breaks form the
exit, so an unresolved or failed Wait from an earlier iteration remains visible at every later
break.
Each Task is a Move handle whose compiler-known origin names its spawning group and generation. Local moves,
reassignment, block tails, and value-producing control flow preserve that origin; calls, returns,
captures, imports, and aggregate reconstruction do not. `get()` checks that exact still-active
group. A nested group preserves outer facts: its own `wait()` cannot authorize an outer Task, while
handling an outer Wait Result inside it updates the outer group. Current task results are primitive
Copy values, so `get()` is non-consuming and repeatable. Exiting a group removes every proof that
names it, including proof on its block result; handling that Result outside cannot authorize an
outer Task. Proofs for still-active outer groups remain. Owned results remain future work.

### Safety

Normal code:

```text
safe
```

Dangerous operations:

```text
unsafe
```

Only inside an unsafe block: the `raw.*` flat-memory ops
(`null`/`alloc`/`free`/`load`/`store`/`offset`) and
a foreign call. A C function is declared `extern "C" fn name(params) -> ret` (or a braced group) and
called like any other function, but only inside `unsafe` — foreign code is outside the safe core. A
direct call or non-escaping pipeline/reducer/sort callback requires that lexical `unsafe`
invocation site. An extern declaration cannot become a first-class function value because the
function-value type carries no visible unsafe-call permission. The
declaration is bodyless and bound to the C symbol. Parameters admit integer and float scalars,
`raw`, `str`, numeric `slice<T>` (including `bytes`), and an eligible non-empty `layout(C)`
struct. Returns admit `()`, integer and float scalars, `raw`, and an eligible non-empty
`layout(C)` struct; views never return. A foreign call is a direct native `call` (no marshaling — Align is
AOT-via-LLVM with no GC), which is the keystone of the library strategy: `std`/`pkg` own the memory
wrappers and borrow C engines via FFI.

A trusted compile-time library producer may accept one exact named or non-capturing lifted Align
function and form a nominal static descriptor plus a producer-owned C-ABI trampoline. The producer
contract fixes the source signature, effects/provenance, native ABI, lifetime, cleanup, validation,
and malformed-input behavior. This forms neither a closure environment nor an ordinary function
value. Application source cannot construct the descriptor, obtain a callback pointer, choose an
arbitrary native signature, or use this narrow mechanism as general reverse FFI; callback views are
invocation-scoped and cannot escape the trampoline or cross a `spawn`/`par_map` worker boundary.
Direct, imported, and indirect helper calls preserve that non-Send fact or fail closed when it is
unavailable. (`draft.md` §15 for the extern/`unsafe` rules, §8 for the producer trampoline.)

A function containing `unsafe` is inferred Impure, so it can **never** be a `par_map` callee — the
danger stays visible and traceable.

`raw.store(p, offset, value)` and `raw.load(p, offset)` move one inferred flat value at a byte
offset. Store takes its type from `value`; load takes it from the expected result type. Admitted
values are primitive scalars, `raw` pointers, and eligible non-empty `layout(C)` structs. Pointer
slots therefore retain native handles without integer casts, while pointer validity, allocation
size, and effective type remain the enclosing `unsafe` block's obligation.
`raw.null()` is the sole explicit null-pointer constructor for native ABI arguments and sentinels;
ordinary Align values still have no null model, and a raw pointer is tested with `p.is_null()`.

A normal (non-`layout(C)`) struct has an **unspecified field order**: the compiler reorders fields by
descending alignment to eliminate padding (`{ a: i8, b: i64, c: i8 }` → 16 bytes, not 24), a
by-name-invisible cache-density win. A `layout(C)` attribute (`layout(C) Point { … }`, composes with
`align(N)`) is the escape hatch — it pins a struct to a stable, C-compatible flat layout (declaration
order, natural alignment, no reordering). Among structs, only such a struct may be written to / read
from `raw` memory (`raw.store`/`raw.load` of a whole struct) — the pointer-based FFI pattern. Its fields must be
FFI-mappable scalars. On **x86-64 Linux (SysV AMD64) only**, a `layout(C)` struct in the ABI's
register classes and no larger than 16 bytes may also cross by value; a struct that the ABI would
classify MEMORY — or that is larger — is **rejected** rather than silently passed in memory, and
every other platform ABI stays pointer-only. The same boundary is enforced under **register
pressure**: SysV puts a struct in registers only if all its eightbytes fit the class registers left
after the preceding arguments, so a signature where a by-value struct argument would fall to memory
(a two-eightbyte struct after five integer arguments, say) is rejected too — reorder it earlier or
pass it by pointer.

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

An FFI wrapper may declare an opaque Move resource:

```align
import pkg.db.internal.resource

pub resource conn = pkg.db.internal.resource.drop_conn
```

The hook is a `pub fn(raw) -> ()` in the package's allowed `internal` subtree and performs native
destruction inside an `unsafe {}` body; there is no `unsafe fn` syntax. The resource-declaring
producer synthesizes a non-user-callable hidden support thunk whose symbol/ABI fingerprint crosses
interfaces, so imported cleanup remains linkable without importing the internal module. The thunk
runs exactly once on ordinary cleanup. Construction/raw extraction/ownership transfer are
restricted to the declaring module's descendant subtree; a safe public API exposes neither `raw`
nor manual destroy. The raw-only Drop-hook module need not import the declaring module, so the
privilege does not create a module cycle. `resource_ref<R>` is a Copy view tied to the owner
generation and is
invalidated by owner move/Drop or mutable borrow. Resources are non-Send by default.

`resource.into_raw` accepts only a standalone initialized resource local or by-value resource
parameter owned by the current function. Fields, elements, projections, borrowed/out parameters,
and temporaries are rejected so raw transfer does not require per-field ownership bits.

`resource.from_raw_borrowed(ptr, parent_ref)` creates a Move child resource tied to one parent
generation, so the parent cannot move/drop before the child. The private unsafe
`resource.view_from_raw(owner_ref, ptr, len)` returns an `Option<str>` or
`Option<slice<FFIScalar>>` tied to that generation after shape/alignment/UTF-8 checks; foreign range
validity remains the wrapper's unsafe obligation. No owner-free safe raw-to-view conversion exists.

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

Using `json.*` / `fs.*` / `io.*` / `log.*` without its `import`, or importing a non-existent
module, is a compile error. The language-syntactic core (`Option`/`Result`/`?`/`else`, `arena`, the
array pipeline, numeric methods, `template`) is always in scope and needs no import. `core` is
language-intrinsic and `std` the OS boundary; both are compiler builtins today. FFI is shipped, but
the remaining std-in-Align and distribution prerequisites keep that migration future work without
changing these module contracts.

A program also spans **user modules**: a non-entry file declares `module geom` and exports
functions and types with `pub`; `import geom` resolves by filename to `geom.align` in the entry's
directory (nested `import util.math` → `util/math.align`). A cross-module reference is qualified —
`geom.area(...)` for a function, `geom.Point` for a type — reaching only `pub` members; a bare name
resolves within the calling module (an imported type must be qualified). A qualified `pub` function
may also be passed to a pipeline/reducer (`xs.map(geom.area)`) or bound as a function value
(`f := geom.area`) under the same import and visibility rules. Its function-value type retains
`ByValue`/`Out`/`Borrow`/`BorrowMut` for every parameter plus inferred
return-borrow/region/capture provenance and the Move-return cleanup ABI; indirect calls do not erase
modes, provenance, or ownership. Each module has its own
function and type namespace, so two modules may reuse a name. A `pub` item's signature may name only
`pub` types (a `pub` fn's params/return, a `pub` struct's fields, a `pub` sum type's payloads;
transitively, through arrays/tuples/generics) — a private type cannot leak through a public interface,
so a module's public interface is self-contained. A **generic** `pub` fn's *body* is part of its
interface (its template is instantiated in importers), so it may reference only `pub` same-module
items — a private same-module fn/type/const in a generic `pub` body is rejected. The import graph must be a DAG —
cyclic imports are a compile error. An imported sum type's variant is constructed with the fully
qualified type receiver: `geom.Color.Red` or `geom.Color.Code(40)`. (`draft.md` §17.)

Bare type lookup first checks a declaration in the current module, then the closed compiler-provided
alias table defined under Error handling. Thus a non-entry module may reuse a builtin's bare type
name without taking that name away from any other module. Provider-qualified builtin lookup follows
that table's exact import rule, while the entry module still rejects an unmangled canonical
collision.

Hermetic input discovery includes reachable `.align` units and exact static files explicitly
registered by compiler-known constructors. Such a constructor cannot scan directories, run code,
read environment state, or contact the network. Static file content hashes participate in the owning
unit's cache/implementation identity.

**Packages (the `pkg` layer).** A *package* is a distribution-layer subtree under `pkg/<name>/` (root
`pkg/<name>.align` + optional submodules), discovered from imports + the filesystem with no manifest —
the compiler adds no new concept, only two pure path rules on import edges: (1) the **`internal`**
rule — a module path containing an `internal` segment is importable only from within the subtree
rooted at that segment's parent (`pkg.web.internal.router` reaches out to `pkg.web.*` only); (2)
**layering** — a `pkg/` module may import only `core`/`std`/`pkg`, never the consuming project. The
first import segment is a trust tier (`core`/`std`/`pkg`/project); calls stay fully qualified
(`pkg.web.get(...)`) with no aliases. Vendoring is copying the subtree; one version per tree by
construction. (`draft.md` §17 "Packages" / §18.3.)

### Formatter and lints

The official formatter is **mandatory**. It normalizes only meaningless variation — spacing, `;`
placement, trailing commas, alignment — and deliberately does not force the one-line versus
multi-line choice.

The standard lint set is eleven checks: allocation in loop, huge struct copy, unnecessary clone,
unnecessary heap, unhandled `Result`, branch in hot loop, string re-scan, implicit copy, lossy
conversion (narrowing / float→int / wide-int→float / `char`-narrowing `as`), wasteful default type
(a large literal array left at the `i64`/`f64` default), and index-walk in loop (walking an array by
index inside a `loop` instead of writing a pipeline). (`draft.md` §16.)

## Core library

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
core.test
core.codec

core.hash
core.math
```

Every name above is an importable module except `core.array_builder`: `array_builder<T>()` is a
language-intrinsic global (like `builder()`), listed as a core area rather than an `import` target.

`array_builder<T>()` retains its individually owned heap/zero-copy-freeze form.
Besides Copy scalars and `string`, it accepts nonempty naturally aligned declared records composed
recursively of Copy scalars, owned `string`, the same record class, `Option<T>` over the accepted
field grammar, and `array<E>` whose element is a Copy scalar, `string`, or an accepted record.
Options may nest; arrays of options or arrays remain outside the type representation. Such records
contain no views. A Move record is pushed only when every reachable string and dynamic-array owner
is free-standing. A complete Move-record rvalue from a local, fresh literal, function result,
value-carrying branch/match/else, transparent block, or successful `?` unwrap (including
`map_err(...)?`) can be pushed;
push moves and nulls the selected source, while an incomplete, borrowed, already-consumed, or
allocation-mode-ambiguous source rejects before growth. Unfinished-builder Drop recursively cleans
the initialized prefix in both stack-local and boxed-header modes, including active Options,
string-array elements, and arrays of Move records; build transfers the same buffer to the ordinary
deeply dropped `array<T>`. `append` remains Copy-scalar-only. The nominal
type plus its versioned interface graph and compiler Drop plan is the sole record identity; two
same-shape declarations remain distinct and no runtime record descriptor is exposed.
`array_builder<T>(out: region)` is the caller-region form for recursively plain values. It uses
arena chunks with no hidden heap allocation and performs one documented compacting pass at
`build()`. Shorter-lived views must first use `clone_in(out)`. Both forms remain one Move owner and
a helper may push through a `borrow mut` parameter but cannot store, return, or consume that borrowed
builder. The heap owner may move through an ordinary typed parameter or return; the region-backed
owner cannot outlive its explicit region and therefore retains its existing boundary restrictions.

`core.hash`: one canonical non-crypto mixer (`wyhash`) over a byte view — `hash64(str|slice<u8>) ->
u64`, `hash128(...) -> (u64, u64)`. No `Hash` trait; deterministic within a build; not crypto/DoS-
resistant (crypto → `std.crypto`). `core.bitset` is the M6 SIMD layer (`vec`/`mask`), not built yet.

`core.codec` is the implemented canonical columnar data-batch format, not RPC. V1 uses the fixed
`ALNCOL01` envelope and at most 1024 ordered unique names over exactly `i64`, `f64`, `bool`, and `str` columns.
Its non-null child buffers match Arrow physical layouts: contiguous little-endian 64-bit values,
LSB-first bool bits, and signed i32 UTF-8 offsets plus data. The envelope is Align-specific, not
Arrow IPC/C Data. `codec.open(input: slice<u8>) -> Result<codec.batch, Error>` validates the complete canonical
input once without allocation; malformed input is `Error.Invalid`. The Copy batch and every name or
`codec.i64_column` / `codec.f64_column` / `codec.bool_column` / `codec.str_column` projection are
zero-copy and region-bound to the input storage generation. Metadata/kind lookup and every typed
column's `len`/`at` operations are total through `Option`; alignment-1 little-endian loads make
input-address alignment irrelevant.
`codec.encoder(rows)` is an explicit Move accumulator; its four transactional `put_*`
methods copy exact-length named columns, and consuming `finish()` returns an owned `buffer`.
Negative rows, empty/duplicate names, a 1025th column, length mismatches, and v1 limits return `Error.Invalid` before
mutation; OOM aborts. Nulls, nested/dictionary columns, compression, reflection, Arrow IPC/RPC, and
dataframe operations are outside v1. Exact bytes and rules: `impl/core-design/codec.md`.

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
std.log
std.encoding
std.regex
std.compress
std.rand
std.crypto
std.http
```

`std.io`: concrete builtin **Move** types `reader` / `writer` / `file` (each owns an fd, `Drop`
closes it) — one type, many constructors (`fs.open`, `io.stdin`, `io.stdout.buffered()`), not a
trait; `stream` is named but its surface is not yet specified. `io.stdout`/
`io.stderr` are `writer`; `io.stdout.buffered()` unifies buffering into `writer` rather than a
separate type. `r.read(b: mut buffer) -> Result<i64, Error>` fills `b` up to its capacity (0 =
EOF); `w.write(str|bytes|builder)` / `w.flush()`; `io.copy(r, w) -> Result<i64, Error>` is always
`O(buffer)` memory (a portable fixed-buffer loop is the v1/reference; a `sendfile`/`splice`/mmap
fast path may follow without an API change). `bytes.as_str() -> Result<str, Error>` is the one
bytes→text boundary: it validates UTF-8 and returns a zero-copy `str` view region-bound to the
receiver.

`file` is the **random-access** handle: every access carries an explicit offset, so there is no
cursor and no `seek` (hidden mutable state), and no read-only constructor. `f.pread(b: mut buffer,
off)` reads one window, `f.pwrite(data: bytes, off)` writes all of `data` (extending past EOF), and
`f.len()` is a live `fstat`. A **negative** offset is a programmer bug and aborts.

**Line reads** are explicit because they need lookahead: `r.buffered()` upgrades a reader (the read
dual of the buffered writer), and `read_line` is a **buffered-reader-only** method. It fills `b`
with the line body, exactly one `\r?\n` already stripped, and returns the bytes consumed including
the terminator (0 = EOF). Unlike `read`, it **grows** `b` as needed, up to a 64 MiB line cap
(`Error.Invalid` beyond it). The per-iteration line view (`buf.bytes()` / its `as_str()`) must not
be hoisted across iterations — the next `read_line` overwrites `buf` — so `.clone()` a line you keep.

**A v1 restriction:** an owned handle (`reader`/`writer`/`file`/`buffer`, and a buffered
`io.stdout.buffered()`) must be bound to a local before a method call — `w := fs.create(p)?` then
`w.write(d)?`, never `fs.create(p)?.write(d)?` — because an unbound owned temporary never runs its
`Drop`. The unbuffered borrowed standard streams own no fd and may be used inline. All native
buffer-fill operations (`read`, `read_line`, `pread`, `recv_from`, `crypto.random`) additionally
require the buffer argument to be a bare local declared with `mut`; temporaries and immutable locals
are rejected before the operation is formed.

`std.fs`: `read_file`/`write_file`/`open`/`create`/
`create_exclusive`/`rename_no_replace`/`open_beneath`/`create_exclusive_beneath`/`exists`/`remove`/`read_dir`, plus `create_rw` / `open_rw`
(the `file` constructors — `O_RDWR` fresh or must-exist), `read_file_view` (a `str` mmap view — requires an enclosing
arena, escapes via `.clone()`) and `read_bytes_view` (its binary sibling — the same arena mmap
without UTF-8 validation, returning a `bytes` view so a GGUF/binary asset maps zero-copy).
`read_dir` returns owned strings and **excludes** any entry whose name is not valid UTF-8, so a
listing can silently be short. `fs.create_exclusive(path: str) -> Result<writer, Error>` creates
one new regular file with native exclusive-create semantics; an occupied final entry, including a
symlink or special file, returns `Error.Code(native EEXIST)` without opening, truncating, replacing,
or removing it. The returned writer uses the existing Move/Drop contract and a failed write may
leave a partial file for explicit caller cleanup. `fs.rename_no_replace(source: str,
destination: str) -> Result<(), Error>` performs one native no-replace rename and never follows,
removes, or replaces an occupied destination. It moves the source entry as the OS does, maps
native failures through the fixed errno table, does not emulate cross-device or unsupported
filesystems, and makes no crash-durability or pair-transaction promise. Both operations borrow
NUL-free valid UTF-8 paths for the call; validation and ephemeral NUL-terminated marshalling occur
before the native operation, with source before destination for rename. A checked capacity
overflow is `Error.Invalid`; actual allocation failure is terminal OOM. `std.fs` does not classify
filesystem types or hide a publication cleanup lock.
`fs.open_beneath(root: str, relative: str) -> Result<reader, Error>` and
`fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>` validate both
complete paths, then walk from a retained root without following root, intermediate, or final
symlinks. The second path is strict and relative. The input constructor requires and revalidates
the same regular final file before returning the existing owned reader and reads no bytes itself;
the output constructor performs one exclusive create from the retained final parent and returns the
existing owned writer. They do not change cwd, retain a global root, publish a directory handle,
create parents, normalize paths, or add rollback, durability, or transaction behavior. Missing,
denied, invalid/type, and other native failures use the fixed `Error` mapping, and every ephemeral
path copy and traversal descriptor is released on recoverable failure. A same-final open/create pair
has no implicit exclusion or byte snapshot: open may return `NotFound` before creation or acquire
the new regular inode while its writer is live, so immutable-input consumers reject that overlap.
`std.path`: `join`/`normalize` (owned), `base`/`dir`/`ext` (zero-copy
substring views). `std.process`: `spawn`/`wait`/`kill`/`exec`, `exit` (runs cleanup) vs `abort`
(immediate `_exit(1)`), `cpu_count()`, and the `command` builder — `process.command(cmd, args)` plus
`cwd`/`env`/`env_clear`/`timeout_ns` setters, the optional per-stream
`max_capture_bytes(limit)` bound, `run() -> Result<run_output, Error>` for UTF-8 text capture, and
`run_bytes() -> Result<run_bytes, Error>` for arbitrary bytes. Both output handles expose `code()` as
a Copy `i64`; `stdout()`/`stderr()` are region-bound zero-copy views. An unset capture bound
preserves the existing unbounded behavior; explicit `0` permits only empty streams, exact-limit output succeeds,
and overflow signals the owned process group when present, kills and reaps the direct child, discards
partial output, and returns `Error.Invalid`. The timeout deadline remains active through direct-child wait after pipe EOF; hard
pipe/wait failures signal an owned group when present, kill/reap the direct child, and return
`Error.Code` without partial output.
`std.http` whole-body clients expose
`cl.max_response_body_bytes(limit: i64)` and the request-local
`r.max_response_body_bytes(limit: i64)`. Zero clears/inherits; a positive request value only
narrows the client or fixed 1 GiB default, and an invalid value aborts before mutation. The selected
cap is enforced during Content-Length, chunked, and close-delimited receive. Exact fit succeeds;
the first recognizable explicit-cap excess returns reserved `Error.Code(-1)`, publishes no partial
response, and closes the connection. Bodyless `HEAD`/`204`/`304` metadata is validated but not
compared with the payload cap. With a 262,144-byte cap, live Align-owned response storage is bounded
to 557,056 bytes. The exact framing/error/allocation matrix is in
`docs/impl/std-design/http.md`.
The implemented post-`pkg.db` client streaming surface adds only
`cl.request_stream(req) -> Result<http_read_stream, Error>`: the Move result borrows its client,
retains the final status/header views, and fills a caller-owned fixed-capacity `buffer` with
de-framed body bytes through `read` (`0` = complete). Exact self-delimited completion may return the
connection to that client's pool; mid-body Drop closes without hidden drain. An explicit selected
body cap remains cumulative, while an unset stream has no total cap because it does not materialize
the body. Each `read`/`next` receives a fresh 262,144-byte chunk-framing work allowance. The complete
finite storage grammar is `C ::= stream | Option<C> | Result<C,N> | Result<N,C> | Result<C,C>`,
where `N` contains no stream. Only builtin-tag edges carry the client dependency; every other
storage edge, including anonymous tuples, fails closed under an exhaustive type-discriminator
classifier. `C` may be a local, by-value/borrow/borrow-mut parameter, or function result;
out/global/constant/user-native and borrowed owning-projection positions reject. Captures and
parallel transport are rejected. `request_stream`/`read`/`next` are Impure, while the
ownership-only `sse` transition and state/head getters are Pure. Consuming
`sse()` yields an `http_sse_stream`; `next(buffer)` returns
`Result<Option<http_sse_event>, Error>` with WHATWG-decoded `event`, `data`, persistent
`last_event_id` string views into the fresh output-buffer generation and inline Copy `retry_ms`.
Control-only ID/retry changes commit at a blank line; data-bearing changes commit only with event
publication, and a terminal failure or incomplete EOF rolls back the pending block while preserving
earlier commits. It adds no automatic status/media-type policy, redirect, reconnect, sleep, or
`Last-Event-ID` request; stream accessors still expose committed control-only updates. Either explicit
body or event-output bound uses `Error.Code(-1)` with no partial publication and a closed
connection. Separately, one `next` may scan at most its output capacity plus 262,144 de-framed
source bytes, including ignored and control-only fields; exceeding that structural work guard is
`Error.Invalid`. The exact contract is the client-streaming ledger in
`docs/impl/std-design/http.md`; the surface was implemented on 2026-08-30.
`std.env`: `get`/`set` only — `args` comes solely from
`main(args: array<str>)`, there is no `env.args`. `std.time`: one `i64`-nanosecond timeline, no
`Duration` type — `now()`
(wall), `instant()` (monotonic), `sleep(ns)`. Recoverably fallible `std` functions return
`Result<T, Error>`; absence-only queries may return `Option<T>`, total operations return their value
directly, and programmer errors abort. A failing syscall in a `Result`-returning operation maps
through one fixed errno table (`ENOENT`→`NotFound`, `EACCES`/`EPERM`→`Denied`, `EINVAL`→`Invalid`,
else `Code(errno)`). `Error.Timeout` is **not** in that table — an `ETIMEDOUT` from an unrelated
syscall still maps to `Code(errno)`; `Timeout` is produced only where a deadline is enforced
explicitly (a `command`'s `timeout_ns`, an `std.http`/`std.net` I/O timeout).
(`draft.md` §18.2, M9.)

`std.encoding`: `base64`/`base64url`/`hex`/`percent` (RFC 3986 URI components — everything outside
the unreserved set becomes `%XX`) / `form` (`application/x-www-form-urlencoded` — the same rule but
space is `+`; encode one key or value at a time, the `=`/`&` joining them are structure) encode+decode,
plus `html_escape` (encode-only: `& < > " '` become entities, making one output safe in both element
text and a quoted attribute; reversing HTML needs a parser's full entity table, not a codec)
(decode returns an owned `buffer` — no
UTF-8 invariant on `bytes`; invalid input is `Error.Invalid`) plus `utf8_valid`. `std.rand`
(non-cryptographic): `rand.seed()`/`seed_with(s)` produce a **Copy** `rng` value (state-only, no
fd — unlike `reader`/`writer`); `r.next()`/`r.range(lo, hi)`/`r.shuffle(out xs)`/`r.sample(xs, k)`
take a `mut` receiver. Only `rand.seed()` is OS-seeded (via `getrandom`/`urandom`; a failure aborts
rather than surfacing a `Result`) — `seed_with(s)` is **deterministic**, for tests and
reproducibility. `lo >= hi` (`range`) and `k < 0` or
`k > xs.len()` (`sample`) are programmer errors and abort at runtime, like out-of-bounds indexing.
`std.crypto`: EVP-backed operations use OpenSSL libcrypto, linked only when a used capability
requires it. Most work with OpenSSL 3.0; `argon2id` requires the `ARGON2ID` provider added in OpenSSL 3.2
and returns `Error.Code` when it is unavailable. The designed asymmetric extension adds distinct
Move private/public key types for RS256, ES256, and Ed25519; canonical unencrypted PKCS#8 v1
`PrivateKeyInfo` version-zero private PEM, canonical SPKI public PEM, and already-decoded JWK public
constructors; and per-algorithm sign/verify functions. `OneAsymmetricKey` and relabeled PKCS#1/SEC1
DER reject; the private path uses a PKCS#8-specific decoder and cleanses every wrapper-owned decoded
or re-encoded private DER buffer before free.
RS256 is PKCS#1 v1.5 with SHA-256, ES256 is P-256/SHA-256 with raw 64-byte `r || s`, and Ed25519 is
pure Ed25519. Each key owns an isolated OpenSSL context with the built-in default provider pinned by
exact `provider=default` fetches and provider-pointer checks; global provider configuration cannot
substitute it. Ed25519 construction independently validates canonical RFC 8032 point recovery and
rejects small-order public points instead of trusting provider `public_check`. Sign/verify borrow the
key; malformed constructor/internal-ABI input is `Error.Invalid`. OpenSSL error queues are isolated
per call; only the closed input-rejection set maps Invalid, while empty/unknown/resource/internal/
fetch failures map `Error.Code(0)`. Every post-view signature mismatch is
`Ok(false)`. Construction is trusted setup without a timing promise; signing an admitted key is
constant-time for secret contents at fixed public lengths under the pointer-verified built-in
OpenSSL default-provider dependency. The implementation shipped on 2026-08-30; the exact surface, formats,
bounds, secret cleanup, error precedence, ownership, ABI, timing boundary, and closure matrix are
`impl/std-design/crypto.md`.
`std.cli`: an explicit flag-registration builder (`cli.command`/`c.flag_bool`/`flag_str`/`flag_i64`/
`c.parse -> Result<parsed, Error>`/`p.get_*`/`c.usage`) parsing `main(args: array<str>)`'s
`array<str>` — not a second argv source. Lookups are **total** after a successful `parse` (every
flag has a value or its default, like `json.decode`), but the lookup itself is checked at
**runtime**, not compile time: a `get_*` call for an unregistered name or the wrong type aborts at
runtime (Align has no comptime evaluator to statically validate against the builder's registered
flags); input errors surface from `parse` as `Error.Invalid`. A v1 provisional pending derive — a
future declarative flag-spec can move `get_*` validation to compile time. (`draft.md` §18.2, M10.)

`std.log` is a designed, not-yet-implemented explicit line sink. `log.level` is the Copy closed
order `Debug < Info < Warn < Error < Off`; `log.new(writer, minimum) -> log.logger` consumes the
writer into one nominal Move owner while preserving its exact descriptor provenance and region. An
owning writer still closes its fd, a static standard-stream writer still borrows its process fd,
and a connection-derived logger cannot outlive its `tcp_conn`. A bound logger exposes
`enabled(level) -> bool`, `line(level, str|string|builder) -> ()`, and
`flush() -> Result<(), Error>`. Arguments are eager, so
an `enabled` guard is the explicit way to skip template/builder construction. Enabled records use
the exact prefixes `[DEBUG] ` / `[INFO] ` / `[WARN] ` / `[ERROR] `, escape backslash, LF, and CR as
`\\`, `\n`, and `\r`, retain every other UTF-8 byte, and end with one LF. The allocation-free
O(n) scan makes no atomicity, durability, terminal-safety, or cross-logger ordering promise.
The first writer failure is retained: `line` stops and returns Unit, later lines are suppressed,
and `flush` exposes that first failure through the fixed std `Error` mapping without another write.
Logger Drop delegates to the writer's best-effort flush/close-if-owned cleanup. There is no global
logger, hidden clock or source metadata, structured-field/JSON mode, file constructor, dynamic level setter, async queue,
fatal action, variadic formatter, or new `write_hex`; ordinary templates and builders are the one
formatting path. The exact ledger is `docs/impl/std-design/log.md`. (`draft.md` §18.2.)

`std.regex` is an explicitly compiled library facility: `regex.compile(pattern: str) ->
Result<regex, Error>` creates an owned Move handle; `re.is_match(text)`, `re.find(text)`, and
`re.find_at(text, start)` borrow it. `find`/`find_at` return `Option<regex_match>`, where the builtin
Copy struct `regex_match { start: i64, end: i64 }` stores half-open UTF-8 byte offsets at character
boundaries. Invalid syntax/resource limits are `Error.Invalid`; an invalid `find_at` boundary is a
programmer error and aborts. The engine guarantees automata-style predictable matching and excludes
look-around/backreferences. No regex literal or implicit cache is part of the language. (`draft.md`
§18.2.)

## In-Language Tests

A private top-level `test` declaration carries one ordinary string name and one block. Only the
item-position lookahead `test` followed by a string commits to this declaration; `test {}` and
`pub test {}` remain keyword-less types, while `pub test "..." {}` rejects. A test creates no
callable or exported name. Its body is checked as a compiler-private
`fn() -> Result<(), core.Error>` with one documented implicit `Ok(())` after a Unit fallthrough;
ordinary `?`, Err, cleanup, and hard-error behavior are unchanged. Names are bounded, control-free,
excluding exactly U+0000..U+001F and U+007F..U+009F, and unique per module. Canonical ids are
`<module>::<name>`; the entry uses its declared module path, or `main` only when no module is
declared. Discovery is limited to the explicit entry/import closure in deterministic
dependency-first, then declaration order. If the entry omits its module declaration while an
imported source explicitly declares `module main`, loading rejects before catalog construction;
explicit entry paths retain the ordinary duplicate-module rule.

With `import core.test`, `test.expect(bool)` and `test.expect_eq(left, right)` are standalone
test-body assertions. Equality reuses the language's existing `==` rule, requires its result to be
exact `bool`, and uses left-to-right eager evaluation. Vector/mask equality therefore rejects
instead of acquiring an implicit all-lanes reduction. Failure reports the canonical id and
one-based call location, then returns `Error.Invalid` through the test cleanup edge; no operand
reflection or formatting is added. A final syntactic block-tail assertion is consumed as the final
statement only at root test completion or structural statement placement; every Value edge rejects,
even when its consumer expects Unit.

`alignc test` links the closure once and launches that immutable test artifact in a fresh process
group per test, sequentially. A compiler-private completion record distinguishes normal Ok/Err
return from exit, exec, abort, and crash. Each row has bounded time from pre-spawn through launch,
execution, group signalling, capture drain, and direct-child reap plus bounded per-stream capture;
pre-ack timeout/output is infrastructure failure. Parent control and capture receives are
nonblocking. One native suite cwd is snapshotted after CLI validation and installed for every row;
the child receives exactly fd 0 `/dev/null`, fd 1/2 capture, and fd 3 control, with every fd 4 and
above closed. Every verified terminal path signals the pinned group and then the still-unreaped
direct PID before reap. SIGHUP, SIGINT, SIGQUIT, and SIGTERM receive bounded cleanup. Passing
output is suppressed, while failure replays only the bounded stdout/stderr for that test, so a fully
passing suite always has one summary line. No user `main` is required or automatically invoked. Production
commands complete and freeze the ordinary-source prefix before forming a separate test overlay for
roots and every generated helper, monomorph, type, descriptor, and capability. They validate both
partitions but omit the overlay from production MIR, interfaces, links, and artifacts;
`explain-opt` also omits it from located MIR/remarks. Database Query/command constructors remain
ordinary named top-level descriptor functions and are therefore prefix-owned; tests reuse their
prepared metadata offline, and `db prepare` needs no test mode. The generated harness alone owns
literal `main`; every permitted source-main ABI is encoded as an ordinary internal function without
its production wrapper. Four exact compiler-private runtime functions own launch receive, fd
close-on-exec, acknowledgement, and completion encoding/send. Production prefix selection covers
one-shot/watch, whole/per-unit, ThinLTO, and PGO routes, while each accepted test option has one
fixed terminal consumer. The signal controller remains installed through
summary publication. One lock-free permit prevents a new raw output syscall after a graceful signal
is selected, and each handler preserves the interrupted thread's exact `errno`; the final blocked
recheck uses raw `_exit(128 + signal)`, so the four handled signals
produce numeric statuses 129/130/131/143 (`WIFEXITED`, not `WIFSIGNALED`). Production codegen/cache
identity is the complete span-erased semantic projection; current spans and located output may
shift after an earlier test edit. Structurally ordered expression-ownership facts and semantic
descriptor fields remain in that identity even though their diagnostic spans do not. Test
compilation has a separate versioned cache domain. The
complete grammar, bounds, wire bytes, error precedence, CLI
options, ownership, build-stage cleanup before runner entry, reporting, and
acceptance matrix are in `docs/impl/core-design/test.md`.

Before test cache lookup, native-capability collection, or artifact allocation, the validated
catalog-root call graph rejects every reachable `process.command`, including direct, imported,
function-value, lifted, and concrete-generic routes. An unreachable production helper remains
valid and may remain inert in a frozen-prefix test object; the first capability adds no dynamic
command supervisor.
`process.spawn`, `process.exec`, `process.exit`, and `process.abort` retain their settled row-group
behavior. `align-repl` parses the contextual declaration but rejects an entire submitted entry
containing one before replacement resolution or session mutation; tests run through `alignc test`.

## Packages

The implemented first-party packages in this repository are exactly four vendorable subtrees:

```text
pkg.web            // the zero-copy REST framework (routing included; no separate pkg.router)
pkg.db             // common driver surface: db.value, db.row, db.Driver, db.Error
pkg.db.sqlite      // driver submodule
pkg.db.postgres    // driver submodule
pkg.db.pool        // explicit fixed-capacity connection pool
pkg.frame          // bounded stable inner equi-join over typed codec columns
pkg.auth           // HS256, bounded Argon2id PHC, and opaque session tokens
```

`pkg.kv` is a synchronous RESP2 GET/SET/DEL design candidate, listed below separately. It has no
vendorable source subtree until its reviewed implementation ships.

`pkg/db` is one subtree with four public module boundaries, not four independently versioned
packages. Further drivers (`pkg.db.mysql`, `pkg.db.odbc`, `pkg.db.duckdb`) and every ecosystem
package are ordinary third-party `pkg` subtrees under the same two path rules; the language reserves
no names for them. Not part of the language core. (`draft.md` §18.3.)

`pkg.frame` v1 defines `RowPair { left: i64, right: i64 }`, tag-only
`JoinError { InvalidLimit, LimitExceeded }`, and `inner_join_i64` / `inner_join_str`. Each function
takes exact typed codec columns plus a required nonnegative `max_pairs`, then returns an owned
`array<RowPair>` in left-row-major and ascending-right-ordinal order. Duplicate keys produce the
stable Cartesian product; the right input is always the hash-build side. A negative bound returns
`InvalidLimit` before input access. An unrepresentable right-build index or the first pair beyond
the inclusive caller, i64-length, or target-byte bound returns `LimitExceeded` without output. OOM aborts. Inputs are borrowed only
for the call, strings compare as exact validated bytes, and the result retains only source
ordinals. There is no Frame wrapper, schema/query DSL, materialization, adaptive side choice,
nullable/composite/bool/f64 key, outer join, parallelism, or spill. Exact contract:
`impl/pkg-design/frame.md`.

`pkg.auth` v1 is ordinary source composition with no new compiler or native ABI. It defines
`Argon2Policy { m_cost, t_cost, parallelism }`; `encode_hs256(claims_json, key)`;
`verify_hs256(token, key, now_ns)`; `password_hash(password, policy)`;
`password_verify(password, phc, maximum)`; and `session_token()`. Keys are at least 32 bytes.
JWT claims are bounded strict RFC 8259 unique-key JSON objects. A package lexical precheck rejects
raw C0 string bytes and leading-zero integers before the shipped parser. Verification authenticates the original compact
input before JSON parsing, pins HS256, and checks optional integer-form `exp`/`nbf` seconds against
the required nonnegative caller-supplied Unix nanoseconds. Password hashes use a fresh 16-byte salt,
a fixed 32-byte Argon2id v19 tag, canonical PHC text, and caller-explicit work parameters/verify
ceilings; native Argon2 provider/context/output-reserve failure is `Error.Code(0)` and derive
rejection is `Error.Invalid`. Session tokens are exactly 32 CSPRNG bytes encoded as 43 unpadded
base64url characters.
All operations are Impure, retain no input, read no clock or configuration, and inherit ordinary
non-zeroizing string/buffer Drop. Any import retains the module-wide complete capability set and
libcrypto, including session-only use. Exact errors, bounds, formats, precedence, and non-goals:
`impl/pkg-design/auth.md`.

The `pkg.kv` v1 design candidate proposes this exact root public surface:

```text
pkg.kv.client  // opaque Move resource
pkg.kv.ClientOptions {
  connect_timeout_ns: i64,
  io_timeout_ns: i64,
  max_response_bytes: i64,
}
pkg.kv.SetCondition { Always, IfAbsent, IfPresent }
pkg.kv.SetOptions { condition: SetCondition, expires_in_ns: Option<i64> }
pkg.kv.Error { Invalid, Io(core.Error), Server(string), Decode, ResponseTooLarge, Protocol, Closed }
pkg.kv.connect(host: str, port: i64, options: ClientOptions) -> Result<client, Error>
pkg.kv.get(borrow mut owner: client, key: str) -> Result<Option<string>, Error>
pkg.kv.set(
  borrow mut owner: client,
  key: str,
  value: str,
  options: SetOptions,
) -> Result<bool, Error>
pkg.kv.delete(borrow mut owner: client, key: str) -> Result<bool, Error>
```

No argument has a default. `connect` validates before side effects, in order: a nonempty host
without U+0000, port `1..=65535`, connect and I/O timeouts each in `1..=86400000000000` ns, and a
response cap in `0..=536870912`. The cap is inclusive for a GET/error payload; non-error control
lines have a separate 64-byte cap. Each key/value length is `0..=536870912` bytes.
`Some(expires_in_ns)` is `1..=i64::MAX` and emits checked
`PX ceil(ns / 1000000)`; `None` is persistent SET and removes an existing TTL. The endpoint,
per-address connect timeout, per-read/write I/O timeout, cap, SET condition, and expiry are all
explicit. Connect timeout records a fresh monotonic start and positive budget for each post-DNS
usable address rather than covering DNS or the complete list; it forms no overflowable absolute
deadline. Its shared prerequisite checks nonblocking installation and blocking restoration, closes
and advances on failure, rounds a positive `poll` remainder up to milliseconds, rechecks early zero
returns, and lets immediate/readiness results win. Usable addresses are attempted in resolver
order only after successful resolution. A nonzero `getaddrinfo` result returns first:
`EAI_NONAME`/`EAI_NODATA` becomes `Io(core.Error.Invalid)`; every other EAI uses
`encoded := AL_CODE.saturating_add(eai.saturating_abs())` and becomes
`Io(core.Error.Code(encoded - AL_CODE))`. The connection output stays null, and no socket is
attempted. For a successful empty/all-skipped list,
the substrate returns `AL_INVALID` and package source returns `Io(core.Error.Invalid)`; otherwise
first success wins and all attempted failures return the last socket/connect/mode status. Either
receive- or send-timeout installation failure retires and closes the selected unpublished
connection and does not try another resolved address; a send failure may have changed receive before
close. A positive I/O timeout
rounds up to a normalized microsecond `timeval` and covers one blocking wait for progress rather
than the whole command; kernel scheduling may return later than either logical/option deadline.
The same conversion reaches `std.http` socket timeouts. `process.command` shares the poll conversion
and likewise uses monotonic start-plus-budget arithmetic for the complete positive-i64 range while
retaining its existing post-syscall timeout-wins order. Zero-timeout behavior is unchanged for every
consumer.

The package emits only typed canonical RESP2 GET, SET, and single-key DEL over plaintext TCP. GET
returns an owned optional string; SET maps `Always`/`IfAbsent`/`IfPresent` to no token/`NX`/`XX`
and reports whether the write applied; DEL accepts only integer value zero or one. Inputs are
call-bounded, one `borrow mut` excludes overlap, and Drop is the only public close. A complete
bounded grammar-valid Simple Error payload admits NUL/invalid UTF-8 but excludes CR/LF; CRLF is its
sole terminator. It or a fully consumed non-UTF-8 GET reply yields reusable UTF-8 `Server` or
non-UTF-8 `Decode` only after grammar/cap/framing/trailing validation; a Simple Error CR/LF violation
is `Protocol` and retires the client. Transport, oversized,
malformed/truncated/trailing, or initial-EOF failure retires the
client and later use is `Closed` without I/O. `ClientOptions`, `SetCondition`, and `SetOptions` are
Copy and Pure; `Error` is Move only because `Server` owns its string. All four operations are
Impure. A successful connect retains exactly four allocations: package state, TCP connection, and
non-owning reader and writer shells. Empty GET/`Server` strings use canonical `{null, 0}` without a
result buffer; nonempty results own one. `Invalid` precedes I/O, and cleanup never replaces a
selected terminal package error. A malformed private resource record is not a `Closed` producer:
any public operation or Drop reaches the explicit existing `ProcessAbort` dependency before native
I/O or untrusted pointer access. The per-unit resource record pins `client`, empty type parameters,
arity zero, representation version one, `__align_resource_drop$pkg.kv$client`, and
`b"align-res-drop-1"`. Existing cache scope remains exact: any own-source byte edit misses its
frontend; a public interface edit misses transitive reverse dependencies, and
a private dependency-body edit rebuilds that dependency and relinks while unchanged consumer
frontend/objects hit; a semantic no-op may re-hit its structural object. There is no AUTH, TLS,
RESP3/HELLO negotiation, generic command/reply surface, pipeline, redirect, replay, reconnect, or hidden retry;
the client relies on the server's default RESP2 mode. One source-reachable runtime row remains
planned and inactive until implementation: `align_rt_tcp_conn_set_io_timeout: i32(ptr, i64)` for
checked receive/send timeout installation, reusing A04. It returns `AL_INVALID` for null then
out-of-range input before fd access. Every non-null compatible caller supplies one live, unfreed,
exclusively held connection with no live reader/writer shell derived from it and no other value
retaining one at entry, and excludes read/write/configuration/reader-or-writer construction/free/Drop
overlap. From pre-armed
option state `{R0,S0}` and requested `T`, receive failure leaves `{R0,S0}`, send failure leaves
`{T,S0}`, and success leaves `{T,T}`. The row never rolls back, closes, or consumes; either option
failure requires caller retirement, forbids later read/write/configuration/reader-or-writer
construction/retry, and requires one later free/Drop, while success preserves usability. A later
exclusive overwrite is compatible only after all success-derived shells and retaining values Drop.
The package uses a fresh unpublished connection before shell construction and closes
either failure. The compiler recognizes its fixed ABI symbol for typed extern compatibility,
collision, and reachability without adding a language/HIR/MIR operation. Ordinary
package source imports `std.process`, explicitly decodes native status zero as success, `1..=4` to
the four `core.Error` categories and `>=5` to `Code(status-5)`, and exhausts invalid-negative,
admitted-negative, zero, positive, and oversized reader counts against the raw buffer-view length
and pointer representation with checked i32 narrowing. Invalid-negative/oversized-positive abort
before reading that header. Negative/zero requires zero length and never
dereferences either empty pointer form; positive requires exact length and non-null pointer before
typed-slice construction. Every impossible status/count/view-length/view-pointer/output product calls the
existing `process.abort()` before parsing or publication, retaining keyed `ProcessAbort` in whole
and per-unit output. SIGPIPE safety then hardens the existing
connection-derived writer in place with `MSG_NOSIGNAL` or checked `SO_NOSIGPIPE`; both slice and
builder write overloads reach that path, file and standard-stream writers retain their existing
path, and no writer ABI identity/count changes. The
timeout substrate and writer hardening are separate prerequisite capabilities; the new row lands
with its package consumer. Exact revised candidate contract: `impl/pkg-design/kv.md`; its first
independent review found contract gaps, the fresh complete review found four remaining native/wire
boundary gaps, and the next complete review found two P3 consistency gaps in the timeout action
lists and malformed-state error partition. The following review found one remaining P2 in the
pre-existing-derived-shell entry state. A fresh complete review has not yet accepted the fourth
repair.
