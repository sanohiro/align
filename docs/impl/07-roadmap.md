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

- move of owning types and use-after-move errors, explicit `clone()`.
- Pass-by-value of small structs, lint for large-struct copies.
- `arena {}` block → calls to `align_runtime`'s arena allocator and bulk free.
- Arena view escape checking.

Completion condition: data allocated inside `arena {}` is correctly freed at block exit, and escapes become errors.

## M4 — Array Processing Core (Align's protagonist)

- `array<T>` / `slice<T>`, `out` arguments.
- chains such as `map` / `filter` / `where` / `reduce` / `sum`.
- **loop fusion** in MIR (`map().where().sum()` into a single loop).
- field projection such as `.score`.

Completion condition: the example in `draft.md` §19 (the array-aggregation part, excluding JSON) runs as fused code.

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
