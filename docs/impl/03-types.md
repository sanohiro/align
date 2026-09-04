# Type System, Inference, Safety Checks (draft)

Working draft for `align_sema`. It handles 3 passes: (2) type inference / type checking, (3) move checking / arena escape checking / effect checking. ((1) name resolution: see `01-pipeline.md`; here it is assumed already resolved.)

Design principles (`draft.md` §3.3 / `design-notes.md`):

```text
Don't surface lifetimes      both move and arena lifetime are inferred by flow analysis; only mistakes become errors
Inference-first              local inference + bidirectional typing. No global-HM-style complexity
Predictable                 the same code always resolves to the same type. If ambiguous, demand an annotation
Hand info to the compiler    put no-alias / non-null / region / cold path on the HIR so MIR/codegen don't recompute
```

This document records the implemented type-system model together with a small set of remaining
refinements. Historical slice labels are retained where they explain why a restriction exists.

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
  Resource(DefId, [Ty])     // opaque package-defined Move owner
  ResourceRef(DefId, [Ty])  // Copy view of one resource owner generation
  RegionCap                 // scope-bound `region` allocation capability
  Tuple(TupleId)            // anonymous product `(T, U, ...)`; interned by element list
  Fn(
    [(ParamMode, Ty)],
    Ty,
    Effect,
    ReturnBorrowSummary,
    ReturnRegionSummary,
    ReturnCleanupAbi,
  )                            // lambda / function value
  Var(id)                   // inference variable (during inference only)
```

`Named` is **nominal** (identity determined by name). Both struct and sum type are represented as `Named`, and the definition (fields/variants) is looked up via `DefId`.

`Tuple` is **structural**: identity is the element-type list, so it is interned (deduplicated) into a tuple table — the anonymous dual of the struct table — and `Ty::Tuple(id)` indexes it. Multi-value return is returning a tuple (no separate mechanism). Elements: primitive scalars (Copy / `Static`), `str` (a Copy view — a tuple holding one is region-tracked, region-tied to the view's source, the struct-with-`str`-field rule), and owned `string`/`array<T>` (which make the tuple **Move**). Tuple Drop recursively dispatches each owned element through its concrete type, including deep `array<string>` and `array<Move-struct>` elements. An owned tuple is restricted to a **temporary** — returned or destructured, not bound to a variable or passed as a parameter — so it never occupies a drop slot; building `(a, b)` from owned locals nulls those source slots (move-out), and the destructure targets are ordinary owned locals freed by the normal drop set. `partition`/`chunks` and their tuple/view machinery have shipped; lifting the owned-tuple binding/parameter cut remains an additive follow-up. Lowered to an anonymous LLVM struct (by-value construct/index, like a small struct).

### Region and owner-generation provenance

View-bearing values (`Slice`, `Str`, recursively view-bearing aggregates, and `ResourceRef`) carry
inferred provenance. Users never write a lifetime. It appears only in diagnostics and exported
function summaries.

```text
Region =
  Static        // string literal / const pool
  Frame(root)   // view into a caller/local owner generation
  Arena(id)     // from a specific arena block
```

An owner-root/generation fact is tracked alongside the region for views obtained from mutable
storage or a resource. Ending that generation invalidates the view even when its lexical region
would otherwise continue. A checked imported call receives the same parameter-root fact through
`ReturnBorrowSummary`; a concrete closure target may additionally resolve target-relative capture
slots through its selected environment. No producer body is required.

---

## 2. Default type of numeric literals

The type is determined by context (annotation / inference). The default type is applied **only when left unconstrained to the end**.

```text
integer literal default = i64    (modern/64bit default. Safe against id overflow etc.)
float literal default   = f64
```

An explicit type comes from the **binding annotation** (`y: i32 := 10`) or the **`as` operator**
(`10 as i32`) — there is no literal *suffix* (`10i32`). A suffix would be a third, redundant way to
type a literal: for a literal, `10 as i32` is exactly `10i32`, and a binding annotation covers the
rest, so a suffix only adds a second spelling of something `as` already does — against "one way".
`// OPEN:` lint for when the i64 default is wasteful in large arrays (noting that i32 suffices).

### Integer overflow (settled, draft.md §5)

Integer arithmetic is **not UB**. The default is two's-complement wrap (identical across all builds, branch-free, doesn't impede vectorization). codegen emits ordinary `add`/`mul` etc. as-is. `checked_*`(→Option) / `saturating_*` / `wrapping_*` are provided by the library as explicit ops. During development only, an overflow-checked build and lint catch bugs, but the semantics are unchanged. Division by zero etc. is separate from overflow: never silent, always an error (trap or Result).

