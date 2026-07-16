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

**Current (2026-07-15): M0–M15 are complete.** M11–M13, the LLVM 19→22 checkpoint,
M14's LTO ceiling/runtime-bitcode slices, and M15 separate compilation (unit interfaces,
per-unit codegen/link, default-on incremental object cache, parallel codegen, and the SV
verification bundle) are complete. The workspace is 2137 green (2136 passed + one ignored manual
probe) and clippy-clean. The `http_server_no_fd_leak_across_cycles` timing flake is hardened as
#457, qualified cross-module function values shipped as #458, wrapper-hidden local-slice returns
are rejected as #459, and shared intra-frame borrow-liveness dataflow shipped as #460. The first
broader escape-analysis structural gate shipped as #461: expression provenance, local-slice
provenance, and their type classifiers are exhaustive, so a new HIR/type variant cannot silently
fall through a permissive wildcard. The first flow-sensitive gate shipped as #462: a unified
region/local-slice state joins `if`/`match`/`else` continuations, excludes diverging paths, reaches
loop-head fixpoints, and keeps early-return arena cleanup classification intact. Path-local MIR drop
flags shipped as #463: every resource-owning slot now transfers explicit individual-vs-arena
ownership state across assignments, moves, destructuring, branches, loops, and cleanup. This makes
region-changing owned reassignment legal when its lifetime target is valid, without leaks or double
frees. Compact checked-HIR escape CFG extraction shipped as #464: exhaustive syntax lowering emits
explicit branch/break/loop edges, and one worklist owns all region/local-slice joins and fixpoints
while diagnostics replay once in source order. Function-value effects shipped as #465: concrete
`FnTy` entries carry inferred `Pure` / `Impure` / `Unknown`, mutable locals join assigned targets,
imported summaries and FFI pointers use the same representation, and indirect consumers read the
type bit instead of treating address-taking as a call edge. Unknown HOF parameters remain
fail-closed. The value-carrying-control-flow matrix shipped as #466: `draft.md` records region
composition and owned move/drop behavior for block / `if` / `match` / `else`-unwrap / `?`, and the
10-cell regression file pins both facts for every form. The audit closed four heap-leak gaps by
carrying the selected arm's individual-vs-arena bit through the same MIR join as its value. No
mandatory implementation slice or receiver-specific borrow patch remains queued. Fully-escaping
function values remain deliberately deferred pending a settled heap-owned environment/drop model
and a consumer.

**Historical snapshot (2026-07-10; superseded by the current line above):** **M0–M10 are complete
and formally closed** — the language core
(M0–M5 + Memory Model v2, tuples, lambdas/closures, sum types + `Error`, minimal generics), M6
SIMD (`vecN`/`maskN`/`soa`/columnar `group_by`/`align(N)`), M7 concurrency (`par_map`/`chunks`/
purity + `task_group` on real threads), M8 tooling (`align_fmt`, `unsafe`/`raw.*`, `extern "C"`
FFI, the profile-independent lint slice), M9 std phase 1 (`io`/`fs`/`path`/`env`/`time`), and
M10 std phase 2 (`encoding`/`rand`/`cli`). **M11 (std third wave) is IN PROGRESS:** `std.net` /
`std.process` / `std.compress` / `std.crypto` COMPLETE; **`std.http` is now COMPLETE** — Slices 1–2
merged (#391/#392), Slice 3 (keepalive pool + R6 bench, R3 met) on branch `http-slice3-pool`,
Slice 4 (server primitive `serve`/`accept`/`respond` + `response_builder` + the five inbound
smuggling guards) on branch `http-slice4-server`, `get_many` (R5) on branch `http-get-many`
(input-order all-or-Err batch on a dedicated bounded blocking-I/O claim-loop pool + the prerequisite
`array<response>` opaque-Move-handle-array capability + the R5 bench — 15.4× overlap at degree 16,
Rust-pool parity; R6 met in full), and **Slice 5 (HTTPS/TLS, client-side) DONE on branch
`http-slice5-tls`** — `https://` over OpenSSL libssl through the unchanged `cl.get/post/request` +
`cl.get_many` surface, mandatory system-trust verification + hostname binding, `(scheme,host,port)`
pool key with the live `SSL*` pooled, per-thread `pthread_sigmask` SIGPIPE discipline, and the
Denied/Code/Invalid taxonomy. With std.http done, **all M11 std-module work is complete** — the
formal M11 close is the orchestrator's call (verify against `open-questions.md` before flipping the
milestone header).

**Historical next sequence (superseded):** finish M12 (only the A5-SSE slice remains in flight) →
**M13 codegen quality & link hygiene** (the pre-LLVM-upgrade wave — see its section) → the
**LLVM/inkwell upgrade checkpoint** → the post-upgrade wave (ThinLTO → runtime bitcode → PGO → BOLT). The
consumer-gated execution-plan items (decode fusion, pushdown, algorithm portfolio, Sink/Source
MIR) stay recorded in the consultation digest + runway records until their align-LLM consumers
arrive.

**Historical build sequence (2026-06 snapshot — kept for the build-order and decision record;
every item below has since completed as recorded in the per-milestone sections, except
`core.bitset` in item 6, still unbuilt for lack of a consumer):**
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
  int/str holes → builder writes → `str`). **Historical milestone note:** `str + str`
  concatenation shipped here, but the 2026-07-02 settlement removed that surface; the checker/MIR
  hard cutover was completed 2026-07-15, including removal of the obsolete MIR lowering.
- [done] arena-backed builder: when a `template` runs inside an `arena {}`,
  the result is allocated in the arena (bulk-freed, no leak); outside, it is leaked
  (process-lifetime).
- [done] `str` escape checking: an arena-backed `str` cannot escape its arena (return /
  arena-block value / assign-to-outer) — `EscapeCheck` now tracks `str` regions like
  `box`. A literal `str` is region-0 and freely returnable. Arena-free template ownership is a
  confirmed gap in audit 13; do not treat its current process-lifetime leak as the desired model.
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

  **Correctness correction (audit 2026-07-13):** the vector-shape completion remains valid only for
  operations safe to speculate. The implementation also evaluates a general reducer/`any`/`all`
  predicate and every stage after `where` on rejected elements. Pure does not imply non-trapping;
  this is now a confirmed P0. The implementation plan's ordinary-Pure requirement also conflicts
  with the normative draft and accepted Impure sequential stages; settle it before adding a new
  rejection. Restore a real guard for every inactive-lane-unsafe suffix/reducer, then recover masked
  SIMD only under conservative legality. See `12-pipeline-closure-memory-io-simd-audit.md` §3.1-3.2.

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
  ranges on a process-lifetime `ParPool`; helpers and the caller drain one shared range cursor and
  join a total-range completion barrier. Intended race-freedom comes from inferred Pure `f` plus disjoint output ranges, but the
  2026-07-12 audit found a P0 lifted-capturing-closure effect edge; **fixed 2026-07-13**, together
  with a fail-closed higher-order unknown-target gate; #465 later moved concrete callable effects
  into `FnTy` and removed address-taking call edges. The saturated `task_group -> par_map`
  forward-progress P0 is also fixed by the shared caller-draining cursor and a watchdog gate. A *staged*
  `par_map` (`where(p).par_map(f)`) and a capturing `par_map` still use the sequential collect loop.
  Results are identical to the sequential lowering when the Pure premise holds.
- [done] **first-class closures (escape-driven)** — slices ①–③ (PRs #104–108): non-capturing
  function values + indirect call (①), a lambda as a first-class value with typed parameters (②a),
  the fat-pointer closure ABI (②b-1), capturing closures with a **frame-local** environment (②b-2),
  and higher-order functions (fn-typed parameters, ③). A non-escaping pipeline lambda stays inlined
  (captures-as-params); a lambda captured by `spawn` snapshots into the `task_group` region. Escape
  analysis picks the representation, so the offload-ready pipeline path is untouched.
- [done] **`task_group` / `spawn` / `wait` (I/O concurrency)** — slices ④a–④c (PRs #110–117; #117
  "closures arc COMPLETE"). A structured scope like `arena {}`: `spawn(fn { … })` takes a lambda
  (captures snapshotted into a fresh per-spawn **region env**), returns `Task<R>` (a region-tied
  result slot); `wait()?` joins all through caller-participating atomic claims on the persistent
  `ParPool` and propagates a failing task's `Err`; `t.get()` reads a result after the join (a
  `get`-before-`wait` use is a compile-time flow error). Tasks may be impure (I/O); safety from
  by-value capture. Runtime: `align_rt_tg_begin/alloc/register/wait/end`.
- [deferred] **fully-escaping function values** — returning a fn value from a function, or storing
  one in a struct field / array element, is **not** supported: it needs a **heap-owned** closure
  environment with its own drop (the "escapes every region" model), whose design is not yet settled
  and which has no consumer today (`task_group` uses the region env). Deliberately deferred — see
  `open-questions.md` "First-class closures + task_group" (the escape-every-region note).
- async/await is not included (`non-goals.md`).

**Parallel correctness/output-IR companion record (2026-07-12):**
`11-parallel-execution-optimization.md` confirmed two previously unrecorded P0s: an Impure lifted
capturing closure could be laundered through a function value into `par_map`, and a saturated
`task_group -> par_map` re-entry deadlocked because only `task_group` drained work on its caller.
Both are fixed and regression-pinned as of 2026-07-13: effects fail closed at the Pure boundary,
and `par_map` helpers plus callers drain one shared range cursor under a child-process watchdog. The record makes the existing
whole-chunk specialization plan concrete, and gates new capture-context, integer
transform-reduce, staged-pipeline, low-lock latch/batching, work-aware grain, and split execution
domain candidates. It proposes no new source syntax.

**Pipeline/closure/memory/I/O/SIMD companion record (2026-07-13):**
`12-pipeline-closure-memory-io-simd-audit.md` is the durable follow-up. It preserves the positive
fusion/vectorization/capture/I/O findings, corrects the post-`where` speculation premise, records
closure lifetime + Unit-indirect-ABI + buffered-`io.copy` blockers, the ordinary-effect contract
conflict, and the now-complete allocation-size hardening. It gates per-callsite arena initialization,
exact-destination codec/hex-SIMD, macOS copy-path, HTTP-copy, and sequential compaction candidates.
Its P0 slices precede any SIMD or parallel widening; it adds no source syntax.

