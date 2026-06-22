# Implementation Roadmap

Milestones. The principle is as in `00-overview.md` — **fix the whole design first, drive a vertical skeleton through the implementation first, and plug each feature into all stages**. Each M has the completion condition of "works end to end (`.align` → run → output verification)." Do not create tasks that do not run vertically through the stages (e.g. doing the whole type system first).

## M0 — Skeleton Traversal (walking skeleton)

Goal: the whole pipeline connects, including crate boundaries. Features minimal.

```align
fn main() -> i32 {
  x := 1
  return x
}
```

Completion conditions:
- The six crates lexer / parser / sema (integer types only) / MIR / LLVM codegen / driver exist and are linked.
- `alignc run` produces an executable and returns an exit code.
- One end-to-end integration test is green.

At this point only `i64`/`i32` integers, `:=`, `fn`, `return`, and the four arithmetic operations. Type inference and move checking are minimal.

## M1 — The Bones of the Language (functions, control, struct, bool)

- [done] `fn` (normal form + `= expr` short form), multiple arguments, function calls.
- [done] `if` / comparison operations / `bool`.
- [done] `mut` and reassignment.
- [done] One `print` equivalent of `std.io`, wired directly to the runtime (for output
  verification). M1 form: integers only, decimal + newline, via `align_rt_print_i64`;
  `bool`/string and a no-newline variant wait for `std.io` (M5).
- [done] `struct` definitions and value literals, field access (AoS first). M1 cut:
  primitive fields only; structs live in slots (construct via `:=`, read/write fields);
  passing/returning/copying a whole struct waits for the move model (M3).
- [done] The full set of primitive types: `i8..u64`, `f32`/`f64` (float literals incl.
  exponents, `fadd`/`fmul`/`fcmp`, no implicit int↔float mixing), and `char` (Unicode
  scalar literals with escapes, equality/ordering; arithmetic on `char` is rejected).
  `print` stays integer-only until `std.io` (M5).

Completion condition (met): control-flow + struct/float/char programs compile and run
(`examples/point.align`, `examples/circle.align`, `examples/hello.align`).

## M2 — Errors and Existence (Result / Option / ?)

- [done] `Option<T>` (no null), extraction via `else` (braced diverging form or a
  value fallback).
- [done] `Result<T,E>` and the `?` operator → desugared in MIR to early return + cold
  path (the `Err` edge).
- [done] The `pub fn main(...) -> Result<(), Error>` form (lowered to `align_main`
  with a generated C `main` wrapper → `align_rt_report_error` + exit code).
- [done, minimal] A single `Error` type — **M2 placeholder: an i32 code**. The full
  error-type design (messages, categories, `Error.Variant`) stays Open in
  `open-questions.md` and is not yet built.

Status: **M2 vertical slice COMPLETE.** `examples/option.align` and
`examples/result.align` run (`result.align` propagates `Err(7)` out of `main` →
`error: code 7`, exit 7). Constructors `Some`/`None`/`Ok`/`Err`/`error` are sema
builtins; payloads are scalar-only (the documented M2 cut).

Completion condition (met): an example propagates a failure via `?` (using the
`error(code)` builtin fixture, **not** full `std.fs` — see scope below).

### M2 implementation decisions (locked, to avoid rework)

Keep M2 a vertical slice. Do **not** start `std`/`string`/`array`/`import` here
(that is M5); a missing-resource failure is modeled by a thin builtin fixture so
`Result` + `?` get a real end-to-end path.