```align
x := 10;            // unconstrained → i64
y: i32 := 10;       // annotation → i32
z := 10 as i32;     // `as` → i32 (no literal suffix; `as` is the one expression-position form)
s := xs.sum();      // xs: array<i32> → i32 (determined by context)
```

### Numeric conversion — `as` (settled, draft.md §3)

There is **no implicit numeric coercion** (not even widening); the explicit `as` operator is the only conversion. It applies between the numeric primitives (`i8..u64`, `f32`/`f64`) and `char`, and is **zero-UB by design** (matching the overflow model).

Pipeline: a new `As` token (lexer); `parse_cast` sits between unary prefix and the binary operators (so `-x as i64` is `(-x) as i64`, `a + b as i64` is `a + (b as i64)`), producing `ast::ExprKind::Cast { expr, ty }`. sema's `check_cast` resolves the target, checks both sides are numeric/`char` (the source may still be an int/float inference var — width is irrelevant to legality), forbids `char`↔float, and emits `hir::ExprKind::Cast(inner)` (the target is the node's own `ty`, the source is `inner.ty`). MIR lowers it to `Rvalue::Cast { operand, from, to }` (eliding a same-type cast). codegen's `gen_cast` picks the LLVM op by the from/to kinds:

```text
int → int      build_int_cast_sign_flag  (trunc / sext / zext; sign from the SOURCE)
int → float    {s,u}itofp                 (source signedness)
float → float  fpext / fptrunc
float → int    llvm.fpto{s,u}i.sat        (SATURATING — out-of-range → MIN/MAX, NaN → 0; no UB)
```

`char` is treated as a 32-bit unsigned integer for the conversion. The source/target must be concrete — casting a generic type parameter is rejected (deferred). `bool` and composite types do not participate.

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
match must be exhaustive or it is an error; variant and or-pattern coverage is checked, while
a wildcard arm covers all remaining variants
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
A `.ident` at argument position is typed, from the receiver element type `E`, as a function value
`Fn([(ByValue, E)], type_of(E.ident), Pure, return_borrow, None, return_cleanup)`, where
`return_borrow` is `Roots { params: [0], captures: [] }` when the projected field recursively
contains any view backed by `E`; otherwise it is `None`. This is the same recursive provenance walk
used for named function returns, including views nested under structs, tuples, fixed arrays, and sum
payloads. `return_cleanup` is `DynamicBit` exactly when the projected field is recursively Move,
otherwise `None`.

```align
users.where(.active)   // .active : Fn([(ByValue, User)], bool, Pure, None, None, None)
```

---

## 5. Option / Result / ? / else

```text
?         for expr: Result(T, E), where the enclosing function returns Result(_, E) → the value is T
          ? on anything but Result is an error (draft.md §5)
else      lhs: Option(T) or Result(T, _).
          rhs either (a) diverges (return etc.) or (b) supplies a T. The result is T
```

```align
data := fs.read_file(path)?;             // Result(String,E) → String, failure propagates
user := find_user(id) else return ...;   // Option(User) → User
port := get_env("PORT") else { 8080 };   // the else arm supplies an i64
```

`?` / `else` are kept as dedicated nodes in HIR, and desugared in MIR to early return + cold path
(`04-mir.md`). `?` performs no implicit error conversion: use `result.map_err(f)?` to make an
`E → E'` conversion visible.

---

## 6. Ownership and move checking (pass 3, no lifetimes)

### Copy types and Move types
```text
Copy (value, safe to bit-copy)
  bool / integer / float / char / Unit
  Vec / Mask / Bitset
  structs whose fields are all Copy
  Slice (copying the view; the pointed-to data is not copied. Region constraints handled separately)
  ResourceRef (copying the view; owner generation constraints handled separately)
  RegionCap (copying the scope capability; escape/storage restrictions handled separately)

Move (owning, linear)
  Array / String / Buffer / Builder
  Heap box
  Resource
  structs containing a Move type
```

Copy/Move is field-derived, not controlled by an ABI-size threshold. A large all-Copy struct remains
Copy; passing it by value may be diagnosed as a performance lint without changing ownership
semantics (`draft.md` §6.2).

`Option`, `Result`, structs, and user sums share one recursive `DropPlan` derived after nominal type
resolution and generic monomorphization:

