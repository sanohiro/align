# Type System, Inference, Safety Checks (draft)

Working draft for `align_sema`. It handles 3 passes: (2) type inference / type checking, (3) move checking / arena escape checking / effect checking. ((1) name resolution: see `01-pipeline.md`; here it is assumed already resolved.)

Design principles (`draft.md` §3.3 / `design-notes.md`):

```text
Don't surface lifetimes      both move and arena lifetime are inferred by flow analysis; only mistakes become errors
Inference-first              local inference + bidirectional typing. No global-HM-style complexity
Predictable                 the same code always resolves to the same type. If ambiguous, demand an annotation
Hand info to the compiler    put no-alias / non-null / region / cold path on the HIR so MIR/codegen don't recompute
```

This document is a **draft**. Open items are at the end under "Open items" + `// OPEN:` in the body.

---

## 1. Type representation (Ty)

The internal type representation inside `align_sema`.

```text
Ty =
  Bool
  Int(width, signed)        // i8..i64 / u8..u64
  Float(width)              // f32 / f64
  Char
  Unit                      // ()
  Str | String | Bytes | Buffer | Builder
  Array(Ty)                 // owning, contiguous memory
  Slice(Ty, Region)         // view. Carries a Region
  Vec(n, Ty) | Mask(Ty) | Bitset
  Option(Ty)
  Result(Ty, Ty)
  Named(DefId, [Ty])        // struct / sum type. Generic actual arguments
  Tuple(TupleId)            // anonymous product `(T, U, ...)`; interned by element list
  Fn([Ty], Ty, Effect)     // lambda / function value
  Var(id)                   // inference variable (during inference only)
```

`Named` is **nominal** (identity determined by name). Both struct and sum type are represented as `Named`, and the definition (fields/variants) is looked up via `DefId`.

`Tuple` is **structural**: identity is the element-type list, so it is interned (deduplicated) into a tuple table — the anonymous dual of the struct table — and `Ty::Tuple(id)` indexes it. Multi-value return is returning a tuple (no separate mechanism). Elements so far: primitive scalars (Copy / `Static`) and `str` (a Copy view — a tuple holding one is region-tracked, region-tied to the view's source, via the same rule as a struct with a `str` field). Owned (`string`/`array<T>`) elements — which would make a tuple Move and need element-wise drop — are the additive follow-up. Lowered to an anonymous LLVM struct (by-value construct/index, like a small struct).

### Region (lifetime tag)
Only view-like types (reference-like types such as `Slice` / `Str`) carry it. Users never write it. It appears only in error messages.

```text
Region =
  Static        // string literal / const pool
  Heap          // from explicit heap
  Value         // inside an owned value (shares the value's lifetime)
  Arena(id)     // from a specific arena block
```

---

## 2. Default type of numeric literals

The type is determined by context (annotation / inference). The default type is applied **only when left unconstrained to the end**.

```text
integer literal default = i64    (modern/64bit default. Safe against id overflow etc.)
float literal default   = f64
```

Can be made explicit with a suffix: `10i32` / `2.0f32`. `// OPEN:` lint for when the i64 default is wasteful in large arrays (noting that i32 suffices).

### Integer overflow (settled, draft.md §5)

Integer arithmetic is **not UB**. The default is two's-complement wrap (identical across all builds, branch-free, doesn't impede vectorization). codegen emits ordinary `add`/`mul` etc. as-is. `checked_*`(→Option) / `saturating_*` / `wrapping_*` are provided by the library as explicit ops. During development only, an overflow-checked build and lint catch bugs, but the semantics are unchanged. Division by zero etc. is separate from overflow: never silent, always an error (trap or Result).

```align
x := 10;            // unconstrained → i64
y: i32 := 10;       // annotation → i32
z := 10i32;         // suffix → i32
s := xs.sum();      // xs: array<i32> → i32 (determined by context)
```

---

## 3. Inference and checking (bidirectional)

Local inference + bidirectional typing. Two modes are used as appropriate.

```text
check(expr, expected)   when an expected type exists (annotation / argument position / return / unifying both if arms)
infer(expr) -> Ty       when there is no expected type (the RHS of := etc.)
```

