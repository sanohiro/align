# Implementation Roadmap

Milestones. The principle is as in `00-overview.md` — **fix the whole design first, drive a vertical skeleton through the implementation first, and plug each feature into all stages**. Each M has the completion condition of "works end to end (`.align` → run → output verification)." Do not create tasks that do not run vertically through the stages (e.g. doing the whole type system first).

---

## Status & forward plan (snapshot)

This section is the **live sequence** — what is done and what is next, in order. The per-milestone
detail further below is the historical / spec record; consult it for *how* a milestone is built,
but read the order from here.

**Standing principles** (full text in `CLAUDE.md`):
- **Ideal form, or defer** — ship only the ideal / unified / philosophy-aligned form; if a feature
  can't be done that way, present the design and defer it. Never a half-hearted compromise.
- **No backward compatibility** (pre-release) — change APIs outright; no aliases / shims / dual paths.
- **Finish all of `core` + the language before `std`** — the OS-boundary layer (`std`/`pkg`) waits.

**Done:** M0–M3 (skeleton · functions/control/struct/bool · Result/Option/`?` · move/value/arena) ·
**Memory Model v2** (borrow-region propagation + owned heap/drop) · **M4** (array-processing core) ·
**M5** (strings, templates, `json.encode`/`decode`). Cross-cutting since: first-class **tuples**
(incl. partial field moves); **lambdas** in every stage & reducer with **capture** (lifted,
escape-driven design settled); **`sort_by_key`**; whole-struct **`arr[i]`** by value (struct fields
enforced Copy-only); and most of **M7** — `par_map` (real threads) + `chunks` + purity inference.

**Forward plan (ordered — finish the language, then std):**
1. **core.math** — explicit-overflow arithmetic `checked_*` / `saturating_*` / `wrapping_*` for
   add/sub/mul **DONE** (methods on integers; `checked_*` → `Option<T>`, via LLVM `.sat` /
   `.with.overflow`). Scalar `abs` / `min` / `max` **DONE** (methods on numerics; `a.min(b)`
   pairwise is arity-dispatched alongside the `arr.min()` reduction; LLVM `abs`/`fabs`/
   `{s,u}min`/`{s,u}max`/`minimum`/`maximum`). Float `sqrt`/`floor`/`ceil`/`round`/`trunc`/`pow` DONE (float-only methods, LLVM intrinsics; `round` = ties-away-from-zero). **core.math DONE.**
2. **core.bytes / core.buffer** — design SETTLED, **build deferred until a consumer**:
   `bytes` = `slice<u8>` (no separate type — largely exists already); `buffer` = a distinct
   growable owned byte container (distinct from fixed `array<u8>` and the text-only `builder`).
   Its real consumers are binary I/O (`std`) and `core.hash`, not yet built — building it ahead
   of them risks the wrong op set (premature). See `open-questions.md` "bytes / buffer". So the
   next *build* is #3.
3. **first-class closures (escape-driven) → `task_group`** — M7 concurrency. Design SETTLED
   (`open-questions.md`); closures are the foundation, `task_group` the consumer. Built in slices:
   **① non-capturing function values DONE** — a top-level fn used as a value is a function pointer
   (`Ty::Fn`, Copy/`Static`, no env), and calling such a local is an indirect call (`f := double;
   f(5)`; scalar signatures). **②a lambda-as-value DONE** — a non-capturing lambda with typed
   parameters (`f := fn x: i32 { x*2 }`) lifts to a function value (params from the annotations,
   no use site to infer from; captures rejected). **②b-1 closure ABI DONE** — a `Ty::Fn` value is
   now a `{ fn_ptr, env_ptr }` fat pointer with the env-ABI `fn(env, args)`; non-capturing / named
   functions are wrapped by a thunk `X$fnval(env, args) = X(args)` (env null). Behavior-preserving;
   the foundation for captures. **②b-2 captures DONE** — a capturing lambda (`f := fn x: i32 { x +
   k }`) copies its captures into a frame-local env (hoisted alloca); a per-lifted-fn thunk
   `lifted$clos(env, args) = lifted(args, env.0, …)` unpacks it. Scalar (Copy) captures; no escape
   check needed yet (a `Ty::Fn` value can't leave its frame — no fn-typed returns/fields/params).
   **③ higher-order functions DONE** — the `fn(T) -> R` type syntax + fn-typed parameters, so a
   function value (named fn, lambda, or capturing closure) can be **passed** to a function
   (`fn apply(f: fn(i64)->i64, x: i64) = f(x)`). Sound with the frame-local env: the closure's env
   outlives the call. A fn-typed **return** is rejected (it would carry a frame env out of the
   frame); struct fields already reject `Ty::Fn`. Next: **④ `task_group`** (`draft.md` §11) — the
   structured concurrency scope. It uses the **region-owned env**: each `spawn` snapshots its
   captures into a **fresh environment in the `task_group` region** (an arena-like region freed at
   scope end). The ②b-2 frame-local env (one hoisted slot per closure site) cannot back a spawned
   closure — a `spawn` in a loop would reuse that slot, so a deferred task reads the final value
   and a concurrent task races the next iteration (this is why the settled design specified the
   region env; `spawn` is the escape that triggers it). Sub-slices: **④a DONE** (walking skeleton)
   — `task_group {}` scope + `spawn(fn{…}) -> Task<R>` + `wait()` + `t.get()`, `spawn`/`wait` valid
   only inside the scope. Tasks run **eagerly** (`spawn` calls the zero-arg closure immediately;
   `Task<R>` is represented identically to `R`; `get` is identity, `wait` a no-op) — correct
   sequential results, and eager execution sidesteps the env-reuse race so the ②b-2 frame env is
   fine here. **④b-1a DONE** — `Task<R>` is now a **`box<R>` in the task_group region** (the scope opens an
   arena; `spawn` boxes its result there, `get` is a box load), so a task handle is region-tied
   (cannot escape the scope) and the result lives in a region slot — the memory model real threads
   need. Still eager + primitive-scalar `R` (owned/view results deferred). **④b-1b DONE** — deferred
   execution: a `task_group` is now a runtime `TaskGroup` (a region + a task list); `spawn`
   snapshots its captures into a fresh region env and registers a task (it does **not** run yet);
   `wait()` runs all tasks (sequentially) via a per-`R` trampoline that writes each result slot;
   an early `return`/`?` out of the scope joins + frees it. (`get`-before-`wait` reads an
   uncomputed slot — rejected by the ④c check.) **④b-2 DONE** — `wait()` now runs the tasks
   on **real threads**: it spawns a worker thread per registered task and joins them all (fork-join).
   Safe by construction — each task's env/slot are a fresh, private region allocation (no sharing;
   env read-only, slot write-only), all allocated at `spawn` time so no thread mutates the region
   during the run, and the region outlives the join. **④c-1 DONE** — the `get`-before-`wait`
   compile-time check, done soundly by **dominance**: a per-`task_group` `wait`-state flag (`spawn`
   clears it, `wait` sets it) merged across `if`/`else` as `then && else`, so `get()` is allowed
   only when a `wait()` ran on *every* path to it (a conditional `wait()` in one branch does not
   suffice — sound, not a linear approximation). **④c-2 DONE** — the `wait()?` error boundary. A
   task closure may return `Result<R, Error>` (so `check_spawn` lifts the literal lambda directly
   rather than via a scalar-ret `Ty::Fn` value); a per-group `fallible` flag types `wait()` as
   `Result<(), Error>` (else `()`); the per-`(R, fallible)` trampoline returns an `i32` error code
   (storing the `Ok` payload / returning the `Err` code), `tg_wait` collects the workers' codes and
   returns the first nonzero, `wait()` builds a `Result` from it, and `wait()?` propagates. For a
   fallible group only a *successful* `wait()?` enables `get()` (a bare `wait()` does not). **The
   first-class-closures → `task_group` arc (slices ①–④c) is COMPLETE**: closures as values /
   captures / higher-order arguments, and a real parallel `task_group` with structured join,
   sound `get`-before-`wait`, and a `wait()?` error boundary.