```text
DropPlan =
  None
  Leaf(kind)
  Struct(indexed field plans, including None)
  Option(payload plan)
  Result(ok plan, error plan)
  Enum(indexed variant payload plans)
```

A composite is Move iff `plan.needs_drop()` is true; all-Copy composites retain their exact
topology with a cached false bit. The topology records the active-payload rule. User enums and the
recursive `Option<string>` field/temporary path lower it with a tag test; standalone legacy
`Option`/`Result` scalar-payload slots retain their zero-initialized inactive-field cleanup until
L1b unifies the remaining tagged lowering. The same plan drives move/null-source and drop-old
classification, so a table-free helper cannot accidentally call a Move enum/struct Copy. Cyclic
plans are rejected with the existing recursive-type diagnostic. Nominal struct and sum nodes are
memoized by type ID, shared, and carry their computed Move bit, so both construction and
classification of a repeated acyclic subgraph remain linear in the resolved type graph rather than
expanding as a tree. Cycle detection and memoization use sparse ID-keyed state, so work is
proportional to the nominal subgraph reachable from the queried type rather than the complete
program type table; a deep nominal chain remains linear as well. Collection element eligibility is
separate and does not follow merely from having a Drop plan.

### Checking
Flow analysis over the CFG. When a Move-type value is consumed (assigned as a value / passed as a value argument / returned by value), the original binding becomes dead. Using a dead binding is a **compile error**.

```align
data := fs.read_file(path)?;
other := data;        // moves data
print(data);          // error: data has already been moved
```

Copying is explicit via `clone()`. This constraint does not apply to `Copy` types.

### Borrowed parameters

Each named-function parameter has one mode:

```text
ByValue     existing rule; a Move argument is consumed
Out         writable non-alias slice destination
Borrow      shared access; caller retains ownership
BorrowMut   exclusive access; caller retains ownership and the old generation ends
```

For `Borrow`, the callee cannot move, replace, or drop the parameter. A returned view may be tied
to that parameter's caller-side root and generation. For `BorrowMut`, the call site must provide a
writable bound place. Every other argument is checked, whether its mode is `ByValue`, `Borrow`,
`BorrowMut`, or `Out`. Direct place overlap or any recursively embedded view, resource reference,
dependent-resource parent, or aggregate provenance rooted in the invalidated generation rejects
the call, including two distinct holder aggregates. The check is structural and never recognizes a
package-specific Row name. The owner's previous
generation becomes dead before the call; returned views belong to the new generation. An unbound
temporary is rejected for either borrow mode. Shared `Borrow` accepts a stable bound Copy or Move
place; Copy preserves ownership by value but explicit borrow avoids the structural copy while
retaining the same checked-place and pointer ABI. `BorrowMut` also accepts a writable Copy place so field mutation
updates the caller instead of a discarded copy.

Checked HIR infers `ReturnBorrowSummary::Roots { params, captures }` by recursively walking every
possible view in the return value. Named exported functions have an empty capture set and serialize
the parameter roots. A concrete closure target records sorted capture-slot roots and resolves them
through its selected environment at an indirect call. Whole-program and interface-only checking
must produce identical parameter roots and diagnostics. This is the same
`BorrowState`/owner-root mechanism used for intra-frame view liveness, not a second reference type
or a package-name table.

The same `ParamMode` entries are retained in `Fn`/`FnTy`. Concrete function values additionally
carry `ReturnBorrowSummary`, `ReturnRegionSummary`, and `ReturnCleanupAbi`. Function-value
assignment and control-flow joins union parameter-index sets while the runtime-selected target
retains its capture-slot metadata/environment; an unresolved higher-order parameter whose result may carry
provenance conservatively names every compatible input, including a by-value Move value with an
embedded view/dependent-resource root. Call checking snapshots embedded provenance before move/null
and transfers it to the result; callee escape checking still rejects a bare view whose consumed
owner dies there. Interface codecs, direct/indirect call checking, and codegen must agree on these
facts; there is no mode- or provenance-erasing adapter. The inferred effect and return summaries
remain outside written source-signature equality, but parameter modes participate.

Every recursively Move return uses `ReturnCleanupAbi::DynamicBit`: the direct, indirect, or
imported ABI returns a path-selected cleanup bit beside the value. The caller stores it in the
result slot, and Drop consults it. Copy returns use `None`. This dynamic ownership result is
separate from return region/borrow summaries and is included in function/interface ABI identity.