```text
Scope
- Option<T> / Result<T,E> with payloads restricted to *scalars* (i8..u64, f32,
  f64, bool, char, Error, ()). No struct/string/nested-composite payloads yet.
- Constructors: Some(x) / None / Ok(x) / Err(e). `else`-unwrap for Option.
  `?` for Result (and Option in a Result/Option context is deferred — Result only).
- A fixture builtin `try_*` that returns Err to exercise propagation (no real I/O).

Error (M2 minimal)
- `Error` is an opaque i32 error code (placeholder; the full message/category
  design in open-questions stays Open). The `error(code)` builtin makes one.
  `align_rt_report_error` prints "error: code <n>" to stderr and returns the exit
  code: the original code clamped to a nonzero u8 (`code.clamp(1, 255)`), so a
  failure never reads as success (exit 0) and never wraps past the 8-bit range.

Type representation in the compiler (keeps `Ty: Copy`)
- Add a Copy `Scalar` enum (the var-free scalar subset) and
  `Ty::Option(Scalar)` / `Ty::Result(Scalar, Scalar)` / `Ty::Error`.
- Payloads must resolve to a concrete scalar at the constructor (an unconstrained
  int/float literal defaults there, exactly like a bare literal). This sidesteps
  inference variables living inside a composite type — acceptable for M2.

Runtime ABI for Result-returning main (locked)
- M2 `main` takes no arguments (sema rejects params); `main(args: array<str>)`
  (`draft.md` §17) is future. Both `-> i32` and `-> Result<(), Error>` are allowed.
- `fn main() -> i32` stays the C entry unchanged (M0/M1 behavior preserved).
- `fn main() -> Result<(), Error>` is lowered under the symbol `align_main`;
  codegen emits a C `main` wrapper that calls it, and on `Err(code)` calls the
  runtime `align_rt_report_error(i32) -> i32` (reports + returns the clamped exit
  code) and returns that, else returns 0.
  (Matches `06-runtime-std.md` §9's align_rt_start intent, minimal form.)

Lowering
- Option<T> = { i8 tag, T value }; Result<T,E> = { i8 tag, T ok, E err }
  (both payload slots present — a plain struct, not a packed union — for M2).
  tag: 0 = None/Ok, 1 = Some/Err.
- `?` desugars in MIR to: branch on tag; Err → early-return the propagated
  Result (cold edge); Ok → continue with the unwrapped value.
```

Build order: generic type syntax → `Ty::Option`/`Ty::Result`/`Scalar` → AST/HIR
`Some`/`None`/`Ok`/`Err`/`Try`/`else`-unwrap → MIR cold error edge → LLVM aggregate
lowering + the `main` wrapper. (`std.fs.read_file` later landed as a real builtin reading a file
into an owned `string` — see the M5 §19 note below.)

## M3 — Memory Model (move / value / arena)

- [done] move of owning types and use-after-move errors, explicit `clone()`.
- [done] `arena {}` block → calls to `align_runtime`'s arena allocator and bulk free.
- [done] Arena view escape checking.
- [deferred] Pass-by-value of small structs / large-struct-copy lint — structs are
  still slot-only (M1 cut); whole-struct move/copy + the size threshold are revisited
  when arrays/large aggregates arrive (M4).

Status: **M3 vertical slice COMPLETE.** Anchor owning type = the **heap box**
(`heap.new(x) -> box<T>`, allocated in an arena; `.get()` copies the scalar out,
`.clone()` deep-copies). Move checking (use-after-move) and arena escape checking are
HIR flow analyses (`align_sema`: MoveCheck / EscapeCheck). `examples/arena.align` runs
(exit 42); the arena is freed in bulk at block end and on every early exit (`return`/`?`).

### M3 implementation decisions (locked)

```text
- Anchor owning type: heap box `box<T>` (T a scalar), the only Move type so far.
  heap.new is REQUIRED inside an arena (M3) — free-standing heap with per-binding
  drop insertion is deferred. The box lives in the arena and is bulk-freed.
- Arena form: anonymous `arena {}` (no explicit-allocator `arena a {}` yet). Nested
  arenas are handled by region = arena nesting depth; a box's region is the depth at
  which it was allocated. (Resolves the open-questions "explicit-allocator arena" item
  for M3: anonymous now; named allocator deferred.)
- Region/escape inference: a box escapes if it reaches a shallower depth than its
  region — via `return`, assignment to an outer binding, or the arena block's value.
  Regions are inferred (flow analysis); never written.
- Runtime: chunked bump allocator (align_rt_arena_begin/alloc/reset/end); pointers are
  chunk-stable. Cleanup is emitted in MIR before every exit out of the arena scopes.
- Method syntax: `recv.method(args)` is supported only for the M3 builtins
  (`heap.new`, `box.get`, `box.clone`) via 2-segment-path call dispatch; general
  method resolution is later.
```

Completion condition (met): data allocated inside `arena {}` is freed at block exit
(and on early exits), and escapes are compile errors.