4. **Language-spec stock-take (2026-06-24) → the "big three" expressiveness gaps.** Before `std`,
   the language itself needs three interlocked features to be expressively complete (model any
   domain, handle errors well). Validated against an external review pass (Codex/Gemini); these are
   the genuine gaps — everything else they raised is either std-adjacent, perf-tier, or off-philosophy
   (see the "not adopted" note below). Build in order — each unblocks the next:
   - **4a. Sum types + exhaustive `match`** *(design SETTLED, `open-questions.md`)* — the keystone.
     Keyword-less `Name { Circle(f32), Rect(f32, f32) }`, `Type.Variant` construction, a
     mandatory-exhaustive `match` expression; works on `Option`/`Result` too. The OOP-free way to
     model variants, AI-friendly (exhaustiveness), and lower-risk than generics. Slices S1–S4 in
     `open-questions.md`. **S1a DONE** — tag-only enums end to end: keyword-less decl
     (`Color { Red, Green, Blue }`, disambiguated from a struct by content), `Type.Variant`
     construction, and a mandatory-exhaustive `match` expression (missing-variant / unknown-variant
     / non-enum scrutinee / duplicate-arm all diagnosed). `Ty::Enum(id)` interned into
     `Program.enums`, repr = the variant tag (`i32`); `match` lowers to a tag-compare branch chain
     with a result slot (like `if`); enums are Copy values usable as locals / params / returns.
     **S1b DONE** — scalar variant payloads: `Shape { Circle(f64), Rect(f64, f64) }`,
     `Type.Variant(args)` construction (arity/type checked), and `match` arms binding the payload
     positionally (`Circle(r) => …`, scoped to the arm). The enum now lowers to a non-union tagged
     struct `{ i32 tag, <every variant's payload flattened> }` (the `Result` `{tag, ok, err}` shape
     generalized), built/read via SSA insert/extract-value (MIR `MakeEnum` / `EnumTagEq` /
     `EnumPayload`); payloads are primitive scalars. **S3 DONE** — `match` on the builtin
     `Option`/`Result` (`match o { Some(x) => …, None => … }`, `match r { Ok(v) => …, Err(e) => … }`):
     `check_match` derives the variant list from the scrutinee type (a `match_variants` helper
     covering enum + Option + Result uniformly), and MIR lowers these two-variant types as a single
     `IsSome`/`IsOk` branch reusing the existing `Option`/`Result` unwrap rvalues (order-independent,
     no negation). `else`-unwrap and `?` remain the ergonomic shorthands. **S2 DONE** — plain-data
     struct variant payloads (`Dot(Point)`); `str`-field structs + tuple payloads deferred.
     **S4 (or-patterns) DONE** — `A | B | ...` shares one arm (bare variant names, binds nothing,
     counts toward exhaustiveness). **Guards and recursive (boxed) enums reviewed and not adopted:**
     guards cross the settled "`match` = variants, `if` = conditions" One-Way line; recursive enums
     run against the data-oriented core and need a larger box-rework track — both deferred (rationale
     in `open-questions.md`). So **4a (sum types + exhaustive `match`) is complete** for the planned
     surface. (A space-optimal union layout instead of flattened fields is a deferred codegen
     optimization — no surface change.)
   - **4b. Error type** *(DONE)* — built **on** sum types: `Error` as a sum type of
     categories + structured payloads, an explicit value (no unwinding / no stack-trace alloc),
     static/predictable `?` conversion, structured (position-bearing) errors. Replaces the M2 i32
     placeholder. **4b-1 DONE (foundation)** — errors can be
     **user-defined sum types**: `Scalar::Enum(u32)` makes an enum a first-class `Option`/`Result`
     payload, so `Result<T, MyError>` works end to end. **4b-2 DONE** — the canonical **`Error` is a
     builtin sum type** `{ NotFound, Invalid, Denied, Code(i32) }` (a reserved type name):
     `Error.NotFound` / `Error.Code(c)` construct it (`error(c)` = sugar), `match` discriminates,
     `?` propagates. Every fallible builtin returns `Result<_, Error>` (wrapping its i32 status as
     `Error.Code`); `main` maps the error to an exit code (`Code(c)`→c, category→tag+1); and the
     **task_group fallible path was reworked** to carry the full `Error` across threads via a
     per-task err-slot (`tg_wait` returns the first errored slot). **4b-3 DONE** — explicit error conversion is `result.map_err(f)` (no implicit `?` coercion). **4b-4 DONE** — position-bearing **structured errors** work on the 4b-1 + S2 foundation (a variant carrying a `Pos` struct, `?`-propagated, `match`-read — `examples/structured_error.align`); free-form **`.with_context` was reviewed and NOT adopted** (off-philosophy: structured sum-type payloads are the context mechanism, not dynamic string chaining — rationale in `open-questions.md`). **So 4b (the Error type) is complete** for the planned surface. (ErrCode removed; richer `str`-carrying error payloads remain deferred with S2's `str`-field payloads.)
   - **4c. Minimal generics + constraints** — the riskiest; approach minimally (tiny builtin bounds,
     explicit monomorphization, no turbofish, no Rust-trait complexity). **4c-1 DONE (the
     unconstrained walking skeleton)** — `fn f<T>(...)` monomorphized per distinct concrete
     instantiation (`Ty::Param(i)` substituted *before* the flow analyses / MIR, so MoveCheck/drop /
     codegen see only concrete types; mangled `id$i32`; transitive instantiation to a fixpoint).
     Type arguments are inferred (no turbofish); a type parameter is **opaque** (no operations — the
     template is checked abstractly, an uninstantiated generic is not checked, C++-template-like);
     skeleton cut = bare positions only (no `array<T>` / nested), no lambda/pipeline in a generic.
     (`tests/generics.rs`, `examples/generics.align`.) **4c-2 DONE (the constraint model)** — a type
     parameter may carry a **builtin bound** `fn f<T: Ord>` from the fixed hierarchy `Num` ⊃ `Ord` ⊃
     `Eq` (`Num` = arithmetic+ordering+equality on numerics, `Ord` = ordering+equality on
     numerics/`char`, `Eq` = equality on numerics/`char`/`bool`/`str`); the bound gates which
     operations a `Param` value allows in the template (`x + x` needs `Num`, `a > b` needs `Ord`,
     `a == b` needs `Eq`), and a concrete type argument is checked against it at instantiation. No
     user-defined trait bounds. (Closes a 4c-1 hole where `==`/`>` on an unconstrained `T` slipped
     through.) **4c-3 DONE (type parameters in `Option`/`Result` positions)** — `T` may be nested in
     an `Option<T>` / `Result<T, E>` payload (param or return), so generic combinators
     `fn unwrap_or<T>(o: Option<T>, d: T) -> T` / `fn ok<T>(x: T) -> Result<T, Error>` work. New
     `Scalar::Param`; structural inference (`match_param`) binds `Param` bare or nested + seeds a
     return-only param from the expected type; a nested param finalizes eagerly at the call, a bare
     one stays deferred. (`box`/`slice`/`array`/tuple positions still rejected.) **4c-4 + 4c-5 DONE —
     generic structs.** `Pair<T> { a: T, b: T }` works end to end: the resolver refactor landed
     (`resolve_type` takes a `TyCx` bundling the interners; `structs` grows *during* resolution, a
     `&mut Vec` like `tuples`/`fn_types`; a `Pair<i32>` type interns a monomorph `StructDef` on
     demand, deduped by mangled name; templates with `Param` fields live in a separate registry kept
     out of codegen; concrete struct ids get reserved slots so monomorphs never shift them). A
     generic struct literal `Pair { a: 1, b: 2 }` infers its type arguments from the field values
     (no turbofish) then monomorphizes. (`examples/generic_struct.align`.) **4c-6 DONE — generic sum types.**
     `Opt<T> { Some(T), None }` works end to end (the enum analogue of 4c-5: `enum_templates`, the
     `enums` table grows during resolution with reserved slots + `enum_mono` dedup, `Opt<i32>` interns
     a monomorph `EnumDef`, and `Opt.Some(7)` infers the type args from the payload then
     monomorphizes). `examples/generic_sum_type.align`. **→ 4c is CLOSED.** Minimal generics is
     complete (functions + builtin bounds + generic structs + generic sum types); per the philosophy
     it is deliberately *not* extended further. The leftovers are not generics: generic **containers**
     (`Stack<T>`/`array<T>` fields) fold into #5 `group_by` if a consumer needs them; **`vec<N,T>`** is
     M6; a generic-def-inside-a-generic-fn is an optional refinement. The "big three" (4a/4b/4c) are
     done.
5. **group_by** — design the return type first (needs a map-like container, which needs 4c); then build.
6. **core.bitset / core.hash** — design (also map-like / generic-aware), then build.
7. **LLVM optimizer pipeline (`run_passes`) + M6 SIMD** (`vec` / `mask` / SoA / `align(N)`) + the
   LLVM-version upgrade — the perf tier. **Optimizer DONE** — `write_object` runs the default `-O2`
   pipeline before emitting, so the lifted lambdas inline and the fused `map`/`where`/`reduce` loops
   vectorize (`xs.map(dbl).sum()` → one SSE2 `paddq` loop with `dbl` inlined; verified via `objdump`,
   all e2e tests correct under `-O2`). Remaining: the explicit `vec`/`mask`/SoA/`align(N)` surface,
   the LLVM-version upgrade, and the other backend levers — M6 proper (`open-questions.md` Future).
8. **M8 — tooling**: the formatter, the standard lints, `unsafe` blocks + `raw.*`.
9. **Then `std`** (OS boundary) and `pkg`.

Deferred-on-purpose until their slot (not gaps): GPU backend, FFI (before the `pkg` DB drivers /
`std.compress` that wrap C engines — `unsafe`-required, `layout(C)` ABI, `{ptr,len}` views),
`task_group` cancellation / timeout (needs `std.time` / `std.net` I/O checkpoints — cooperative,
never an implicit kill), reflection — see `non-goals.md` / `open-questions.md` Future.

**Reviewed but NOT adopted (off-philosophy or low-value now):** a general Zig-style `comptime` /
user CTFE — rejected as a *second* computation model that erodes One-Way / AI-friendliness; Align's
compile-time story stays **builtin-driven static data** (JSON field tables, `template` analysis,
literal/hash tables) only. A standalone allocation-monitor / profiler is tooling (folds into the M8
lints — "allocation in a loop", "unnecessary clone"), not a language feature.

---

## M0 — Skeleton Traversal (walking skeleton) — DONE

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

## M1 — The Bones of the Language (functions, control, struct, bool) — DONE

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

## M2 — Errors and Existence (Result / Option / ?) — DONE

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

## M3 — Memory Model (move / value / arena) — DONE

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

## M4 — Array Processing Core (Align's protagonist) — DONE

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
  (`sum`/`reduce`/`count`/`any`/`all`) were already complete.
- [done] **`sort_by_key(f)`** — materialize the surviving (primitive scalar) elements and sort
  ascending by `f(element)` (an orderable scalar key: int/float/char). Reuses the MIR insertion
  sort (`lower_array_sort` gained an optional `SortKey`), comparing `key(a) > key(b)` instead of
  `a > b`; the element need not be numeric (it is ordered by the key). `f` is a named function or
  a lambda and **may capture** (the key call is in the enclosing loop, so captures are ordinary
  arguments). Struct-element sort stays deferred (needs struct-array materialization, like `sort`).
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
  `Tuple` local).