### Opaque resources

A `Resource(DefId, args)` is always Move and owns one non-null native handle. Its declaration
resolves a `pub` internal Drop hook and records a producer-owned hidden support thunk symbol/ABI
fingerprint; imported cleanup calls the thunk without importing the hook module. Module checking
restricts representation intrinsics to the declaring module's canonical descendant subtree. The
raw-only hook module need not import the resource root, so driver construction remains acyclic.
`ResourceRef` is Copy but inherits the precise owner generation and is invalid after move,
replacement, Drop, or `BorrowMut`.

A resource created from `from_raw_borrowed` also carries the parent `ResourceRef` provenance.
The child is still an owner, but it recursively tracks a borrow: moving the child transfers that
fact, dropping it releases the fact, and any attempted parent invalidation while the child lives is
rejected. A view built by `resource.view_from_raw` carries the supplied resource root/generation
through its `Option` and any later aggregate wrapper.

Resources and resource references fail the task-capture/Send check. A resource may occur in a
one-owner Move aggregate and uses the existing recursive Drop and path-local cleanup flag.
Resources are excluded from Copy arrays, pipeline elements, region builders, equality, printing,
and safe FFI signatures. The exhaustive type-class checks are structural; they must not inspect
resource names.

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

`arena {}` introduces an `Arena(id)` region into the block. `arena out {}` introduces the same
region and additionally binds `out: region`. Passing that capability to an ordinary function
substitutes the exact caller arena for the callee's region parameter; allocations through it and
returned owned values receive `Arena(id)`.

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

Region propagation is inferred by flow analysis; users write no lifetime. Only on violation does
the error message surface a region (for example, "this view is bound to an arena block"). Nested
arenas use the implemented total order `Static ⊐ Frame ⊐ Arena(k)`.

A `region` capability cannot be returned, placed in an aggregate/`Option`/`Result`, assigned to a
binding outside the arena, captured by any parallel worker (`spawn` or `par_map`), or passed to FFI.
The implemented worker-sendability gate follows every concrete callable target and its environment
captures recursively; moves, joins, nested closures, and helper calls preserve that provenance,
and an unavailable target or environment fails closed. A noncapturing function has an empty
environment. This non-Send rule
is independent of effect; sequential closures and pipelines may capture the capability under the
ordinary lexical-region proof. An interface signature with a
`region` parameter carries `ReturnRegionSummary::Roots { params, captures: [] }`, allowing an imported caller to
tie returned owned data to the selected arena. `clone_in(out)` is the explicit copy from a
shorter-lived view into that region; the checker never inserts it.

---

## 8. Effect checking (purity of par_map, pass 3)

Functions passed to parallel processing cannot have side effects (`draft.md` §11). Effects are
**inferred** (not annotated); ordinary sequential data-processing callables may be Impure and retain
exact guarded source order (`draft.md` §8).

```text
Effect = Pure | Impure(reason)
A function/lambda has its effect inferred from its body:
  modifying an outer mut binding   → Impure
  modifying storage rooted only in a `borrow mut` parameter → Pure (explicit exclusive input)
  writing through a `slice`/`soa` view (including `map_into` and `vec.store`) → Impure
  calling a side-effecting std fn (I/O etc.)  → Impure
  if none of the above             → Pure
```

Every callable moved into `par_map` requires `Pure`. The worker-sendability gate also requires every
staged or terminal capture to be worker-Send through its complete callable-target/
environment provenance. The Copy `region` capability is non-Send at any reachable capture depth
and must reject independently of effect through the same authority as `spawn`. Ordinary sequential `map` / `where` / `reduce` / `scan` /
`partition` / `any` / `all` accepts Impure callables; effect and inactive-lane legality restrict
fusion/vectorization without changing evaluation order or count.

```align
mut total := 0;
users.par_map(fn u { total = total + u.score });  // error: modifies an outer mut (Impure)
total := users.reduce(0, fn acc, u { acc + u.score });  // OK: Pure
```

The `Fn` type carries parameter modes, an effect, and both return-provenance summaries, so call ABI,
purity, and result lifetime can all be checked through a function value.