**String/array allocation-copy and short-input companion record (2026-07-13):**
`13-string-array-allocation-short-input-audit.md` is the durable follow-up for text and array
ownership, allocation count, copy count, and `0..64` behavior. C0 first restores UTF-8 slice
boundaries and settled concat rejection, then gives unbound owned expression temporaries
view-aware lifetime/drop handling (**shipped 2026-07-15**) and removes the arena-free template
lifetime leak. Confirmed
implementation work then removes borrowed-path copies, builder freeze copies, staged directory/DNS/
path/group outputs, and direct-consumer chunk headers. UTF-8 crossovers, repeated-needle plans, JSON
escape SIMD, and large constant arrays remain measure-first. Its language-surface section contains
questions for Claude Code, not adopted syntax or semantics; `open-questions.md` remains authoritative.

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
copy), `prefer-pipeline-over-vecN` (no firing surface — Align has no loop construct to convert) (Update
2026-07-09: a `loop` expression is now design-settled — `open-questions.md` Settled → "Sequential
control" — so this lint gains its firing surface once `loop` is implemented.),
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
    kernel/loop surface exists) in `open-questions.md` under the M8 lint candidates. (Update
    2026-07-09: a `loop` expression is now design-settled — `open-questions.md` Settled →
    "Sequential control" — so this lint gains its firing surface once `loop` is implemented.)
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
  exists. (Update 2026-07-09: a `loop` expression is now design-settled — `open-questions.md`
  Settled → "Sequential control" — so this lint gains its firing surface once `loop` is
  implemented.)
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
*temporary* was not dropped at the M9 milestone; the general synthetic-owner fix shipped
2026-07-15 (the bound-receiver surface restriction remains separate). **Not blockers, tracked as post-M9
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
  `fs.create` → `writer.write(buf.bytes())`; historical shape — this idiom migrates to the `loop`
  expression once it is implemented, per the 2026-07-09 sequential-control decision), plus
  per-method tests (EOF, `NotFound`/`Denied`
  mapping, `io.stdin`, moved-reader rejection, buffer-view escape rejection) — `tests/m9_io.rs`.
  Historical minor gap (fixed 2026-07-15): an **unbound Move temporary** lacked a synthetic Drop.
  General expression temporaries now clean up path-locally; the v1 bound-receiver rule for owned
  handles remains a separate source-surface restriction.
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
  **Audit correction (2026-07-13, FIXED):** the original byte-exact tests started from an empty
  reader buffer. `io.copy` used to skip unread lookahead retained by a prior `read_line`; it now uses
  the shared reader path, and a byte/count regression pins `read_line -> io.copy` before any future
  fast path (`impl/12` §3.6). The O(buffer) result remains valid.
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
- **Move-temporary drop — DONE 2026-07-15.** Unbound Move expressions have path-local synthetic
  owners with view retention and per-iteration cleanup. The bound-receiver rule remains separate.
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
  `cli.command("x").flag_bool("v")` and `c.parse(args)?.get_bool("v")` remain rejected by the v1
  receiver surface even though general Move-temporary drops landed 2026-07-15; `flag_*` do **not** require `mut` (mutate in place through the handle, like a
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

## M11: std third wave — net / process / compress / crypto / http — COMPLETE (formally closed 2026-07-10)

All five modules are DONE with reviews reflected and their per-module completion conditions met:
`std.net` (#371–#374), `std.process` (#376–#378), `std.compress` (#380–#381), `std.crypto`
(#383–#388), and `std.http` in full — Slices 1–5 + `get_many` (#391, #392, #398, #409, #411, #412),
with **R1–R6 all met** (keepalive 2.86×, get_many 15.4× overlap at degree 16 + Rust-pool parity,
benches in `bench/http_client/README.md`). Deferral lists live per-module below; none gated
completion (verified against `open-questions.md` at close — the loop slice shipped separately as
#402, `process.exit` semantics settled+built, the crypto engine settlement stands).

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
    `tracks_region` comment — the precise fix needs interprocedural return-borrow summaries, not
    another intraprocedural CFG change.
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
- **`std.http`** — the last M11 module, now **COMPLETE** (Slices 1–5 + `get_many`). Client-side
  HTTPS/TLS ships (Slice 5, OpenSSL libssl, mandatory verification — `https://` connects over TLS,
  never a plaintext downgrade); server-side TLS + revocation stay recorded post-v1. `get_many` builds
  on the net substrate via a dedicated bounded blocking-I/O claim-loop pool — the #301 lesson.
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
  - **Slice 3 (the keepalive connection pool + the R6 benchmark) — DONE.** `Ty::HttpClient` goes
    from a ZST to a real Move type owning a **keepalive connection pool** — and, notably, this is a
    **pure runtime change**: the compiler already treats `HttpClient` as an opaque handle pointer
    (codegen emits a pointer; Drop already calls `align_rt_http_client_free`), so the ZST→state change
    is invisible to sema/MIR/codegen (no compiler edits — the Slice-2 "ZST behind the same FFI" design
    paying off exactly as planned). Key decisions: (a) **pool shape** — `Mutex<HashMap<(host, port),
    Vec<IdleConn>>>` (the `Mutex` future-proofs a shared-across-threads `get_many`; single-threaded
    v1 never contends; the lock is held only for the O(1) take/put, never across I/O). (b) **reuse by
    default (R3)** — consecutive `get`/`post`/`request` to the same `(host, port)` reuse a live idle
    conn with zero opt-in; the language surface + FFI ABI are unchanged. (c) **reuse verdict
    (correctness-critical)** — a finished conn is pooled **iff** keep-alive (HTTP/1.1 default;
    `Connection: close` / non-1.1 → not reused, via `http_head_keep_alive` on the response head) AND
    Content-Length-framed (read-to-close ends at the conn close → not reused) AND no leftover bytes
    beyond the framed message (leftover ⇒ dirty conn ⇒ dropped — reusing it would misframe the next
    response, a data-corruption class bug). (d) **stale-conn retry** — a reused idle conn the server
    dropped fails before any response byte; that one case is transparently retried once on a fresh
    conn (the request was almost certainly never processed — the idle-close race); a fresh conn's
    failure, or a mid-response failure, surfaces directly. (e) **SIGPIPE safety** — the client write
    path uses `send(MSG_NOSIGNAL)` (Linux) / `SO_NOSIGPIPE` (macOS), so writing to a dropped reused
    conn returns `EPIPE` (→ retry) instead of killing the process; no global signal handler installed.
    (f) **Drop closes all pooled conns (P5)** — no fd leak across pool churn. (g) **bounds** — ≤ 8
    idle conns/host (overflow closed); idle-expiry reaps conns idle > 90 s on take. (h) **I/O timeouts
    stay deferred** — the Slice-2 note tied a connect/read timeout follow-up to "the Slice-3 pool's
    per-conn deadline bookkeeping," but that conflated the pool's *idle-expiry* (shipped) with an *I/O
    deadline* (a separable, larger change whose ideal home for connect is the net rail's non-blocking
    substrate, and whose read side has no v1 config surface without expanding the frozen signatures);
    per "ideal form, or defer," it is recorded as the standing v1 limitation, not half-shipped. **R6
    met:** `bench/http_client` (a standalone harness driving the shipped pool's C-ABI vs a plain-Rust
    `std::net` baseline over a localhost server) records **2.86× keepalive speedup** (floor 1.48× —
    MET) and **parity with hand-written Rust** on the reuse path (`bench/http_client/README.md`).
    Tests: `align_runtime` units (pool reuses one conn across 3 gets; `Connection: close` not pooled;
    stale-conn retry; `http_head_keep_alive` decision table) + a driver test (two gets reuse one conn,
    observed via the server's accept count). `cargo test --workspace` green; clippy `-D warnings`
    clean.
  - **Slice 4 (the server primitive) — DONE** (branch `http-slice4-server`). Three new Move types
    (`http_server`, `http_request_ctx`, `response_builder`) took the full Gate-1 twin-mirror sweep —
    `Ty` for all three; `Scalar` for the two `Result` Ok payloads (`http_server` from `http.serve`,
    `http_request_ctx` from `srv.accept`); `response_builder` is `Ty`-only (returned directly, like
    `http request`). Surface: `http.serve(host, port) -> Result<http_server, Error>` (wraps net's
    `tcp.listen` — SO_REUSEADDR + backlog 128 — then lifts the listening fd out); `srv.accept() ->
    Result<http_request_ctx, Error>` (streams the request, parses via the **new**
    `http_parse_request_head`, closes the conn + returns `Error.Invalid` on a malformed request while
    the listener keeps serving); `ctx.method()/path()/header(name)/body()` (views region-bound to
    `ctx`, #297); `http.response(status)` -> `response_builder` + `rb.header`/`rb.body`;
    `ctx.respond(rb) -> Result<(), Error>` (**consumes both**, one-write R4, closes the fd). The
    request-head parser adds the **five inbound smuggling guards** the client-lenient response parser
    lacks (strict CRLF / no space-before-colon / no Transfer-Encoding / origin-form target only /
    method-token+CR-LF-NUL). `respond` mirrors the client serialize (auto Content-Length iff a body
    was set; caller `Content-Length`/`Transfer-Encoding`/`Connection` rejected; no auto Date/Server)
    and additionally always emits `Connection: close` (RFC 9112 §9.6 mandate for v1's one-request-per-
    conn close). The `null_moved_source` MIR arm for the respond double-consume was the one
    easy-to-miss twin-mirror site. **Security caveat (recorded in http.md):** the blocking single
    accept loop is a slow-loris DoS on an untrusted network — v1's trust assumption is a
    localhost/trusted-network gateway; a read/accept deadline is the first post-v1 hardening. Tests:
    `align_runtime` units (the request-head parser + each of the five guards + serialize framing +
    fd-leak across N accept/respond cycles) + driver e2e (`m11_http_server.rs`: an Align server driven
    by a Rust client, **plus a dogfood run of the shipped Align `cl.get` client against the Align
    server**, plus the Gate-1 compile rejections). `cargo test --workspace` green (1718); clippy
    `-D warnings` clean; expr-depth driver test still 5/5. **Slice 5 (HTTPS/TLS) + `get_many` (R5)
    remain.**

## M12: align-LLM runway — offset I/O / typed accumulation / streaming / arena checkpoint — COMPLETE (2026-07-11)

The M12 set = the align-LLM runway A-list remainder (`open-questions.md` Open → "align-LLM
runway"; A1–A3 + A5's server half shipped pre-M12 as #399/#401/#402/#409). Items marked
*(general)* are ordinary fast-systems needs, not engine-specific. Both new Move types inherit
the standing v1 bind-to-local receiver rule (general unbound Move Drop landed later, on 2026-07-15).
**All slices resolved:**
A4 offset file I/O (#413), A6 `array_builder<T>` (#414), A7 streaming line reads (#415), A8
per-request arena reuse (#416, measured-below-gate record-and-close — the drop-the-re-zero
follow-up carries the 13.5× upper bound), A5-SSE `respond_stream`/`http_stream` (#417). With this,
**the align-LLM Phase-0→4 + gateway (Phase 7) language prerequisites are all in place** (GGUF
inspect, trace aggregation via read_line→array_builder, alignpack relayout via offset I/O, the
gateway's http server + get_many + client TLS + SSE streaming). `cargo test --workspace` 1813
green. **Next: M13** (codegen quality & link hygiene, the pre-LLVM-upgrade wave).

- **Slice A4 — `std.fs`/`std.io` offset-addressed file I/O — DONE (2026-07-11).** Shipped the
  new Move type **`file`** (`Ty::File`/`Scalar::File`): `fs.create_rw`/`fs.open_rw` →
  `Result<file, Error>`; `f.pread(b: mut buffer, off)` (buffer-window discipline mirrored from
  `reader.read`, actual count / 0=EOF), `f.pwrite(data, off)` (loops-to-full via `write_all_at`,
  past-EOF extends), `f.len()` (live fstat); negative offset aborts; Drop closes the fd; owned →
  Static, never region-tracked (full twin-mirror vs Reader/Writer). Constructors land in `std.fs`
  (import-gated), the type + methods are io-family handles (dispatched on receiver type like the cli
  precedent; `f.len()` via `check_len`). MIR/codegen file ops route through one `#[inline(never)]`
  dispatcher each (the #296 expr-depth frame lesson — the delegate arm takes `(b, e)` to stay
  frame-flat; verified expr-depth 5/5). Tests: driver `m12_file_io.rs` (create/pwrite-at-offsets/
  past-EOF-hole/pread-back/len/open_rw-missing-Err/negative-offset-abort + the move/print/==/import
  gates) + `align_runtime` units (pwrite/pread roundtrip, short-read-at-EOF, in-place update,
  fd-leak across N cycles). `cargo test --workspace` green; clippy `-D warnings` clean. Deferred:
  `copy_range`, `open_ro`, buffering.
  <br>Original settled design (2026-07-11, two-lens review): A new Move type **`file`** = the random-access block-WRITE handle with
  read-back; **no `seek` ever** (a settable cursor is hidden mutable state — every access takes
  an explicit `off`), and **no read-only constructor** (pure random reads stay reader | mmap
  `fs.read_bytes_view`; a third read path would break One-way — if a VA-constrained consumer
  ever needs non-mmap random reads, `fs.open_ro` is the recorded escape hatch,
  deferred-with-trigger). Surface: `fs.create_rw(path) -> Result<file, Error>`
  (O_RDWR|O_CREAT|O_TRUNC — the fresh-alignpack output) and `fs.open_rw(path)` (O_RDWR, must
  exist — in-place update), both O_CLOEXEC (the net/process fd discipline);
  `f.pread(b: mut buffer, off) -> Result<i64, Error>` (returns the ACTUAL count; 0 = EOF —
  the `reader.read` precedent; a file's length is not statically knowable, so no
  out-of-range abort, unlike `bytes.u32_le(off)`); `f.pwrite(data: bytes, off) ->
  Result<i64, Error>` (**loops to full internally** — a relayout must never silently
  short-write; the `write_all` precedent; past-EOF extends per POSIX); `f.len() ->
  Result<i64, Error>` (live fstat, not cached — your own pwrite changes it); Drop closes.
  Negative offset = **abort** (programmer bug). Deferred: `copy_range`
  (`copy_file_range`-class zero-copy fast path — the io.copy sendfile-dispatch pattern),
  read-only opens, any buffering. v1 is structurally single-threaded (Move → no par_map/spawn
  capture). *(general)*
- **Slice A6 — growable `array_builder<T>` → owned `array<T>` — DONE (shipped 2026-07-11;
  design SETTLED 2026-07-11, same review).** Shipped exactly to the settled record: new Move
  type `Ty::ArrayBuilder(Scalar)` (no `Scalar::ArrayBuilder` — never rides an aggregate);
  `array_builder()` constructor with the element type inferred from the binding annotation
  (`b: array_builder<i64> := array_builder()` — the `json.decode` context-inference precedent,
  no turbofish); `b.push(v)` / `b.append(xs: slice<T>)` on a `mut`-bound receiver (Pure);
  `b.build()` consumes into `array<T>` (zero-copy ptr+len retype). Storage is
  `align_rt_alloc`-family memory grown via the NEW `align_rt_realloc` (amortized doubling,
  `checked_mul` growth math); the runtime header `{data,len,cap,elem_size}` is a `Box`, `build`
  hands off `data` and frees only the header. Element set v1 = Copy scalars + `string`
  (`string` push MOVES the source in and nulls it; the builder's `Drop` deep-frees
  pushed-not-frozen strings via `align_rt_array_builder_free_strings`, the `free_string_array`
  class; scalar builders free flat via `align_rt_array_builder_free`). `append` is scalar-only
  (a borrowed `slice<string>` can't be bulk-moved — string elements use `push`). Twin-mirrored
  through every Move-handle pass (capture/move/drop/region/null-source/llvm-type); new
  ExprKinds/Rvalues routed through `#[inline(never)]` dispatchers in all five sema visitors +
  `lower_expr` + `gen_rvalue` (the #296 frame lesson — expr-depth 5/5 holds). 19 driver tests
  (`m12_array_builder.rs`) incl. both mandatory guardrails (builder-outside-loop survives
  per-iteration drops; capture into `spawn`/`par_map` rejected); `cargo test --workspace` = 1781
  green. `draft.md` §18.1 core.array_builder. **Deferred:** Copy-struct elements
  (struct-array store path unverified); `append` of `string` elements. The third member of the
  grow-then-freeze family (`builder`→`string`,
  `buffer`→bytes, now typed): a builder holds **no views**, so amortized realloc can never
  invalidate one — memory-safe by construction, which is exactly why growable `array<T>`
  itself was rejected. Surface: `array_builder<T>()`; `b.push(v)` / `b.append(xs: slice<T>)`
  on a `mut`-bound receiver (**Pure** — in-memory growth, the `BufferPut` class);
  `b.build() -> array<T>` consumes the builder (`.to_array()` rejected — it already means
  eager-materialize). **Freeze is zero-copy**: storage is `align_rt_alloc`-family memory grown
  via a NEW `align_rt_realloc` (amortized doubling), so `.build()` is a pure ptr+len retype
  (a Rust-`Vec`-backed store was rejected — its buffer cannot cross the allocator boundary to
  the C-free that frees `array<T>`). Element set v1 = **Copy scalars + `string`**
  (`array<string>` deep-drop is shipped end-to-end via read_dir; the builder's own Drop
  deep-frees pushed-not-frozen strings via the same helper); Copy structs deferred
  (struct-array store path unverified), Move handles excluded (the settled exclusion).
  Standard Move-handle exclusions (no Result/Option/array riding, print/== rejected).
  Mandatory tests: a builder declared outside a `loop` body survives per-iteration drops
  (#402 `body_locals` range); capture into `par_map`/`spawn` rejected (`ty_capture_is_move`).
  *(general — the natural `loop` accumulate-unknown-count output)*
- **Slice A7 — streaming line reads — DONE (2026-07-11).** Shipped exactly the settled record:
  `r.buffered()` (the read dual of the buffered writer — a per-local buffered-provenance set in sema
  statically enforces the buffered receiver, both stay `Ty::Reader`). **Known v1 limitation of that
  provenance set (gate-review finding): it marks only direct `let`-inits, so a rebind, an
  `if`/`match`-expression init, or a fn-returned buffered reader is over-REJECTED ("call
  `.buffered()` first") even though the value is buffered — a UX false-negative, never unsound
  (the runtime defensively upgrades any reader on `read_line` entry, so even a stale sema mark
  cannot corrupt).** `r.read_line(b: mut buffer)`
  (memchr-scanned, refill across boundaries, one `\r?\n` stripped, grows to a 64 MiB cap →
  `Error.Invalid`, `0` = EOF), the interleaving contract (a buffered `read` drains the lookahead
  first; unbuffered path byte-identical), and the generic `bytes.as_str()` validating region-bound
  view. **Recorded future form (2026-07-11 consultation):** a scoped zero-copy variant —
  `r.for_each_line(fn(line: str) { … })`, the line view valid only inside the callback (escape
  forbidden) — is the SAFE version of the rejected lookahead view; deferred until the copy cost is
  measured to matter. New runtime FFI: `align_rt_io_reader_buffered` / `_read_line` / `align_rt_bytes_as_str`; new
  HIR/Rvalues `ReaderBuffered`/`ReaderReadLine`/`BytesAsStr` registered through every sema pass +
  the MIR/codegen `#[inline(never)]` dispatchers (the #296 expr-depth budget held at 5/5). Consumer
  = the multi-GB `expert_trace.jsonl` per-line decode loop (align-LLM Phase 2); *(general — any
  log/JSONL/record processing)*.
  - **Prerequisite half: the buffered READER.** The shipped `reader` is a bare fd handle;
    line reading needs lookahead (bytes past the `\n` must survive to the next call), and
    hidden lookahead bolted onto the raw reader would silently corrupt `read`/`read_line`
    interleaving. So the lookahead is **explicitly constructed**: `r.buffered() -> reader` —
    the read dual of the settled buffered *writer* (mirror its surface exactly). On a buffered
    reader **every read drains the lookahead before touching the fd** (the interleaving
    contract); `read_line` on an unbuffered reader is a **sema error** ("call .buffered()
    first"). One-way + nothing-hidden both hold: you built the buffer, so it exists.
  - **`r.read_line(b: mut buffer) -> Result<i64, Error>`** (buffered receiver only): fills
    `b` with the **line body, terminator already stripped** (`b.len()` = body length — the
    strip lives in read_line, which alone knows where the `\n` was; no `line(n)` companion
    call, no `n-1` trap); **returns bytes consumed including the terminator, 0 = EOF** (an
    empty line returns 1 with body length 0 — unambiguous). A final unterminated line yields
    its body as-is and returns its bare length. **One memcpy per line** from the lookahead
    into the caller's buffer — a zero-copy view into the lookahead was REJECTED (invalidated
    by the next refill; the A6 view-invalidation class). The buffer **grows** as needed —
    unlike `r.read`, which caps at capacity: a line has no caller-chosen bound (state this
    asymmetry in draft.md §18.2) — up to a **64 MiB line cap** → `Error.Invalid` (bounds
    growth on a terminator-free/binary input; a *record*-sized cap, deliberately below the
    http 1 GiB *body* ceiling — each cap is sized to its thing).
  - **NEW generic boundary op `bytes.as_str() -> Result<str, Error>`** — the validating
    VIEW sibling of the settled `bytes.to_string()`; region-bound through the receiver chain
    (the #297 arm; a view of a buffer's bytes stays pinned to the buffer). This is the one
    bytes→text path; a bespoke line-view op on `buffer` was REJECTED (a second bytes→str
    path, and a text op leaking into the deliberately-binary container).
  - Canonical loop: `loop { n := r.read_line(buf)?; if n == 0 { break };
    line := buf.bytes().as_str()?; rec := json.decode(line)?; …array_builder… }`.
  - Documented edges: exactly one `\r?\n` is stripped; a lone `\r` (old-Mac) is not a
    terminator; a BOM is never stripped (no hidden transformation — json fails line 1;
    stripping is the consumer's call); a garbage line is an ordinary `Err` — skip-and-continue
    is already expressible with `match` instead of `?`; the per-iteration line view must not
    be hoisted across iterations (the next `read_line` invalidates it — currently uncaught,
    the recorded Borrow-liveness gap; one warning sentence in draft.md).
  - Recorded perf follow-up (not this slice): `json.decode` re-validates its already-invariant
    `str` input, so the JSONL hot loop validates each line twice (as_str + decode). After an
    audit that no unvalidated str-mint path exists (unsafe/FFI seams), decode may skip
    validation — one validation per line. Cheap either way (validation is memcpy-class).
  - Engineering notes: `read_line` is a buffer-MUTATING op (the push/put view-invalidation
    class — register it with their sema handling; a new mutator that skips that pass is the
    "new IR variant skips a pass" bug class); needs one new FFI write path (grow + set len);
    reader struct gains the lookahead fields behind the `buffered` flag.
- **Slice A8 — per-request arena reuse — MEASURED, BELOW GATE, RECORD-AND-CLOSE (2026-07-11).**
  The pool was built exactly to the settled design and benchmarked on the gateway shape; it came in
  at **~1.06×** over the pre-pool baseline — short of the **≥ ~1.15×** ship gate — so it was
  **reverted (not shipped)**. The shipped runtime is byte-identical to pre-A8. Measured matrix
  (Ryzen 9 5950X, 500k iters, best-of-15, `bench/arena_pool`): (a) pre-pool **555.8 ns/iter**;
  (b) pooled + re-zero **523.9 ns/iter** (1.06×); (c) pooled *no* re-zero **41.2 ns/iter** (13.5×);
  (d) `bumpalo` reset **19.2 ns**; (e) `malloc`/`free` **23.7 ns**. Diagnosis: the mandated re-zero
  is a full-64-KiB memset that costs ≈480 ns and dwarfs the ≈32 ns of malloc/free the pool removes —
  so pooling *with* the re-zero cannot clear the gate. (b) vs (c) shows the whole win lives in the
  recorded **drop-the-re-zero follow-up** (13.5× upper bound), which is a separate, provably-safe
  slice — not this one. The bench + `bench/arena_pool/README.md` are the durable negative record; the
  full feature-gated prototype (runtime pool + 7 unit tests) is preserved in this branch's git
  history for that follow-up. Original settled design (retained for the follow-up) below.

  Consumer = a long-running server loop resetting per request (the gateway);
  *(general — any flat-footprint request/event loop)*.
  - **The `checkpoint()`/`rollback(cp)` API is REJECTED for v1, with a precise reopen
    trigger.** Grounds: (1) it would be a second way — `loop { arena { …transients… } }`
    already composes two shipped constructs into the per-iteration bulk-free shape
    (empirically verified working; `break` sits outside the arena block, so the #402
    restriction never bites); (2) it is unsound without flow-sensitive epoch tracking (a
    view allocated after the checkpoint and used after rollback dangles — exactly the
    MIR-dataflow escape work still Open). Honest framing: the scoped block is not the full
    checkpoint pattern — it **restricts to the safe subset** (no survivors interleaved with
    rollback victims), and that restriction is what makes it sound with zero new machinery.
    The one shape it cannot express is **data-dependent checkpoint depth** (speculative /
    backtracking stream parsers — the Open entry's original citation); recursion expresses
    it, but the no-TCO guarantee makes deep backtracking fragile. **Reopen iff BOTH: the
    MIR-dataflow escape checker lands, AND a measured streaming-parser consumer appears
    that recursion + pooling cannot serve.** (The old "after MMv2" gate is moot — MMv2 is
    complete.) The flagship consumer has zero friction: requests are independent (no
    cross-iteration accumulator); durable state visibly lives outside the arena
    (Static / `array_builder`) — the aligned form anyway. No guide/spec doc teaches
    checkpoint-style thinking (grep-verified), so nothing rewrites.
  - **The missing piece is pure runtime (~30–50 LOC, no MIR/codegen change — begin/end
    symbols unchanged):** today every `arena_begin`/`end` pair mallocs AND memsets fresh
    fixed-size 64 KiB chunks (no doubling — so bumpalo's keep-largest-chunk does NOT
    transfer; the memset, ∝ CHUNK not request size and above glibc tcache, is the real
    per-iteration cost). Settled mechanism: a **`thread_local!` one-slot pool of the whole
    `Box<Arena>`** — `arena_end` runs `unmap_all` (mandatory, unchanged — `maps` are never
    pooled; every exit path still munmaps), resets `off = 0`, and stashes the Box (chunks +
    Vec capacities + the Box itself reused) **iff under the pool cap: only 64 KiB chunks are
    pooled, total ≤ a few MiB — an oversized single-alloc chunk (a 1 GiB body) is never
    pooled** (it would pin that memory per worker thread forever). `arena_begin` pops the
    slot. **Reused chunks are RE-ZEROED in v1** — fresh chunks' incidental zero-init has no
    LLVM-level contract but may be relied on; dropping the re-zero is a separate,
    provably-safe follow-up, not this slice. Thread-locality is sound: an arena block's
    begin/end are lexically paired (the MIR arena stack), and par_map thunks carry no arena
    handle, so a pooled chunk never crosses threads (this also preserves malloc's per-thread
    tcache locality). `align_rt_arena_reset` (exported, never emitted by codegen today)
    stays pool-agnostic.
  - **Measure-first gate (the benchmark-driven rule):** bench the gateway shape (KB-class
    per-iteration template/parse allocations, iterations amortized) across (a) main,
    (b) pooled + re-zero, (c) pooled no-re-zero (upper bound), (d) Rust `bumpalo` reset,
    (e) plain Rust malloc/free. **Ship iff (b) ≥ ~1.15× over (a); else record-and-close**
    (the builder-capacity / group_by-interleave negative-result precedent). Honest
    expectation: ~1.2–1.5× on the realistic shape — the recorded ≈3× belongs to
    alloc-dominated micro-loops, and must not be claimed for the gateway.
- **A5 remainder — SSE/chunked streaming response — DONE** (design SETTLED 2026-07-11, shipped;
  full record = http.md slice-plan item 7): `ctx.respond_stream(rb)` (header-only rb → abort,
  consumes both, auto-TE, single-sourced head serializer `http_serialize_head`) + Move `http_stream`
  (`send` one-frame-one-write via http_send_all; `send("")` = no-op; HTTP/1.0 → close-delimited raw
  mode via threaded version info) + **`finish()` as the sole clean terminator** (Drop = close-only —
  amends the original Drop-terminates commitment; poisoned flag on failed sends). Ships the new
  `Ty::HttpStream`/`Scalar::HttpStream` Move handle riding the `Result` Ok payload, `send`/`finish`
  bound-receiver methods, and the runtime `http_respond_stream`/`http_stream_send`/`_finish`/`_free`
  FFI. `cargo test` green (runtime frame-encoder/version/poison units + an end-to-end 1.1-chunked /
  1.0-raw / truncation / poison driver suite in `m12_http_stream.rs`).


## M13: Codegen quality & link hygiene — the pre-LLVM-upgrade wave (COMPLETE 2026-07-11)

**Formally closed 2026-07-11** — all slices merged with the standing slice-flow (#418 Slice 1
internalization, #419 Slice 2 capability linking, #420 Slice 3a optimized-IR + vectorize_shapes,
#421 Slice 3b explain-opt, #422 Slice 4 profiles + `alignc size`, #423 Slice 5 re-scoped
runtime-contract attrs, #424 Slice V verification bundle). The completion condition is met: the
shape/size/bench regression net (link_hygiene + capability_linking + vectorize_shapes incl. the
canonical-loop skeletons + target_cpu_isa + the storage-form audits + bench/binary_size +
bench/clang_ir_compare) is green — **it is now the LLVM-upgrade gate**. `cargo test --workspace`
1878 green at close. Next: the LLVM/inkwell upgrade checkpoint below.

Source: the 2026-07-11 external optimization consultation (adoption record in
`open-questions.md` Open → "External optimization consultation"). Three gaps were empirically
confirmed before planning (zero linkage settings in codegen; `emit-llvm` is pre-optimization only;
the unconditional `-lz -lzstd -lcrypto -lssl -lpthread -ldl -lm` link). Pre-release rules apply in
full: breaking interface changes are fine, no compat shims. **This wave deliberately lands BEFORE
the LLVM/inkwell upgrade** — its IR-shape tests, size benchmarks, and remarks tooling are the
regression net that validates the upgrade.

- **Slice 1 — symbol internalization + constant hygiene. DONE (#418, 2026-07-11).**
  Non-exported functions → `internal`; compiler-generated helpers → `private`;
  string/descriptor constants → `private unnamed_addr`. Unlocks LLVM
  IPO/DCE/inlining/`constmerge`. Both completion conditions met: the IR-shape test
  (`align_driver/tests/link_hygiene.rs`, 6 tests, mutation-checked) pins the linkage map — the C
  entry `main` (incl. the Result-main wrapper) is the SOLE external definition; `align_main` +
  all program fns + lifted lambdas = internal; the four thunk classes (`$fnval`/`$clos`/
  `tramp$R`/`$parthunk`) = private; runtime/`extern "C"` declares stay external (undefined);
  `@str`/`@jfields`/`@jphf` = private unnamed_addr constant (safe — string `==` is content
  compare via `align_rt_str_eq`, never address identity). Size smoke: `pipe.o` −33%, no
  regressions; O0 machine code unchanged (linkage is metadata). Key fact recorded at the
  decision site: Align has no separate compilation — one `Program` → one object, `pub` resolves
  at sema — so `emit-obj` output is a whole-program object, not a separately-linkable unit.
  Adversarial gate: SHIP, zero CONFIRMED findings.
- **Slice 2 — capability-based linking + link hygiene. DONE (#419, 2026-07-11).** The unconditional
  `-lz -lzstd -lcrypto -lssl` link is gone; those four are now GATED. MIR collects an
  `align_mir::Capability` (`Zlib`/`Zstd`/`Crypto`/`Tls`) from the builtin `Rvalue`s a program uses
  (`rvalue_capability` — a focused match; `CompressCompress/Decompress{kind}` → Zlib/Zstd,
  `CryptoHash/Hmac/Hkdf/Aead/Argon2` → Crypto, the four `HttpClient*` ops → Tls; `CryptoCtEqual`
  (const-time compare) and `CryptoRandom` (OS getrandom) map to nothing) and appends the required
  `-l<name>`s to `Program.link_libs` in `lower_program`, so the driver's existing per-lib loop links
  only what is used. **Collection point = MIR** (deliberate): MIR is the last stage before codegen
  where every builtin is a distinct total `Rvalue`, and `link_libs` already rides on the `Program`;
  the walk is a flat CFG scan (no recursion → no frame inflation). Fail-closed net: the driver links
  ONLY the collected libs, so an external-lib op added but not classified drops its lib and its
  `build_and_run` test fails to link — the `m11_compress`/`m11_crypto`/`m11_http` + new
  `capability_linking` suites are that net. **Runtime NOT split** — verified the ideal call: the
  runtime is one crate → one archive member (alloc + compress + crypto + http all in one `*.rcgu.o`),
  so member granularity does NOT isolate a feature; what does is `--gc-sections` over Rust's default
  per-function sections. Driver now passes `-Wl,--gc-sections -Wl,--as-needed` unconditionally
  (safe, no profile) and keeps `-lpthread -ldl -lm` always (Rust-std support the runtime *core*
  references independent of any Align feature; merged into libc on modern glibc so `--as-needed`
  makes them free; portable on older). **Capability collection and gc-sections are COUPLED**: a
  GNU ld quirk means once a candidate library on the link line resolves *some* of the single member's
  symbols, ld stops garbage-collecting the member's *other* external references — so `Crypto`/`Tls`
  transitively retain the compress libs (`Capability::link_libs` is a monotonic SUPERSET,
  `Tls ⊇ Crypto ⊇ {compress}`; always correct — `--as-needed` drops any truly-unused lib from
  `DT_NEEDED`). Completion conditions BOTH met: `capability_linking.rs` asserts (via `readelf -d`
  then; via `llvm-readobj --needed-libs` since the 2026-07-12 portability slice, which also made
  the ELF-only flag set format-selected — see the Codex work queue below)
  that `fn main() -> i32 = 0` AND `hello` link none of z/zstd/crypto/ssl while `gzip` keeps only
  `libz`; `bench/binary_size/` records before/after (fail-loud on build errors). Release numbers:
  `hello` 5.52 MB / 4 gated deps → 4.27 MB / **0** gated deps (−22.6 %), `gzip` → libz only,
  `https` → all four (correct). Full suite green (1829, +10 — the gate review added a binary-level
  crypto-superset pin and a gzip libzstd-absence assert). Adversarial gate: SHIP, zero defects
  (fail-closed proven by mutation — unmapping Compress fails `m11_compress` at link; server/net/SSE
  proven to reach no SSL/EVP symbol). gemini's link-order "high" was empirically disproven (shared
  libs resolve their own deps — a crypto-before-http program links fine) but its canonical
  dependent-first emission order (ssl → crypto → zstd → z) was applied on determinism +
  static-archive-robustness grounds. **Deferred (not blocking):** fine-grained per-C-library isolation for
  crypto/tls (→ `libcrypto` alone) needs a runtime-crate split by feature area — see
  `open-questions.md` Open → "Runtime staticlib feature-split". This entry supersedes the
  "always-linked" linking notes in `std-design/{compress,crypto,http}.md`.
- **Slice 3 — optimized-IR emission + remarks translation. Design SETTLED 2026-07-11** by a
  two-lens review (compiler-integration / user-surface+AI-loop), integrated record =
  **`docs/impl/09-explain-opt.md`** (the implementation source of truth). Split in two:
  - **Slice 3a — `emit-llvm --stage raw|optimized` + the vectorization IR-shape suite.
    DONE (#420, 2026-07-11).** Shared `run_opt_pipeline` extracted from `write_object`;
    `--stage raw` (default, byte-identical to the old output) | `optimized` (the `default<O2>`
    view); panic-free CLI. The 8-kernel suite (`vectorize_shapes.rs`, 12 tests, pinned
    `x86-64-v3` + `v2` variants, x86-64-gated) asserts presence AND absence on OPTIMIZED IR,
    2 mutation tests prove it reads optimized IR — **this suite is the LLVM-upgrade gate**.
    Pinned reality: map+sum / where+sum / where+min (`reduce.smin`) / map+reduce-mul /
    `map_into` / `.to_array()` vectorize; `scan` (loop-carried) and ordered-FP sum are the
    negative controls. **Empirical findings recorded in `09-explain-opt.md`:** k7 — `map_into`
    already vectorizes with ZERO `vector.memcheck` (scoped `!alias.scope` metadata present;
    mechanism vs inlined-alloca provenance not isolated) → **Slice 5's fn-level `noalias`
    re-scopes to cross-function/opaque-provenance cases**; k4 pinned over i64 (i32 slices not
    literal-constructible — DX note). Follow-up applied from review: one `TargetMachine` per
    compile (`build_module`/`write_object` take `&TargetMachine`). Gate: SHIP zero blocking
    defects; gemini's 2 mediums verified and applied. 1844 green (+15).
  - **Slice 3b — debug-loc anchoring + remarks capture + `alignc explain-opt` — DONE
    (#421, 2026-07-11).** Per-block `stmt_lines` MIR plumbing (populated only in located
    lowering — `lower_program_located`; a normal build is byte-identical) + opt-in inkwell DI
    emission (one `DIFile`/CU, one `DISubprogram` per fn, per-statement `set_current_debug_location`;
    the "Debug Info Version" module flag stamped manually — inkwell does NOT). Capture = the only
    C-API path: process-global `LLVMParseCommandLineOptions(-pass-remarks*)` behind `Once` +
    `LLVMContextSetDiagnosticHandler` (flat `file:line:col: message` strings; structured RemarkName
    needs a C++ shim, deferred). New verb `explain-opt` (report not build; exit 0 regardless of miss
    count; 1 on compile error / bad args), missed/actionable by default + one-line success summary +
    bucket count, `--verbose` for passed/raw `[llvm …]`/compiler-internal. Translation (keyed on the
    REAL LLVM-19 strings, `09-explain-opt.md`): loop-vectorize passed + missed (reason codes incl. a
    new `FpReorder`), slp passed → summary, inline passed → summary; inline MISSES → bucket (no
    lambda-inline-miss string exists to key on — every pipeline lambda inlines — so the actionable
    inline path is deferred, honest scoping). Honesty rule enforced (cost-model decline says only
    that). `Vec<OptRecord>` built first, rendered second — `--format json` / score / CI gates stay
    pure extensions. 1858 green (+14: 5 subprocess driver tests + 9 translation-table unit tests
    incl. the mutation-check); clippy `-D warnings` clean; the 3a `vectorize_shapes` sentinel and
    `emit-llvm` output byte-identical. Adversarial gate: SHIP zero confirmed defects
    (byte-identity proven vs main across 10 programs raw/optimized/obj; the `stmt_lines` parallel
    invariant structurally verified — single push site; handler lifetime traced incl. a probed
    double-invocation); its one robustness note (RAII detach guard for the diagnostic handler —
    unwind-safe by construction) applied pre-merge. gemini: zero findings. Full outcome +
    deviations in `09-explain-opt.md`.
- **Slice 4 — build profiles + `alignc size` — DONE (#422, 2026-07-11; adversarial gate SHIP —
  default-path invariance proven bit-for-bit vs main, all 4 test mutations caught; gemini 2
  applied / 1 rejected-with-reason; 1868 green).** `--profile dev/release/fast/small/tiny`
  selects the STOCK pipeline `default<O0|O2|O3|Os|Oz>` (no custom pass order — the consultation's
  "deliberately NOT a custom pipeline" clause) + the profile-dependent strip choice. The whole
  mechanism is **one enum**, `Profile` (in `align_codegen_llvm`, re-exported by the driver): it owns
  the pipeline string (`Profile::pipeline`, threaded through the Slice-3a `run_opt_pipeline`) *and*
  the linker strip decision (`Profile::strip`) — no scattered `match profile` ifs.
  - **Default = `release`** (= today's `default<O2>`): no behavior change without the flag. Exact
    names only, no aliases; a bad `--profile` is a diagnostic + exit 1, never a panic.
  - **Linker flags**: `--gc-sections`/`--as-needed` stay unconditional for **every** profile
    (correctness-neutral hygiene, not worth a second link path — even `dev` keeps them). `--strip-all`
    is applied only by the **size** profiles (`small`/`tiny`); the speed profiles (`dev`/`release`/
    `fast`) keep symbols so a crash backtrace / `perf` stays useful. (Pre-release, changeable.)
  - **explain-opt + `emit-llvm --stage optimized` stay pinned at `default<O2>`** — they are
    diagnostic *lenses* ("what release does"), not builds, so they do NOT take a profile (this also
    keeps the 3a `vectorize_shapes` gate on its exact O2 path). Documented at `run_opt_pipeline`.
  - **`alignc size <file.align> [--profile p]`** builds the source with the profile, then reports on
    the produced executable: total size, per-section sizes (largest first), the top-10 largest
    symbols, the relocation count, and the `DT_NEEDED` list. Input = an Align source (builds it), not
    a pre-built binary — `size` tracks the profile that made the artifact. Implemented by shelling to
    **binutils** (`readelf`/`nm` — already implicit toolchain deps via `cc`/`ld`) then; migrated to
    version-matched **`llvm-readobj`/`llvm-nm`** (both ELF and Mach-O) by the 2026-07-12 portability
    slice — see the Codex work queue below. No new crate dep;
    every tool call is failure-tolerant (a missing/erroring tool degrades one report block to a note).
  - **Tests** (`build_profiles.rs`, +9; +1 `group_digits` unit): the enum mapping pinned exactly
    (pipeline strings / strip set / exact-name parse / default = release); an in-process
    codegen-differs check (O0 vs O3 objects differ → the pipeline reaches LLVM); subprocess CLI tests
    for the `size` report shape, the `tiny` strip note, and the bad-`--profile` diagnostic. 1868 green
    (+10); clippy `-D warnings` clean; the 3a gate stays green on its pinned O2 path.
  - **Bench** (`bench/binary_size/profiles.sh`, + a `pipe.align` pipeline prog): per-profile size +
    stripped-state + gated-deps table. Representative (x86-64, release runtime): `hello`
    dev/release/fast ≈ 4,274,568 B (symbols) → small/tiny = 324,496 B (stripped); `pipe`
    ≈ 4,290,816 B → 336,784 B. The strip win dominates (the runtime staticlib's symbol/debug info);
    the O-level difference is negligible on these runtime-dominated programs. LLVM does NOT guarantee
    `Oz ≤ Os ≤ O2` byte-for-byte, so the table reports reality, asserts no fragile ordering.
- **Slice 5 — RE-SCOPED 2026-07-11 by a two-lens design review** (mechanics/soundness lens +
  payoff/sequencing lens, independently convergent). The original "big one" premise —
  per-argument attribute derivation + internal-ABI flattening — is **contradicted by the
  whole-program reality**: since Slice 1 every fn is internal, LLVM inlines hard at O2, and for
  the survivors (recursion, par thunks) **LLVM's FunctionAttrs already infers**
  `memory(none)`/`nocapture readonly`/`nonnull` etc. with zero codegen help (IR-verified:
  `fib` got `nofree nosync nounwind memory(none)` + fastcc; the `$parthunk` params got
  `nocapture readonly/writeonly` — all inferred). Aggregates are already passed as first-class
  by-value LLVM values that SROA/mem2reg scalarize immediately — hand-flattening measured a
  no-op. The k7 finding (fn-level `noalias` not the map_into unlock) generalizes. **What ships
  as Slice 5:**
  - **5A — runtime-declare contract attributes.** The one non-inline-redundant lever: LLVM
    cannot see the Rust bodies behind the ~250 opaque `align_rt_*` declares and those calls
    never inline. A hand-curated per-function attribute table (each entry reviewed against the
    actual runtime body — wrong = miscompile, that review IS the cost): `memory(...)`/`nofree`/
    `nosync`/`willreturn` on provably-pure-finite fns (`hash64/128`, `utf8_valid`, codec fns —
    the align-LLM hot path: a loop-invariant call becomes LICM-hoistable); `noreturn` on the
    abort-family decls (free hygiene — MIR already places `unreachable`); `nocapture`/
    `readonly`/`writeonly`/`nonnull` only where the contract is unambiguous. Fail-safe default =
    no attribute. NEVER `willreturn`/`mustprogress` on anything abortable. **A8-style gate: the
    effect-attr batch ships iff a probe shows a real LICM hoist / DCE / shape improvement;
    else record below-gate.**
  - **5B — regression-net additions.** Bool-storage (today: SSA `i1`) + alloca-in-entry audits
    as pinning tests, and ~3 canonical-loop-skeleton assertions folded INTO `vectorize_shapes`
    (single bounds check in the indexed body, one canonical induction phi) — a second suite
    would be duplicate coverage, so fold, don't add.
  - **Measurements to record while there:** the nsw/nuw scratch probe (hack `nsw` onto index
    adds locally, diff the shape suite; kernels already vectorize → expected below-gate).
  - **LANDED 2026-07-11 (#423; adversarial gate SHIP — every attribute entry independently
    re-verified against the runtime source, A8 test mutation-proven; gemini 2 mediums applied
    incl. a real fn_body test-helper anchoring bug; 1874 green).** 5A shipped as one
    table-driven mechanism (`rt_contract` +
    `apply_rt_contract_attrs` in `align_codegen_llvm`, applied to every `align_rt_*` declare;
    fail-safe = no attribute). Curated set: `memory(argmem: read)` + `willreturn`/`nofree`/`nosync`
    + `ptr nocapture readonly` on `hash64`/`hash128` and the `str_eq`/`str_cmp`/`eq_ignore_case`/
    `starts_with`/`ends_with` compare family (provably argument-memory-only). `utf8_valid` and the
    memchr-backed `str_contains`/`find`/`rfind` keep the pure-finite flags + `nocapture readonly`
    but **`memory(...)` WITHHELD** — their `is_x86_feature_detected!` / memchr dispatch reads+writes
    a process-global CPU-feature cache (non-argument memory), so any `argmem`/`read` claim would be
    unsound on the first call. `noreturn` on the six abort-family decls. **A8 gate: ABOVE-gate** —
    a loop-invariant `hash64("literal")` in a `map` mapper is hoisted out of the loop into the
    pre-header (computed once), which then lets the `+hash` map VECTORIZE (`<4 x i64>`); without the
    attributes the opaque in-loop call blocks both the hoist and vectorization. Pinned by
    `a8_hash64_loop_invariant_hoist_enables_vectorization` (`vectorize_shapes`) +
    `rt_contract_attrs_pin_encoding_and_curation` (which also pins the version-sensitive
    `MemoryEffects` bitmask via its textual form). 5B: `allocas_live_only_in_entry_blocks` +
    `bool_and_tag_storage_forms_are_pinned` (bool = `i1` in SSA and slot; `Result`/`Option` tag =
    `i8`; general user sum tag = `i32`) as codegen unit tests, and 3 canonical-loop-skeleton
    assertions + a canonical-induction-phi assertion folded into `vectorize_shapes`. nsw scratch
    probe: **below-gate confirmed** — hacking `nsw` onto the synthesized index add changes only the
    raw spelling (`add nsw i64`); the whole optimized shape suite (widths, `vector.body`,
    reductions, guards) is byte-for-byte unchanged (kernels already vectorize). Reverted.
  **DEFERRED with reasons (revisit post-M14 ThinLTO/runtime-bitcode — the wave that creates
  real non-inlined boundaries where argument attributes stop evaporating):** internal-ABI
  signature flattening (SROA already achieves it; FFI boundary correctly kept aggregate in the
  separate extern pass); type-derived per-program-fn param attributes (`readonly`/`nocapture`/
  `noalias`/`memory` — all need new analysis, all miscompile-if-wrong, all redundant with
  FunctionAttrs today); `AddProvenNoOverflow` nsw/nuw MIR distinction (no range analysis
  exists; generated and user-wrap arithmetic are the same `Rvalue::Bin` — a sound version is a
  large new pass for a benefit SCEV largely recovers post-inline, at the highest miscompile
  risk in the slice).
- **Slice V — verification bundle — DONE (#424, 2026-07-11; gate SHIP — ISA tests
  mutation-verified both directions, moot premise independently reproduced, harness honesty
  verified at both tiers; gemini's one high qualified-then-hardened: the shipped invocation
  shape survives — errexit is suppressed in the `$(facts ...)` substitution — but all four
  greps now carry `|| true` for invocation-shape safety; 1878 green).** (a) `BuildTarget::Cpu(name)` passes an
  empty feature string — objdump-verify the CPU name alone selects the right ISA per target, or
  fix; (b) cold-edge `!prof` weights on `?`/bounds/abort edges — MEASURE first, ship iff it wins
  (the A8 gate precedent); (c) the Clang-IR comparison harness (compile semantically-equal C with
  the same LLVM, diff optimized IR/assembly) for 3–5 core kernels. **Verdicts:**
  - **(a) empty feature string is CORRECT — no fix; PINNED.** `create_target_machine(cpu, "")`
    derives the ISA feature set from the CPU *name* itself (LLVM `getFeaturesForCPU`), so
    `x86-64-v3` enables AVX2 and the backend selects `ymm`/`vpaddq`; `x86-64-v2` stays SSE
    (`xmm`/`paddq`, no `ymm`); a named CPU (`skylake`) enables AVX2 too. `vectorize_shapes.rs`
    already pinned the *IR* widths (`<4 x i64>` at v3, `<2 x i64>` at v2); the new
    `target_cpu_isa.rs` pins the residual — actual **instruction selection** — via objdump,
    gated on x86-64 host + backend + `objdump` (skips cleanly otherwise).
  - **(b) cold-edge `!prof` — MOOT; below-gate; SHIPPED NOTHING.** Gate stated before measuring:
    ship iff a targeted microbench wins consistently >3% OR objdump shows a better hot-path
    layout. Finding: LLVM already infers the coldness. Bounds/abort/div branches have their
    taken-successor block end in `unreachable` (MIR emits it after the diverging call), and Slice
    5 marked the fail family `noreturn` — so BranchProbabilityInfo's unreachable/noreturn
    heuristics already assign the edge ~0 probability and MachineBlockPlacement already sinks the
    fail path off the hot fall-through. A prototype that attached explicit `!prof branch_weights`
    to exactly those branches produced **byte-for-byte identical machine code** across three
    kernel shapes (vectorized bounds loop, scalar gather with data-dependent index, div-checked
    loop) — a strictly stronger result than a wall-clock tie. Reverted; the mechanism that makes
    it moot (`noreturn` on the fail family) is already pinned by Slice 5's `rt_contract` test.
    (`?`-propagation edges are NOT cold — an `Err` is an ordinary value returned, not
    `unreachable` — so they correctly received no weight; the roadmap's grouping of `?` with
    bounds/abort was optimistic.)
  - **(c) harness SHIPPED at `bench/clang_ir_compare/`.** Five kernel pairs (map+sum, masked
    where+sum, map_into two-slice, hash-fold recurrence, scan negative control) compiled through
    the same LLVM 19 (Align `emit-llvm --stage optimized` vs `clang-19 -O2`, same
    `--target-cpu`/`-march`), diffing the load-bearing optimized-IR shape (vectorized? width?
    reduction intrinsic? memcheck?). **Baseline: all five MATCH** — Align's declarative pipeline
    lowers to the same optimized vector shape as idiomatic C. Divergences recorded (findings, not
    fixed): clang interleaves the vector loop more aggressively (same width/reduction, more ops
    per body — a throughput lead); clang `-O2` emits *numbered* blocks so the `vector.body`/
    `vector.memcheck` *strings* don't transfer cross-toolchain (Align keeps LLVM's named blocks —
    so `vectorize_shapes.rs`'s string assertions are Align-internal only; cross-toolchain signals
    must be semantic); and Align's `out dst` gives k3 its no-alias (no memcheck) for free at the
    type level where C needs `restrict`. A `clang_ir_compare.rs` smoke test runs the harness when
    clang-19 is present (skips otherwise).

**Completion condition:** all slices merged with the standing slice-flow; the shape/size/bench
regression net is green and becomes the LLVM-upgrade gate.

## LLVM/inkwell upgrade checkpoint (AFTER M13) — DONE 2026-07-12 (LLVM 19 → 22)

The standing mid-term note (recorded with the SVE/sme2 lean) says to schedule this before
targeting newer ISAs. Sequencing settled 2026-07-11: **M13 first** — its IR-shape suite,
vectorization shapes, size benchmarks, and ~1800-test corpus are exactly the net that makes the
version jump safe; **ThinLTO/bitcode work waits until AFTER** the upgrade (doing bitcode plumbing
on LLVM 19 and then jumping versions would redo the compatibility work). Scope: inkwell
`llvm19-1` → newest supported; re-verify the shared-only Debian linkage stance
(`LLVM_SYS_*_PREFER_DYNAMIC`); rerun the full net; re-measure the Slice-V cold-metadata and
vectorization shapes (pass-pipeline changes across versions are expected).

**Outcome (2026-07-12).** Upgraded to **LLVM 22** (inkwell `llvm22-1`, llvm-sys 221) from
apt.llvm.org. `cargo test --workspace` **1878 green** (unchanged from the M13-close baseline),
clippy clean, binary sizes byte-identical to LLVM 19. Linkage: kept `prefer-dynamic`
(`LLVM_SYS_221_PREFER_DYNAMIC=1`) — the shared-only-Debian rationale is obsolete (apt.llvm.org
llvm-22 ships static archives; Polly is no longer a `--libs` component) but dynamic is retained as
the smaller/rustc-matching choice. The M13 net caught exactly three shifts, all re-pinned to
IR-verified-equivalent shapes (no Align codegen regression):
1. **SCEV constant-folds reductions over constant arrays.** LLVM 22 folds a reduction over a
   compile-time-constant array indexed by a runtime-length prefix into a closed form (a `select`
   over boundary partial-sums) — strictly better, but the old `vectorize_shapes` / `target_cpu_isa`
   / clang-compare reduction kernels stopped exercising the vectorizer. Fix: seed the array *values*
   from `args.len()` so the data is genuinely opaque; under opaque data LLVM 22 vectorizes to the
   same width and same reduce intrinsic as LLVM 19.
2. **`vector.body` block name is unreliable** for an inlined reduction loop (renamed at v3, present
   at v2 and for materialize loops). The suite now keys reductions on the block-name- and
   init-noise-independent signals: the mangled reduce intrinsic (`llvm.vector.reduce.<op>.v<N>i64`)
   + `vector.ph`.
3. **`nocapture` → `captures(none)` param attribute.** LLVM 22 removed the `nocapture` parameter
   attribute in favour of `captures(...)`. The no-capture contract is now emitted **directly** as
   `captures(none)` (the `captures` kind id 92 + value 0, pinned against `llvm/Support/ModRef.h`) —
   see Codex audit item 9 (`docs/open-questions.md`). The A8 hoist+vectorize gate depends on the
   contract and passes; the `rt_contract` pin is the canonical `ptr readonly captures(none)`.

Known follow-up: ~~the `none` shorthand does not round-trip through `llvm-as-22`~~ **RESOLVED
(2026-07-13, Codex audit item 9).** The broken round-trip had a concrete cause: the pre-fix code
emitted the *removed* `nocapture` name, which `get_named_enum_kind_id` resolves to kind id `0` on
LLVM 22, so `create_enum_attribute(0, 0)` produced the bare, un-reparseable `ptr none` shorthand.
Emitting the modern `captures(none)` attribute directly makes the printer emit
`ptr readonly captures(none)`, which `llvm-as-22` accepts — proven by the tool-gated gate
`align_driver::tests/llvm_as_roundtrip::emitted_ir_round_trips_through_llvm_as` (feeds `alignc
emit-llvm` output to `llvm-as` and asserts it assembles). The textual `emit-llvm | llvm-as-22` dev
path is no longer broken; the M14 bitcode/ThinLTO boundary no longer inherits this as a caveat.

## Post-upgrade wave (M14 candidates, in order — bitcode compatibility dictates the sequence)

Original candidate order: ThinLTO → runtime-as-bitcode → instrument PGO → sample PGO / BOLT.
**RE-SCOPED 2026-07-12 by a two-lens design review** (architecture/feasibility + payoff/measurement),
run right after the LLVM 22 upgrade landed (#425). Three findings changed the shape:

- **"ThinLTO across Align modules" is MOOT today.** Align has no separate compilation — one
  `Program` → one whole-program module (the M13 Slice 1 record) — so the only cross-module
  boundary in any Align binary is Align ↔ runtime-staticlib. ThinLTO's summary/lazy-import
  machinery is unjustified at two modules; the right shape is full (monolithic) **in-process**
  LTO. Revisit ThinLTO once multi-module separate compilation exists — which is now PLANNED:
  see **M15** below (owner-mandated 2026-07-12).
- **The "Rust-vs-Align LLVM version alignment" wall dissolved with the 22 upgrade.** rustc 1.96
  emits LLVM **22.1.2** bitcode; the alignc toolchain is 22.1.8 — same major. Verified
  end-to-end: rustc `.bc` parses under `llvm-dis-22`, `llvm-link-22` merges it into Align IR,
  internalize + `default<O2/O3>` produces real cross-boundary constant-fold + DCE, and Rust std
  does NOT leak into the IR (only the `align_rt_*` wrapper defines + external declares such as
  `__rust_alloc`; std stays opaque machine code). The consultation's option (a) same-toolchain
  build is simply TRUE now; options (b) rewrite-core-in-C/Align and (c) stable-ABI boundary
  demote to fallbacks. Future skew guard: if rustc jumps to a newer LLVM major than inkwell's,
  the driver must fail LOUDLY back to the machine-code `.a` path, never silently miscompile.
- **The payoff surface is narrow (measured on optimized IR at v3).** The numeric data-oriented
  core already vectorizes with ZERO in-loop `align_rt_*` calls — LTO wins nothing there. The win
  surface is per-element string/hash primitives left as opaque calls inside hot loops
  (`str_eq`/`str_cmp`/`hash64` confirmed live in loop bodies; the trivial-wrapper set ≈
  {`str_eq`, `str_cmp`, `hash64/128`, `starts/ends_with`, `utf8_valid`, `bytes_as_str`}). The
  memchr-backed `find`/`contains` family and the group/compress/crypto/syscall families would
  NOT benefit (their loops live inside the runtime; inlining is pure bloat). Expected shape of
  the win: scalar constant-factor (call overhead + fast-path exposure), NOT vectorization.

**ThinLTO design SETTLED (2026-07-16, two-lens review: soundness/cache-key + mechanics/driver;
S0 spike GO the same day). This paragraph is the S1/S2/SV implementation source of truth.**
M15 shipped, so the 2026-07-12 "MOOT" verdict above is lifted; ThinLTO is the wave head again.
Settled decisions: **(a) mechanism = a 3-entry C++ shim** (`align_codegen_llvm/cpp/thinlto_shim.cpp`,
~300 LoC, `cc`-built by the crate's `build.rs` against the same LLVM 22 with a loud
`llvm-config-22` major assert): (1) pre-link — `buildThinLTOPreLinkDefaultPipeline` +
`buildModuleSummaryIndex` + `WriteBitcodeToFile(EmitSummaryIndex)` → summary-bearing per-unit
bitcode; (2) thin-link — `readModuleSummaryIndex` combine + `computeDeadSymbolsWithConstProp` +
`ComputeCrossModuleImport` → per-unit import lists `(src module, GUID, kind)`; (3) backend —
`FunctionImporter::importFunctions` + `buildThinLTODefaultPipeline` + object emission. A shim is
the ideal form, not a workaround: llvm-sys 221 structurally cannot emit module summaries
(`bit_writer.rs` has no `EmitSummaryIndex`) nor drive `FunctionImporter` (zero C surface), and
the legacy `ThinLTOCodeGenerator` C API it does expose SIGSEGVs on summary-less bitcode (S0
fork-probe evidence), hides per-unit import lists, and runs its own thread pool + cache —
incompatible with cache-first identity. **(b) Backend cache key = the PRECISE digest**: own
prelink-bc digest ⊕ inbound import list ⊕ the prelink-bc digests of import-source units. Sound
because thin-link always runs (measured ~70 µs at spike scale — the one cheap serial global
step) and computes fresh import lists; a dep private-body edit therefore misses exactly the
importing units' backends, preserving the M15 headline win where ThinLTO permits it. Determinism
pinned by canonical sorted ingestion (import decisions verified order-independent after edge
sort) + a build-twice byte-identity gate. [**S2 correction (2026-07-16):** the backend key also pins
this unit's OUTBOUND export set (its cross-module-referenced locals) — entry-3 promotes a unit's own
locals per its export flags, so an imported-FROM leaf still rewrites its object; keying inbound-only
would let a backend hit serve a stale-promotion object, caught by the cold-vs-hit byte-identity gate.
And the digest is NOT a field inside the shared codegen key: prelink and backend are separate phase
keys with their own CAS namespaces (`prelink`/`thinbackend`), which is why the M15 reserved
`cross_unit_opt_digest` field was removed outright at S2 — see "ThinLTO S2 SHIPPED".] **(c) Flag = opt-in `--thin-lto`**, legal only on
`release`/`fast` (the `--rt-lto` precedent), never folded into a profile in v1. **(d) N=1 skips
all three phases** → byte-identical to today; the flag-off path stays byte-identical.
**(e) Preserve set fail-closed in v1** = {`main`} ∪ `--export` ∪ all `pub` fns; cross-unit `pub`
internalization is a deferred follow-up win. **(f) `--rt-lto` composes**: its merge keeps the
pre-opt placement inside phase 1 and the attr-xor shed rule holds for merged bodies.
**(g) Artifact**: the CAS gains a `prelink-bitcode` part-kind at S2; thin-link output is NOT
cached in v1; no `InterfaceSummary` format change in this arc; `CACHE_KEY_FORMAT_VERSION` bumps
at S2 so old empty-digest entries fall out cleanly. **(h) Non-goals**: no full-LTO-over-N (at
most a measured stopgap, never the design), no linker-plugin LTO, `explain-opt` and
`emit-llvm --stage` stay per-unit-in-isolation (the honest zero-cross-unit-opt truth), no
profile-guided import thresholds (PGO territory, sequenced after). **S0 spike GO record
(2026-07-16, `thinlto-spike` feature, 6 ignored tests):** summary emission, per-unit import
lists, and cross-module inlining all proven in-process (the imported callee's relocation
disappears from the caller's object); the `cc`-built shim links cleanly against prefer-dynamic
`libLLVM-22.so` and coexists with llvm-sys in one process; inkwell `LLVMModuleRef` crosses the
FFI and `llvm::unwrap` recovers `Module*`. LLVM-22 frictions recorded: `GlobalValue::getGUID` →
`getGUIDAssumingExternalLinkage`; the `thinlto_*` C API lives in `libLTO.so` (only the rejected
minimal variant needs it); the combined index keys modules by MemoryBuffer identifier (S1 must
pass stable chosen ids); ThinLTO requires an explicit datalayout on inkwell-built modules.
Timing at spike scale: prelink ≈ 690 µs ×2, thin-link ≈ 70 µs, backend ≈ 2.2 ms. S1 shim
final form: thread the serialized import list into entry 3 instead of recomputing (the spike
shortcut), real `isPrevailing`/preserve policy from driver symbol visibility, and the driver's
own `TargetMachine`/cpu/features. **Slice plan: S1** = serial correctness behind `--thin-lto`
(gates: a cross-unit `pub` call inlined — IR-shape mutation-checked both directions; the M13
Slice-5 wide-tuple `sret` positive; N=1 byte-identity; multi-file run-parity corpus;
`--export`/preserve survival). **S2** = cache composition + parallelism (digest population, the
part-kind, `FirstDiff` phase split, invalidation-matrix rows, parallel == `-j 1` byte-identity).
**SV** = build-twice determinism, cold-vs-hit byte-identity through both phases, a
stale-summary fail-closed mutation, and an explicit compile-time regression bound.

**ThinLTO S1 SHIPPED (2026-07-16): serial cross-unit optimization behind `--thin-lto`.** The shim
is now a production component compiled into every `alignc` (libLTO stays spike-only); the driver
runs the three phases serially over private staging, `--thin-lto` is legal only on `release`/`fast`
+ `build`/`run`/`size` (loud rejection elsewhere), N=1 skips all phases (byte-identical, gated),
any shim failure aborts loudly naming phase+unit (no silent fallback), and the object cache is
BYPASSED under the flag until S2 integrates the precise digest. One recorded deviation from the
settled shim shape, correctness-forced: entry 2 reports imports AND exports, and entry 3 threads
both plus `thinLTOInternalizeAndPromoteInIndex` + an explicit `renameModuleForThinLTO` — importing
a fn that references its unit's private local (e.g. a string constant) requires consistent
promotion on both sides, and a leaf unit that imports nothing still must promote its own exports
(the `undefined str.llvm.<hash>` link failure and the reparse-identifier SIGSEGV are both pinned
by the diagnosis: bitcode reverts a module's identifier to the source filename, so the loader
restamps the stable unit id). Still zero thin-link recompute in backends. Gates green: cross-unit
inline mutation-checked both directions (nm/relocation), the M13 wide-tuple sret positive at 4/8
i64, N=1 byte-identity, run-parity corpus vs whole-program, `--export`/pub preserve survival,
profile/verb rejection, flag-off byte-determinism, and an extra build-twice determinism pin
(de-risks SV). Compile-time cost at corpus scale: milliseconds (prelink ~0.7 ms/unit, thin-link
~0.1 ms, backend 2-6 ms/unit). Next: S2 per the settled slice plan.

**ThinLTO S2 SHIPPED (2026-07-16): cache composition + parallelism.** A `--thin-lto` build now
composes with the M15 object cache as two cacheable phases per unit plus the one serial global step,
and the S1 object-cache BYPASS is removed outright (no-backward-compat). **Phase 1 prelink** (parallel
over misses, `CacheStage::ThinLtoPrelink`): a new `PrelinkKey` = today's codegen key MINUS the pure
backend/target knobs (cpu/features/reloc/code-model/machine-opt) — triple + object-format stay
(datalayout identity), everything else that can change the summary-bearing `.bc` is present
(impl_hash, transitive dep interface hashes, profile/pipeline, LLVM version, `--rt-lto` digest,
compiler build id). Cached artifact = the summary-bearing prelink `.bc` under the CAS `prelink`
action namespace. **Phase 2 thin-link** (serial, NEVER cached): runs every build over all units'
prelink bitcode (stable ids, DAG order, preserve set) → per-unit import edges + the global export set.
**Phase 3 backend** (parallel over misses, `CacheStage::ThinLtoBackend`): a new `BackendKey` = own
prelink-bc content digest ⊕ inbound import list `(src, GUID, kind)` ⊕ **this unit's outbound export
set** ⊕ the prelink-bc content digests of import-source units ⊕ the backend/target bits. The outbound
export set is a **correctness-forced refinement** of the settled "inbound import list" text: entry-3's
`thinLTOInternalizeAndPromoteInIndex` + `renameModuleForThinLTO` promote THIS unit's own locals per
its export flags (a leaf that is imported-FROM still rewrites its object), so a backend hit is only
provably valid if the key pins the unit's outbound exports too — proven by the cold-vs-hit
byte-identity gate. Cached artifact = the final object under the CAS `thinbackend` namespace.
`CACHE_KEY_FORMAT_VERSION` bumped 1→2 (old S3a entries fall out) and `MANIFEST_FORMAT_VERSION` 1→2
(the codegen-key wire layout changed). The M15-reserved `CodegenKey::cross_unit_opt_digest` field +
its `FirstDiff::CrossUnitOpt` reason were **removed outright** (no-dead-code): ThinLTO composes via
the separate phase keys/namespaces above, not by populating a shared-key digest, so the reserved
field's recorded "ThinLTO later populates it" story is corrected at doc-10 §6.2 and the two roadmap
paragraphs. `FirstDiff` instead gains `PrelinkInput` (own code changed) + `CrossUnitImports`
(inbound/outbound/import-source changed);
`CacheStage` extended with the two phases; `--cache-stats` prints per-phase per-unit lines + a
per-phase summary. Parallelism = the S3 `std::thread::scope` atomic-claim pattern (fresh `Context` per
prelink, fresh `Context`/`TargetMachine` per backend), one claim pass per phase with the serial
thin-link between; `-j`/`ALIGNC_JOBS` honored; N=1 skips ThinLTO and shares the ordinary object-cache
namespace (byte-identical). Fail-closed: a corrupt/unparseable cached prelink blob is evicted (loud
corruption note) + rebuilt; digest-verify on every part read; full-key equality checked before
materializing. Gates green (`thin_lto_cache.rs`, 8): headline private-body precision (edit C →
C prelink miss, B backend miss on CrossUnitImports, D imports-nothing-from-C hits both, main
transitive backend miss); pub-signature transitive prelink+backend miss; import-sensitive precision
(importer prelink hit + backend miss); toggle isolation (disjoint `codegen`/`prelink`/`thinbackend`
namespaces); cold-vs-hit object+exe byte-identity through both phases; parallel==`-j1` byte-identity;
cross-process second build all-hit; corrupted-prelink-blob rebuild. Flag-off + all existing S3a cache
gates stay green untouched (fresh temp roots); clippy `-D warnings` clean in both feature states;
workspace 2219 pass. SV runway remains (explicit compile-time-regression bound; stale-summary
mutation deepening).

**M14 Slice 1 (re-scoped): the LTO ceiling probe — measurement-first, A8-style.** Manually link
the runtime bitcode into the three confirmed kernels (str_eq-filter / str_cmp-filter /
hash64-map over ~1M short strings): `llvm-link-22` + internalize-to-main + one `default<O2>`,
bench against the shipped `.a` path, with a numeric `sum_sq_pos` kernel as the non-regression
control; also record the compile-time cost of link+reoptimize over the full runtime `.bc`.
**Gate = wall-clock ≥ 1.15× on at least one kernel.** Below gate → record-and-close items 1+2
together (mechanism + numbers recorded here; no driver infrastructure built — the #416
precedent). Above gate → **Slice 2 builds the real thing:** a small bitcode artifact for the
trivial-primitive set (built `codegen-units=1`), driver in-process `Module::link_in_module` +
internalize to the Slice-1 entry set + a single `default<O*>` run, behind an opt-in
flag/profile (never linker-plugin LTO — the driver-knows-everything structure stays);
`rt_contract` split — merged symbols SHED their curated attributes (FunctionAttrs infers from
the real bodies; a stale hand-curated attr shadowing a visible body is a latent miscompile),
opaque symbols keep them, strict per-symbol xor; `.bc` staleness folded into the existing
runtime staleness check; fail-loud fallback to the `.a` path on unparseable bitcode; gates =
in-loop call-absence IR shape (mutation-checked both directions) + `bench/binary_size` guard +
an explicit compile-time-regression bound.

**M14 Slice 1 probe RESULT — ABOVE GATE (2026-07-13; proceed to Slice 2, re-scoped below).**
Ran on WSL2 / AMD Zen 3 (znver3), LLVM 22.1.8 (`opt-22`/`llc-22`/`llvm-link-22`/`llvm-as-22`),
rustc 1.96 runtime bitcode (LLVM 22.1.x). **Round-trip repair confirmed:** `alignc emit-llvm
--stage optimized | llvm-as-22` round-trips cleanly (the #440 `captures(none)` fix), so the
straightforward pipeline was used with NO `ptr none` sed workaround. Kernels (no-`main` object,
`--export`ed, driven by a Rust harness over ~1M short strings, alternating-min timing, ratio =
shipped-`.a` / LTO, >1 = LTO faster): `eq_count` = `s.where(x == "hello").count`, `lt_count` =
`s.where(x < "mmmmmmmm").count` (`Ord(str)`/`str_cmp`), `hash64_sum` = `s.map(hash64).sum`, plus the
numeric `sum_sq_pos` non-regression control. Pipeline: `emit-llvm --stage optimized` (native) →
`llvm-as` → `llvm-link-22` with the release `align_runtime.bc` **and** `align_hash.bc` (so `wyhash`
is inlinable, giving `hash64` its best shot) → `opt-22 -passes="internalize,default<O2>"` keeping the
export set → `llc-22 -O2`. **Baseline = the shipped path** (native `emit-obj` kernels + the generic
release `libalign_runtime.a` — there is no `-Ctarget-cpu` anywhere, so the shipped runtime is generic
x86-64; the driver links exactly this `.a`). Compile-time cost of link+reoptimize+`llc` over the full
runtime `.bc` ≈ **0.25 s** (small). Median of 7 runs, N=1M, 300 rounds each:

```text
                LTO native (= real Slice-2)      LTO generic codegen (isolates pure
kernel          median  best   worst             LTO-visibility, no native tuning)
str_eq           2.119  2.140  2.095             2.353   ← ABOVE gate, robust
str_cmp          0.717  0.723  0.708             0.757   ← LTO REGRESSES it
hash64           1.631  1.642  1.624             1.017   ← win is native-tuning ONLY
sum_sq_pos       1.000  1.012  0.997             (n/a — generic de-vectorizes control)
```

**Verdict: gate cleared** (`str_eq` ≥ 1.15× robustly, 2.1×). Decomposition (the generic column
splits the two effects the LTO mechanism bundles — cross-module inlining vs native-recompiling the
runtime primitive): (a) **`str_eq` is a genuine LTO-visibility win** — IR-confirmed: `align_rt_str_eq`
inlines into the loop and the compile-time-constant target's length (5) folds into an inline `icmp
len, 5` fast path, so the majority (length≠5) elements are rejected with zero call/`bcmp`; it holds
even at generic codegen (2.35×). *Caveat: it leans on a constant-length target (the idiomatic `x ==
"literal"` filter); a runtime target keeps only call-overhead removal.* (b) **`hash64`'s win is purely
native-runtime-tuning** (1.63× native vs 1.02× generic) — `wyhash` stays real per-element work, so LTO
visibility buys nothing; this win is equally (and more cheaply) captured by shipping a per-target-cpu
runtime `.a`, no bitcode plumbing. (c) **`str_cmp` REGRESSES under the post-link `default<O2>`
(~0.72×)** — hard evidence that Slice 2's `rt_contract` per-symbol xor split + `binary_size`/IR-shape
gates are NOT optional: a blanket merge would ship this regression. (d) control `sum_sq_pos` = 1.000×
confirms the pipeline is non-regressing on the already-saturated numeric core (zero in-loop runtime
calls — nothing for LTO to do). **Consequent (Slice 2, re-scoped by this probe):** build it, but
scope the trivial-primitive bitcode set to the **inlinable fast-path string primitives** (`str_eq`
and its kin — `starts/ends_with`, `eq_ignore_case`, `utf8_valid`), **per-symbol guarded** so
`str_cmp`'s regression is excluded; do NOT chase `hash64` through LTO — pursue its native-tuning win
via the deferred **per-target-cpu runtime variant + cache key** (already parked on this slice), the
cheaper lever. PGO/BOLT sequencing unchanged.

**M14 Slice 2 design SETTLED (2026-07-14, two-lens review: soundness + build-integration).**

- **Guarded set v1 = the memcmp-class four:** `str_eq`, `starts_with`, `ends_with`,
  `eq_ignore_case`. **`utf8_valid` EXCLUDED from v1** — it is the sole candidate touching
  non-argument memory (the SIMD feature-detect global its `rt_contract` entry already withholds
  `memory` for), its body is a SIMD kernel whose inlining is bloat, and its win is unmeasured;
  add later only behind its own ≥ 1.15× bench. `str_cmp` stays out per the probe.
- **The minimal artifact is a CORRECTNESS requirement, not a size nicety.** Once a body is
  visible in the merged module, `default<O2>` inlines it regardless of `internal` linkage —
  linking the full runtime `.bc` would reintroduce the measured 0.72× `str_cmp` regression.
  The per-symbol guard is therefore realized *structurally*: only guarded bodies exist in the
  `.bc`; every other `align_rt_*` stays an external declare resolved from the `.a`.
- **One source of truth, compiled twice.** The four fns + their sole private callee
  `safe_slice` move to `crates/align_runtime/src/str_prims.rs` (`mod str_prims; pub use`),
  compiled (a) into the normal staticlib and (b) standalone to bitcode by an `align_driver`
  `build.rs` (`rustc --emit=llvm-bc -O -Ccodegen-units=1`, `rerun-if-changed`), **baked into
  `alignc` via `include_bytes!`** and parsed from an in-memory buffer at link time. Baking
  dissolves the staleness question (the same `cargo build` regenerates it; the same rustc
  builds both sides, so no LLVM-major skew for this artifact) — the earlier "fold `.bc` into
  the runtime staleness check" idea is moot under this shape.
- **Pipeline placement: link into the RAW module, then the ONE existing opt run.** The merge
  (parse + `link_in_module` + internalize + shed) slots between `build_module` and
  `write_object`'s single `run_opt_pipeline`; never a second optimization run (the probe's
  double-opt over already-optimized IR is exactly what regressed `str_cmp`). Post-link, every
  `align_rt_` symbol that now has a body is set `internal` **directly** (`mark_internal`),
  never via the internalize pass — the `{main} ∪ --export`-roots model stays untouched by
  construction, and no runtime symbol is externally defined in the merged module, so no
  duplicate-external vs the `.a` at final link.
- **Attr xor = "never curate what you'll merge" + a safety-net shed.** With the flag on,
  `apply_rt_contract_attrs` SKIPS the guarded set; a post-link pass strips exactly
  `rt_contract(name)`'s attrs from any body-carrying symbol. A blanket all-attr shed is
  rejected (it would also strip rustc's own body-derived attrs and *weaken* the result).
  Strict per-symbol xor pinned by a test: over all `align_rt_` fns,
  `(has body) != (carries its curated attrs)`.
- **Flag surface: explicit orthogonal `--rt-lto`** on `build`/`run`/`emit-obj`/`size` and
  `emit-llvm --stage optimized` (the observation lens for the gates); NOT folded into `fast`
  in v1 (the win is string-workload-specific and leans on constant-length literal targets;
  Nothing-hidden favors naming the mechanism). Rejected with a diagnostic on `dev` (O0 cannot
  inline) and `small`/`tiny` (the `optsize` sweep conflicts with fast-path inlining). Default
  (flag-off) path byte-identical; `explain-opt` and the default lenses never auto-enable it.
- **Fail-loud fallback:** unparseable baked bitcode → loud diagnostic naming the cause, fall
  back to the flag-off object+`.a` path **and re-annotate the guarded declares** (a fallback
  must not silently drop their curated contract).
- **Gates:** (1) IR-shape positive — an `x == "literal"` kernel: `call @align_rt_str_eq`
  absent under `--rt-lto`, present without; mutation-checked BOTH directions. (2) `str_cmp`
  negative — an `Ord(str)` kernel under `--rt-lto` still contains
  `call @align_rt_str_cmp` AND its declare keeps the curated attrs. (3) artifact symbol-set
  pin — defined `align_rt_` symbols in the `.bc` == the guarded set exactly; undefined ⊆ a
  small allowlist (`memcmp`/`bcmp`/…; no Rust-std leakage). (4) the attr-xor test. (5)
  `--export` + `--rt-lto`: exported symbol stays external, no `align_rt_` external define in
  the object. (6) OFF-path byte-identity: the existing IR-shape/lens suites unchanged. (7)
  end-to-end bench through the real driver (`alignc build --rt-lto`) + the `bench/binary_size`
  guard + an explicit compile-time bound (target ≤ ~100 ms over flag-off on a small program;
  the probe's 0.25 s was the FULL runtime `.bc`).
- **Spikes before the main work:** inkwell `MemoryBuffer`/`parse_bitcode_from_buffer`/
  `link_in_module` under `llvm22-1` (unused in the workspace today); the `build.rs`-invokes-
  rustc standalone compile (str_prims must be dependency-free — it is: slice ops only);
  triple/datalayout mismatch on `link_in_module` (clear the `.bc` module's triple/datalayout
  before linking if LLVM objects).

**M14 Slice 2 SHIPPED (2026-07-14, #443).** Implemented exactly per the settlement above; all
seven gates landed (`crates/align_driver/tests/rt_lto.rs`, 9 tests incl. the gate-review
additions). End-to-end through the real driver: **`eq_count` 2.95× under `--rt-lto`**
(identical hit counts ON/OFF), numeric control 1.01×, compile-time delta **+2 ms**
(bound ≤ ~100 ms), flag-off objects proven byte-identical to pre-change main, `bench/rt_lto/`
records the numbers, `bench/binary_size` unaffected. Mutation teeth verified both directions
twice (implementer + independent adversarial gate; gate verdict SHIP, zero confirmed defects).
Three recorded deviations from the settlement, all verified: (a) inkwell 0.9's
`MemoryBuffer::create_from_memory_range[_copy]` asserts a trailing nul and passes `len-1` to
LLVM — a raw `include_bytes!` slice would lose its last bitcode byte, so the parse path
nul-appends into a temp `Vec` (confirmed against inkwell source); (b) `-Cpanic=abort` on the
artifact compile drops `rust_eh_personality`, leaving `bcmp` the sole undefined symbol;
(c) `emit-llvm --stage raw --rt-lto` also links (the pre-opt lens gate 4 needs). Gate
follow-ups shipped in the same PR: the unparseable-bitcode fallback + re-annotation is
regression-pinned (drives `emit_llvm_ir` with garbage bytes through the public `Option` seam),
the dev-profile/non-build-verb rejections have negative CLI tests, and the datalayout
force-overwrite became a fail-loud guard (mismatch → loud diagnostic + fallback + re-annotate;
today's layouts are identical so no current-path change). gemini review reflected pre-merge
(3/3 applied: `--target $TARGET` on the build.rs bitcode compile for cross-compiled alignc;
portable `now_ms()` replacing GNU `date +%s%N` in `bench/rt_lto`; underscore-tolerant symbol
matching in the `.bc` pin). Still parked on this slice: the **per-target-cpu runtime variant +
cache key** (the `hash64` native-tuning lever, deferred with the doc-10 §2 key spec) and
`utf8_valid` behind its own ≥ 1.15× bench.

**PGO stays sequenced after — do NOT reorder it ahead on "LTO is thin" grounds:** its
block-layout/hot-cold-split win is already MOOT per M13 Slice V (`noreturn` attrs make fail
edges cold; a real `!prof` prototype was byte-identical and reverted); instrument PGO is
mid-size infrastructure (InstrProfiling pass + profile runtime hook + an `llvm-profdata` merge
stage + `PGOOptions` likely via raw llvm-sys — inkwell 0.9 does not expose it); its unique
lever, hotness-gated multiversioning, is a separate unimplemented feature. Sample PGO / BOLT
unchanged (evaluate later, driver-managed external pipeline).

**Codex binary-optimization audit (2026-07-12) — adopted work queue.** The owner's external
audit (pre-#425 HEAD; full triage = `open-questions.md` → "External binary-optimization audit
(Codex, 2026-07-12) — adoption record") queues code waves that are independent of the LTO probe
and M15: **wave 1, measurement portability** (three CONFIRMED bugs — bench export roots broken
by the #418 internalization **[DONE 2026-07-13: `emit-obj`/`emit-llvm --export <name>` export-
roots mechanism, fail-closed against the lowered MIR; `bench/run.sh` + every sub-bench updated
and re-verified — full record in `open-questions.md` item 1]**, ELF-only linker/size tooling
breaks macOS builds **[compiler slice DONE 2026-07-12: `ObjectFormat`-selected linker policy +
`llvm-readobj`/`llvm-nm` size report + macOS regression net; the `bench/binary_size` script port
DONE 2026-07-13: shared `lib.sh` (`filesize`/`llvm_tool`/`gated`/`stripped`), no more `stat -c`/
`mapfile`/`readelf` — full record in `open-questions.md` item 2 sub-item (a)]**, build profiles
never reach the TargetMachine/`optsize`/runtime
variant **[code slice DONE 2026-07-13: `Profile::codegen_opt_level` threaded into the
TargetMachine + small/tiny `optsize`/`minsize` definition-only fn-attr sweep, diagnostic lenses
pinned to codegen=Default/no-attrs so the IR-shape suite stays byte-identical, release object
bit-for-bit unchanged; the per-profile runtime variant + cache key is deferred to the M14
runtime-bitcode slice + doc-10 §2 cache layer]**); **wave 2, quick wins** (O(n²)
`sort`/`sort_by_key` **[DONE 2026-07-13: stable bottom-up merge sort + insertion base case +
decorate-sort-undecorate `sort_by_key` (keys computed once), all in MIR — record in
`open-questions.md` item 4]**, tiny-`par_map` pool-before-threshold cold start, zero-size arena 64 KiB
chunk, attribute-kind fail-loud + modern `captures(none)` emission **[DONE 2026-07-13: fail-loud
`enum_kind_id` panic + `captures(none)` via the `captures` kind; the `llvm-as-22` textual
round-trip follow-up above is RESOLVED — tool-gated gate added]**); **wave 3, measure-first** (JSON
decode double-allocation **[EXACT-COUNT SCALAR PATH REJECTED 2026-07-16: count+direct was
0.71-0.73x at 1K-1M elements; retain one-pass staging]**, I/O buffer zero-fill). Doc-debt items ride
along with their slices.

## M15 — Separate compilation (multi-module compilation units) — COMPLETE 2026-07-15

**Directive.** On reading the M14 re-scope note "Align has no separate compilation — one
`Program` → one whole-program module", the owner ruled this must not remain true: multi-module
compilation is REQUIRED and belongs on the near-term roadmap. To be precise about today's state:
*language-level* modules already exist (`import`, `pub`, module files, the `std.*` tree) — what
is single is the **compilation unit**: the driver compiles the whole program as one `Program`
and emits one object; every build is a full rebuild.

**Why it matters** (beyond the directive): build scalability and incremental compilation (any
real codebase — align-LLM included — recompiles everything on every edit today), compiled-library
distribution for the future pkg ecosystem, and CI/agent iteration latency (four-way alignment:
the Compiler axis includes fast feedback).

**Cache-first companion record (2026-07-12):** `10-cache-first-optimization.md` is the detailed
source for artifact identity, publication, invalidation, and the newly found output/runtime cache
candidates. Its P0 basename-derived shared temporary artifact defect is fixed as of 2026-07-13:
objects and transient executables use private staging, and requested executables publish by atomic
same-directory rename. The future complete content key and CAS must build on that invariant before
M15 compiles units in parallel. The same
record pins the already-promising reproducibility baseline and the cache validation matrix; do not
build a throwaway mtime cache that M15 immediately replaces.

**Design questions to settle FIRST (a two-lens design review before any code; record the
settlement here + `open-questions.md`):**
1. **Unit boundary** — module, module subtree, or package? What is the stable build artifact
   (object + interface summary? bitcode, anticipating ThinLTO)? Where does the unit graph live
   (driver-discovered vs manifest)?
2. **Cross-unit inference boundaries** — escape/region inference, purity/effect inference, and
   MoveCheck are whole-program analyses today. Unit interfaces must carry summaries (effect
   bits, region-bearing signatures, Move/Copy classification) or make conservative assumptions;
   this must not silently weaken soundness (the fail-open-wildcard bug class — every gate the
   audits closed assumed whole-program visibility).
3. **Generics across units** — monomorphization strategy: instantiate-in-consumer (needs body
   availability → IR/bitcode in the artifact) vs pre-instantiated exports (closed instantiation
   sets). Interacts with the "generic fn over generic struct unsupported" gap.
4. **M13 interactions** — Slice 1 internalization is "safe by construction" ONLY under
   one-object whole-program; separate compilation needs a real symbol-visibility model
   (`pub`-driven export sets). Capability-based linking must collect per-unit capability sets
   and union them at final link. `emit-obj`'s "whole-program object" contract changes.
5. **ThinLTO un-moots** — with N Align units, the M14 item-1 machinery becomes the natural
   cross-unit optimizer; the runtime-bitcode plumbing from the M14 probe/Slice-2 is the same
   substrate. Sequence ThinLTO *after* unit boundaries exist.
6. **Incremental driver** — per-unit staleness/caching, parallel unit compilation, and how
   `alignc build/size/explain-opt` surfaces multi-unit builds. Settle content/action identity,
   interface-vs-implementation hashes, exact compiler/LLVM/target/profile keys, runtime/link
   digests, atomic publication, corruption recovery, and hit/miss reasons per
   `10-cache-first-optimization.md`; "mtime is older/newer" is not a sufficient cache contract.

**Sequencing:** design review can start right after the M14 Slice 1 probe verdict; M14's
remaining items (PGO/BOLT evaluation) are independent and may be reordered around it at the
owner's discretion. The cache record's C0 artifact-correctness slice is independent of that verdict
and may land immediately; it is mandatory before M15 parallel compilation.

**M15 design SETTLED (2026-07-14, two-lens review: language/soundness + driver/artifacts/cache;
implementation NOT started).** Per-question decisions:

1. **Unit = one module (one `.align` file).** Already the boundary for namespaces, visibility,
   and mangling (`module$name`); subtree/package units would invent an aggregation concept the
   compiler has no representation for (pkg-layer, deferred). **Unit graph = driver-discovered
   from the existing BFS import walk; NO manifest file** (a second source of truth that can
   drift). **One new language RULE, no new syntax: cyclic imports become a hard error** —
   tolerated today by the BFS `seen`-dedup collapse; the unit DAG is required for bottom-up
   effect-bit computation and incremental/parallel ordering. Update `draft.md` §17.
2. **The unit interface is COMPLETE — the headline soundness result.** Restriction-driven:
   three of the four whole-program analyses need NO body summary. Escape/region is already
   body-blind (a call's result region = conservative fold of the caller's own argument
   regions; sound cross-unit because Align has no hidden escape channel — no mutable globals,
   escapes only via arg-regions/return; GUARD recorded: any future mutable-global feature
   forces a stores-arg-i summary). Move/Copy is a pure function of type definitions. MoveCheck's
   uniform by-value-move ABI is caller-derivable from types alone. The ONE genuinely
   whole-program analysis — purity/effect — reduces to a **3-valued per-`pub`-fn effect bit
   (Pure / Impure / Unknown)** computed bottom-up over the unit DAG from dependencies' already-
   emitted interfaces; **fail-CLOSED: missing or Unknown ⇒ Impure + unknown-indirect ⇒ rejected
   at `par_map`/parallel boundaries** (the exact shape of the shipped #433 unknown-HOF
   rejection; the audited fail-open-wildcard class is designed out). Interface contents =
   exported signatures **incl. `out[i]` markers** + FULL exported type/enum/tuple definitions
   (fields/payloads incl. `align(N)`/`layout(C)`) + const values + effect bits + generic `pub`
   template bodies + the unit's capability set.
3. **Generics: instantiate-in-consumer** — today's monomorphizer already IS one (templates as
   AST, worklist + `mono_args` in sema; MIR asserts no `Ty::Param` survives). The producer
   serializes generic `pub` template ASTs into the interface; consumers instantiate with the
   EXISTING machinery, emitting monomorphs into their own object as **`internal`** symbols.
   **v1 accepts duplicate monomorphs across consumer objects** (correct under internal
   linkage; bounded bloat) — cross-unit dedup via `linkonce_odr` + stable deterministic
   mangling is the recorded deferral, natural at the ThinLTO boundary. Honest cost, accepted:
   a generic `pub` fn's body is part of its interface (C++-template-like) — editing it
   invalidates consumers. The pre-existing generic-over-generic gap is orthogonal; deferred.
4. **Symbol visibility replaces "internalize is safe by construction":** external =
   `{main} ∪ CLI --export ∪ each unit's pub fns` (globally mangled, collision-free);
   internal/private = private fns, lifted thunks, consumer-side monomorphs. Whole-build
   internalization of never-imported `pub` fns is deferred (v1 relies on the always-on
   gc-sections/as-needed). Capabilities: per-unit `link_libs` unioned deterministically at
   final link (the same monotonic-superset soundness as today's single-TU set). `emit-obj`'s
   contract becomes per-unit object + interface. **`extern "C"` export-of-body stays OUT of
   M15** — the `map_into`/out-param noalias trust chain (sema's own separate-compilation
   warning) requires every caller to be Align-checked; import-only FFI is unaffected.
5. **Cross-unit optimization: v1 ships NONE.** When it lands (post-v1) it is real ThinLTO
   (summary-based, parallel — it COMPOSES with incrementality), NOT full-LTO-over-N
   (re-monolithizes and kills the incrementality M15 buys; at most a measured opt-in stopgap).
   The artifact anticipates ThinLTO with no format break: reserved (v1-absent) bitcode +
   thin-summary envelope parts, plus an explicitly-keyed **cross-unit-opt input digest in the
   codegen key that is EMPTY in v1** (ThinLTO later populates it, deliberately trading hit
   rate for cross-module inlining). [**S2 correction (2026-07-16):** ThinLTO did NOT populate this
   reserved field — it composes via separate `prelink`/`thinbackend` phase keys + CAS namespaces
   (structurally stronger toggle isolation), so the empty `cross_unit_opt_digest` field was removed
   outright at S2. See "ThinLTO S2 SHIPPED".] **`--rt-lto` under multi-unit v1 merges the baked bitcode
   per unit** into each unit's raw module before that unit's single opt run (hot loops live in
   arbitrary units; merged bodies are internal and DCE'd post-inline, so duplication is
   negligible — the "merge once" shape returns when a final-link opt run exists).
6. **Incremental driver on the doc-10 contract** (never mtime). Per-unit artifact = a keyed,
   extensible envelope of CAS-addressed parts: interface summary (**interface hash**),
   implementation object (**impl hash**), link summary (capabilities + export roots).
   Consumers depend on interface hashes ONLY → a private-body edit recompiles that unit +
   relinks, all dependents hit (THE headline win). The interface hash INCLUDES effect bits and
   generic template bodies — the two places an "implementation-only" edit is actually an
   interface change; both lenses independently flagged this as the most likely place to get
   the invalidation boundary wrong. Action keys: frontend = unit source hash · **transitive**-dep
   interface hashes (corrected 2026-07-15 by the S3 design review — this line originally said
   "direct-dep", a stale-check hole as written: interface hashes do NOT chain, foreign type refs
   are rendered by name, so a transitive-only dep's `pub` change must invalidate the top consumer;
   the shipped `PerUnitCheck` already records the transitive set, per the S1b gate (2))
   · builtin schema · frontend schema id; codegen = doc-10 §6.2 per unit
   (+ rt-lto bitcode digest + the empty-in-v1 cross-unit-opt digest); link = doc-10 §6.3
   (ordered object digests · runtime CONTENT digest replacing the mtime check · format/flags/
   linker identity · deterministic capability union). Atomic publication extends the shipped
   C0 private-staging per unit; corruption recovery = digest-verify on read, evict + rebuild;
   structured hit/miss with first-differing-key-component reasons from slice 1 (tests assert
   invalidation, not elapsed time). **Parallel unit compilation is UNBLOCKED** (verified:
   codegen creates a fresh LLVM `Context` per entry, C0 is shipped; the only process-global is
   explain-opt's remark cl::opts — that verb stays serial). CLI: `build`/`run` merged (+
   optional per-unit hit/miss summary); `size` merged with optional per-unit breakdown;
   `emit-llvm` per-unit (= the truth with no cross-unit opt); `explain-opt` per-unit in its
   own located-MIR namespace.

**Migration: hard cutover, no dual path — separate compilation at N=1 IS whole-program
compilation** (a single-file program → one unit → one object, byte-identical to today; this
protects the reproducibility baseline and the rt-lto byte-identity gates, and satisfies the
no-back-compat rule with a single code path). **Recorded honest trade (owner-visible):**
multi-file programs LOSE cross-module inlining in v1 — today every file merges into one module
and inlines freely; v1 makes cross-unit `pub` calls opaque at unit boundaries. Single-file
programs are unaffected. ThinLTO restores the optimization later, and the format is built so
it can. Consistent with the owner rubric: the goal was chosen deliberately, and the format
does not block the optimizer's return.

**M15 slice plan:** **S0** cyclic-import hard error + `draft.md` §17 no-cycles rule (tiny,
shared prerequisite) — **SHIPPED 2026-07-14 as #444** (white/grey/black DFS over
dedup-independent import edges; diamond reconvergence pinned legal; 5 tests, 1945 green). **S1** interface summary — canonical serialization (no map-iteration
order, no process-local ids) + interface/impl hashes + per-unit sema against imported
summaries (effect bits, type defs, template ASTs). Split in implementation: **S1a (producer)
SHIPPED 2026-07-14 as #445** — new crate `align_interface` (no codegen dep): per-unit
`InterfaceSummary` (signatures incl. `out[i]`, full exported type defs, consts, the 3-valued
effect bit taken from the SAME `compute_effect_sets` the parallel gates use — single source,
no drift; generic template bodies as source text; capabilities as data, deliberately outside
the interface hash), hand-rolled versioned fail-closed codec (fuzzed panic-free incl.
allocation-bomb prefixes), `interface_hash` includes effect bits + template bodies,
`impl_hash` = source bytes with `TODO(m15-s2)` → per-unit MIR; driver `emit-interface` verb;
12 tests incl. the 5 hash-split gates; adversarial gate SHIP. **S1b (consumer) had TWO entry
gates recorded by the S1a review: (1) MANDATORY sema rejection of a `pub` signature exposing a
non-`pub` type** — a private type is summarized by name only, so its layout change would NOT
flip the interface hash → stale-object miscompile once consumers exist. **Gate (1) SHIPPED
2026-07-14 as #446**: sema pass 0a-2 walks every `pub` fn signature / struct field / sum
payload, `check_type_exposure` recurses exhaustively over all `ast::Type` constructors (no
wildcard) incl. generic args of qualified types; cross-module private access was already
rejected so the check is same-module-only; `pub const` (scalar/`str`-only) and `extern`
(FFI-scalar-only) are structurally exempt, documented + tested; 12 tests; 8 existing SoA
fixtures had used exactly this hole (private struct through `pub fn` `soa<T>`) and were
corrected — the rule bites on real code. `draft.md` §17 + the language-spec digest carry the
rule; the S1a "known finding" note is flipped to an ENFORCED invariant. **(2) still standing
for S1b proper: consumers must key on the TRANSITIVE set of imported units' interface hashes**
(foreign type references are by-name in the canonical surface). **S1b (consumer) SHIPPED
2026-07-14**: per-unit sema against imported summaries. Seam = **summary→source→re-parse** (NOT a
second resolver): an imported unit's public surface is rendered back to Align source
(`align_interface::summary_to_source`) and re-parsed by the EXISTING parser into an interface-only
`Module` (`Module::interface_only`), so every sema table-building + resolution pass is reused unchanged
— ONE resolution path (generic templates and const values must be re-parsed regardless, so
render-to-source unifies the whole reconstruction). Cross-unit effect bits seed
`compute_effect_sets`/`fn_effects`/`check_parallelism` via the new `check_program_with_effects`; a
callee absent from the seed map is **fail-closed** to impure + unknown-indirect (never optimistically
Pure). Driver `check_per_unit` walks the DAG bottom-up (topo post-order), reconstructs each transitive
dep from its summary, checks each unit, re-derives its own summary, and records the **transitive
(unit, interface_hash) dependency set** per unit — the S3 cache-key input (`PerUnitCheck`; dev verb
`alignc check-per-unit`). Gates (25 new tests): differential accept/reject parity vs whole-program
`check_program` over a 20+-case corpus (cross fns/types/sums/consts/generics/effects/Move-structs +
negatives); blindness (a dep private-body edit leaves a dependent's verdict + interface hash + dep-hash
set identical); effect fail-closed (Pure accepts / Impure·Unknown·ABSENT reject at `par_map`;
sequential impure imports stay legal); cross-unit generics instantiated in-consumer with effects
recomputed on instantiation; the A→B→C transitive-hash test (a C `pub`-signature change flips main's
dep set; a C private-body OR non-generic-`pub`-fn-body edit does not); N=1 whole-program path
byte-identical (`check_program` delegates to `check_program_with_effects` with an empty seed map).
Honest remainder: per-unit summary production runs per-unit MIR lowering for capabilities only
(capabilities are outside the interface hash, so this does not affect any S1b gate); the
private-cross-unit-access diagnostic differs by design ("unknown" per-unit vs "private" whole-program —
verdict identical). **MERGED as #447 (2026-07-14)** after the adversarial gate (SHIP-with-fixes)
and gemini. The gate verified the render-to-source seam FAITHFUL end-to-end (`layout(C)`/
`align(N)`/field order/`out[i]`/Move classification/const escaping all round-trip; const
"injection" content stays a string value; no external summary-loading surface exists this
slice) and proved the whole-program path unaffected (every callee is in `program.fns`, so the
effect-seeding loop is a no-op). Its one confirmed defect (D1) drove a **new language rule
extending #446: a generic `pub` fn's body may reference only `pub` items** — a private
same-module fn/type/const in a template body is rejected at the definition (sema pass 0e,
exhaustive expr/stmt/type/pattern walker with a lexical scope stack — locals shadow item
names; runs in `check_program_with_effects`, so both checkers agree by construction; the
`<interface:…>` synthesized-location leak is gone; match-pattern variant names deliberately
not checked — a private-enum value cannot reach a `match` except through already-rejected
construction/signature paths). No existing fixture used the hole. gemini's finding applied:
parsed interface ASTs are cached — each dep is rendered with its OWN transitive closure
(importer-independent, hence soundly cacheable) and parsed once per walk (O(N²) → O(N)).
Final: **2006 green** (1969 + 37), clippy clean. Pre-existing bug surfaced by the gate
(NOT S1b, reproduces on untouched whole-program `check`): a by-name fn-value reference
(`map(dbl)`) fails with "undefined function" inside a NON-ENTRY module (direct calls and
`map_into` work) — recorded in `open-questions.md` Open. **S2**
per-unit codegen + N-object link
(visibility model, capability union, per-unit rt-lto). **S2 first stage SHIPPED 2026-07-14**
(dev surface, additive — the default `build`/`run`/`emit-obj` stay whole-program and byte-identical
until S2b flips them): `build_per_unit` walks the DAG bottom-up producing one `PerUnitArtifact`
(per-unit MIR + summary + dep hashes) per cleanly-checked unit; a new `alignc build-per-unit` verb
links the N objects. Visibility model wired end-to-end: a non-entry `pub` user fn gets `external`
linkage (`hir::Fn.exportable` → `mir::Function.exportable`, set only by `lower_program_per_unit`;
whole-program `lower_program` forces every fn internal for byte-identity), the entry unit's fns stay
internal (nothing imports the entry — this is also what makes N=1 byte-identical), and imported `pub`
callees become external Align-ABI declares (`hir::Program.imported_fns` → `mir::Program.imported_fns`
→ codegen `declare_imported_fn`, mirroring `declare_fn`'s signature/ABI so the linker binds the call;
generics stay consumer-side monomorphs, never cross-unit declares). Capability sets union
deterministically across units at final link. Per-unit `--rt-lto` merges the baked bitcode into each
unit's raw module (the guarded-skip/shed logic is already per-module). `impl_hash` upgraded off the
S1a source-byte stand-in to the unit's own MIR functions, stable-printed + location-free
(`partition_impl_hashes`) — a comment-only edit that lowers identically no longer over-invalidates.
Gates (6 new, `crates/align_driver/tests/per_unit_codegen.rs`): (a) N=1 object byte-identical to the
whole-program object + identical run; (b) multi-file cross-unit calls / imported struct / Move-array
return / in-consumer generic run identical to whole-program; (c) `llvm-nm` visibility — non-entry
`pub` fns external-defined, privates/monomorphs not external, entry `main` external, cross-unit call
site an undefined extern reference; (d) capability union is libz-only when one unit uses compress and
another none; (e) per-unit `--rt-lto` inlines `align_rt_str_eq` in the NON-entry unit, flag-off still
calls it; (f) private-body edit flips impl_hash only, comment-only edit flips neither. 2019 green.
**Honest remainders (S2b/S3):** the default-path flip to per-unit (S2b); per-unit `alignc size` /
`explain-opt` / `emit-llvm` / `emit-obj --export` surfaces (S2b); incremental caching + parallel unit
compilation (S3); consumer-side monomorphs are attributed to their template's unit in the
`impl_hash` partition (sound — a template-body change flips the producer's interface hash, which the
consumer keys on).

**S2b SHIPPED 2026-07-14 — default flip + full per-unit CLI surface (hard cutover, no dual path).**
The per-unit path is now the ONLY build path; the whole-program build path and the `build-per-unit`
dev verb are DELETED (no compat shim). Settled by a two-lens review and implemented exactly:
- **Default flip / path merge.** `build`/`run`/`emit-obj`/`size` now go through `build_per_unit_to`
  (per-unit walk → per-unit `emit_object` → deterministic capability union → `link_objects`). The
  whole-program `build_to` helper and the `build-per-unit` verb are gone; `build-per-unit` is now an
  unknown-command usage error, and it is dropped from the usage text and the `--rt-lto` valid-verb
  list (whose rejection text no longer names it). Success message unified to `alignc: built
  executable: <path>` (the "(per-unit, N unit(s))" variant is gone — per-unit is no longer a
  variant). `lower_program` (whole-program lowering) SURVIVES as the primitive behind
  `build_interface_summaries`/`emit-interface` + library tests — it is just no longer a build path.
- **`check` STAYS whole-program** (better diagnostics: "private to module" vs a per-unit "unknown";
  verdicts were proven identical in S1b). `check-per-unit`/`emit-interface` stay as dev verbs.
- **`emit-llvm` + `emit-mir` per unit.** N>1 emits each unit bottom-up, each preceded by a banner
  (`; ==== unit: <mod> ====` for LLVM, `// ==== unit: <mod> ====` for MIR); `--stage raw|optimized`
  applies per unit (each unit optimized in isolation — the truth under zero cross-unit opt, so a
  cross-unit `pub` call stays an opaque call while an intra-unit call inlines). N=1 = no banner,
  byte-identical to the pre-flip output.
- **`explain-opt` per unit, SERIALLY** (LLVM remark capture uses process-global `cl::opt`s), bottom-up,
  one aggregated report with a per-unit `==== unit: <mod> (<file>) ====` section header when N>1;
  N=1 = no header, byte-identical. Needed a new `lower_program_per_unit_located` (per-unit lowering
  carrying BOTH the `exportable` bits and `stmt_lines` line plumbing), factored through the existing
  `lower_program_impl` (no lowering-body duplication); each `PerUnitArtifact` now carries its source
  `file` so the unit's own `DebugInfo` attributes remarks to the right basename.
- **`emit-obj` N>1 + `--export`.** N=1 unchanged/byte-identical (`<stem>.o` or the given `[out.o]`).
  N>1 writes one object per unit named `<module-path>.o` (e.g. `util.math` → `util.math.o`); a single
  `[out.o]` positional with N>1 is a HARD ERROR ("one object per unit; omit the output path").
  `--export` is ENTRY-UNIT-ONLY: validated fail-closed against the ENTRY unit's MIR and applied only
  to the entry's `emit_object`; a name defined in a non-entry unit is a hard error naming that unit
  and telling the user to mark it `pub` (a non-entry `pub` fn is already external — the one way to
  export it); a name defined nowhere is the existing listed unknown-export error. Never a silent
  no-op.
- **Tests.** 13 new gates (`crates/align_driver/tests/per_unit_surface.rs`): N=1 object+exe identity
  across every profile {dev,release,fast,small,tiny} (+ `--rt-lto` where legal); CLI
  `build`/`run`/`size` byte-match a library whole-program reference + the removed verb errors;
  `emit-obj` multi-file filenames/visibility/`--export` (applied / wrong-unit / unknown / `[out.o]`
  rejected); `emit-llvm` banner+determinism+N=1 identity+opaque cross-unit boundary; `explain-opt`
  per-unit sections + N=1 no-header; `size` multi-file total == the built exe's on-disk size; a
  ≥3-unit DAG builds byte-identically twice; CLI capability union is libz-only; `--rt-lto` multi-file
  inlines in the non-entry unit + rejections. **Workspace 2035 green, clippy clean.**
- **Honest remainders = S3:** incremental caching + parallel unit compilation; per-unit `size`
  breakdown / interface-file emission (out of S2b scope by settlement); the pre-existing qualified
  cross-module fn-value bug (`map(util.dbl)`, already Open in `open-questions.md`).

**Measured cross-unit aggregate follow-up (2026-07-14; directional probe, worth retaining only as
a ThinLTO gate):** M15 creates the real non-inlined boundary that the M13 Slice-5 ABI-flattening
deferral said could change the result, so tuple return was re-probed rather than assumed. On the
Ryzen 9 5950X host (`x86_64`, LLVM 22.1.8, release/O2, native CPU), one entry unit called one imported
unit 200 million times and immediately summed either a returned tuple or an equivalent scalar
result. Median of seven balanced AB/BA runs:

| result | tuple | scalar control | tuple/control |
|---|---:|---:|---:|
| 2 x `i64` | 221 ms | 219 ms | 1.01x |
| 4 x `i64` | 306 ms | 221 ms | 1.38x |
| 8 x `i64` | 408 ms | 222 ms | 1.84x |

The machine code explains the crossover: two values return in `rax`/`rdx`; four and eight use a
hidden 32/64-byte return buffer, with the producer storing every field and the consumer loading every
field. The same sources on the whole-program path take 1-2 ms because inlining/SROA removes the
boundary and folds the loop. This is deliberately a call-heavy positive-case upper bound, not a
claim that ordinary tuple construction is slow. **Verdict:** retain a wide-tuple param/return case in
the future cross-unit ThinLTO acceptance matrix; do NOT add tuple-specific field reordering, type-
interning work, or a custom flattening ABI now. ThinLTO is the one already-settled mechanism that
removes the producer/consumer boundary and recovers scalar replacement for structs, tuples, and
other aggregates together. The gate must require the 4/8-value `sret` store/load round trip to vanish
for the inlinable positive case while a deliberately non-inline dynamic control retains a valid
cross-unit ABI.

**S3** the incremental cache per the
doc-10 contract + parallel unit compilation + hit/miss observability. **SV** verification
bundle: the doc-10 §7 invalidation matrix per-unit, N=1 byte-identity vs today, cold-vs-hit
byte-identity, and fail-closed effect-bit gates incl. a stale/absent-interface mutation.
Deferred-with-records: ThinLTO + monomorph dedup (`linkonce_odr` + deterministic mangling),
whole-build `pub` internalization, cross-host executable caching, manifest/package-level
units, caching failed frontend results (blocked on the doc-10 §5 diagnostic-determinism fix),
escape precision bits (borrows-arg-i/returns-fresh), the generic-over-generic fix, and
`extern "C"` body export.

**M15 S3 design SETTLED (2026-07-15, two-lens review — soundness/key-correctness +
driver/cache-layout/scheduling/observability — integrated; this record is the S3 implementation
source of truth).**

- **v1 caches the CODEGEN stage only: per-unit object bytes.** `walk_per_unit` (sema + lowering)
  always runs — it is cheap relative to LLVM optimize+emit and it *produces* the key inputs
  (`impl_hash`, interface hashes); frontend caching would also collide with the doc-10 §5
  failed-diagnostics nondeterminism blocker, so it is deferred whole. Link always re-runs
  (executable caching is host-local/non-hermetic, low value; object CAS is the portable
  high-value layer — its key is not weakened chasing a link hit). The headline win falls out:
  a private-body edit flips only that unit's `impl_hash` → only its object misses; dependents'
  keys (own MIR digest + transitive dep INTERFACE hashes, which a private edit leaves
  byte-identical) are unchanged → hit; relink.
- **Cache location/layout:** user-level cache `${XDG_CACHE_HOME:-~/.cache}/alignc/<schema-ver>/`
  with `ALIGNC_CACHE=<path>` relocate and `ALIGNC_CACHE=off` disable (per-project `.align-cache`
  rejected: source-tree litter, no cross-checkout sharing). `cas/<digest[0..2]>/<digest>` =
  immutable blobs; `actions/codegen/<key-digest>` = a tiny versioned manifest holding the result
  digest PLUS the decomposed key components (what makes first-differing-component miss reasons
  possible). Manifest = the repo's hand-rolled versioned length-prefixed fail-closed codec
  (`align_interface::codec` style), never JSON. Eviction: none in v1 (doc-10: bounded eviction
  only after correctness + telemetry).
- **Codegen action key, v1 components (doc-10 §6.2):** cache-format version · compiler build id ·
  frontend schema id (which also namespaces located vs normal MIR, so an `explain-opt`-shaped
  entry can never be shared) · the unit's `impl_hash` (stable location-free MIR print; its
  Vec-index type ids are cross-process-deterministic — doc-10 §4's byte-identity evidence — and
  SV pins this with an explicit cross-process `impl_hash` stability gate) · explicit export/root
  set · target triple + object format · RESOLVED cpu + feature set (never the string `native`;
  codegen already resolves via `get_host_cpu_name`/`get_host_cpu_features`) · profile + pass
  pipeline + TargetMachine opt level · reloc/code model · exact LLVM version · rt-lto mode +
  merged-bitcode digest · the empty-in-v1 cross-unit-opt digest. `impl_hash` is one component,
  never the whole key; any lowering change that could renumber type ids also flips the compiler
  build id, so no stale reuse.
- **Fail-closed matrix:** digest-verify every CAS read → mismatch = unlink + miss + rebuild +
  an always-on stderr corruption note (not hidden behind `--cache-stats`); publish = private
  `ArtifactStage` staging + same-directory atomic rename (a partial entry is never visible; an
  interrupted publish never creates a hit — the C0 pattern per entry); version/schema skew =
  key components (old entries simply unreferenced) + fail-closed versioned manifest decode;
  concurrent same-key producers = both stage privately, atomic-rename last-writer-wins over
  byte-identical content (determinism), no lock needed for correctness.
- **Hash decision (recorded deviation from the `hash.rs` header intent, owner-visible):** v1
  keys AND CAS addressing stay on the in-tree 128-bit wyhash `Hash128` — the cache is local and
  non-adversarial (birthday bound ≈ 2^64 keys; ~10^6 lifetime artifacts ⇒ P(collision) ≈ 10^-27),
  and a real BLAKE3/SHA-256 would be a new dependency the repo forbids without need. FIRM
  recorded trigger: any shared / cross-host / networked cache MUST first swap to a cryptographic
  256-bit digest (one-type swap behind `Hash128` + a cache schema-ver bump). Digest-verify does
  NOT protect against key collision (a collision is a "valid" entry) — width is the protection;
  that is why the trigger is firm.
- **Parallel compilation:** sema stays serial (bottom-up DAG order, mutates shared walk state);
  unit CODEGEN parallelizes over cache MISSES only via `std::thread::scope` (no rayon — first
  compiler-side threading; fresh LLVM `Context` per entry verified). Required serializations:
  LLVM target-init once on the main thread before the scope; `explain-opt` stays serial
  (process-global remark cl::opts). Determinism: results collect into a Vec indexed by DAG
  position; capability union + link order iterate DAG index order, never completion order.
  `-j N` flag + `ALIGNC_JOBS`, default `available_parallelism()`.
- **Observability from slice 1:** the build API returns structured
  `CacheOutcome { stage, unit, hit, miss_reason: Option<FirstDiff> }` where `FirstDiff` names the
  first differing key component (diffed against the manifest's decomposed components) — tests
  assert the enum, never elapsed time. Human surface: silent on all-hit; `--cache-stats`
  (build/run/size) prints per-unit hit/miss(reason) lines + summary counts.
- **CLI surface:** cache applies to `build`/`run`/`size`/`emit-obj` (`--export` folds into the
  key); `emit-llvm`/`emit-mir` (diagnostic truth lenses), `explain-opt` (serial, located
  namespace), and `check`/`check-per-unit`/`emit-interface` (frontend-only) stay uncached by
  design. `alignc cache clear` ships in S3b.
- **Rollout: S3a** = cache substrate (`align_driver::cache`) + serial codegen-stage cache +
  `CacheOutcome`/`FirstDiff` model + the gate net, **opt-in** (`ALIGNC_CACHE=on`); **S3b** =
  parallel codegen + `--cache-stats`/`-j`/`cache clear` + the runtime-archive freshness check
  upgraded mtime → content digest (also kills the recorded post-merge `cargo test` staleness
  papercut) + **default-ON flip gated on the cold-vs-hit byte-identity gate**. The `off` hatch is
  doc-10-mandated operability, not a compat shim — a disabled cache is an always-miss lookup on
  the one code path. N=1 byte-identity and `multi_unit_dag_builds_byte_identically_twice` stay
  green throughout: the miss path is today's codegen verbatim; the hit path is digest-verified
  identical bytes.
- **SV additions demanded by this review:** the cross-process `impl_hash` stability pin (the one
  place a future lowering change could silently introduce a stale hit); transitive A→B→C
  invalidation (C `pub`-signature change forces A's miss; C private-body edit does not);
  an absent/stale-interface fail-closed mutation. Also recorded: doc-10 §3's code pointers
  (`build_to`, `main.rs#L307–346`) are stale post-S2b — update them when S3a touches the driver.

**M15 S3a SHIPPED — MERGED as #454 (2026-07-15; workspace 2051 green + clippy `-D warnings`
clean).** New `crates/align_driver/src/cache.rs` (CAS blobs via private staging + atomic rename;
versioned length-prefixed fail-closed manifest codec in the `align_interface::codec` style —
allocation-bomb-safe: lengths bounds-checked against the real buffer before any allocation) wired
serially into `build_per_unit_to` (build/run/size) + `run_emit_obj` (`--export` in the key);
`emit-llvm`/`emit-mir`/`explain-opt`/`check*` uncached by design. Key sourcing: compiler build id
= memoized runtime hash of the running `alignc` binary (a compile-time version constant would
false-hit across dev rebuilds); exact LLVM version via `LLVMGetVersion`, never the hand-typed
tool constant; cpu/features/triple/reloc/code-model from a `resolve_cpu_features` now SHARED with
`create_target_machine`, so the key hashes exactly what codegen uses. Opt-in
(`ALIGNC_CACHE=on|<path>|off`, disabled by default until the S3b gated flip); a disabled build
does ZERO cache-key work (gated on `CacheContext::is_enabled()`, pinned by a
disabled-cache-verbatim-zero-disk gate). **One recorded implementation deviation:** the settled
record's single `actions/codegen/<key-digest>` manifest became full-key actions PLUS a
stable-slot index — full-key addressing gives edit-then-revert hits (old entries never
overwritten), the slot index is what makes first-differing-component miss reasons computable (a
changed key otherwise lands at a fresh path with nothing to diff); the slot index is
observability-only, a hit still requires the full-key action + a digest-verified blob. 10
integration gates (`cache_codegen.rs`) + 5 unit tests: no-op all-hit, dep private-body edit →
one miss + dependents HIT + correct exe (the headline), transitive A→B→C invalidation,
comment-only hit, edit-revert hit, corrupted blob → evict + stderr note + rebuild + correct
binary, cold-vs-hit byte-identity (objects AND final exe), profile/`--export` `FirstDiff`
reasons, rt-lto key split, cross-process second-build hit. gemini review reflected pre-merge:
staging-leak-on-error-return APPLIED (orphaned `.cache-stage-*` on failed write/rename — every
error path now cleans up); Windows `%LOCALAPPDATA%` cache root REJECTED with a documented
platform story (Windows is a fail-closed unsupported target — dead code); its
compile-time-build-id suggestion surfaced a REAL defect (unconditional key construction with the
cache off — fixed) while the constant swap itself was rejected (dev-rebuild false-hits).
Remainders → S3b (all per the settled record): parallel codegen over misses, `--cache-stats` /
`-j` / `ALIGNC_JOBS` / `cache clear`, runtime-archive mtime → content digest, default-ON flip
gated on the cold-vs-hit gate; then SV.

**M15 S3b SHIPPED — MERGED as #455 (2026-07-15; workspace 2061 green + clippy `-D warnings`
clean). S3 is COMPLETE.** All five S3b items per the settled record: **(1) parallel unit codegen
over cache misses** — serial lookups first (they produce the DAG ordering), then
`std::thread::scope` workers claim misses via a shared atomic index (claim-once RMW; fresh LLVM
`Context` per unit; native target initialized exactly once on the main thread pre-scope via a
`Once`); results, capability union, and link order iterate DAG index order, never completion
order; `-j/--jobs` + `ALIGNC_JOBS` (flag wins; garbage/zero/empty/overflow diagnosed, never a
panic), default `available_parallelism()`; `explain-opt` stays serial + uncached; the cache
entry split into `lookup` + `publish_after_miss` so serial and parallel callers share one
implementation of each primitive. **(2) `--cache-stats`** on build/run/size (silent on the
default path; per-unit hit/miss(reason) lines + summary counts; corruption notes always-on).
**(3) `alignc cache clear`** — removes only the cache-owned `cas`/`actions`/`index` subtrees
(an explicit shared `ALIGNC_CACHE` dir is never nuked wholesale); symlink entries are unlinked,
never followed — the self-review hardening that makes `clear` unable to recurse outside the
resolved root. **(4) runtime-archive freshness mtime → content digest** — `build.rs` bakes a
sorted-walk digest of `align_runtime/src` (via the in-tree `align_hash`, now a driver
build-dependency; no external dep) and `ensure_archive_fresh` compares source digests: the
recorded post-merge `cargo test` false-stale papercut is gone, a genuinely stale archive still
fails loud, and the `build.rs` ↔ `lib.rs` algorithm identity is pinned by test;
`runtime_archive_digest()` is prepared for the future doc-10 §6.3 link key. **(5) default-ON
flip, last, gated on the cold-vs-hit byte-identity gate**: unset `ALIGNC_CACHE` → enabled at
the XDG root, `off` disables, path relocates; all 7 non-cache alignc-spawning test files pinned
to `ALIGNC_CACHE=off` (uniform isolation), and a default-ON smoke gate (temp XDG root) proves
second-build all-hit + byte-identity. gemini's one finding (fail-fast on parallel codegen
error) APPLIED deliberately minimal: an `AtomicBool` checked at the top of the claim loop —
workers stop claiming after the first error, an in-progress emit is never interrupted (no torn
objects), `Relaxed` suffices (best-effort hint; errors ride the `Mutex`; final read
happens-after the scope join), zero success-path change; single-error reporting stays
deterministic (lowest DAG index), the multi-independent-failure collected set is documented as
timing-dependent; left untested-but-simple (a deterministic multi-unit codegen failure needs a
fault-injection seam the CLI lacks). 7 new `cache_parallel.rs` gates (parallel == `-j 1` bytes,
≥3-unit parallel-twice byte-identity, jobs override, cache-stats shape, clear → all-miss →
all-hit, digest freshness both directions, default-ON smoke). **Next M15 step = SV** (the
verification bundle: doc-10 §7 invalidation matrix per-unit, N=1 byte-identity vs today,
cold-vs-hit byte-identity, fail-closed effect-bit gates incl. a stale/absent-interface
mutation, plus the S3-review additions: cross-process `impl_hash` stability pin, transitive
A→B→C invalidation, absent/stale-interface fail-closed mutation). Still queued behind the M15
slices: the `http_server_no_fd_leak_across_cycles` flake-hardening slice (it recurred during
the S3b full-suite runs — same signature, passes in isolation) and the qualified cross-module
fn-value remainder (`map(util.dbl)`, open-questions).

**M15 SV SHIPPED — MERGED as #456 (2026-07-15; workspace 2068 green + clippy `-D warnings`
clean). M15 is COMPLETE.** The verification bundle closes the doc-10 §7 matrix at the implemented v1 boundary:
the frontend always re-runs and link always re-runs by design, while every codegen/object-cache
identity and publication expectation is automated. Existing gates already pinned N=1 object/exe
identity across profiles + `--rt-lto`, cold-vs-hit object/exe identity, private-body vs public
surface invalidation, transitive A→B→C invalidation, exact-revert reuse, corruption recovery,
cross-process `impl_hash` stability, runtime content freshness, and absent/Unknown/Impure effect-bit
rejection. SV added the missing teeth:

- the interface reader now recomputes the canonical public-surface hash and rejects a stale or
  modified signature/layout/effect surface before it can seed sema; a mutated Impure→Pure effect
  artifact is the permanent negative gate, alongside the pre-existing absent-effect gate;
- unimported-file edits leave every reachable unit hot; baseline/native resolved CPU identities
  occupy distinct namespaces; and the unit key's complete `FirstDiff` component matrix is pinned;
- a killed producer's orphan private staging file is invisible, so the next build safely rebuilds
  and publishes a complete hit; four identical cross-process producers converge on one immutable
  action per unit with no staging residue;
- concurrent different programs with the same basenames retain both distinct implementation
  actions while sharing byte-identical dependency actions; both executables are correct and both
  builds subsequently hit in full.

The matrix intentionally does not claim frontend/link cache hits: v1 caches per-unit object bytes
only, as settled in S3, so frontend and link rows are verified as safe reruns rather than cached
stages. Deferred cache layers retain their doc-10 gates. The first recorded post-M15 item, the
`http_server_no_fd_leak_across_cycles` flake hardening, shipped as #457; qualified cross-module
function values then shipped as #458. Both recorded follow-ups are complete.

Gemini's one high performance finding was valid and applied before merge: interface decode records
the canonical surface boundary and hashes that input slice directly, avoiding a second O(N)
serialization/allocation while preserving the same fail-closed check. The inline thread was
answered and resolved, and the PR carries the English review-response/validation summary.

**HTTP server fd-leak timing flake HARDENED — MERGED as #457 (2026-07-15; workspace 2068 green +
clippy `-D warnings` clean).** The test samples a process-wide Linux fd table while Rust runs the
same test binary concurrently, so unrelated transient sockets could inflate a single post-cycle
snapshot. It now participates in the existing fd-sensitive network-test lock and, only after an
over-threshold first sample, retains the lowest successful fd count from a fixed 20 × 10 ms drain
window. The acceptance threshold remains `after <= before + 2`; a real cycle leak persists across
every sample. That property was mutation-checked by deliberately retaining 12 fds (`4 -> 16`, the
expected failure), while the unmodified test passed 50 consecutive targeted runs and the complete
parallel `align_runtime` library suite passed 20 consecutive runs. Gemini's one medium finding was
valid and applied: transient `/proc/self/fd` read failures are handled without `unwrap` panics in
both the initial post-cycle count and retries. The inline thread was answered and resolved, and an
English review-response/validation summary was posted before merge.

**QUALIFIED CROSS-MODULE FUNCTION VALUES SHIPPED — MERGED as #458 (2026-07-15; workspace 2074
green = 2073 passed + one ignored manual probe; clippy `-D warnings` clean).** Named callable
positions previously discarded a qualified module prefix even though direct calls already
understood it. A shared named-function reference now preserves bare or complete dotted module
paths and uses the same import / `pub` classifier as direct calls. The common checked resolver
drives `map`, `where`, `reduce`, `scan`, `partition`, `any`, `all`, `par_map`, and `sort_by_key`,
plus ordinary bound values such as `f := util.dbl; f(21)`. Quiet signature peeks share resolution
without premature diagnostics; local leftmost-name shadowing remains value-field semantics.
Whole-program and per-unit checking agree for nested modules, visibility/import failures, and
qualified Pure/Impure `par_map` effect bits. Gemini reported no findings, thread-aware inspection
found no unresolved review comments, and an English validation summary was posted before merge.
The two recorded post-M15 follow-ups are therefore complete. No mandatory implementation slice is
queued; fully-escaping function values stay deferred until the heap-owned environment/drop model
is settled and has a consumer.

**WRAPPER-HIDDEN LOCAL-SLICE ESCAPES FIXED — MERGED as #459 (2026-07-15; workspace 2078 green =
2077 passed + one ignored manual probe; clippy `-D warnings` clean).** The direct local-array slice
return check only ran when the outer expression itself was `slice<T>`, so `Ok(xs[..])` or
`Some(xs[..])` could hide a frame borrow and return a dangling pointer. The local-storage
provenance check is now type-transparent through `Option`/`Result`, tuples, structs, calls, and
value-carrying control flow. Locals and reassignments retain it, and tuple destructuring plus
`match` payload bindings propagate it only to slice-bearing locals through checked HIR lookups.
The fix deliberately remains separate from `region_of`, preserving the safe case where a slice of
an arena-local array leaves the inner arena but not the function. Regression tests reject direct,
wrapper-local, and `match`-payload escapes while accepting a wrapped caller-provided slice. Gemini
reported no findings, thread-aware inspection found no review threads, and the English validation
comment was posted before merge. The common borrow-liveness design then shipped as #460 below.

**INTRA-FRAME BORROW LIVENESS ENFORCED — MERGED as #460 (2026-07-15; workspace 2102 green =
2101 passed + one ignored manual probe; clippy `-D warnings` clean).** Region analysis already
bounded where a view could escape, but a source could still be moved, replaced, or reallocated
inside that frame while the old view remained usable. `MoveCheck` now carries a shared
`BorrowState`: borrow-producing expressions flatten to owner-local roots, invalidation records a
dead source generation, borrower reassignment clears it with fresh provenance, branches join only
fallthrough states, and loop heads compute a finite may-state fixpoint. The producer sweep covers
string/slice views, buffer/CLI/TCP/HTTP handles, response-array elements, aggregate and wrapper
forms, calls/tasks/control flow, and pipeline captures. Buffer `append`, scalar `put`, and
`read_line` invalidate views because they may reallocate. Owner-slot roots are kept alongside but
distinct from copied view-element roots, closing `rs[0].body()` after response-array replacement
without rejecting primitive SoA materialization after its source moves. Twenty-four new tests pin
the rejection matrix, re-borrows and diverging branches, loop back edges, diagnostics, and safe
materialization. Gemini reported no findings; thread-aware inspection found no review threads; an
English validation comment was posted before squash merge. The broader escape/region structural
work continues through #461–#463 below.

**PATH-LOCAL OWNED DROP FLAGS SHIPPED — MERGED as #463 (2026-07-15; workspace 2116 green =
2115 passed + one ignored manual probe; clippy `-D warnings` clean).** Checked HIR now records every
resource-owning local separately from the subset of values that require individual cleanup, and
each owned assignment carries its new-value cleanup provenance. MIR allocates a private boolean
flag beside every resource-owning slot; initialization, reassignment, direct moves, tuple and
`match` destructuring, loop edges, `break`, return, and early cleanup explicitly update or transfer
it. Cleanup branches around `Drop` unless the live path holds an individually owned value, then
clears the flag after the taken edge. This removes the temporary fail-closed rejection of
region-changing owned reassignment while preserving conservative escape-region joins for Copy
views, and also fixes owned self-assignment ordering. Regression coverage spans arena→heap and
heap→arena paths, bypasses, loops, joined-region moves, payload bindings, calls with shorter-lived
borrow arguments, and region-free builders. Gemini's one medium finding identified two slot-growth
paths that failed to grow parallel flag metadata; both were fixed, the thread was answered and
resolved, and the English validation summary was posted before squash merge. The compact CFG item
then shipped as #464 below; function-type effects subsequently shipped as #465.

**COMPACT ESCAPE-FLOW CFG SHIPPED — MERGED as #464 (2026-07-15; workspace 2118 green = 2117
passed + one ignored manual probe; clippy `-D warnings` clean).** `EscapeCheck` now lowers the
already-checked HIR into compact basic blocks containing references to transfer operations.
`if`, `match`, `else`-unwrap, loop backedges, and `break` produce explicit edges; a single finite
may-state worklist computes every branch join and loop fixpoint. The probe pass suppresses repeated
loop diagnostics, then reachable operations replay once in original syntax order from their fixed
block inputs, preserving stable diagnostics and cleanup metadata on diverging paths. A `break` edge
is split before unreachable syntax so later checked statements cannot contaminate its snapshot.
Regression gates cover that false-positive direction and the fail-open direction where multiple
reachable break predecessors must join at the loop exit. Gemini reported no findings, thread-aware
inspection found zero review threads, and the English validation comment was posted before squash
merge. The escape-analysis CFG structural follow-up is complete.

**FUNCTION-VALUE EFFECT TYPES SHIPPED — MERGED as #465 (2026-07-15; workspace 2127 green =
2126 passed + one ignored manual probe; clippy `-D warnings` clean).** `FnTy` now stores the inferred
three-valued effect for each concrete function value/local while source annotations and signature
identity remain effect-free. A least-fixpoint refinement covers named functions, lifted closures,
mutable target joins, interface-only imports, and FFI pointers; unresolved HOF parameters stay
`Unknown`. `CallFnValue` and `ResultMapErr` consume the type bit, so unused Impure values do not
taint a function while every actual indirect invocation stays sound. Same-signature concrete values
have independent effect cells, including after `Pure` / `Impure` refinement. Cross-unit parity and
recursive Pure cycles are regression-pinned. Gemini's one high finding identified unstable derived
equality over the mutable cell; manual parameter/return equality fixed it, the inline thread was
answered and resolved, and the English validation summary was posted before squash merge. The next
audit structural item is the explicit value-carrying-control-flow region/move/drop matrix and its
1:1 tests.

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
