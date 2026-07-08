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
enforced Copy-only); and **M7** — `par_map` (real threads) + `chunks` + purity inference +
first-class closures (①–③) + `task_group`/`spawn`/`wait()?` (real threads). Only **fully-escaping
fn values** (return / struct-field / array-element) stay deferred.

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
3. **first-class closures (escape-driven) → `task_group`** — M7 concurrency. **DONE** (slices ①–③
   + ④a–④c, PRs #104–117; only fully-escaping fn values — return / struct-field / array-element —
   stay deferred, see the M7 section above). Design SETTLED (`open-questions.md`); closures are the
   foundation, `task_group` the consumer. Built in slices:
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
- [done] **`out` no-alias check** — at a call site an `out` argument must not share its root buffer
  with any other argument (`fill(a, a)`, `fill(a, s)` with `s := a`, and sub-slice forms
  `fill(xs, xs[0..2])` all rejected; `fill(xs, ys)` fine), a conservative root-buffer comparison via
  `FnSig.out` + `arg_root_local` (sees through slice provenance and `SliceRange`). The language-level
  no-alias guarantee.
- [done] **`map_into(out dst)` + scoped `!noalias` emission** — the materializing terminal that
  writes a length-preserving pipeline into a caller `out`/`mut` slice (`src.map(f).map_into(dst)`,
  the `to_array` sibling; `dst.len() == src.len()` or abort; yields `()`). This is the reachable
  two-slice-param loop that makes `noalias` worth emitting, so the metadata landed with it: the fused
  loop's source `SliceIndex` load and `dst` store carry the loop's disjoint `in`/`out` alias scopes
  (`MIR SliceIndexNoalias`/`PtrStoreNoalias` → codegen `!alias.scope`/`!noalias`, a fresh domain +
  scope pair per loop, `alias_scope_lists`). The alias-soundness gate (both roots a *known* backing
  buffer — a parameter or a real array local, via `hir::Local::is_param` + `slice_root_is_known`;
  unknown-origin views and same-root aliasing rejected) is the precondition. Verified: at `-O2
  -force-vector-width=4` the loop's overlap guard drops 3 → 0 `diff.check`/`or.cond` vs. the
  metadata-stripped IR. (`crates/align_driver/tests/map_into.rs`; `open-questions.md` "`out`
  parameters + `noalias`" DONE.)
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

## M6 — SIMD / vec / mask — DONE (2026-07-03)

Both completion conditions (below) are met and independently re-verified: `emit-llvm` on a `vecN<T>`
program shows real `<N x T>` IR (`add <4 x i32>`, `extractelement`, …), and `where(p).<reducer>()` is
branch-free for every reducer (`sum`/`count`/`min`/`max`/`any`/`all`/`reduce`), matching the plain
(no-`where`) path. Shipped: `vecN<T>`/`maskN<T>` (construct, elementwise arithmetic, compare/`select`,
`sum`/`sum_where`/`dot`/`min`/`max`/`fma`, broadcast, lane read/write, array `load`/`store`), the SoA
surface (borrowed `soa<T>` view, field projection, gather/scatter, `group_by`, `json.decode → soa`),
and `align(N)` (struct form + scalar-array-binding form + the aligned-load fast path). **Not a
blocker, tracked as post-M6 backlog** (own section below): owned SoA columns, `soa_slice<T>`,
packed-bool columns, and the arena/heap `align(N)` over-alignment gaps.

- `vec2/4/8/16<T>`, `mask<T>`, `bitset`.
  - **`vecN<T>` slice 1 — DONE.** The explicit fixed-width vector type `vec2`/`vec4`/`vec8`/`vec16`
    of a numeric scalar (`Ty::Vec(Scalar, N)`, a Copy/`Static` register value lowering to the LLVM
    `<N x T>`). **Construction** reuses the array literal under a `vecN<T>` annotation (the annotation
    picks the SIMD representation — no new syntax, "Nothing hidden"; a dedicated `hir::VecLit` that
    lowers to a value `Rvalue::MakeVec` insertelement chain, unlike the slot-based `ArrayLit`).
    **Elementwise `+`/`-`/`*`/`/`/`%`** route through `gen_bin` to inkwell's vector `build_int_*`/
    `build_float_*` (one lane-wise instruction). Integer `/`/`%` carry the same lane-wise divisor
    guard as scalars (MIR `lower_vec_div`, the SIMD mirror of `lower_int_div`): `any(divisor == 0)`
    (a `mask` reduced by `Rvalue::MaskAny`) aborts via `div_fail`, and a signed `INT_MIN / -1` lane is
    remapped-and-`select`ed to the wrapped result; float `%` is IEEE `frem`, unguarded. **Lane read
    `v[i]`** (a constant lane,
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
  `examples/align_attr.align`, `draft.md` §9). **Over-aligned-struct size padding DONE** — the struct's
  LLVM type is size-padded up to its alignment (an `[K x i8]` tail at the one `set_struct_body` seam),
  so a fixed `[align(N) S]` array has a tight, over-aligned element stride (`round_up(size, align)`,
  what C does); a fixed array literal of an `align(N)` struct now compiles, and the sema/codegen layout
  parity test covers over-aligned cases. **`align(N) data := […]` binding form DONE** — the prefix
  parses to `ast::Stmt::Let.align` → `hir::Local.align` → `mir::Function.slot_align`, and codegen
  over-aligns the alloca of a *numeric* scalar array (int/float — a `str`/`bool`/`char`-element array
  or a struct array is rejected) via `max(declared, natural)`, the same conservative rule. The
  **aligned-vector-load fast path** rides on it: `data[..].load(i)` on a whole borrow of an `align(N)`
  binding is emitted as an *aligned* `<n x T>` load whenever `(start+i)*sizeof(elem)` is a compile-time
  `N`-multiple (`proven_vec_load_align`, MIR); every other case (non-const index, a cross-function
  `slice<T>` param, a non-`N` offset) stays a plain element-aligned load — the alignment is never
  over-stated (UB). (`tests/aligned_binding.rs`, `examples/aligned_load.align`, `draft.md` §9.)
  Deferred: arena/heap over-alignment — and, tied to it, a *dynamic*
  `array<align(N)Struct>` (stride is correct, but the heap buffer can't be over-aligned yet); an
  `align(N)` struct as a *struct field* (needs a struct-type ABI alignment LLVM can't express); and
  the *cross-function* aligned-load path (a fat `slice<T>` that carries an alignment through a call).
  **SoA surface — largely DONE:** the `soa<T>` borrowed view,
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
  scalar field, so no move/region concern. The **dynamic `array<Struct>` (`DynStructArray`)
  element-field write** `arr[i].field = v` on an owned `{ptr,len}` view now ships too — the same
  surface (`hir::Stmt::AssignElemField`) lowers by slot layout to the pointer-based
  `Stmt::StoreElemFieldPtr` (the write dual of `Rvalue::IndexFieldPtr`: extract the buffer pointer,
  GEP `%Struct, ptr, index, pfield(field)`), bounds-checked against the view's runtime length and
  `mut`-gated. Scalar (POD) fields only — a `str`/owned field write is gated off in sema (the
  pointer-based store has no per-element drop of the overwritten field), deferred below. **Whole-element
  write `s[i] = value` / `arr[i] = value`** also ships (`hir::Stmt::AssignElem`, new `Place::Elem`): the
  write counterpart of the `s[i]` gather / `arr[i]` whole-element read — a soa scatters the value's
  fields into their columns (`StoreColumn` per field), a fixed struct array stores the whole aggregate
  into the element (`StoreIndex`, a `[0,index]` GEP).
  **`str` column WRITES — DONE.** A `str` view is a Copy 16-byte `{ptr,len}`, so a str-bearing soa is
  still a view-Copy aggregate (no owned buffer, no per-field drop): both the single-field write
  `s[i].name = v` and the whole-element scatter `s[i] = value` ride the *existing* store machinery
  unchanged — the per-field `StoreColumn` scatter is already str-capable (it built the `to_soa` /
  `json.decode → soa` str columns), and the store's escape is caught by the `AssignElemField` /
  `AssignElem` region rule (`region_of(value).outlives(region_of(base_soa))`). A stored view that does
  not outlive the soa (an inner-arena view scattered into an outer-arena soa, directly or via a
  struct literal whose `StructLit` region folds to the shorter field) is a compile error — the dual of
  the `s[i]`/`s[i].name` read escape check. Sema change was a one-predicate gate relax (`str_view`:
  every field a Copy scalar incl. `str`, soa-only); MIR/codegen needed nothing. Tests:
  `str_column_single_field_write`, `str_column_field_write_cannot_store_shorter_lived`,
  `str_column_whole_elem_write_scatters`, `str_column_whole_elem_write_cannot_store_shorter_lived`,
  `str_column_whole_elem_write_via_literal_cannot_store_shorter_lived` (`tests/soa.rs`).
  Deferred — **owned columns** (`string`/`array<T>` fields): the real remaining SoA weight. A `string`
  column is *owned* per element, so (a) `s[i] = value` / `s[i].name = v` must **drop the overwritten
  element's owned field** before the store (`StoreColumn` has no drop today) and **move** the RHS in
  (null its source, like the fixed-array Move path), (b) dropping the whole soa must free every owned
  element of every owned column (no per-column drop exists), and (c) `region_of` must treat an owned
  column as self-contained (arena/frame), not a borrow of the input — a gather `s[i]` of an owned
  column stops being a free Copy (it would deep-copy or move). None of this is a *new analysis
  mechanism* — it is drop-insertion + move-null wiring per owned column — but it is a real slice, so it
  is deferred until pursued (see `open-questions.md`). Also still deferred: the multi-column
  `soa_slice<T>` sub-view (`s[a..b]` over *every* column — needs a `{ptr,total_len,start,count}` view
  repr since column stride depends on the *original* row count), the dynamic `array<Struct>`
  element-field write of a `str`/owned field (needs a pointer-based per-element drop of the overwritten
  field), the fixed `array<Struct>` whole-element write of a `str`/nested struct (region-checked
  aggregate store — the soa path ships; the AoS path stays with the rest of the AoS str element work),
  and bitset/bool packed columns.

Completion condition: confirm that the vectorized code contains vector instructions at the LLVM IR
level; and extend the branchless `where` form beyond `sum`/`count` to every reducer —
identity-select where a fixed identity exists (`min` / `max` / `any` / `all`, identities
`+∞` / `−∞` / `false` / `true`), and the accumulator-select form
`acc = mask ? f(acc, v) : acc` for generic `reduce`, whose user-supplied `f` has no known identity
(`init` is the starting accumulator, not an identity of `f`) — so all reductions are
predication-ready, the forward-compatible shape for scalable-ISA tails (`05 §5`).

- **Branchless `where` for all reducing terminals — DONE.** `where(p).{min,max,any,all,reduce}` now
  lowers branch-free like `sum`/`count`: `lower_array_reduce` AND-folds the predicates into one mask
  and each reducer `select`s each masked-out lane to its identity (`min` → `+∞` / `max` → `−∞` — the
  same `extreme_of` fold seed; `any` → `false` / `all` → `true`), while generic `reduce` uses the
  accumulator-select form `acc = mask ? f(acc,v) : acc`. `min`/`max` additionally moved from a
  compare-and-branch update to the `select(cur `cmp` acc, cur, acc)` idiom, so **both** the `where`
  and the plain (no-`where`) paths are now branch-free and vectorize (one lowering, no dual
  mechanism). Semantics are byte-identical to the branch form they replaced: same ordered
  comparison (so NaN elements are still skipped by `min`/`max`), same empty-selection result (`min`/
  `max` → the extreme seed, `reduce` → `init`, `any` → `false`, `all` → `true`). Verified: `emit-mir`
  shows no per-element predicate branch for any reducer; `objdump` shows `xs.where(p).min()` over a
  `slice<i32>` emitting `pminsd`/`pcmpgtd`/`movdqu` on the `x86-64-v2` baseline where the branch form
  emitted purely scalar code with 10 branches. `dot` is out of scope — `a.dot(b)` is a two-array
  kernel with no `where`, already branch-free. (`tests/branchless_where.rs`, `tests/optimizer.rs`.)
  Materializing terminals (`to_array`/`scan`) keep a real skip-branch (they must not *append* a
  masked-out element — not an identity op). The completion condition is met.

### Post-M6 backlog (not M6 blockers — M6 is closed; these are future perf/feature slices)

None of the below gated M6's completion conditions (vector IR + branchless `where`, both met above).
They are deferred SoA/alignment slices, tracked for whenever picked up next; full detail + rationale
already lives in `docs/open-questions.md`.

- **Owned SoA columns** (`string`/`array<T>` fields in a `soa<T>`): needs per-column drop on
  overwrite, whole-soa recursive drop, and `region_of` treating an owned column as self-contained
  rather than a borrow. See `docs/open-questions.md` → "Memory layout — `soa<T>`" (the "owned columns"
  sub-item, near "the largest remaining soa gap").
- **`soa_slice<T>`** (multi-column `s[a..b]` sub-view over every column): needs a
  `{ptr, total_len, start, count}` view repr since column stride depends on the original row count.
  See `docs/open-questions.md` → "Multi-column `soa_slice<T>`".
- **Packed-bool columns** (`bitset`-backed `bool` fields in a `soa<T>`, instead of one byte/lane).
- **Dynamic over-aligned arrays**: a heap-allocated `array<align(N)Struct>` (element stride is already
  correct; only the heap buffer's own alignment is missing) and arena/heap-buffer over-alignment in
  general; an `align(N)` struct as a struct field; the cross-function aligned-load path (a fat
  `slice<T>` carrying its alignment through a call). See `docs/open-questions.md` →
  "Struct/array alignment attribute `align(N)`" ("Still deferred").

## M7 — Parallelism — DONE (par_map/chunks/purity, first-class closures ①–③, task_group ④a–④c; only fully-escaping fn values deferred)

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
- [done] **first-class closures (escape-driven)** — slices ①–③ (PRs #104–108): non-capturing
  function values + indirect call (①), a lambda as a first-class value with typed parameters (②a),
  the fat-pointer closure ABI (②b-1), capturing closures with a **frame-local** environment (②b-2),
  and higher-order functions (fn-typed parameters, ③). A non-escaping pipeline lambda stays inlined
  (captures-as-params); a lambda captured by `spawn` snapshots into the `task_group` region. Escape
  analysis picks the representation, so the offload-ready pipeline path is untouched.
- [done] **`task_group` / `spawn` / `wait` (I/O concurrency)** — slices ④a–④c (PRs #110–117; #117
  "closures arc COMPLETE"). A structured scope like `arena {}`: `spawn(fn { … })` takes a lambda
  (captures snapshotted into a fresh per-spawn **region env**), returns `Task<R>` (a region-tied
  result slot); `wait()?` joins all on **real threads** (fork-join via `align_rt_tg_*` +
  `std::thread::scope`) and propagates a failing task's `Err`; `t.get()` reads a result after the
  join (a `get`-before-`wait` use is a compile-time flow error). Tasks may be impure (I/O); safety
  from by-value capture. Runtime: `align_rt_tg_begin/alloc/register/wait/end`.
- [deferred] **fully-escaping function values** — returning a fn value from a function, or storing
  one in a struct field / array element, is **not** supported: it needs a **heap-owned** closure
  environment with its own drop (the "escapes every region" model), whose design is not yet settled
  and which has no consumer today (`task_group` uses the region env). Deliberately deferred — see
  `open-questions.md` "First-class closures + task_group" (the escape-every-region note).
- async/await is not included (`non-goals.md`).

## M8 — Tooling and Quality — DONE (2026-07-03)

All four completion conditions are met: the **formatter** (#233) normalizes only meaningless
variation and round-trips idempotently + meaning-preservingly; **`unsafe`/`raw.*`** (#262–264)
gates every raw memory op behind an explicit block, inferred impure; **FFI v1** (`extern "C"`,
#265–269) plus **by-value struct passing shipped beyond the v1 boundary** (x86-64 SysV, #329)
covers extern decls, scalar/`raw`/`()`/`layout(C)`-pointer/view/by-value-register signatures, and
`link("name")`; and the lint suite ships its full **profile-independent** slice — five lints that
fire on structure alone, never on runtime frequency, so none needs `--profile` data and none can
false-positive: unhandled-`Result` (#138), huge-struct-copy (#234), lossy-cast +
wasteful-default-element (#313), unnecessary-heap narrow form (#323). **Not blockers, tracked as
post-M8 backlog** (own section below): the frequency-dependent lints (allocation-in-loop, the
broader unnecessary-clone/unnecessary-heap forms, branch-in-hot-loop, string re-scan, implicit
copy), `prefer-pipeline-over-vecN` (no firing surface — Align has no loop construct to convert),
and the hot/cold field-split suggestion (needs heuristic design).

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
    `examples/huge_struct_copy.align`.) Lint batches 1–2 add the four below; the rest of the enumerated
    set (allocation-in-loop, the broader unnecessary-clone/unnecessary-heap forms, branch-in-hot-loop,
    string re-scan, implicit copy) is not started.
  - **lossy conversion — DONE (lint batch 1).** A narrowing int→int, saturating float→int,
    wide-int→float (past the target float's mantissa: f32 = 24, f64 = 53), narrowing float→float, or
    `char`-narrowing `as` is a **warning** ("… — this is defined behavior, not an error"): the
    conversion is zero-UB and never blocked (Settled, "Numeric conversion — as"), but the silent loss
    is surfaced. Lossless conversions (widening, same-width, a same-width sign change like `u8 as i8`
    that keeps every bit) and an unconstrained-literal source (`1 as i8`, still an inference variable —
    an explicit annotation, not a value being narrowed) stay silent. `cast_loss` in `check_cast`.
    (`tests/lint_lossy_cast.rs`.)
  - **wasteful default element type — DONE (lint batch 1).** A literal array of at least
    `DEFAULT_ELEM_LITERAL_ARRAY_LEN` (= 64) elements whose element type is left to the i64/f64 default
    (Settled, "Numeric literal typing") is a **warning** suggesting a narrower annotation — the default
    is correct but spends 8 bytes/element (an 8× cost vs an `i8` element at a cache-line scale). Silent
    below the threshold, when a context/annotation constrains the element type (a typed pipeline stage),
    or when the element type comes from a concrete value rather than a defaulted literal. Emitted in
    `check_array_lit`. (`tests/lint_default_elem_array.rs`.)
  - **unnecessary heap — DONE (lint batch 2, narrow slice).** `heap.new(x).get()` — a box allocated in
    the arena only to immediately read its scalar straight back — is a **warning** ("use the value
    directly; a stack value suffices"): the allocation serves no purpose (a `box<T>` payload is a
    scalar in M3, so `.get()` is a plain copy-out). Detected purely locally in `finalize_expr` — a
    `BoxGet` whose receiver is the allocating `HeapNew` itself — so it reuses no escape-analysis state,
    is profile-independent (structural like huge-struct-copy, not in the deferred frequency-dependent
    allocation-lint bucket), and never false-positives. The broader "box bound to a local, only ever
    `.get()`-ed, never escaping" form is **deferred** — it needs a whole-function box-use scan (the
    escape pass keeps no reusable per-box escape fact), recorded in `open-questions.md` under the M8
    lint candidates. (`tests/lint_unnecessary_heap.rs`.)
  - **prefer-pipeline-over-vecN — DEFERRED (lint batch 2).** The lint targets a *hand-written `vecN<T>`
    loop* (a counted `for` doing vec-load → arith → vec-store) that should be a width-agnostic pipeline.
    But Align has **no loop construct** — iteration is only ever `map`/`reduce`/… pipelines, and a bare
    `vecN<T>` expression (`a + b` over `vec4<i32>`) is the *correct* hand-tuned-kernel use, not a
    convert-to-pipeline candidate. So there is no mechanical "hand-written vecN loop" form to detect in
    the current language; the firing condition is recorded (with a proposed shape for when a
    kernel/loop surface exists) in `open-questions.md` under the M8 lint candidates.
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
  **External library linking — DONE.** An `extern "C" link("name")` clause names a library to link;
  sema validates + dedupes the names into `hir::Program.link_libs` → `mir::Program.link_libs`, and
  the driver's `link_executable` appends `-l<name>` after the objects/runtime (libc/libm stay
  auto-linked). `ast::ExternBlock.link`, parser `parse_link_clause`. (`tests/ffi_link.rs`,
  `examples/ffi_link.align`.)
  **FFI v1 is COMPLETE** (extern decls + unsafe-gating, scalar+`raw`+`()` signatures, `layout(C)`
  struct-by-pointer, `str`/`slice`/`bytes` views, `link("name")`). **By-value struct passing shipped
  beyond v1** (#329, 2026-07-03): a `layout(C)` struct ≤ 16 bytes passes/returns in registers on
  **x86-64 SysV only**, emitting clang's exact coercion and verified against a compiled-C-helper
  round-trip harness; codegen refuses (rather than guesses) on any non-SysV target, a >16-byte
  MEMORY-class struct, or a signature that would fall to the stack under register pressure (see
  `docs/open-questions.md` → "FFI" for the full classification). Still deliberately out of v1 (draft
  §15 "Not in FFI v1"): **AAPCS64 / other-arch by-value classification** and the MEMORY-class
  `byval`/`sret` path (both wait on a concrete cross-arch/large-struct consumer — struct-by-pointer
  already covers that shape). `bool`/`char` as FFI types — use the integer types (a C `_Bool` = `u8`,
  `char` = `i8`/`u8`, `char32_t` = `u32`; Align `char` is a Unicode scalar, not a C `char`), keeping
  one unambiguous way and dodging the `i1`-`zeroext` subtlety. `raw.ptr_cast<T>` — a typed reinterpret
  is meaningless with one opaque pointer type; it waits on typed/external pointers.

### Post-M8 backlog (not M8 blockers — M8 is closed; these are future lint/perf slices)

None of the below gated M8's completion conditions (formatter, `unsafe`/`raw.*`, FFI v1 + by-value
SysV, and the five profile-independent lints, all met above). Full rationale + firing-condition
proposals already live in `docs/open-questions.md` → "M8 lint candidates".

- **Frequency-dependent lints** — need runtime/size evidence to avoid false positives, so parked
  pending a `--profile` mechanism or a documented heuristic: allocation-in-loop, the broader
  unnecessary-clone form, the broader unnecessary-heap form (`p := heap.new(x); … p.get()`, needs a
  whole-function box-use scan — the escape pass keeps no reusable per-box fact to piggyback on),
  branch-in-hot-loop, string re-scan, implicit copy.
- **`prefer-pipeline-over-vecN`** — no firing surface exists: Align has no loop construct, so the
  lint's target (a hand-written `vecN<T>` loop) cannot be written today; a bare `vecN<T>` expression
  is the correct hand-tuned-kernel use, not a lint candidate. Deferred until a loop/kernel surface
  exists.
- **Hot/cold field-split suggestion** — needs heuristic design (when the mix of scanned vs
  rarely-read fields actually matters) before it can ship without noise; suggestion-only, never an
  automatic layout change (explicit layout is Settled).
- **`par_map` cost-threshold lint** (cheap-`par_map`-loses-to-sequential) — recorded with the perf-rail
  lint work, not yet built.

## M9 — std (I/O, filesystem, path, env, time) — DONE (2026-07-04)

All four slices are done and their completion conditions independently met (design settled #336;
shipped #337–#340). **`std.io`** ships `reader`/`writer` as concrete Move types (own an fd,
`Drop`-closed; `io.std*` streams borrow theirs) plus the minimal owned `buffer`, with `io.copy` as a
non-consuming portable fixed-buffer transfer (`Result<i64, Error>`). **`std.fs`** is complete —
`open`/`create`/`write_file`/`exists`/`remove`/`read_dir` plus the `mmap`-backed `read_file_view`
(arena-scoped, `munmap`ped at every arena exit, with an owned-copy fallback for special/
untrustworthy-size files). **`std.path`/`std.env`/`std.time`** round-trip (`join`/`normalize`/
`base`/`dir`/`ext`, `get`/`set`, `now`/`instant`/`sleep`). A single errno→`Error` table (`draft.md`
§18.2) backs every fallible call across the fallible modules (std.io, std.fs, and std.env). **v1 restrictions, not blockers:** each
owned handle passed to `io.copy` must be a bound local (the `io.std*` streams are exempt — the
bound-receiver restriction, which also covers a bound `.buffered()` writer); and an unbound Move
*temporary* isn't dropped yet, so a one-shot `io.stdout.write(x)?` leaks its small handle (any bound
handle drops correctly) — a general MIR gap, not std.io-specific. **Not blockers, tracked as post-M9
backlog** (own subsection below): `io.copy` syscall fast paths (`sendfile`/`splice`/`io_uring`), no
`SIGBUS` handler on post-`mmap` truncation, non-UTF-8 filenames from `read_dir` (a caveat for
downstream `string` ops that assume UTF-8), Move-temporary dropping, streaming×pipeline integration,
and the M10+ module set (`std.net`/`std.http`/`std.cli`/`std.process`/`std.encoding`/`std.compress`/
`std.rand`/`std.crypto`).

Scope: `std.io`, `std.fs`, `std.path`, `std.env`, `std.time` — the five modules an ordinary CLI
program needs. Full API shape (types + signatures) is in `draft.md` §18.2; the five underlying
decisions are recorded in `docs/open-questions.md` Settled → "M9 std design". As with every prior
`std`/`core` module, the implementation is Rust runtime (`align_rt_*`) + sema builtin dispatch +
required `import` — the `core.json` pattern, not yet Align-over-FFI library code.

- **Slice 1 — std.io core — DONE.** `reader`/`writer` as concrete Move types (`Ty::Reader`/
  `Ty::Writer`, own an fd, `Drop` closes it — a file fd; `io.stdin`/`io.stdout`/`io.stderr` borrow
  the std fd); `io.stdin` / `io.stdout` / `io.stderr` / `io.stdout.buffered()` / `fs.open` /
  `fs.create` are the constructors (one type, many constructors — the old `Ty::BufWriter` and the
  `io.stdout.write` special-case collapsed into `writer`); `r.read(b: mut buffer)` / `w.write(x:
  str|bytes|builder)` / `w.flush()`, all `Result<_, Error>`. `core.buffer`'s build (deferred in
  `open-questions.md` "`bytes`/`buffer`" until a consumer) landed here as the minimal owned growable
  `Ty::Buffer` (`buffer(cap)` / `.bytes()` → `slice<u8>` / `.len()`), the sink `reader.read` fills.
  The **errno→`Error` fixed table** (`draft.md` §18.2) is one runtime helper (`io_error_to_status`)
  + one MIR decode (`make_error_from_status`, branchless), shared by `fs.read_file`/`fs.open`/
  `fs.create`/read/write/flush. **Completion condition met:** an Align program byte-exact-copies a
  file through a tail-recursive `read`/`write` loop (`fs.open` → `reader.read(buf)` loop →
  `fs.create` → `writer.write(buf.bytes())`), plus per-method tests (EOF, `NotFound`/`Denied`
  mapping, `io.stdin`, moved-reader rejection, buffer-view escape rejection) — `tests/m9_io.rs`.
  Known minor gap (a general MIR limitation, not std.io-specific): an **unbound Move temporary**
  isn't dropped, so a one-shot `io.stdout.write(x)?` leaks its ~40-byte writer handle (bound writers
  / readers / buffers drop correctly); the fix is dropping Move temporaries, a separate improvement.
- **Slice 2 — `io.copy` — DONE.** `io.copy(r: reader, w: writer) -> Result<i64, Error>`, the
  portable fixed-buffer loop (`align_rt_io_copy`: a 64 KiB transfer buffer matching `BUF_WRITER_CAP`,
  read→write with `EINTR` retry + partial-write handling shared with `writer.write`, errno
  sign-encoded like `reader.read`) — the v1/reference implementation (`docs/open-questions.md`
  "Transparent zero-copy I/O"), returning bytes transferred. `io.copy` is a **non-consuming**
  builtin (`ExprKind::IoCopy` / `Rvalue::IoCopy`): it *borrows* both Move handles (fd ownership does
  not move — like `print`'s argument), so MoveCheck does not consume `r`/`w` and both stay usable
  after the call. v1 keeps the Slice-1 bound-receiver restriction (each owned handle must be a bound
  local; the borrowed `io.std*` streams are exempt — `io.copy(io.stdin, io.stdout)` is the `cat`
  form). **Completion condition met:** byte-exact transfer below/at/above the buffer boundary + empty
  file + `cat` + a non-consumption test + an `O(buffer)`-not-`O(file size)` RSS test on a 64 MiB file
  (peak `VmHWM` bounded via a stdout/stdin handshake), `tests/m9_io.rs` + `examples/io_copy.align` +
  runtime unit tests. Fast paths (`sendfile`/`splice`/mmap/`io_uring`) are explicitly **post-M9** —
  see that Future entry, unchanged in shape by this slice (the runtime marks the dispatch site).
- **Slice 3 — std.fs complete — DONE.** `write_file` (`str`/`bytes`/`builder`, the same three
  forms as `writer.write`), `exists` (a plain `bool` — every `stat` failure folds to `false`, never
  a `Result`), `remove`, `read_dir` (an **owned `array<string>`** — a new `PrimScalar::String` lets
  it be a `Result` payload, and its `Drop` is a **deep** free `align_rt_free_string_array`: each
  element buffer, then the header, distinct from a scalar `array<T>`; Move-element *indexing* stays
  deferred project-wide, so v1 uses the array whole — `.len()`, move/return), and `read_file_view`
  (an `mmap` view; requires an enclosing `arena {}` — sema-checked like `heap.new`; the mapping is
  registered on the runtime `Arena` and `munmap`ped at arena end, so **every** exit — block end /
  `return` / `?` — releases it via `ArenaEnd`, no separate `Drop`; the view's region is the arena,
  so escaping it is rejected and `.clone()` copies out). The errno→`Error` table (`draft.md` §18.2)
  is shared via `io_error_to_status` + `make_error_from_status`. Guardrails (`docs/open-questions.md`
  "Transparent zero-copy I/O" ~2383–2396): `fstat` gates `mmap` to a **regular, nonzero** file; a
  special / `/proc` / character-device / zero-length file takes an **owned arena copy** fallback
  (`read_file_view_into_arena` — correctness over a broken zero-copy on a file whose size can't be
  trusted; the cost class changes to a copy — recorded); no `SIGBUS` handler is installed (a
  process-global handler is a hidden global side effect Align forbids — the mapping size is fixed at
  `mmap` time and concurrent truncation is a documented v1 limitation). **Completion condition met:**
  the `draft.md` §19 program type-checks and runs end to end (`fs.read_file` → `json.decode` →
  fused pipeline → `builder` → `io.stdout`, and a `read_file_view` variant feeding the same
  pipeline); a byte-exact view read (incl. multi-page); the view escape rejected at compile time;
  write/exists/remove/read_dir round-trips + errno mapping — `tests/m9_fs.rs` (17) + `align_runtime`
  unit tests (8).
- **Slice 4 — std.path / std.env / std.time — DONE.** `path.join`/`normalize` (freshly-allocated
  owned `string`; `normalize` is pure POSIX lexical `.`/`..`/`//` resolution — **no** symlink /
  filesystem access) and `path.base`/`dir`/`ext` (**zero-copy `str` views** of the input — their
  region is **inherited from the input** via a `region_of` arm, so a view of an arena `str`
  (`fs.read_file_view`) is rejected from escaping the arena; the #297-class trap avoided). The
  view-safe POSIX edge choices: `dir` of a path with no separator is the **empty** view (not `.`,
  which isn't a substring); an all-`/` path's `base`/`dir` is `/`; `ext` of a dotfile (leading `.`)
  is empty. `env.get` -> `Option<string>` (owned — the environment is volatile, so a view would
  dangle after a later `env.set`; a present-but-empty value is `Some("")`, distinct from `None`);
  `env.set` -> `Result<(), Error>` (plain `setenv`; concurrent `env.set` from another `task_group`
  task is documented **undefined** per POSIX — no hidden serializing lock). `time.now` (`CLOCK_REALTIME`
  via `SystemTime`) / `time.instant` (`CLOCK_MONOTONIC` via a process-lazy `Instant` base, guaranteed
  non-decreasing) / `time.sleep` (`std::thread::sleep`, which retries `EINTR` with the remaining
  time; a non-positive `ns` is a no-op) — one `i64`-nanosecond timeline, no `Duration` type. All
  three are Impure; `path.*` are Pure (lexical byte ops). Implementation: sema builtin dispatch +
  MIR `Rvalue` + `align_rt_path_*`/`env_*`/`time_*` runtime, the `core.json`/std.fs precedent.
  **Completion condition met:** a round-trip per module — `path.join` then `dir`/`base`/`ext` recover
  the pieces; `env.set` then `env.get` round-trips (and an unset name is `None`); `time.instant()`
  around `time.sleep(ns)` shows elapsed `ns` monotonically increasing — plus `normalize`
  representative + edge cases, the base/dir/ext view region escape rejection, and the invalid-name /
  import-required negatives (`tests/m9_path_env_time.rs`, 10 + `align_runtime` unit tests, 7).

**M9 — all slices done.** Slices 1–4 (std.io / io.copy / std.fs / path·env·time) are all complete and
their completion conditions met; the five M9 modules (`std.io`/`std.fs`/`std.path`/`std.env`/`std.time`)
are implemented end to end.

### Post-M9 backlog (not M9 blockers — M9 is closed; these are future std/perf slices)

None of the below gated M9's completion conditions (all four slices done and verified above). Full
rationale for the I/O fast-path and mmap items already lives in `docs/open-questions.md` →
"Transparent zero-copy I/O".

- **`io.copy` syscall fast paths** (`sendfile`/`splice` on Linux, `io_uring`, Direct I/O into
  huge-page-backed arenas) — the portable fixed-buffer loop shipped in Slice 2 is the v1/reference
  implementation; fast paths are validated against it and drop in later behind the same `io.copy`
  signature, no API change.
- **`SIGBUS` on post-`mmap` truncation** — v1 installs no handler (a process-global signal handler is
  exactly the hidden global side effect Align forbids); concurrent truncation of a
  `read_file_view`-mapped file is a documented v1 caller contract, not a language-level guarantee.
- **Non-UTF-8 path/file names** — `fs.read_dir` stores each entry's raw OS bytes losslessly into an
  Align `string` (Unix filenames are not guaranteed valid UTF-8); this is a caveat for any later
  `string` operation that assumes valid UTF-8, not yet resolved.
- **Move-temporary drop** — an unbound Move temporary (e.g. a one-shot `io.stdout.write(x)?`) isn't
  dropped and leaks its handle; a general MIR improvement, not std.io-specific (Slice 1 known gap).
- **Streaming × pipeline integration** — a `reader`/`writer` as a `map`/`where`/`reduce` pipeline
  source/sink is not wired up.
- **M10+ modules** (unstarted, out of this milestone's scope): `std.net`, `std.http`, `std.cli`,
  `std.process`, `std.encoding`, `std.compress`, `std.rand`, `std.crypto`.

## M10 — std (encoding / rand / cli) — DONE (2026-07-04)

**All three slices shipped (#346 encoding, #347 rand, #356 cli) and the milestone is formally
closed.** Scope: `std.encoding`, `std.rand`, `std.cli` — three modules that close over mechanisms
the language already has, with **no new effects, no concurrency, and no FFI engine**
(`str`/`bytes`/`buffer`, `mut` slice, and `main(args: array<str>)`'s `array<str>` already exist).
The original scoping note said "zero new Move types"; the settled per-module designs superseded
that — Slice 1 added the `Scalar::Buffer` owned payload, Slice 3 the `CliCommand`/`CliParsed` Move
handles (each swept through every pass, the reader/writer discipline). Full API shape is in
`draft.md` §18.2; the scope decision + rationale is recorded in `docs/open-questions.md` Settled →
"M10 scope decision". Implementation stays the `core.json`/M9 pattern: Rust runtime (`align_rt_*`)
+ sema builtin dispatch + required `import`.

- **Slice 1 — std.encoding — DONE.** `base64_encode`/`base64_decode`, `base64url_encode`/
  `base64url_decode`, `hex_encode`/`hex_decode`, `utf8_valid` — pure functions over `bytes`/`str`,
  no state, no Move types. Encode takes a byte view (`str`/owned `string` (auto-borrowed)/`slice<u8>`,
  the `hash64` accepted forms) and returns an owned `string`; decode takes a `str` and returns
  `Result<buffer, Error>` (invalid input -> `Error.Invalid`, mapped through the same fixed table as
  the syscall paths — the runtime just returns `AL_INVALID`); `utf8_valid` takes `bytes` and reuses
  the shared SIMD/scalar UTF-8 validator (`#344`). A new **`Scalar::Buffer`** owned-Move payload
  (the `reader`/`writer` precedent) lets `Result<buffer, Error>` carry the decoded handle, dropped
  via `align_rt_buffer_free` on the Result's `Drop`. v1 is a **scalar** reference implementation
  (correctness before speed); SIMD (Lemire's Base64-at-memcpy-speed) is a later optimization behind
  the same signatures. Implementation: sema builtin dispatch (`import std.encoding`) + MIR `Rvalue`
  (`EncodingEncode`/`EncodingDecode`/`Utf8Valid`) + `align_rt_base64*_encode`/`hex_encode`/
  `*_decode`/`utf8_valid` runtime, the `core.json`/std.fs precedent. **Completion condition met:**
  encode/decode round-trip for all three encodings including empty input, the 1/2/3-byte padding
  boundaries, and every byte value 0..=255 (runtime unit tests); the RFC 4648 `"foobar"` vectors;
  invalid input (bad symbol / bad length / wrong alphabet / odd-length or non-hex) rejected as
  `Error.Invalid`; and `utf8_valid` positive/negative cases — `tests/m10_encoding.rs` (11) +
  `align_runtime` unit tests (7).
- **Slice 2 — std.rand — DONE.** `rand.seed()`/`rand.seed_with(s)` produce a **Copy** `rng`
  ([`Ty::Rng`], the 256-bit Xoshiro256++ state as `[4 x i64]` — a value, not a Move handle: it owns
  no fd, so it is passed/returned/reassigned by value and is *never* on the Move/drop/escape path);
  `r.next()`/`r.range(lo, hi)`/`r.shuffle(out xs)`/`r.sample(xs, k)` take a **mut** receiver and
  advance the state in place (the runtime mutates the slot through a pointer). All rand nodes are
  **Impure** (seed reads OS entropy; the rest produce/advance mutable state), so an rng-using closure
  is excluded from `par_map`. Implementation: a new `Ty::Rng` swept through every pass (Copy/`Static`
  everywhere — `ty_is_move`/`tracks_region`/`region_of`/`null_moved_source` all leave it out) + new
  HIR `RandSeed`/`RandSeedWith`/`RandNext`/`RandRange`/`RandShuffle`/`RandSample` + MIR `Rvalue`s +
  `align_rt_rng_*` runtime (SplitMix64 deterministic seed / `getrandom` OS seed → **abort** on the
  rare failure; Xoshiro256++ `next`; Lemire nearly-divisionless `range`, `lo >= hi` aborts; in-place
  Fisher-Yates `shuffle`; partial-Fisher-Yates `sample` → a fresh owned `array<T>` (heap, `Drop`-
  freed), `k < 0`/`k > len` aborts). `shuffle`/`sample` are element-size-generic (byte swaps /
  copies), primitive-scalar elements. **Completion condition met:** `seed_with` deterministic +
  portable (pinned outputs); `range` half-open `[lo, hi)` (single-value ranges exact) + reachability
  smoke check; `shuffle` preserves the multiset (Fisher-Yates permutation); `sample` returns `k`
  distinct items; `lo >= hi` / `k` out of range abort; `rng` is Copy (value pass / reassign);
  `import std.rand` required — `tests/m10_rand.rs` (12) + `align_runtime` unit tests (5).
  v1 `sample` uses an O(n) index-permutation scratch (correctness before speed); an O(k) Floyd's-
  sample is a later optimization behind the same signature.
- **Slice 3 — std.cli — DONE.** `cli.command`/`c.flag_bool`/`c.flag_str`/`c.flag_i64`/`c.parse`/
  `p.get_bool`/`p.get_str`/`p.get_i64`/`c.usage` — a flag-registration parser over
  `main(args: array<str>)`'s `array<str>`, not a second argv source. Two new **Move** handle types
  (`Ty::CliCommand`/`Ty::CliParsed`, the `reader`/`writer`/`buffer` precedent) swept through every
  pass (`ty_is_move`/`is_owned_droppable`/`scalar_arg`/`null_moved_source`/drop-insertion/MIR/codegen)
  + a new **`Scalar::CliParsed`** owned-Move payload so `c.parse(args)` returns `Result<parsed, Error>`
  (dropped via `align_rt_cli_parsed_free` on the Result's `Drop`). **`parse` borrows `c` (never
  consumes it)**, so `c.usage()` stays callable *after* parse — including on the `Err` path, which is
  exactly when help is printed. Getters are **total** after a successful parse; an unregistered name /
  wrong kind is a **runtime abort** (the settled #345 policy — Align has no comptime, so a `get_*` can
  neither be statically checked against a runtime flag set nor silently default; it aborts like an OOB
  index). `get_str` returns a `str` **view** into `parsed` (region-bound — a `Frame` view, so an
  escape is a compile error; `.clone()` copies out — the #297 arm). v1 bound-receiver gate: an owned
  handle temporary cannot be a method receiver (the `check_reader_method` precedent), so
  `cli.command("x").flag_bool("v")` and `c.parse(args)?.get_bool("v")` are rejected until Move-
  temporary drops land; `flag_*` do **not** require `mut` (mutate in place through the handle, like a
  `buffer`). v1 argv grammar: `--name` (bool), `--name value`, `--name=value` (str/i64); `args[0]` is
  the program name, skipped. All cli ops are **Pure** (no syscalls — argv is already captured).
  Implementation: sema builtin dispatch (`import std.cli`) + HIR `CliCommand`/`CliFlag`/`CliParse`/
  `CliGet{Bool,I64,Str}`/`CliUsage` + MIR `Rvalue`s + `align_rt_cli_*` runtime (owned Rust `String`
  flag table / value map, deep-freed on `Drop`; the tokenizer walks the `AlignStr` argv buffer).
  **Completion condition met:** bool present/absent, str/i64 default+override, both `--name value` and
  `--name=value`, unknown/missing/malformed → `Error.Invalid`, `get_*` unregistered/wrong-kind abort,
  `get_str` escape rejected (`.clone()` OK), `usage()` renders all flags and stays callable after a
  parse `Err`, and the import + bound-receiver + array-element gates — `tests/m10_cli.rs` (17) +
  `align_runtime` unit tests (5). Review-driven refinement (#356): the cli method names dispatch as
  **type-guarded arms in the tail method match** (the `trim`/`map_err` shape), so a same-named
  method on any other receiver type falls through to standard resolution instead of an eager
  cli-specific error. v1 is a scalar reference parser; struct-decode (the `json.decode`-shaped
  ideal) waits for derive.

**Explicitly deferred to M11+ (not part of M10):** `std.net`, `std.http`, `std.process`,
`std.compress`, `std.crypto` — each carries a new Move type (socket / child-process handle), an FFI
engine dependency, or an unsettled design question that would block a scope-closing milestone:
`std.process` needs `process.exit`'s Drop/arena-cleanup semantics settled first
(`docs/open-questions.md` Open → "`process.exit` Drop semantics"); `std.crypto` needs its
constant-time requirement verified, not just specified; `std.http` depends on TLS (an FFI engine);
`std.compress` depends on `libzstd`/`zlib-ng` (FFI). encoding/rand/cli ship first specifically
because they close over already-existing mechanisms.

## M11: std third wave — net / process / compress / crypto / http (IN PROGRESS)

Design source of truth per module: `docs/impl/std-design/*.md` (#348). Order: net first (new
Move types, no external engine), then process, then the FFI-engine modules (compress/crypto)
and http last (needs net + TLS).

- **`std.net` — DONE (2026-07-06, PRs #371–#374, one slice per PR).**
  - Slice 1 `dns.resolve(host) -> Result<array<string>, Error>` (#371): getaddrinfo →
    owned deep-dropping `array<string>` (the `fs.read_dir` #339 template followed arm-by-arm);
    EAI mapping = `EAI_NONAME`/`EAI_NODATA`/no-address → `Error.Invalid`, everything else →
    `Error.Code(|eai|)`; `AddrInfo` FFI layout cfg-gated Linux/macOS with the BSD-port surface
    (layout + `AF_INET6` + `EAI_*` change together) documented; `ai_addrlen` guarded before the
    fixed-offset sockaddr read.
  - Slice 2 `tcp_conn` (#372): first Move fd type (`Ty::TcpConn`, Drop = close); `tcp.connect`
    (per-address getaddrinfo walk, SO_KEEPALIVE on, port 1..=65535); `c.reader()`/`c.writer()`
    borrow the **M9 reader/writer unchanged** (`owns_fd:false` — only the conn's Drop closes;
    net adds socket lifecycle, not a new I/O path — the slice's core proof). The #297-aware arm:
    `tracks_region(Reader/Writer)` flipped to true with `region_of(ConnReader/ConnWriter) =
    Frame ∩ region_of(conn)` so a borrowed stream can't escape its conn; direct owned
    constructors stay `Static`; the one conservatism (an owned reader threaded through a user
    call with a non-Static arg is rejected on return) is test-pinned and honest in the
    `tracks_region` comment — the precise fix rides the escape→MIR-dataflow follow-up.
  - Slice 3 `tcp_listener` (#373): twin Move type; `tcp.listen` (AI_PASSIVE, empty host = null
    node = wildcard bind, SO_REUSEADDR, backlog 128), `l.accept() -> Result<tcp_conn, Error>` —
    accept **borrows** the listener (accept loops move-check-pinned); EINTR asymmetry documented
    (accept retries; connect fails that address and moves on).
  - Slice 4 `udp_socket` (#374): third twin; `udp.bind` (SOCK_DGRAM, wildcard on empty host),
    `u.send_to(data: bytes, host, port)` (non-consuming byte-view arg, EINTR-retried atomic
    sendto, per-call resolve = documented v1 cost), `u.recv_from(buf) -> Result<i64, Error>` —
    the `reader.read(buf)` cap/len shape reused verbatim. **Datagram peer deferred**: Result Ok
    payloads are single Scalars (no `Scalar::Tuple`), and an owned peer would make net.md's
    "small Copy struct" a Move aggregate — count-only is the ideal v1; peer waits for
    first-class builtin-struct returns (recorded in net.md EN+ja).
  - **Deferred with record (not blockers):** port 0 / kernel-assigned bind (needs a
    `local_addr()`-style accessor), datagram peer (above), intra-frame borrow invalidation
    (reader outliving a reassigned/moved conn — the `buffer.bytes()`/`cli.get_str` class,
    sharpened note in open-questions), send_to destination cache, non-blocking/epoll/io_uring
    backends (later Linux backend behind the same signatures), BSD-family port surface.
  - Process note: every slice shipped through an independent adversarial gate review before its
    PR (slices 3/4 came back zero-finding), plus the gemini review reflected before merge.
- **`std.process` — DONE (2026-07-06, PRs #376–#378, one slice per PR).**
  - Slice 1 `process.exit`/`process.abort` (#376): the settled Drop-semantics decision BUILT —
    `exit(code)` emits the current frame's pending cleanup via the **same** `emit_exit_cleanup`
    a `return` uses (drops → task_groups reversed → arenas reversed, innermost-first; no second
    mechanism), then `exit(code)` (low-byte truncation documented); `abort()` = immediate
    `_exit(1)`, no cleanup — the named-dangerous escape hatch, distinct from `panic_abort`'s
    SIGABRT. No `Ty::Never` exists, so both are typed `Ty::Unit` v1 (statement position; a
    proper `Ty::Never` is a recorded deferral); the block gets `Unreachable`, and the normal-path
    cleanup is `is_terminated`-guarded (no double drop/arena-end). Global flush machinery
    proven unneeded (print is write-through; every writer is a local flushed by its Drop in
    the exit cleanup). v1 gap recorded: current-frame cleanup only; full stack unwind is the
    documented ideal. The open-questions "process.exit Drop semantics" entry moved
    Open → Settled.
  - Slice 2 `child` + `spawn` + `wait` (#377): Move type `Ty::Child` (pid + reaped flag);
    **Drop reaps via blocking waitpid** (P2 — no zombies; NO `SA_NOCLDWAIT`, it breaks explicit
    `wait()` with ECHILD); `spawn` = fork+execvp with argv marshalled fully pre-fork (the child
    branch is execvp + `_exit(127)` only — exec-not-found surfaces as wait()==127, the honest
    fork/exec contract); args = full argv incl. [0], accepted as a fixed literal (the existing
    `ArrayToSlice` coercion — no new mechanism), dynamic array, or slice; `wait()` borrows
    (non-consuming, mirrors accept), EINTR-retried, WEXITSTATUS / signal → 128+sig, double-wait
    → clean Err before any syscall (recycled-pid safe). **P3 CLOEXEC sweep** landed on all
    existing fd constructors (std.net sockets via SOCK_CLOEXEC/accept4 + fcntl fallback; fs.open
    already O_CLOEXEC, test-proven). The self-review caught a real Gate-1 miss pre-PR
    (`null_moved_source` lacked `Ty::Child` — a moved child would have double-reaped), fixed +
    test-pinned.
  - Slice 3 `ch.kill` + `process.exec` (#378): `kill(sig)` borrows the child, reaped/pid<=0
    guard before the syscall (no stray/broadcast signal — the kill(0/-1) POSIX semantics), sig
    0 allowed (liveness probe), 0..=64 bounds the i64→i32 narrow; `exec` = execvp in-process,
    returns only on error — deliberately NOT noreturn (the failure path must run), lowered as
    a plain fallible call; **no cleanup runs on a successful exec** (image replaced —
    abort-class, documented loudly), and CLOEXEC'd Align fds don't survive into the new image
    (0/1/2 do). Marshalling shared with spawn (one runtime helper, one sema `check_argv`).
  - **Deferred with record (not blockers):** a proper `Ty::Never` for diverging exprs;
    multi-frame exit unwind; `detach()` for dropping a running child without blocking;
    posix_spawn / pre-resolved-PATH exec (the fork-from-multithreaded-parent allocator caveat,
    documented).
- **`std.compress` — DONE (2026-07-07, PRs #380–#381, one slice per PR).** The first FFI-engine
  module — own the memory wrappers, borrow the engine (draft §15): the runtime wraps `libz`/
  `libzstd` via `extern "C"`, Align allocates the output. Byte→byte, borrowed `bytes` view in,
  **owned `buffer` out** (reuses #346 — no new Move type); Impure (extern-calling →
  par_map-rejected, test-pinned); both `-lz` and `-lzstd` join the driver's base link set
  unconditionally (the runtime staticlib always carries the wrapper symbols; conditional linking
  would need a per-module sema→driver flag — not worth it for universal system libs).
  - Slice 1 gzip via libz (#380): `compress.gzip_compress(data: bytes, level: i64) ->
    Result<buffer, Error>` / `gzip_decompress(data: bytes)` behind `import std.compress`.
    Strict gzip framing both ways (windowBits 15+16 — zlib/raw streams rejected; magic
    test-pinned); level `0..=9` total-or-abort (#345, mirrors `rand.range`), checked before any
    allocation; **decompress-bomb guard** = hard 1 GiB output cap → `Error.Invalid` (compress
    output is input-bounded, no cap); corrupt/truncated → `Error.Invalid`, genuine engine/OOM →
    `Error.Code(|code|)` through the existing `AL_CODE` catch-all — no new `Error` variant.
    MIR refactor: the status+buffer Result tail shared with `EncodingDecode` extracted into
    `emit_status_buffer_result` (verbatim; encoding ABI unchanged). The gemini review's two
    valid highs were reflected pre-merge: cap enforcement moved from `capacity` to `len`
    (allocator over-allocation — `try_reserve_exact` may round up — makes capacity an unreliable
    cap proxy) and the inflate spare clamped to `max_cap - len`, both regression-tested.
  - Slice 2 zstd via libzstd (#381): `zstd_compress`/`zstd_decompress`, twin-mirror parity —
    extends `CompressKind` (codegen kind-arms exhaustive `Gzip | Zstd`, a future codec is a
    compile error; the 5 HIR-walking passes treat the kind opaquely by design — the codecs are
    semantically identical). Level `0..=22` (`0` = zstd default; negative fast levels excluded —
    one non-negative range). Compress = one-shot `ZSTD_compress` sized by `ZSTD_compressBound`;
    decompress = streaming `ZSTD_decompressStream` through the shared hardened grow loop + the
    same 1 GiB cap — `ZSTD_getFrameContentSize` is never trusted for allocation
    (attacker-controlled header); `ZSTD_freeDStream` on every path; the hardcoded
    `ZSTD_ErrorCode` constants (64/66) verified against `zstd_errors.h`'s stable section and
    `ZSTD_getErrorCode` confirmed in the stable dynamic ABI. Trailing bytes after the first
    complete frame are ignored (matches zlib's first-member semantics). Zero-finding adversarial
    gate review and zero-finding gemini review.
  - **Deferred with record (not a blocker):** a graceful libz/libzstd-absence story — v1 assumes
    both libs present (a build without them fails at link); feature-gating is the recorded
    option in compress.md P3. v1 scope is whole-buffer byte→byte per the design (streaming /
    dictionary surfaces were never in scope).
- **`std.crypto` — DONE (2026-07-07, PRs #384–#388, one slice per PR + the #383 engine-decision
  docs PR).** Engine: **OpenSSL libcrypto (EVP), floor ≥ 3.2, `-lcrypto` always-linked** — settled
  pre-implementation via two independent design reviews (security + dependency lens, #383);
  **blake3 deferred with record** (no system engine; no BLAKE2b aliasing). The module's hard
  requirement — constant-time as *verified*, not specified — was met literally: every slice
  passed an independent adversarial gate review, and the CT/security-critical properties were
  checked against **compiled machine code**, not source.
  - Slice 1 (#384) `constant_time_equal` + `crypto.random`: the ONE self-hosted primitive —
    byte-diff OR-reduction (no early exit → no memcmp idiom) + `black_box` barrier, **disassembly-
    verified branchless in both shipped profiles** (release: vectorized `pxor`/`por` + lone
    `sete`; every conditional jump is on public length/loop bounds); length is public
    (`sodium_memcmp` contract). Pure → allowed in `par_map` (pinned). `crypto.random` = the
    generalized `fill_os_random` drain loop (getrandom short-read/EINTR; macOS 256-byte chunks)
    shared with `rand.seed`; abort on failure (key material); Impure (pinned).
  - Slice 2 (#385) sha256/sha512: shared one-shot `EVP_Q_digest` wrapper; owned `array<u8>`
    digest via the RandSample `{ptr,len}` return path (fixed-size `array<u8; N>` not expressible
    in the runtime-return ABI — documented dynamic fallback); engine failure aborts (no
    invalid-input case, never a silent wrong digest); NIST vectors pinned. The gemini review's
    three "may not compile" highs were disproven against the green build and rejected with
    reasons (the #372 false-positive class).
  - Slice 3 (#386) hmac_sha256 + hkdf_sha256: `EVP_Q_mac` one-shot; HKDF via `EVP_KDF` with a
    hand-built `#[repr(C)] OsslParam` mirror of `ossl_param_st` (avoids by-value-struct FFI
    returns; layout/constants/keys verified against core.h/params.h/core_names.h); public `len`
    bounds 1..=8160 (RFC 5869) before the engine; RFC 4231 + RFC 5869 vectors pinned.
    **Shipped only after a real regression was caught and root-caused**: the lowering arms'
    locals, written inline in the recursive MIR `lower_expr`, inflated its per-recursion frame
    (debug builds reserve all arms' locals) and overflowed the default 2 MiB test stack at
    `+`-chain depth 40 — fixed by extracting `#[inline(never)]` free helpers (now the standing
    convention), with measured frame parity vs main (ceiling exactly 40 both). Surfaced a
    pre-existing gap recorded in open-questions: front-end depth cap 128 vs ~40 full-pipeline
    default-stack ceiling.
  - Slice 4 (#387) AEAD aes_gcm + chacha20_poly1305: one `CryptoAead {cipher, dir}` node, two
    shared runtime impls over `EVP_CIPHER_fetch`, exhaustive 4-way dispatch (no default arm);
    combined `ct || 16-byte tag` format; key 32 / nonce 12 validated pre-engine; 1 GiB cap with
    `checked_add`. **The P2 all-or-nothing shape verified line-by-line**: staged plaintext via
    internal buffer, `SET_TAG` before `DecryptFinal_ex`, publish only on `Final == 1`, and
    `OPENSSL_cleanse` on the failure exit **confirmed present in the optimized artifact** (GOT
    relocation + live call — not elided); total error opacity on open (every failure = the one
    opaque `Error.Invalid`). KATs = NIST GCM Test Case 16 + RFC 8439 §2.8.2, independently
    confirmed canonical. gemini's three dangling-ptr+len-0 "technically UB" findings were
    rejected with reasons (valid-for-zero-length per Rust's own model; OpenSSL supports inl==0;
    the shipped Slice-2/3 convention — one idiom, not piecemeal guards).
  - Slice 5 (#388) argon2id: `EVP_KDF_fetch("ARGON2ID")`; **`argon2_params {m_cost, t_cost,
    parallelism, len}` is the language's first builtin struct** (reserved-name injection like the
    builtin `Error`, ordinary struct-literal machinery, zero special cases — non-literal paths
    all work; redeclaration cleanly diagnosed). Bounds pre-engine: parallelism 1..=2^24-1
    (checked first so `8*parallelism` can't overflow), t_cost 1..=u32max, m_cost
    8*parallelism..=4 GiB-in-KiB, len 4..=1 GiB; engine `threads` pinned to 1. KAT = the
    canonical phc-winner-argon2 reference vector, reproduced independently via the OpenSSL CLI.
    Implementation self-caught two real bugs pre-review: a wrong `OSSL_PARAM_UNSIGNED_INTEGER`
    constant (2, not 6) and an `Rvalue` enum-size growth re-triggering the expr-depth ceiling —
    fixed by boxing the payload (`Argon2Args`).
  - **Deferred with record (not blockers):** blake3 (#383); zeroize-on-drop key buffers (P6 —
    buffer Drop just frees; callers holding key material should overwrite first; a zeroizing
    buffer variant is the recorded candidate); a nonce-generating seal convenience (P3);
    `OSSL_set_max_threads` for parallel argon2 lanes; fixed-size `array<u8; N>` digest returns
    (needs a runtime-return ABI extension).
- **`std.http`** — the last M11 module (plaintext-only v1 per http.md — TLS deferred, `https://`
  rejected not downgraded; builds `get_many` on the net substrate via task_group + the par_map pool
  — the #301 claim-loop lesson).
  - **Slice 1 (request/response types + HTTP/1.1 serialize/parse, NO sockets) — DONE.** Two new
    Move handle types `Ty::HttpRequest` / `Ty::HttpResponse` (+ `Scalar::HttpResponse`), full
    twin-mirror Gate-1 sweep. Language surface behind `import std.http`: `http.request(method, url)`
    (total — URL not parsed here), `r.header(name, value)` / `r.body(data)` (mutate in place, bound
    receiver), `http.parse(bytes) -> Result<response, Error>`, `resp.status()` / `resp.header(name)`
    (case-insensitive `Option<str>` view) / `resp.body()` (`slice<u8>` view) — the two getters
    region-bound to `resp` (#297; escape past `resp`'s `Drop` is a compile error). All ops **Pure**
    (no sockets in this slice — the Impure network client is Slice 2). Key decisions:
    (a) **URL validation is deferred to serialize** (not `http.request`) so a runtime-supplied URL
    never aborts the builder; `https://` / non-`http` / empty-authority / malformed → `Error.Invalid`
    at serialize/send time (P1). (b) **`http.parse` is exposed** as the response constructor + codec
    primitive (Slice 2's client reuses the same runtime engine) — needed so the response type +
    getters + region-escape are reachable and test-pinned now; it is a permanent primitive, not a
    throwaway. (c) **serialize stays a runtime-internal codec** (`align_rt_http_serialize`,
    unit-tested; not a language builtin in Slice 1 — Slice 2's client renders + one-writes it, R4).
    (d) **P6 (CR/LF/NUL header injection) aborts** at `r.header()` (build-time, total-or-abort).
    (e) **Auto-header policy:** serialize emits `Host` (from the URL authority) and `Content-Length`
    (iff body non-empty); a caller-supplied `Host` / `Content-Length` is **rejected** (`Error.Invalid`)
    — CL duplication is a smuggling vector, safer than a silent override. (f) **chunked
    Transfer-Encoding → `Error.Invalid`** (v1 is Content-Length framing only; de-chunking that
    honours R1 is deferred, loud). (g) **Caps:** ≤ 128 headers, ≤ 1 GiB body → else `Error.Invalid`.
    (h) **R1 zero-copy:** `HttpResponse` owns ONE byte buffer + an offset table (`HttpHeaderSpan`
    name/value offsets + body_start/len); getters return views, no per-header `String`, no body copy;
    scanning rides the `memchr` crate (R2). Tests: `crates/align_driver/tests/m11_http.rs` (16 —
    builder build/drop, P6 abort, Move/unbound/array-element/import gates, parse round-trip,
    case-insensitive header, 404-is-data P2, malformed-is-Err-not-abort, body + header view escape
    #297 in both direct and `match`-arm-unwrap forms) + `align_runtime` units (9 — serialize golden
    bytes, request-line/method injection reject, parse offsets/framing, conflicting-Content-Length
    reject, error mapping, null-safe frees). **Adversarial-review fixes (post-first-cut):**
    (i) **general pattern-binding region propagation** in `EscapeCheck` — a `match`-arm payload
    binding now inherits the scrutinee's non-Static region (mirrors `LetTuple` + the `OptionSome`/`Try`
    pass-through), closing a confirmed use-after-free where `resp.header()`'s `Option<str>` view
    escaped via `match resp.header(..) { Some(v) => v, .. }` (the codebase's first `Option<borrowed
    view>`; env.get's is owned so it never exposed the gap). The **ideal general fix**, not a point
    patch — it closes it for every future `Option<view>`/`Result<view>`; no regression across the
    cli/net/crypto view-escape suites. (ii) serialize now validates the **method is an RFC 7230
    token** and the URL-derived authority/path carry **no CR/LF/NUL/SP** (the permanent codec must not
    let `http://a/x\r\nEvil: 1` smuggle a header). (iii) parse **rejects a conflicting duplicate
    Content-Length** (RFC 7230 §3.3.3; an identical repeat is accepted). **Slices 3–5 (pool reuse,
    server primitive, HTTPS) remain.**
  - **Slice 2 (the plaintext HTTP/1.1 client — get/post/request over one net `tcp_conn`) — DONE.**
    One new Move handle type `Ty::HttpClient` (a ZST in v1 — no `Scalar`, never rides an aggregate),
    full twin-mirror Gate-1 sweep. Language surface behind `import std.http`, all **Impure**
    (network): `http.client()`, `cl.get(url) -> Result<response, Error>` / `cl.post(url, body) ->
    Result<response, Error>` / `cl.request(req) -> Result<response, Error>` (bound-receiver gate,
    reader/writer/cli precedent; `cl` borrowed, `request` **consumes** its Move `req` — the runtime
    frees it, MIR nulls the source slot). Key decisions: (a) **one request = one fresh `tcp_conn`**
    (connect → send → read → parse → close), reusing the net rail (`align_rt_tcp_connect` — DNS +
    connect + SO_KEEPALIVE) and the Slice-1 codec/parse core, with NO pool yet (Slice 3 keepalive) —
    but the FFI entry points already take `*mut HttpClient` so Slice 3 adds pooling behind the same
    language surface. (b) **R4 syscall discipline is shipped, not aspirational:** `TCP_NODELAY` on the
    conn (`IPPROTO_TCP`/`TCP_NODELAY`, stable across Linux/macOS), the whole request rendered by the
    Slice-1 `http_serialize_core` and sent with **one** `write_all`, and the response streamed in 32
    KiB reads (never per-line) to Content-Length. (c) **P1 honesty:** `https://` / a malformed URL is
    `Error.Invalid` at request time (split-URL rejects it before connect) — never a silent plaintext
    downgrade. (d) **P2:** a 4xx/5xx is `Ok(response)` with that status; only transport/parse failures
    are `Err` (the shared out-slot + i32-status lowering treats `0` = ok regardless of HTTP status).
    (e) **Parser refactor to `Incomplete`/`Invalid`:** the Slice-1 `http_parse_core` was split so a
    streaming read distinguishes "valid prefix, read more" from "malformed, stop" over ONE shared
    decoder (no duplicated header scan; the framing helper `http_parse_head` computes the target
    length without copying the body). Content-Length (or read-to-close) framing; chunked stays
    `Error.Invalid`. (f) **`http.client` is a ZST Move handle** — Slice 2 genuinely has no client
    state, so the honest representation is a unit struct (not a half-built disabled pool); the Box
    round-trip is sound for a ZST, and `http_client_free`/Drop is a null-safe no-op until Slice 3 owns
    pooled conns (P5). Tests: `crates/align_driver/tests/m11_http.rs` (+10 — get 200 round-trip with
    body/header views, 404-is-Ok P2, post sends Content-Length + body, https/malformed URL error P1,
    request-consumes-req use-after-move, unbound-receiver / array-element / import gates, response body
    view escape via the client path) + `align_runtime` units (+7 — `http_split_authority` forms,
    get/post/request socket round-trips against an in-process server, https/malformed reject,
    new/free/null-out safety). `cargo test --workspace` 1601 green; expr_depth 5/5 default env; clippy
    `-D warnings` clean. **Recorded Slice-2 limitations (docs-only, no code defect — an independent
    adversarial review found none):** (i) **no read/connect timeout (G3-1, inherited):** a server that
    completes the TCP handshake then stalls (sends nothing, dribbles under the caps, or sends less than
    `Content-Length` and holds the socket) blocks the calling thread indefinitely — the byte caps bound
    memory, not time; this is the net rail's documented no-timeout behavior (`align_rt_tcp_connect`),
    now inherited on connect + read. Timeout support is a **follow-up landing with the Slice-3 pool
    work** (same deadline substrate; not a semantic change). (ii) **`https://` rejection is coarse
    (DC-1):** correctly rejected pre-connect (P1 honesty met) but as the bare `Error.Invalid` — no
    "HTTPS not supported in v1" message, because the `Error` enum carries no payload; structural, tied
    to the message-less error story, not a slot-in fix. (iii) **R6 perf gate NOT yet met (DC-2,
    process):** `bench/http_client` does not exist yet and R6 (benchmark-gated latency/throughput +
    `get_many` scaling vs a Rust baseline) gates **module** completion, not Slice 2 — the bench harness
    lands with Slice 3, since keepalive is what R6 measures. No wording here claims the perf gate is
    met. (Full detail: `docs/impl/std-design/http.md` "Known v1 limitations".)

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