> **Implementation note (2026-07-15, #465):** the effect bit is implemented end to end. Concrete
> named functions and lifted closures receive independent `FnTy` effects, mutable fn locals join
> assigned targets, imported summaries and FFI pointers feed the same least fixpoint, and indirect
> consumers read the stored bit. A function-typed parameter with no concrete target remains
> `Unknown` and fails closed only at a Pure/`par_map` boundary; ordinary sequential HOF calls remain
> legal. Source annotations still omit effects, and signature equality ignores the inferred bit.
> See [`12-pipeline-closure-memory-io-simd-audit.md` §3.2](12-pipeline-closure-memory-io-simd-audit.md).

---

## 9. Generics (minimal) — shipped core and L7 composition

Monomorphization (specialize per use site). No Rust/C++ trait/template complexity (`non-goals.md`).

**Settled & built (4c-1, the unconstrained skeleton):**
```text
- A function declares type parameters: `fn f<T, U>(...)`. `Ty::Param(i)` represents one inside a
  template (var-free; its Copy/Move class is fixed after substitution); it is substituted before
  the flow analyses / MIR run.
- Monomorphization unit = the function, specialized per concrete type-argument tuple, generated
  in sema AFTER type-checking and BEFORE MoveCheck/EscapeCheck/MIR (so those passes and codegen see
  only concrete types — answers the "before or after MIR" question: BEFORE). Mangled `name$arg$arg`.
- Type arguments are inferred (no turbofish): a `Param` parameter binds from its argument (all
  occurrences unified); a return-only `Param` from the expected type; finalized after whole-function
  inference (so a literal argument's type can flow from the call's context).
- A template is checked abstractly (T = Param) — operations needing a capability are rejected; an
  uninstantiated generic is not checked. Instantiations are discovered transitively to a fixpoint.
- 4c-2: builtin bounds `fn f<T: Bound>` — a fixed hierarchy `Num` ⊃ `Ord` ⊃ `Eq` (`FnSig.bounds` +
  `Checker.param_bounds`). The bound gates operations on a `Param` value in `check_binary`
  (`Num`→arith, `Ord`→ordering, `Eq`→equality); a concrete type argument is checked against the
  bound at instantiation (`finalize_expr`). No user-defined trait bounds.
- 4c-3: type parameters nested in `Option<T>` / `Result<T, E>` (param/return). `Scalar::Param(u32)`
  (template-only, like `Ty::Param`); deep `subst_param_ty`; structural inference `match_param`
  (binds `Param` bare or nested, seeds a return-only param from the expected type); a nested param
  is finalized eagerly at the call (a `Scalar` can't hold an inference var), a bare one deferred.
  The shipped compiler still rejects `box`/`slice`/`array`/tuple over `T`
  (`scalar_arg`'s `allow_param`); L7 below replaces the `slice`/`array` part of this restriction.
- 4c-5: generic structs `Pair<T>`. The resolver refactor — `resolve_type` takes a `TyCx` (bundling
  `struct_ids`/`enum_ids`/`struct_templates`/`structs`/`struct_mono`/`tuples`/`fn_types`); `structs`
  grows during resolution; a `Pair<i32>` type calls `instantiate_struct` to substitute the template
  fields and intern a concrete `StructDef` (deduped by mangled name). Concrete structs get reserved
  slots (so monomorphs, appended after, don't shift their ids). Templates (with `Param` fields) are
  kept in `struct_templates`, out of codegen. A literal `Pair { a, b }` infers the args from the
  field values (`match_param`) then monomorphizes.
- 4c-6: generic sum types `Opt<T>` — the enum analogue of 4c-5. `enum_templates` registry; the
  `enums` table grows during resolution (reserved slots + `enum_mono` dedup); `resolve_type` interns
  a monomorph `EnumDef` for `Opt<i32>` (`instantiate_enum`); variant construction `Opt.Some(7)`
  infers the args from the payload (`match_param`) then monomorphizes.
- L7 (shipped): retain nested symbolic type applications in a
  generic template, add recursive inference/substitution for `array`, `slice`, and top-level
  generic struct/sum/resource applications, and add the closed structural `RegionPlain` bound.
```

**Generics remains closed and monomorphized** (see `open-questions.md`), but L7 completes one
required compositional hole for ordinary package APIs. `Ty::Param` may appear recursively under
`Option`, `Result`, `array`, `slice`, and applications of top-level generic struct, sum, and
resource definitions in a generic function's parameters, locals, and return type. Such applications
remain symbolic until concrete arguments are inferred, then monomorphize before MoveCheck,
EscapeCheck, and MIR. This permits `query<P,R>`, `stmt<P,R>`, `rows<R>`, and `array<R>` in ordinary
generic package functions; it does not declare definitions inside functions or add call-site
turbofish.

`RegionPlain` is an additional closed builtin structural bound. It grants only region-plain
construction/builder operations, and concrete instantiation recursively rejects resources,
independently owned fields, raw/function values, and builders. A1/D13 adds the one narrower
`SoaPlain` bound: exactly a nonempty struct of integer/float/`bool`/`char`/`str` fields. It grants
only symbolic `soa<R>` formation in a template. A public template interface preserves the canonical
symbolic application and bound; each instantiation substitutes concrete `Ty::Soa(id)` before
MoveCheck/EscapeCheck/emitted HIR/MIR. There are still no user traits, runtime dictionaries, reflection, or
new concrete container element capabilities. Generic
**containers beyond this symbolic composition** remain on their owning tracks; **`vecN<T>`** remains
M6, and `Opt.None` expected-type decomposition remains an additive inference refinement.

---

## 10. typed HIR (pass output)

AST that passes the checks becomes the **typed HIR**. Almost the same shape as the AST, but the following are placed on it as already-settled so later stages don't recompute type info (anti-rewrite, `00-overview.md`).

```text
- a resolved Ty on every Expr
- Path resolved to a DefId
- .field fixed to either FieldAccess or Project(field)
- field selectors made into concretized closures
- Region of view types
- owner-root/generation provenance for every recursively view-bearing value
- marking of move points (consume positions) and dead bindings
- `ByValue`/`Out`/`Borrow`/`BorrowMut` on parameters and calls
- resource declaration identity, Drop-thunk summary, and path-local cleanup ownership
- canonical recursive `DropPlan` for struct/sum/Option/Result ownership
- `ReturnBorrowSummary` and `ReturnRegionSummary`
- `ReturnCleanupAbi` and the dynamic result cleanup bit for recursively Move calls
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

## 12. `http_upgrade` type boundary (`pkg.ws`; shipped)

The `pkg.ws` capability adds one protocol-neutral `Ty::HttpUpgrade` /
`Scalar::HttpUpgrade`. It is Move, one pointer wide, non-Copy, non-comparable, non-printable, and
Drop-bearing. Its positive carrier grammar is deliberately smaller than the general owned-handle
rule: a raw same-frame local or by-value/shared-borrow/mutable-borrow parameter; plus one unnested
same-frame `Result<http_upgrade, E>` local whose complete `E` graph contains no upgrade handle. It
may come from the constructor or `map_err` and be consumed by `?`, `else`, or `match`, but may not
be a parameter, field, capture, or return. Raw user returns and every Option, reversed/nested
Result, user aggregate, collection, box, global, out, extern, capture, task, or parallel placement
are forbidden. Canonical type record v3 reserves the actual post-`pkg.csv` leaves
`Ty::HttpUpgrade=71` and `Scalar::HttpUpgrade=47`; exact bidirectional and malformed goldens own them.

The constructor borrows `http_request_ctx` and consumes only `response_builder`; success makes the
ctx spent and transfers its fd while retaining the ctx generation for every pump `Ctx` view.
`read_exact`, `write`, `deadline`, and `shutdown` require a bound local receiver; the first three
mutably borrow and end the handle's storage generation. `read_exact` also requires a mutable bare
local buffer, so a returned bytes view remains rooted in the buffer's fresh generation. Caller
arguments precede state: spent read/write/deadline return Invalid without mutation/clock/I/O,
poisoned calls replay their error, and shutdown alone is idempotent on spent. All four
are Impure. Borrow overlap, move/drop/replacement, branch/loop joins, every value-carrying control
form, whole/per-unit function-value pump signatures, and malformed checked HIR are cells in
`pkg-design/ws.md`'s closure matrix. These records and restrictions are active in the compiler.

## 13. Required next refinements

```text
- lint for the numeric default type (when i64 is excessive in large arrays)
- implement borrowed parameter modes and precise return summaries
- implement package-defined resources/resource references
- implement named region parameters and destination substitution
- implement recursive tagged Move payloads
- add the A1 `SoaPlain` symbolic-template/interface completion
```

Error propagation uses explicit `map_err`; match exhaustiveness is checked; struct Copy/Move is
field-derived; nested arena ordering is implemented; and minimal generics monomorphize before MIR.
The library-boundary entries above are settled prerequisites, not open design questions; their
capability dependencies are in `17-library-boundary-prerequisites.md` L1a–L7.