## M4 — Array Processing Core (Align's protagonist)

- [done] `array<T>` (fixed-length, from literals) + chains `map` / `where` / `sum`.
- [done] **loop fusion** in MIR (`[...].map(f).where(p).sum()` → a single loop, no
  intermediate arrays).
- [done] struct arrays (AoS) + field projection (`.field`) and field predicates
  (`where(.active)`) — the draft.md §8 shape `[...].where(.active).pay.sum()` runs as
  one fused loop.
- [done] `slice<T>` views (function parameters, array→slice borrow, pipelines over
  slices with runtime length).
- [done] `reduce(f, init)` terminal (generalizes `sum`; shares the fused loop).
- [done] `count()` terminal — counts the elements surviving the stages (`i64`). Shares the
  fused loop (`acc + 1` per kept element); needs no scalar element, so it works on a struct
  array with only a `where(.field)` filter (`[...].where(.active).count()`).
- [done] `any(p)` / `all(p)` terminals — whether a predicate holds for any / all surviving
  elements (`bool`). Shares the fused loop with a bool accumulator (`||`-fold seeded false /
  `&&`-fold seeded true). The element must be a scalar (project a struct field first).
- [done] `slice<T>` escape checking: a slice that borrows function-local array storage (an
  array literal / local array, including via a slice-annotated `let` or a re-borrowing
  call) cannot be returned — it would dangle when the frame is freed. A slice *parameter*
  borrows the caller and is returnable. (Landed in M5; replaces the M4 "simply forbid
  returning a slice" first cut.) Slice-annotated `let` now also applies the array→slice
  borrow, fixing a latent codegen mismatch (a bare array stored into a slice slot).
- [mostly done, via Memory Model v2] heap-owned dynamic `array<T>` + array type annotations,
  array-valued results, and the materializing terminals `scan`/`sort`/`to_array` all landed on
  the owned/dynamic-heap-array + drop foundation. Non-materializing terminals
  (`sum`/`reduce`/`count`/`any`/`all`) were already complete. Still deferred: `chunks` (needs
  `array<slice<T>>`, a separate type-system track).
- [done] **mutable element writes + `out` parameters (write mechanism)** — `place[i] = v` into a
  `mut` array local or an `out slice<T>` parameter (a writable output buffer the callee fills),
  bounds-checked (abort on out-of-range, like a read). `out` is restricted to `slice<T>` params and
  marks the local writable; the store lowers through the slice buffer pointer (`SlicePtr` +
  `PtrStore`) or a fixed array's slot (`StoreIndex`). First cut: primitive elements only (a `str`
  element store needs a region check; struct/Move need ownership handling). (`examples/out_param.align`.)
- [done] **`out` no-alias check** — at a call site an `out` argument must not name the same local as
  any other argument (`fill(a, a)` rejected; `fill(xs, ys)` fine), a conservative base-local
  comparison via `FnSig.out`. The language-level no-alias guarantee. **Still follow-up:** emitting
  LLVM `noalias` (the actual vectorization payoff) — blocked on the slice ABI (a slice is passed by
  value as `{ptr,len}`, so its buffer pointer is not a standalone param to attribute; needs a
  by-pointer `out`-slice ABI or scoped `!noalias` metadata on the buffer stores).
- [done] **`partition(p)`** — split a pipeline's surviving (primitive-scalar) elements into two
  owned arrays `(array<T>, array<T>)` (predicate true, then false) in one fused loop with two
  buffers + a per-element branch (`lower_array_partition`, the `to_array` collect loop doubled),
  returning the pair as an owned tuple — destructured by the caller `(evens, odds) := …`. Built on
  the owned-tuple work. Each buffer is freed once: inside an arena the buffers are arena-allocated
  and the destructured locals inherit the arena region (so they are not also dropped — the
  EscapeCheck `LetTuple` handler now propagates the tuple's region to the bound locals, closing a
  double-free); outside, they are heap and freed by the destructure targets' drop. Struct elements
  and a tuple-returning chained form are deferred. (`examples/partition.align`.)