- [done] **owned tuple parameters** — `fn f(t: (array<i64>, array<i64>))`. Falls out of the bind +
  drop machinery: an owned-tuple param joins the drop set (like an owned array param), so the callee
  drops it at exit if it doesn't consume it, and a caller passing a bound owned tuple moves it (slot
  nulled).
- [done] **partial field moves** — `a := t.0` moves one owned element out of a bound tuple, leaving
  the other elements usable. MoveCheck tracks moves per field (`MovedKey::{Whole,Field}`): re-moving
  a field, or using the tuple as a whole after a field move, is use-after-move; a borrowing read
  (`t.0.sum()`) does not move. MIR nulls the moved field (`Stmt::NullTupleField`) so the tuple's
  exit `Drop` frees null there. An owned index out of a *temporary* tuple (`f().0`) is rejected
  (it would orphan the other owned elements) — bind it first. **Tuples are now complete.**
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
- Every stage and reducer — `map`/`where`/`reduce`/`par_map`/`scan`/`partition`/`any`/`all` —
  takes a named function **or an inline lambda** (`map(fn x { x * 2 })`,
  `reduce(fn acc, x { acc + x }, 0)`). A lambda is **lifted** to a synthetic top-level function
  (`fn$lambdaN`) in sema (`lift_lambda`), so it flows through the same `Rvalue::Call` +
  fused-loop lowering as a named function — optimized identically (no closure environment). The
  par_map Pure requirement applies to a lifted lambda too. For the two-parameter reducers
  (`reduce`/`scan`), a named fold takes its accumulator/element types from its signature; a
  lambda infers the accumulator from the initial value and the element from the source.
  A lambda may **capture** enclosing locals (slice ③): a captured local becomes a trailing **value
  parameter** of the lifted function, passed at the call site (`stage_call_args` appends it). No
  closure environment — the capture is a loop-invariant argument LLVM hoists. This is something a
  named function cannot do (`map(fn x { x * factor })`). Capture is copy-values only (an owned/Move
  capture is rejected); it works in **every** stage and reducer — `map`/`where` (in `StageKind`)
  and `reduce`/`scan`/`partition`/`any`/`all`/`par_map` (a `captures` field on each node, threaded
  to the per-element call). A *capturing* `par_map` falls back to the sequential path (the parallel
  runtime thunk takes no capture context). All three flow analyses (`MoveCheck`/`EscapeCheck`/
  `EffectScan`) walk stage and node captures. First-class function values remain a follow-up.
