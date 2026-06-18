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
lowering + the `main` wrapper. `std.fs.read_file` stays a thin fixture until M5.

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
- [todo] dynamic-length `array<T>`/`slice<T>` + array type annotations, `out` args,
  more terminals/stages (`reduce`/`scan`/`filter`/`partition`/`sort`/`chunks`),
  array-valued results (materialization), and named-function `map` over struct elements
  (needs struct-by-value params, deferred since M1).

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

- `str` / `string` / `bytes` / `buffer` / `builder`.
- string literal meta and const string pool.
- desugaring of `template` / `html` / `json` strings (`write_static`/`write_value`).
- `json.decode<T>` / `encode<T>`, field table generation from structs, zero-copy view, SIMD structural scan.

Completion condition: the example in `draft.md` §19 runs **in full** (JSON read → aggregate → builder output).

## M6 — SIMD / vec / mask

- `vec2/4/8/16<T>`, `mask<T>`, `bitset`.
- temporary-array-free fusion of the array expression `a = (b+c)*d - e`.
- deterministic lowering of MIR mask to LLVM vector select.
- `sum_where` / `dot` / `select`.

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
purity inference           → finalized in M7 (integral with par_map checking)
presence of SIMD intrinsics → finalized in M6
reflection / FFI           → out of v1 scope. Reconsider after M8
```

## Out of v1 Scope (intentional)

As in `non-goals.md` / `open-questions.md`. GPU backend, distributed execution, incremental compilation, and self-hosting are outside v1. However, keeping MIR backend-agnostic does not obstruct future additions (`00-overview.md`).