- `x := e` → result of `infer(e)` becomes the type of `x`.
- `x: T := e` → `check(e, T)`.
- A function body is `check(body, ret)`. The `= expr;` form is the same.
- Arguments are `check`ed against their declared types.

Unification (unify) is used only to resolve inference variables `Var`; nominal types are not arbitrarily unified by structure. If ambiguous (a `Var` remains), it is an error demanding a type annotation.

### if / match are expressions → unify the arms
Picking up the homework from the frontend.

```text
if c { a } else { b }   : check(c, Bool); T = unify(type(a), type(b)); result T
match s { p1 => e1, ... }: unify each ei. The result is the common type
if with no else          : has no value (allowed only as a Unit statement)
match must be exhaustive or it is an error (// OPEN: details of exhaustiveness checking)
```

```align
label := if s > 80 { "high" } else { "low" };   // both arms str → label: str
```

---

## 4. Field access and projection (resolving the two meanings of `.field`)

The type of `recv.field` is **determined by the receiver's type**.

```text
recv: Named(struct S)        → the type of S.field (ordinary access)
recv: Array(Named S) / Slice → Array(type of field) (projection)
```

```align
u.score              // u: User        → i32
users.score          // users: array<User> → array<i32> (projection)
users.where(.active).score.sum()
//    ^ Slice<User>   ^ Array<i32>   ^ i32
```

A projection is fixed as a `Project(field)` node on the HIR and becomes a fusion target in MIR (`04-mir.md`). Ordinary access is `FieldAccess`.

### Field selector `.ident`
A `.ident` at argument position is typed, from the receiver element type `E`, as a function value `Fn([E], type_of(E.ident), Pure)`.

```align
users.where(.active)   // .active : Fn([User], bool, Pure)
```

---

## 5. Option / Result / ? / else

```text
?         for expr: Result(T, E), where the enclosing function returns Result(_, E') and E is convertible to E' → the value is T
          ? on anything but Result is an error (draft.md §5)
else      lhs: Option(T) or Result(T, _).
          rhs either (a) diverges (return etc.) or (b) supplies a T. The result is T
```

```align
data := fs.read_file(path)?;             // Result(String,E) → String, failure propagates
user := find_user(id) else return ...;   // Option(User) → User
port := get_env("PORT") else { 8080 };   // the else arm supplies an i64
```

`?` / `else` are kept as dedicated nodes in HIR, and desugared in MIR to early return + cold path (`04-mir.md`). The `E → E'` conversion rule is `// OPEN:` (error type design, M2).

---

## 6. Ownership and move checking (pass 3, no lifetimes)

### Copy types and Move types
```text
Copy (value, safe to bit-copy)
  bool / integer / float / char / Unit
  Vec / Mask / Bitset
  structs that are all-Copy and small
  Slice (copying the view; the pointed-to data is not copied. Region constraints handled separately)

Move (owning, linear)
  Array / String / Buffer / Builder
  Heap box
  structs containing a Move type / large structs
```

`// OPEN:` the threshold for "small" (layout size). Passing a large struct by value is a **lint** (not an error, `draft.md` §6.2).

### Checking
Flow analysis over the CFG. When a Move-type value is consumed (assigned as a value / passed as a value argument / returned by value), the original binding becomes dead. Using a dead binding is a **compile error**.

```align
data := fs.read_file(path)?;
other := data;        // moves data
print(data);          // error: data has already been moved
```

Copying is explicit via `clone()`. This constraint does not apply to `Copy` types.

### out arguments and no-alias
`out dst: slice<T>` means "`dst` is a region distinct from the other inputs". Recorded on the HIR as both a check (that `dst` does not alias other arguments at the call site) and optimization info (no-alias), then passed to MIR/codegen (`draft.md` §7).

---

## 7. arena escape checking (pass 3, hide lifetimes with regions)

> **Implemented (Memory Model v2).** The sketch below generalized into one inferred region
> lattice `Static ⊐ Frame ⊐ Arena(k)`: every view producer (slice, `str` borrow, struct field,
> a `json.decode`-d struct/array, a call re-borrowing an argument) carries a region, and
> `EscapeCheck` forbids a view outliving its source. Owned heap values (`string`/`array<T>`/
> `array<Struct>`/`builder`) are freed by per-binding MIR `Drop` outside an arena and bulk-freed
> inside one. The authoritative model + per-slice ledger is `08-memory-model-v2.md`.