- Method chains rely on the slice-0 postfix `.` (FieldAccess); the pipeline is
  collected from the AST at the `sum` terminal and lowered as one loop.
- Arrays are not yet Move-checked (literals are consumed only by the reduction);
  whole-array move/ownership arrives with dynamic arrays.
```

Completion condition (met): an array-aggregation pipeline (`map`/`where`/`sum`) runs
as fused code (one loop). The full `draft.md` §19 example (JSON, struct fields) needs
later slices (struct arrays, M5 strings/JSON).

## M5 — Strings and JSON — DONE

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

## M6 — SIMD / vec / mask — STARTED (explicit `vecN<T>` slice 1 landed)

- `vec2/4/8/16<T>`, `mask<T>`, `bitset`.
  - **`vecN<T>` slice 1 — DONE.** The explicit fixed-width vector type `vec2`/`vec4`/`vec8`/`vec16`
    of a numeric scalar (`Ty::Vec(Scalar, N)`, a Copy/`Static` register value lowering to the LLVM
    `<N x T>`). **Construction** reuses the array literal under a `vecN<T>` annotation (the annotation
    picks the SIMD representation — no new syntax, "Nothing hidden"; a dedicated `hir::VecLit` that
    lowers to a value `Rvalue::MakeVec` insertelement chain, unlike the slot-based `ArrayLit`).
    **Elementwise `+`/`-`/`*`/`/`** route through `gen_bin` to inkwell's vector `build_int_*`/
    `build_float_*` (one lane-wise instruction; `%` deferred). **Lane read `v[i]`** (a constant lane,
    reusing `ExprKind::Index` → `Rvalue::VecExtract`/extractelement). The N-in-name form (`vec4<f32>`)
    needs zero lexer/parser/AST change — `resolve_type` derives N from the name. Completion condition
    met: the IR carries real `<N x T>` types + `add <N x i32>` (verified via `emit-llvm`); per-lane
    run tests confirm correct lane-wise arithmetic. (`tests/vec_simd.rs`, `examples/vec_simd.align`.)
    **Deferred (later M6 slices):** `sum_where`, `dot`/horizontal reductions, scalar broadcast/splat,
    load/store from an `array`/`slice`, the `vec<N,T>` generic-arg spelling, and lane *assignment*
    `v[i] = x`. The LLVM-version upgrade is not needed for this slice (LLVM 19 has full vector support).
  - **`mask` + comparison + `select` slice 2 — DONE.** A `vecN<T>` comparison (`==`/`!=`/`<`/`<=`/`>`/
    `>=`) is elementwise and yields a **`mask`** (`Ty::Mask(N)` → LLVM `<N x i1>`, one bool lane per
    vector lane; element-agnostic, width-only — produced/consumed inline, no written annotation).
    `select(mask, a, b)` (a `core.vec` builtin, `hir::Select` → the existing `Rvalue::Select` with a
    *vector* cond) blends two same-type vectors lane-wise. Comparisons reuse `ExprKind::Binary`
    (`gen_bin` routes a vec operand + comparison op to `gen_vec_cmp` → vector `icmp`/`fcmp`); width is
    checked between the mask and the vectors. (`tests/vec_simd.rs`, `examples/vec_mask.align`.)
  - **scalar broadcast + `sum_where` slice 3 — DONE.** A **scalar on the right** of a vector op
    broadcasts across the lanes (`a + 5`, `scores > 80` — the draft §9 spelling), so `vec OP scalar`
    is elementwise against the splatted scalar. (`check_binary` defers the rhs type when the lhs is a
    vector and reconciles in `vec_binop`; codegen `operand_as_vector` splats a scalar operand via an
    all-lane insertelement chain that folds to a hardware broadcast. The vector must be on the
    **left** — scalar-on-the-left and a vector-literal right operand are deferred.)
    **`vec.sum_where(mask)`** (`hir::VecSumWhere` → `Rvalue::VecSumWhere`) is the masked horizontal
    sum: `select(mask, vec, 0)` then add all lanes → the element scalar (so the draft §9
    `scores.sum_where(scores > 80)` runs). (`examples/vec_sum_where.align`.)
  - **`dot` slice 4 — DONE.** `dot(a, b)` — the dot product of two `vecN<T>` → the element scalar
    `sum(a[i] * b[i])`. A **free-function** builtin (the draft §9 spelling, the vector sibling of
    `select`; `hir::VecDot` → `Rvalue::VecDot`), kept distinct from the array-pipeline terminal
    `xs.dot(ys)` (a method — a fused loop over arbitrary-length arrays); the two never collide (free
    call vs method call). Lowers to a vector multiply then the shared `horizontal_sum` reduction (the
    multiply dual of `sum_where`); int + float. (`examples/vec_dot.align`.)
  - **`min` / `max` slice 5 — DONE.** `v.min()` / `v.max()` — the horizontal min/max of a `vecN<T>`
    → the smallest/largest lane, as the element scalar. **Same surface as the array reduction**
    `arr.min()`/`arr.max()` (a no-arg method); the dispatch routes to the SIMD reduction for any
    **vector-valued** receiver (`(a+b).max()`, `mk().min()`), and to the array reduction only for a
    genuinely pipeline-shaped receiver (a `.map()`/`.where()` stage or a `.field` projection, via the
    syntactic `is_array_pipeline_recv` — so a pipeline can't be mis-checked as a value). `hir::VecMinMax`
    → `Rvalue::VecMinMax` folds the lanes with the **same `llvm.{s,u}{min,max}` / `llvm.{minimum,maximum}`
    intrinsics as the `core.math` scalar `a.min(b)`** (so the reduction matches that semantics exactly);
    int / unsigned / float. (`examples/vec_minmax.align`.)
  - **bare `v.sum()` slice 6 — DONE.** `v.sum()` — the horizontal sum of a `vecN<T>` → the sum of all
    lanes, as the element scalar (the unmasked sibling of `sum_where`). Same dispatch shape as
    `min`/`max` (a vector receiver → the SIMD reduction, an array pipeline `xs.map(f).sum()` → the
    fused array path). `hir::VecSum` → `Rvalue::VecSum` reuses the shared `horizontal_sum`; int +
    float. (`examples/vec_sum.align`.)
  - **array load/store slice 7 — DONE.** `s.load(i) -> vecN<T>` reads `N` consecutive elements of a
    `slice<T>` starting at the runtime index `i` into a vector (`N`/`T` from the target annotation,
    like a vector literal); `s.store(i, v)` writes a vector's lanes back into a **writable** (`mut`/
    `out`) `slice<T>` at `i..i+N`. Both **bounds-checked** (`0 <= i && i + N <= len`, reusing the
    range-fail path). A fixed array is loaded/stored by passing it where a `slice<T>` is expected (the
    array→slice borrow). `hir::VecLoad`/`hir::VecStore` → `Rvalue::VecLoad` / `Stmt::VecStore`: codegen
    GEPs `&buf[i]` and emits a `<N x T>` load/store **at the element alignment** (the GEP yields an
    element-aligned pointer, so the vector access must not assume the wider vector alignment — an
    unaligned-but-valid access). The store reuses the `out`-slice writability rule (`place[i] = v`).
    This is the bridge between bulk array data and SIMD registers. (`examples/vec_load_store.align`.)
  - **lane assignment slice 8 — DONE.** `v[i] = x` writes one lane `i` (a constant in `0..N`) of a
    `mut vecN<T>` local to the scalar `x` — the write counterpart of the lane read `v[i]`. A vector is
    a register value, so it lowers to `v = insertelement(v, x, i)` (a new `Place::VecLane` →
    `hir::Stmt::AssignVecLane` → `Rvalue::VecInsert`, re-storing the local). Reuses the mutable-place
    writability rule (a `mut` local; an immutable vector / dynamic or out-of-range lane is rejected).
    (`examples/vec_lane_set.align`.)
  - **scalar-on-the-left broadcast slice 9 — DONE.** A scalar on the **left** of a vector op
    broadcasts too (`10 + a`, `2 < scores`), completing the broadcast symmetry (slice 3 did vector-on-
    the-left). The operand order is preserved for the non-commutative ops (`20 - a` = `[20-a0, …]`).
    The one-pass checker handles it via a **speculative rhs check with diagnostic rollback**
    (`check_binop_rhs`): the rhs is hinted with the lhs type as usual, but if the lhs is a scalar and
    the rhs is a vector, that hint is rolled back (`Diagnostics::truncate`) and the rhs re-checked
    unhinted, so the scalar broadcasts — no regression to ordinary scalar arithmetic (a generic-call
    rhs still gets the lhs hint). `vec_binop` gained the `(scalar, vec)` case; codegen detects the
    vector in either operand (`operand_as_vector` already splats the scalar). (`examples/vec_broadcast.align`.)
  - **written `maskN<T>` annotation slice 10 — DONE.** A comparison mask is now a **nameable type**
    `maskN<T>` (spelled like `vecN<T>` — `mask4<i32>` is the result of comparing `vec4<i32>`s), so a
    mask can be a `let` annotation, a **function parameter**, or a return type — threading a mask
    through code (`fn blend(m: mask4<i32>, a, b) = select(m, a, b)`). `Ty::Mask(u32)` became
    `Ty::Mask(Scalar, u32)` (element + width): a comparison yields `Ty::Mask(elem, n)`, and
    `select`/`sum_where` now require the mask's element **and** width to match the vectors (the repr
    stays `<N x i1>`, element-independent). `resolve_type` gained the `maskN<T>` arm (mirroring
    `vecN<T>`, via `parse_mask_name`). `draft.md` §9/§13 amended (`mask<T>` → `maskN<T>`, as `vec<N,T>`
    → `vecN<T>`). (`examples/vec_mask_annot.align`.)
  - **element-wise float-vector math slice 11 — DONE.** The unary float math ops `abs`/`sqrt`/`floor`/
    `ceil`/`round`/`trunc` now apply lane-wise to a `vecN<f32>`/`vecN<f64>` (the same `MathFn` surface
    as the scalar versions — "one way"), each lowering to the LLVM **vector** intrinsic
    (`llvm.sqrt.v4f32` etc.) via `call_intrinsic`. Sema accepts a float-vector receiver for the unary
    (`want_args == 0`) ops; codegen classifies `is_float`/`signed` by the element but keeps the vector
    as the intrinsic overload. (`examples/vec_math.align`.)
  - **binary + integer vector math slice 12 — DONE.** Element-wise `a.min(b)`/`a.max(b)` of two
    vectors (any numeric element) and integer-vector `abs` now vectorize too — each maps to one
    lane-wise instruction (`llvm.smax.v4i32`, `pabsd`, …). This was a pure **sema gate** broadening:
    slice 11's codegen was already element-aware with a vector overload, so `check_scalar_math` just
    accepts the vector cases (float vec: unary float ops + min/max; int vec: abs + min/max). `pow` is
    excluded — it lowers to a libcall, not a lane-wise instruction — so it (and `a.min()` with no arg,
    which is the reduction) stays as before.
  - **`fma` slice 13 — DONE.** `fma(a, b, c)` = `a*b + c` with a single rounding — a **free builtin**
    (like `dot`/`select`, not a method), float scalar or vector, lowering to one `llvm.fma`
    (`vfmadd`/`fmla`). The classic SIMD numeric kernel (dot products, FIR filters, Horner polynomials).
    Reuses `MathOp` via a new `MathFn::Fma` (ternary); `check_fma` dispatched alongside `select`/`dot`.
    (`examples/vec_fma.align`.) Still deferred: the generic `vec<N,T>` / numeric-type-arg spelling, an
    aligned-load fast path, a SIMD-unit tree reduction.
- temporary-array-free fusion of the array expression `a = (b+c)*d - e`.
- deterministic lowering of MIR mask to LLVM vector select.
- `sum_where` / `dot` / `select`.
- **SoA (`soa array<T>`) + `align(N)`** land here (layout features whose payoff is
  vectorization / aligned zero-copy interop). Both are retrofit-sensitive — keep array
  field-access lowering layout-parametric and reserve room in field-offset math *before*
  M6 (see `open-questions.md` Open "SoA layout" / "`align(N)`"). **Groundwork landed:** SoA —
  `Ty::DynStructArray` carries a `Layout` (AoS today) and all struct-array element-field addressing
  routes through one MIR seam (`lower_field_access`) where the SoA branch hooks in; `align(N)` —
  `StructDef` carries `align: Option<u32>` and codegen routes all allocation alignment through one
  `type_align` seam. **`align(N)` struct form DONE** — `align(N) Name { … }` parses to
  `StructDecl.align` → `StructDef.align`, and `type_align` returns `max(declared, natural)` (so it only
  over-aligns); the slot alloca / AoS struct-array element pick it up (`tests/align_attr.rs`,
  `examples/align_attr.align`, `draft.md` §9). Deferred: the `align(N) data := …` binding form over a
  scalar array (the aligned-vector-load enabler), arena/heap over-alignment, and over-aligned-struct
  size padding for a tight array stride. **SoA surface — largely DONE:** the `soa<T>` borrowed view,
  field projection (`s.field` → column slice), multi-column / mixed-width pipelines, `.to_soa()` +
  `json.decode → soa` construction, `group_by` over a soa, soa params/returns, and now **whole-element
  gather `s[i]`** (→ a Copy struct via `Rvalue::SoaGather`; `tests/soa.rs`, `examples/soa.align`) all
  ship. **Single-column windowing `s.field[a..b]`** also ships: a projected column is an ordinary
  `slice<FieldTy>`, so the existing slice sub-range (`SubSlice`) applies unchanged — no new type. This
  slice also fixed a latent `Rvalue::SoaColumn` correctness bug: materialising a column as a *value*
  (`c := s.field`, or sub-ranging it) used a flat `len*prefix` byte offset instead of the
  `align_up`-padded `soa_column_offset` that the per-element `IndexColumn`/`StoreColumn`/`SoaAlloc`
  paths use, so a column after a narrower one (e.g. `i64` after `bool`) read mid-padding (a silent
  wrong answer; the pipeline-source path went through `IndexColumn` and was correct, which masked it).
  **In-place element-field write `s[i].field = v`** also ships (with the AoS counterpart `arr[i].field
  = v` for a `mut array<Struct>`): the write counterpart of the `c[i].field` read, a single surface
  (`hir::Stmt::AssignElemField`, new `Place::ElemField`) that lowers by layout — `Stmt::StoreColumn`
  for a soa, slot `Stmt::StoreElemField` for a fixed struct array (both already existed for `.to_soa()`
  construction; now reachable from user assignment), bounds-checked, `mut`-gated. The stored value is a
  scalar field, so no move/region concern. **Whole-element write `s[i] = value` / `arr[i] = value`**
  also ships (`hir::Stmt::AssignElem`, new `Place::Elem`): the write counterpart of the `s[i]` gather /
  `arr[i]` whole-element read — a soa scatters the value's fields into their columns (`StoreColumn` per
  field), a fixed struct array stores the whole aggregate into the element (`StoreIndex`, a `[0,index]`
  GEP). First cut is plain-old-data structs (flat numeric/bool/char fields), so the value is a Copy
  aggregate with no region/move — `str`/nested/owned-bearing structs are gated off and deferred.
  Deferred: `str`/owned columns, the multi-column `soa_slice<T>` sub-view (`s[a..b]` over *every*
  column — needs a `{ptr,total_len,start,count}` view repr since column stride depends on the
  *original* row count; see `open-questions.md`), the dynamic `array<Struct>` (`DynStructArray`)
  element-field write (needs a pointer-based `StoreElemFieldPtr`, the write dual of `IndexFieldPtr`),
  whole-element write of `str`/nested/owned structs (region/move handling), bitset/bool packed columns.

Completion condition: confirm that the vectorized code contains vector instructions at the LLVM IR level.

## M7 — Parallelism — IN PROGRESS (par_map/chunks/purity done; first-class closures → task_group remain)

- [done] **purity / effect inference** (`align_sema` Pass 4, `check_parallelism`): a function is
  Impure iff it transitively performs an observable side effect (`print` / `io.stdout.write` /
  `fs.read_file`, or calls an Impure function — fixpoint over the call graph); everything else is
  Pure (`open-questions.md` "Purity model").
- [done] **`par_map(f)` (sequential first cut)** — apply a **Pure** `f` to each (post-stage)
  element and materialize an owned `array<R>`; an Impure `f` is rejected. Composes with prior
  stages (`where`/`map`/…) and struct-consuming functions. Lowers to the collect loop (`map(f)` +
  `to_array`); **real thread-parallel execution is the remaining piece** — the Pure rule is exactly
  what makes that safe. (`examples/par_map.align`.)
- [done] **`chunks(n)` → `array<slice<T>>`** — split an array/slice of a primitive scalar into
  length-`n` sub-slices (the last may be shorter), the unit of chunk parallelism. New owned type
  `Ty::DynSliceArray(prim)` (`{ chunk_buf, count }`, Move + region-tracked — the chunk slices borrow
  the source); built by the runtime `align_rt_chunks`. Indexing yields a `slice<T>` (reuses
  `SliceIndex` with a slice element, 16-byte stride); `.len()` is the chunk count. Confined to a
  local binding (no annotation/payload syntax for `array<slice<T>>`, so it cannot escape its
  source's scope). (`examples/chunks.align`.)
- [done] **`chunks(n).par_map(f)`** — the chunk-parallel combo (`draft.md` §11 headline). A
  `chunks` result (`array<slice<T>>`) is now a valid pipeline source whose element is `slice<T>`,
  so `par_map(f)` with `f: (slice<T>) -> R` reduces each chunk; the per-chunk results materialize
  into `array<R>` (which a further reduction can fold). The Pure requirement still applies.
  Lowers via the existing collect loop (sequential). (`examples/chunk_parallel.align`.)
- [done] **thread-parallel execution of `par_map`** — the perf widening of the sequential
  skeleton. A direct (no prior stages) `{ptr,len}` / scalar-array / `chunks` source lowers to
  `Rvalue::ParMapParallel`: codegen emits a per-function `void(in, out)` thunk (load element → call
  `f` → store result) and the runtime `align_rt_par_map` splits `[0, count)` into disjoint output
  ranges across `available_parallelism()` threads (`std::thread::scope`). Race-free **by
  construction** — `f` is Pure (no shared mutable state) and the output ranges never overlap. A
  *staged* `par_map` (`where(p).par_map(f)`) still uses the sequential collect loop (a flat split
  can't see through a filter). Results are identical to the sequential lowering.
- [todo] **first-class closures (escape-driven)** — the foundation for `task_group` and for
  function values stored/returned. A lambda that *escapes* gets a heap closure environment; a
  non-escaping pipeline lambda stays inlined (captures-as-params). The existing escape analysis
  picks the representation, so the offload-ready pipeline path is untouched. (Design SETTLED, see
  `open-questions.md` "First-class closures + task_group".)
- [todo] **`task_group` / `spawn` / `wait` (I/O concurrency)** — a structured scope (like
  `arena {}`): `spawn(fn { … })` takes a lambda (deferred work, visible), returns `Task<R>`;
  `wait()?` joins all + propagates the first `Err`; `a.get()` reads a result after the join. Tasks
  may be impure (I/O); safety from by-value capture. Built on first-class closures (above). The
  walking skeleton runs the deferred tasks at `wait` (sequential first, real threads as the
  widening — errors surface at `wait` either way, so the semantics match).
- async/await is not included (`non-goals.md`).

## M8 — Tooling and Quality — STARTED (first lint landed)

- the official formatter (mandatory, `draft.md` §16). — **DONE** (`alignc fmt`, the `align_fmt`
  crate; normalizes only meaningless variation, idempotent + meaning-preserving over every example).
- the standard lints (allocation in loop / huge struct copy / unnecessary clone / unnecessary heap /
  unhandled Result / branch in hot loop / string re-scan / implicit copy).
  - **unhandled `Result` — DONE.** Discarding a `Result` as a statement is a compile **error** (not
    a warning — it fits "errors are visible / handled"): propagate with `?`, branch with `match` /
    `else`, or bind it (`r := …`). Checked inline in `check_block` (a `Stmt::Expr` of `Result` type).
    (`tests/lint_unhandled_result.rs`, `examples/unhandled_result.align`.)
  - **huge struct copy — DONE.** A struct passed or returned **by value** above a threshold (two
    cache lines, `HUGE_STRUCT_BYTES = 128`) is a **warning** (a perf hint, not a hard error — the
    program still compiles/runs): "narrow the struct (split hot/cold fields, `draft.md` §9) or pass a
    `slice`/view." Chosen as the first *perf* lint because it is the only one in the set that is
    **deterministic and profile-independent** — a fixed-size copy at every call boundary, not a
    frequency-dependent cost — so it needs no `--profile` data and never false-positives (unlike the
    allocation/clone/hot-loop lints, which depend on input size and are deferred under the perf-lint
    principle that parked the `par_map` cost lint). Emitted in `check_fn` for a source signature only
    (`mono_args` empty — a monomorph would duplicate it; a generic template's params are the opaque
    `Ty::Param`, never a struct). Struct byte size is a faithful natural-alignment layout computed in
    sema (`struct_size_align`, matching LLVM's default non-packed layout). (`tests/lint_huge_struct_copy.rs`,
    `examples/huge_struct_copy.align`.) The remaining lints are not started.
- `unsafe` blocks and `raw.*`. — **first slice DONE.** `unsafe {}` is a block expression (a plain
  marker block — no region, no runtime effect, strictly simpler than `arena`); the only new mechanism
  is an `unsafe_depth` counter that gates the `raw.*` ops. Shipped: `unsafe {}` + `raw.alloc(size)`
  (→ a `raw` opaque byte pointer, `Ty::Raw`, Copy/`Static`, never auto-dropped) + `raw.free(p)`
  (draft.md §6.5's exact example) + **`raw.store(p, off, v)` / `raw.load(p, off)`** (typed
  load/store at a byte offset — no turbofish, the stored type follows the value and the loaded type
  the expected annotation; primitive scalars only, element-aligned). A `raw.*` op outside `unsafe` is a compile error; a function
  containing `unsafe` is inferred **impure** (reusing the binary purity flag, so it can never be a
  `par_map` callee — the danger stays traceable). `raw` is a nameable type (parameter / `let`).
  Region: `unsafe {}` opens no region (an arena value returned through it is still escape-checked —
  `region_of(Unsafe)` = the block's tail region, not the `Static` fallback). `raw` calls the existing
  flat `align_rt_alloc`/`align_rt_free`. (`tests/unsafe_raw.rs`, `examples/unsafe_raw.align`.)
  Pointer arithmetic is also done — **`raw.offset(p, n)`** advances a `raw` by `n` bytes (a plain,
  non-`inbounds` i8 GEP, so out-of-bounds arithmetic stays well-defined).
  **FFI first slice — DONE.** `extern "C" fn name(params) -> ret` (and the braced group
  `extern "C" { fn … }`) declares a bodyless foreign function bound to the C symbol; a call is only
  valid inside `unsafe {}` (foreign code is outside the safe core — reuses the `unsafe_depth` gate
  and the `unsafe`→impure inference, exactly like `raw.*`). FFI-safe signature types are primitive
  scalars (int/float) and `raw`, plus a `()` (void) return; `bool`/`char`/aggregates are deferred.
  Threaded as a bodyless `hir::ExternFn`/`mir::ExternFn` list (never lowered as a body); codegen
  declares each into the module under its C symbol (mirroring the `align_rt_*` external-decl
  pattern), so a `Rvalue::Call` keyed by that name resolves to a direct native `call`. libc/libm
  symbols resolve with no extra `-l` flag. (`tests/ffi.rs`, `examples/ffi.align`.)
  **`layout(C)` struct ABI — slice 1 DONE.** A `layout(C)` attribute (`layout(C) Point { … }`,
  composes with `align(N)` in any order) pins a struct to a stable, C-compatible flat layout
  (declaration order, natural alignment, no reordering — Align's default, so the marker *locks* it
  and opts the struct into FFI). Only a `layout(C)` struct may be moved through a `raw` pointer:
  `raw.store`/`raw.load` are widened to accept a `layout(C)` struct value (the existing
  `Scalar::Struct` flows through `RawLoad`/`RawStore` unchanged; codegen does an unaligned aggregate
  load/store — no new IR variant). Fields must be int/float (their C mapping is settled). This is the
  pointer-based FFI pattern (hand C a buffer, read/write structs in it). `ast::StructDecl.c_repr`,
  `hir::StructDef.c_repr`. (`tests/layout_c.rs`, `examples/layout_c.align`.)
  **FFI views — DONE.** A `str`/`slice`/`bytes` view is FFI-safe as an extern **parameter**: it
  lowers to its data pointer (C `char*`/`void*`), the length passed separately by the caller
  (`s.len()`) — the C `(ptr, len)` idiom, no hidden arg. Codegen declares such a param as `ptr`
  (`ffi_param_type`) and coerces the `{ptr,len}` argument to element 0 at the call site (keyed by an
  `extern_params` map). A view is *not* a valid return type (a bare pointer has no length) and is not
  NUL-terminated (length-based C fns only). `is_ffi_safe_param`. (`tests/ffi_views.rs`,
  `examples/ffi_views.align`.)
  **Remaining (widen):** the draft's `raw.ptr_cast<T>` (unchecked cast / reinterpret) is still
  deferred — with only `raw` (opaque bytes) a typed cast has nothing to reinterpret *to*; it earns
  meaning once FFI grows typed/external pointers. Later FFI slices: **by-value `layout(C)` struct
  passing** (SysV/AAPCS register + `byval`/`sret` ABI classification), an explicit external-library
  link directive (`-l<lib>`), and `bool`/`char` params — the widening the `std`/`pkg` C-engine
  wrappers need.

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