- [done] **tuples / multi-value return (foundation)** — the anonymous product type `(T, U, ...)`:
  literals, destructuring `(a, b) := expr` (parens required, `_` ignores), positional `.N` access,
  and tuple params/returns. Multi-value return = returning a tuple (no separate mechanism; settled,
  `open-questions.md`). `Ty::Tuple(id)` interns into a tuple table (the struct-table dual); codegen
  is an anonymous LLVM struct (`MakeTuple`/`TupleIndex`). First cut: **primitive-scalar elements**
  (int/float/bool/char) — Copy / `Static`, no drop/region machinery. (`examples/tuples.align`.)
- [done] **`str` tuple elements** — `str` (a view) is now a valid tuple element. A `str`-bearing
  tuple is region-tracked (region-tied to the view's source, the struct-with-`str`-field rule), so
  an arena-`str` tuple is escape-checked and cannot be returned, while a literal-`str` tuple is
  `Static`/returnable. Required threading the tuple table into `EscapeCheck` (`tracks_region` is now
  a method; `region_of` folds `Tuple`/`TupleIndex`) — the infrastructure the owned-element slice
  reuses. Still Copy (no drop). (`examples/str_pair.align`.)
- [done] **owned (`string`/`array<T>`) tuple elements** — `fn split() -> (array<i64>, array<i64>)`
  builds and returns the pair; the caller `(xs, ys) := split()` destructures it. Cut: an owned
  tuple is a **temporary** — it may be returned or destructured, but **not** bound to a variable
  (`t := split()` is rejected — that would need element-wise drop + index-move) or passed as a
  parameter. Because such tuples never occupy a drop slot, no tuple `Drop`/codegen change was
  needed: building `(a, b)` from owned locals nulls them (`null_moved_source` extended to `Tuple`,
  in both `return` and destructure-init positions), and the destructure targets are ordinary owned
  locals freed once by the existing drop set. **Unblocks** `partition` (`(array<T>, array<T>)`).
  (`examples/split_array.align`.)
- [done] **owned tuples bound to a variable** — lifts the temporary-only cut: `t := split()` is now
  allowed. A Move tuple local joins the drop set; codegen `Drop` frees each owned element of the
  tuple aggregate and `DropFlagInit` zeroes it (so a moved-out tuple's `Drop` is a no-op). A
  destructure/return that moves the tuple nulls the slot (`null_moved_source` recognises a Move
  `Tuple` local). Reading an *owned* element by index (`t.0` where `.0` is owned) is rejected — a
  partial move; destructure instead. A Copy element reads fine.
- [done] **owned tuple parameters** — `fn f(t: (array<i64>, array<i64>))`. Falls out of the bind +
  drop machinery: an owned-tuple param joins the drop set (like an owned array param), so the callee
  drops it at exit if it doesn't consume it, and a caller passing a bound owned tuple moves it (slot
  nulled). Only **partial field moves** (`t.0` of an owned element) remain deferred.
- [done] named-function `map` over struct elements — `[Emp{…}].where(.active).map(net).sum()`
  where `net(e: Emp) -> i32`. A struct array stays index-addressed until used; a struct-consuming
  `map` loads the whole element by value just before the call (`lower_struct_elem`): a fixed stack
  `array<Struct>` via the slot load `Index`, an owned dynamic `array<Struct>` via the buffer-pointer
  load `IndexPtr` (the field-less analogue of `IndexFieldPtr`). `.field` / `where(.field)` read the
  *source* element, so they are rejected after a `map` (which yields a computed value, not a source
  element). Map-result struct chaining (`map(f).field` / `map(f).g()` where `f` returns a struct) is
  not supported — projection addresses the source, so a struct map must feed a scalar map or a
  reduction. (`examples/struct_map.align`.)
- [done] named-function `where` over struct elements — a whole-struct predicate (`fn busy(e: Emp)
  -> bool = e.hours > 40 && e.active`), the multi-field companion of the single-field
  `where(.active)`. Same `lower_struct_elem` load as `map`, but `where` *filters* (keeps the
  element unchanged), so unlike `map` it sets no "mapped" flag — a following `.field` /
  `where(.field)` still reads the source. (`examples/struct_where.align`.)

### Dynamic arrays / slices — decisions (from review)

```text
- slice<T> is a borrowed view { T* ptr, i64 len } and is Copy. Forming a slice from
  an array is a *borrow* (no allocation), so an array → slice<T> coercion is allowed
  implicitly without violating "Nothing hidden" (only heap *allocation* must be
  explicit). A slice carries a region; escape checking must keep it from outliving its
  backing — M4 first cut simply forbids returning a slice.
- A heap-owned, growable array<T> (allocation) is separate and must be explicit
  (`.to_array()`-style, or arena-allocated like box) — deferred to a later sub-slice;
  it is a Move type (use-after-move via MoveCheck) and, if arena-allocated, bulk-freed.
- The fused loop gains a pointer/operand-based element path for slices (length from the
  slice's `len`, elements via `ptr[i]`), alongside the existing slot-based path for
  stack array literals.
```

Status: **M4 core slice COMPLETE.** Scalar arrays via literals; `map`/`where` take
named functions; `sum` is the reduction terminal. The whole chain lowers to one
counted loop in MIR (map = inline call, where = a branch skipping to the increment,
sum = the accumulator) — verified fused (`examples/arraysum.align`,
`examples/pipeline.align`). General generics are still deferred — `map`/`where`/`sum`
are compiler-known builtins, monomorphic per element type.

### M4 implementation decisions (locked)

```text
- Scalar element arrays only, created by literals `[...]` (Ty::Array(Scalar, N), the
  length is part of the type). Dynamic arrays / slices / array type annotations and
  array-valued results (materialization) are deferred.
- Pipelines must end in a reduction (sum) so no output array is allocated; map/where
  fuse into that loop. `map`/`where` outside a terminal is an error.
- map/where take a *named function* argument (closures/lambdas deferred). The source
  element type is inferred from the first stage's parameter (or the sum result type).
- Method chains rely on the slice-0 postfix `.` (FieldAccess); the pipeline is
  collected from the AST at the `sum` terminal and lowered as one loop.
- Arrays are not yet Move-checked (literals are consumed only by the reduction);
  whole-array move/ownership arrives with dynamic arrays.
```

Completion condition (met): an array-aggregation pipeline (`map`/`where`/`sum`) runs
as fused code (one loop). The full `draft.md` §19 example (JSON, struct fields) needs
later slices (struct arrays, M5 strings/JSON).

## M5 — Strings and JSON

- [done] `str` view (`{ u8* ptr, i64 len }`), string literals (interned constants),
  `print(str)` via `align_rt_print_str` (M5-A).
- [done] `str` equality (`==`/`!=`); the runtime string `builder`
  (`align_rt_builder_*`); `template "...{ident}..."` desugaring (static parts +
  int/str holes → builder writes → `str`); `str + str` concatenation.
- [done] arena-backed builder: when a `template`/concat runs inside an `arena {}`,
  the result is allocated in the arena (bulk-freed, no leak); outside, it is leaked
  (process-lifetime).
- [done] `str` escape checking: an arena-backed `str` cannot escape its arena (return /
  arena-block value / assign-to-outer) — `EscapeCheck` now tracks `str` regions like
  `box`. A non-arena `str` (literal / leaked concat) is region-0 and freely returnable.
- [done] `slice<T>` escape checking. Slices borrow function-local array storage (a
  different lifetime model from arena regions: the backing array lives in the *frame*,
  not an arena), so `EscapeCheck` tracks a separate set of "local-backed" slice locals.
  A slice that borrows an array literal / local array — directly, via a slice-annotated
  `let`, or via a re-borrowing call — cannot be returned (it would dangle); a slice
  *parameter* borrows the caller and is returnable. Slice-annotated `let` now also
  performs the array→slice borrow (fixes a latent array-into-slice-slot codegen mismatch).
- [done] `{expr}` template holes: a hole is any non-empty `{...}`, whose contents are
  re-lexed and parsed as a sub-expression (arithmetic, calls, inline `str` concat — not
  just a bare name). The hole expression must be printable. An unmatched `{` or
  empty `{}` stays literal. Hole token spans are offset to point into the template literal.
- [done] `print` and template holes render `bool` (`true`/`false`), `char` (its UTF-8,
  incl. multibyte), and floats (`f32`/`f64`) in addition to integers and `str`. Floats use
  the runtime's shortest round-trip decimal (Rust `Display`), with a `.0` appended when the
  rendering would otherwise look like an integer (runtime `align_rt_print_{bool,char,f32,f64}`
  + matching `builder_write_*`; MIR `BoolHole`/`CharHole`/`FloatHole`). `is_printable`
  (numeric | `str` | `bool` | `char`) gates both `print` and template holes.
- [done] `str` struct fields. A struct may hold `str` fields (codegen lays them out as the
  `{ ptr, len }` view via `abi_type`). To avoid reopening the arena-escape gap without
  per-struct region tracking, a `str` field may only hold a **region-0** str (a literal /
  non-arena str); storing an arena-backed str into a field is an `EscapeCheck` error, so a
  struct never carries an arena region and stays freely returnable. box/slice/array/option
  fields remain unsupported.
- [done] `json.encode(s)` — encode a flat struct **or a fixed struct array** into a JSON
  object / array of objects (`str`). Desugars in sema to the `template`/builder machinery
  (static JSON syntax + per-field value holes; the array case is unrolled since the length
  is static, reading each element field via the new HIR `IndexField`). `str` fields are
  emitted as JSON string literals (quoted + escaped per RFC 8259) by the runtime
  `align_rt_builder_write_json_str`. Nested structs, dynamic arrays/options, and
  `json.decode` are not implemented yet.
- [done] `.len()` on `str`/`slice<T>` (the `len` field of the `{ ptr, len }` view; for
  `str` this is the **byte** length) and on a fixed array/struct-array (the static element
  count). Returns `i64`. Reuses the MIR `SliceLen` rvalue.
- [done] whole-struct by-value (pass/return/copy) and struct literals in value position.
  A struct is a Copy aggregate (its fields are scalars + region-0 `str`, all Copy): it can
  be a function parameter, a return type, a call argument, copied via `let y := x`, and
  reassigned whole. Codegen passes/returns the LLVM aggregate by value (`declare_fn` maps
  `Ty::Struct`); params are already stored into their slots, and a struct-literal expression
  materializes into a temp slot then loads. The gateway to `json.decode`.
- [done] composite (struct) payloads in `Option`/`Result` — lifts the M2 scalar-only cut.
  `Scalar` gains `Struct(u32)`, so `Option<Pt>` / `Result<User, Error>` are representable;
  `Some`/`Ok`/`Err` accept a struct, `?` unwraps to a struct, `else` unwraps `Option<Struct>`.
  Codegen threads the struct-type table through the `Option`/`Result` aggregate builders so a
  struct payload lowers to a nested aggregate. The second `json.decode` prerequisite
  (decode returns `Result<T, Error>`).
- [done] `json.decode` (first cut) — parse a `str` into a struct, yielding `Result<T, Error>`.
  The target `T` is inferred from the binding annotation threaded through `?`
  (`let u: T := json.decode(s)?`); `check_try` now passes the expected type inward. There is no
  `<T>` call syntax — settled: Align has no expression-position type arguments (no turbofish;
  `open-questions.md` Settled). MIR fills a zeroed out-struct via the runtime parser (status `i32`)
  then branches into `Ok(<struct>)` / `Err(<code>)`. Codegen builds a field-descriptor table
  (name / type-tag / byte offset via the target layout) and calls `align_rt_json_decode`, a
  minimal object parser (field order irrelevant, unknown keys ignored, missing/malformed →
  error). M5 cut: a flat struct of `i64`/`i32`/`bool` fields.
- [done] `json.decode` for `float` fields (scalars are copied into the struct — no borrow
  concern). Combined with int/bool, `json.decode` now covers **all scalar fields**.
- [done, via Memory Model v2] `json.decode` for `str` fields (zero-copy `{ptr,len}` views
  region-tied to the input), owned `array<scalar>`, and owned `array<Struct>` (AoS) — the last
  is the `draft.md` §19 headline. Field tables are emitted as compile-time constant globals.
  Still deferred: nested-struct fields and SIMD scan. (`<T>` generic-call syntax is not deferred
  but **settled away** — the binding annotation infers the target through `?`; no turbofish.)
- [todo] owned `string` / `bytes`, const string pool, `html`/`json` template variants.

Status (M5-A): `str` is a Copy view, lexed with the common escapes; literals lower to a
private constant + `{ ptr, len }`. `print` accepts `str` or an integer. `examples/strings.align` runs.

Completion condition: the example in `draft.md` §19 runs (JSON decode → aggregate → builder
output). **Met (compiler side), via Memory Model v2** — §19 decodes `array<User>` with a `str`
field and folds `where(.active).score.sum()` into one loop, both delivered by the zero-copy
borrow-region decode + owned `array<Struct>` + fused-pipeline work (MMv2 slices 8d-1/8d-2). The
remaining gap is `main(args: array<str>)`: **`fs.read_file`** reads a file into an owned `string`,
and **`io.stdout.write`** writes a `str`/`string`/`builder` to stdout (no newline), so the §19
body — read file → `json.decode` (into `array<User>`) → `where(.active).score.sum()` → format with
a `builder` → `io.stdout.write(out)` — runs **verbatim** bar the signature. Full `§19` *verbatim*
needs only `main(args: array<str>)`, being added as `str`-in-composites (the ideal form, extending
the MMv2 region model rather than a `main`-only special case), in three steps:
- **[done] PR-A** — `Scalar::Str`: `str` as a composite payload. `Option<str>` / `Result<str,E>`
  construct + unwrap, region-tracked (an arena `str` in an `Option<str>` can't escape — falls out
  of the region model, no new logic); `box<str>` rejected (a view is not boxable). `str` is Copy
  (not Move), so such composites are never dropped — they borrow.
- **[done] PR-B** — `array<str>` / `slice<str>`: `str`-array literals, index (→ `str`), `.len()`
  (element store/load reuses the `[N x {ptr,len}]` scalar-array machinery; `slice<str>` via the
  existing `ArrayToSlice` coercion). A container's region follows its `str` element's: a *fixed*
  `array<str>` is now region-tracked (`tracks_region(Array(s)) = tracks_region(s)`), so an array of
  arena `str`s can't let an element escape via index+return — while `array<i64>` stays
  Static/returnable.
- **[done] PR-C** — `main(args: array<str>)` ABI + argv marshalling. `main(args: array<str>) ->
  Result<(), Error>` is accepted; the C `main` wrapper takes `(argc, argv)` and calls
  `align_rt_args_build` (a buffer of `str` views into argv — argv strings process-lifetime, the
  buffer `Drop`-freed at `main` exit), then `align_main(args)`. `alignc run` forwards trailing args
  to the program. The §19 program now runs from a file path in `args[1]`.
- **[settled, no code] PR-D** — the one apparent residual was the `json.decode<array<User>>(data)`
  generic-*call* syntax. Resolved by **design, not implementation**: Align has no expression-position
  type-argument syntax (no turbofish — `open-questions.md` Settled "Type-argument syntax"). draft.md
  §19 is amended to the inference form `users: array<User> := json.decode(data)?`, which the checker
  already supports (target inferred from the binding through `?`). So §19 runs verbatim **as written
  in the amended spec**, and the `<` disambiguation is avoided outright rather than implemented.
M5 language features are complete (strings, templates, `json.encode` for struct/array,
`json.decode` for scalar / `str` / `array<scalar>` / `array<Struct>`).

## Memory Model v2 — borrow-region + owned heap/drop (foundation; before M6) — DONE

The dedicated phase that the deferred "ideal forms" of M4 and M5 both needed. **The whole
model + the per-slice ledger live in `08-memory-model-v2.md`** (region lattice, owned heap +
drop, zero-copy decode); slices 1–8d are **implemented**. The two foundations it delivered:
- **Borrow-region propagation** — the old point solutions (arena depth, slice "local-backed",
  struct `str` region-0) are unified into one region lattice (`Static ⊐ Frame ⊐ Arena(k)`).
  Every view producer (slice, `str` borrow, struct field, a `json.decode`-d struct/array)
  carries an inferred region, and `EscapeCheck` forbids it outliving its source.
- **Owned / dynamic heap collections + drop** — free-standing owned `string` / `array<T>` /
  `array<Struct>` / `builder`, with per-binding MIR `Drop` (null-on-move drop flags) outside an
  arena and arena bulk-free inside one. Owned payloads in `Option`/`Result` are dropped/moved
  as a unit.

Delivered on top of it: zero-copy `str`/`array<T>`/`array<Struct>` decode region-tied to the
input, with explicit `.clone()` to escape; owned-array materialization; and bounds-checked element
indexing — `recv[index]` (scalar) and `arr[index].field` (a struct-array element's field).
**`draft.md` §19 now runs end-to-end except the `fs`/`io` std boundary** (`json.decode` into
`array<User>` → `where(.active).score.sum()` as one fused loop). Still **deferred** (separate, deliberately
un-rushed tracks, not corner-cut): tuples / multi-value returns (for `partition`),
`array<slice<T>>` (for `chunks`), `array<Struct>.clone()`, and a bare whole-struct element value
`users[i]` (no field) — see `08-memory-model-v2.md` §11 and `open-questions.md`.

## M6 — SIMD / vec / mask

- `vec2/4/8/16<T>`, `mask<T>`, `bitset`.
- temporary-array-free fusion of the array expression `a = (b+c)*d - e`.
- deterministic lowering of MIR mask to LLVM vector select.
- `sum_where` / `dot` / `select`.
- **SoA (`soa array<T>`) + `align(N)`** land here (layout features whose payoff is
  vectorization / aligned zero-copy interop). Both are retrofit-sensitive — keep array
  field-access lowering layout-parametric and reserve room in field-offset math *before*
  M6 (see `open-questions.md` Open "SoA layout" / "`align(N)`"). **Groundwork landed:** SoA —
  `Ty::DynStructArray` carries a `Layout` (AoS today) and all struct-array element-field addressing
  routes through one MIR seam (`lower_field_access`) where the SoA branch hooks in; `align(N)` —
  `StructDef` carries `align: Option<u32>` (None today) and codegen routes all allocation alignment
  through one `type_align` seam. M6 then wires the `soa`/`align(N)` surface syntax + SoA codegen
  into those seams.

Completion condition: confirm that the vectorized code contains vector instructions at the LLVM IR level.

## M7 — Parallelism

- `par_map` (parallel unit = chunk), `chunks`.
- side-effect checking for `par_map` (reusing the M3 analysis).
- `task_group` / `spawn` / `wait` (I/O concurrency).
- async/await is not included (`non-goals.md`).

## M8 — Tooling and Quality

- the official formatter (mandatory, `draft.md` §16).
- the full set of standard lints (allocation in loop / huge struct copy / unnecessary clone / unnecessary heap / unhandled Result / branch in hot loop / string re-scan / implicit copy).
- `unsafe` blocks and `raw.*`.

## Design Issues to Settle in Parallel

Settle each item in `open-questions.md`, tied to its related M (do not defer).

```text
error type design          → finalized in M2
ownership syntax           → finalized in M3
arena API (explicit allocator) → finalized in M3
minimal generics system    → finalized before starting M4 (array operations require generics)
out params + noalias       → right after Memory Model v2 (extends EscapeCheck/MoveCheck)
arena checkpoint/rollback  → std arena API, after Memory Model v2
SoA layout + align(N)      → finalized in M6 (keep array lowering layout-parametric before then)
string SSO                 → settled: NOT adopted (open-questions Settled)
panic / unwinding          → settled: no unwind, plain-call CFG (open-questions Settled)
purity inference           → finalized in M7 (integral with par_map checking)
presence of SIMD intrinsics → finalized in M6
reflection                 → out of v1 scope
FFI                        → out of v1 *language* core; design before std.compress / pkg DB
                             drivers (they wrap C engines via FFI). Reconsider after M8.
backend/runtime perf       → deferrable backlog (VLA/SVE, nontemporal, fast-math, LTO,
                             -march=native, GPU codegen, SIMD JSON/str, perfect hash, mmap/
                             io_uring) — open-questions Future "Hardware & backend optimization
                             backlog". No front-end change; add after the core + std.
```

## Out of v1 Scope (intentional)

As in `non-goals.md` / `open-questions.md`. GPU backend, distributed execution, incremental compilation, and self-hosting are outside v1. However, keeping MIR backend-agnostic does not obstruct future additions (`00-overview.md`).