`arena {}` introduces an `Arena(id)` region into the block. Views derived from allocations inside the block bear this region.

**Escape rule**: a value bearing `Arena(id)` must not outlive its arena block. Concretely, the following are made **compile errors**.

```text
- assignment to a binding declared outside the arena block
- return from the arena block / returning outward as the block's value
- storing into a non-arena container (an outer array etc.)
- capture by a closure that escapes outside the arena
```

```align
mut saved: slice<User> := empty;
arena {
  data := fs.read_file(path)?;
  users: array<User> := json.decode(data)?;   // users has the Arena(a) region
  total := users.where(.active).score.sum();  // OK: a value (i64) carries no region
  saved = users;                              // error: an arena view escapes outward
}
```

Region propagation is inferred by flow analysis; users write nothing. Only on violation does the error message surface a region (e.g. "this view is bound to an arena block"). `// OPEN:` region ordering for nested arenas, and integration with explicit allocators (`arena a {}`, open-questions).

---

## 8. Effect checking (purity of par_map, pass 3)

Functions passed to parallel / data processing cannot have side effects (`draft.md` §11). Effects are **inferred** (not annotated; the purity in open-questions is settled to be an inference policy).

```text
Effect = Pure | Impure(reason)
A function/lambda has its effect inferred from its body:
  modifying an outer mut binding   → Impure
  calling a side-effecting std fn (I/O etc.)  → Impure
  if none of the above             → Pure
```

The closure arguments to `par_map` / `map` / `where` / `reduce` require `Pure`. A violation is an error, with guidance toward `reduce`.

```align
mut total := 0;
users.par_map(fn u { total = total + u.score });  // error: modifies an outer mut (Impure)
total := users.reduce(0, fn acc, u { acc + u.score });  // OK: Pure
```

The `Fn` type carries an effect (`Fn([Ty], Ty, Effect)`), so it can be checked even through a function value.

---

## 9. Generics (minimal, settle before M4)

monomorphization (specialize per use site). No Rust/C++ trait/template complexity (`non-goals.md`).

```text
- type parameters can appear on Named / Fn / array<T> etc.
- for now the baseline policy for constraints is structural "inferred from the operations used"
- whether to introduce explicit bounds (trait-like) is decided before starting M4
```

`// OPEN:` representation of constraints (structural vs explicit bounds), handling of the N in `vec<N,T>` (value generics), and the unit of monomorphization. This is tied to `04-mir.md` (whether monomorphization happens before or after MIR generation).

---

## 10. typed HIR (pass output)

AST that passes the checks becomes the **typed HIR**. Almost the same shape as the AST, but the following are placed on it as already-settled so later stages don't recompute type info (anti-rewrite, `00-overview.md`).

```text
- a resolved Ty on every Expr
- Path resolved to a DefId
- .field fixed to either FieldAccess or Project(field)
- field selectors made into concretized closures
- Region of view types
- marking of move points (consume positions) and dead bindings
- the no-alias flag of out arguments
- the Effect of each function/closure
```

`?` / `else` / `template` / arena are **not yet desugared** (dedicated nodes in HIR). Desugaring happens in MIR (`04-mir.md`).

---

## 11. Error reporting

- Since bidirectional checking holds "expected vs actual", a type mismatch also cites the source of the expected type (annotation / argument / return / if arm).
- A move error points to the position where the move happened.
- An arena escape surfaces the region in the error body (which arena it is bound to). No lifetime syntax is shown.
- Multiple type errors within one function are aggregated (`align_diag`). Where an inference variable remains, it stops with "type annotation required".

---

## 12. Open items (to be settled)

```text
- the rule for E → E' (error type conversion)      → M2 (error type design)
- the exact algorithm for match exhaustiveness checking
- the struct size threshold dividing Copy/Move
- region ordering for nested arenas / integration with explicit allocators  → M3
- generics constraints: structural inference vs explicit bounds / monomorphization unit  → M4
- lint for the numeric default type (when i64 is excessive in large arrays)
```

Reflected into `draft.md` and this document as they are settled.
