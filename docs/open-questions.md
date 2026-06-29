# Open Questions

Design questions are managed in three groups: "Settled", "Open", and "Out of v1 scope". Settled items keep their decision and record location (to prevent reopening).

---

## Settled

### Compiler backend
**Decision: LLVM. But always go through a backend-agnostic MIR.**
"C backend first → LLVM later" is not adopted (deferral trap + loss of SIMD control). Semantics live in MIR; `MIR → LLVM` is pure lowering. Future alternate backends are handled by adding lowering.
Record: `impl/00-overview.md`, `impl/04-mir.md`, `impl/05-backend-llvm.md`

### Syntax: statement termination and layout
**Decision: Go style.** Newline terminates a statement; `;` is an optional separator for cramming onto one line. Blocks use `{}` and indentation is insignificant (not Python). A line starting with `.`/binary operator continues the previous line.
Rationale: simultaneously satisfies "cleanliness (no `;`)", "freedom (one-liners allowed)", and "non-Python (no forced layout)".
Record: `draft.md` §4, `impl/01-pipeline.md`, `impl/02-frontend.md`

### Integer overflow
**Decision: default is two's-complement wrap (not UB, zero-cost, does not hinder SIMD).** Provide explicit ops (`checked_*`/`saturating_*`/`wrapping_*`). Optional checked build for development only. Division by zero etc. is handled separately and is always an error.
Record: `draft.md` §5, `impl/03-types.md`

### Numeric conversion — `as` (DONE)
**Decision: no implicit coercion (not even widening); the explicit `as` operator is the only conversion.** It applies between the numeric primitives (`i8..u64`, `f32`/`f64`) and `char` (the Unicode code point, a `u32`; `char` never pairs with a float), and is **zero-UB by design** — int→int truncates/extends with defined wrap, float→int *saturates* (out-of-range → MIN/MAX, NaN → 0). `bool` and composite types do not participate; casting a generic type parameter is rejected (deferred). Fully implemented end-to-end (`As` token → `parse_cast` → `check_cast` → `hir::Cast` → `Rvalue::Cast` → `gen_cast`).
Record: `draft.md` §3, `impl/03-types.md` §2, `examples/cast.align`, `tests/numeric_cast.rs`

### Top-level named constants (DONE 2026-06-26)
**Decision: a top-level `:=` is a compile-time constant — no `const` keyword.** It reuses the
keyword-less binding form (`NAME := expr` / `NAME: T := expr`), is immutable (`mut` rejected at the
top level), and is **evaluated at compile time** to a scalar / string value that is substituted as a
literal at every use — so a constant never reaches MIR/codegen (zero new backend surface). Its value
is built from literals, unary/binary operators, and references to other constants (cross-module
references *inside* an initializer are deferred; aggregate/struct constants and `as` in a constant
are deferred). A constant's type is **fixed at the definition** (unlike a local it does not infer
from a use site — it must be stable across modules), so an unannotated integer defaults to `i64` /
a float to `f64`. Constants are **per-module namespaced like functions/types** (`module$NAME`
canonical, entry unmangled so single-file programs stay byte-identical): `pub` exports one, an
importer names it qualified (`mod.NAME`), and a name may not be both a function and a constant in
one module. Overflow wraps (defined two's-complement); division by zero, a cyclic definition, and a
type mismatch are compile-time errors. Folded values feed the const string pool (`draft.md` §12).
Record: `draft.md` §3/§4, `docs/language-spec.md`, `impl/02-frontend.md` §3, `examples/constants.align`, `tests/constants.rs`

### Bitwise & shift operators (DONE 2026-06-26)
**Decision: integer operators `& | ^ << >>` + unary `~`, NOT bitset methods.** Bit work on integers
is done with operators (the AI-/human-familiar, terse, "one way" form); the `core.bitset` type (large
SIMD-friendly bit sets) is a *separate* layer built on top, deferred to M6 with `vec`/`mask` — not
bundled here (avoids premature bitset design before the M6 layout/SIMD model). Operators are
**integer-only** (`bool` uses logical `&&`/`||`/`!`; `~` is bitwise complement, distinct from `!`),
with **no implicit coercion** — the shift amount shares the value's type. **Precedence = Go's** (the
settled "Go style" syntax): `<< >> &` bind like `*` (5), `| ^` like `+` (4), so every bitwise/shift
operator binds tighter than comparison (`a & b == c` = `(a & b) == c`, no C footgun). **Shift amount
masked mod the bit width** (defined, zero-cost, SIMD-non-blocking — the exact parallel of the
overflow-wrap decision; codegen masks `n & (width-1)`, constant over-shift is a future lint), `>>`
arithmetic on signed / logical on unsigned. `>>` is **not** a single lexer token (kept as two `>`),
so nested generic type args (`Pair<Pair<T>>`) still close; the shift is formed only in expression
position, where `<`/`>` are comparison-only (no turbofish). Folds in constant expressions.
Record: `draft.md` §5, `docs/language-spec.md`, `examples/bitwise.align`, `tests/bitwise.rs`

### Radix integer literals (DONE 2026-06-26)
**Decision: base-prefixed integer literals `0x` (hex) / `0o` (octal) / `0b` (binary), `_` separators
in any base.** A radix literal is an ordinary integer literal — same `i128` storage, width inferred
from context, narrowed to the binding's type by the defined wrap rule (`0xFFFFFFFF: i32` = -1). The
lexer parses the prefix (greedy alphanumeric run → `i128::from_str_radix`, so an invalid digit / empty
body is a clean error). Decimal `_` separators already worked; this extends them to all bases. Pairs
naturally with the bitwise/shift operators. Record: `draft.md` §3/§5, `docs/language-spec.md`, `examples/bitwise.align`, `tests/radix_literals.rs`

### Numeric literal typing — no suffix (DONE 2026-06-26)
**Decision: a literal's type comes from the binding annotation or the `as` operator — there is NO
literal suffix (`10i32` / `2.0f32`).** A suffix would be a *third* way to type a literal, and for a
literal it is exactly redundant with `as`: `10 as i32` ≡ `10i32`, and a binding annotation
(`x: i32 := 10`) covers the binding case. Two complementary, non-overlapping mechanisms — annotation
(types a *binding*) and `as` (types an *expression*) — beat three overlapping spellings ("one way" /
convergence). The earlier `impl/03-types.md` / `impl/02-frontend.md` suffix claim (it was only ever
in the impl plan, never the authoritative `draft.md`, and never implemented) is **removed**. Default
type when fully unconstrained stays i64 / f64; a "wasteful i64 default in large arrays" lint remains a
Future item. Record: `docs/impl/02-frontend.md` §2, `docs/impl/03-types.md` §2.

### Type declaration syntax
**Decision: keyword-less.** Contains `ident: Type` → struct; `ident`/`ident(...)` → sum type, disambiguated by content. Fields/variants are `,`-separated.
Record: `draft.md` §4, `impl/02-frontend.md`

### Sum types + exhaustive `match` — design SETTLED (the keystone language-spec slice)
**Decision (2026-06-24): keyword-less sum types + a mandatory-exhaustive `match` expression** — the OOP-free way to model domain variants, AI-friendly (a new variant turns every incomplete `match` into a compile error), and the convergence point that will eventually generalize the currently-builtin `Option`/`Result`. Grounded in the actual code: today the parser/AST/`Ty` only have structs (`Item::{Fn,Struct}`, `parse_struct` requires `ident: Type` bodies); `Option`/`Result` are builtin `Ty` variants (scalar payloads); `match` has no keyword/AST node. The keyword-less type-decl decision above already reserves the sum-type half.
- **Declaration (keyword-less, disambiguated by content).** A body of `ident: Type` fields is a struct; a body of bare `ident` / `ident(payload…)` variants is a sum type. A body is wholly one or the other (the parser branches after `Name {` on whether the first variant/field is followed by `:`). Variants are `,`/newline-separated.
  ```
  Color { Red, Green, Blue }                 // tag-only
  Shape { Circle(f32), Rect(f32, f32) }      // positional payloads
  ```
  Payloads are **positional** (tuple-style); a variant needing named fields uses a struct payload (`Node(TreeNode)`). First cut: scalar payloads (later: struct/tuple); **non-recursive** (a self-referential variant needs `box`, a later widening).
- **Construction — qualified `Type.Variant`** (matches the draft's `Error.NotFound`): `Color.Red`, `Shape.Circle(3.0)`. Qualified (no unqualified `Red`) → no cross-type ambiguity, one-way, explicit. In sema this is a `FieldAccess`/`Call` whose base path resolves to a sum-type name.
- **`match` (expression, mandatory-exhaustive).**
  ```
  area := match s { Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h }
  ```
  An expression — every arm unifies to the `match`'s type (or all diverge). Patterns are **unqualified** variant names (the scrutinee's type is known): `Variant` / `Variant(b0, b1)` (binds the payload positionally). **Exhaustiveness is mandatory from day one**: every variant covered, or a `_` wildcard arm; a missing variant with no `_` is a compile error naming the omissions. `match` is for sum types (incl. `Option`/`Result`); value conditions stay with `if` (one way: `match` = variants, `if` = conditions). `A | B` or-patterns landed in S4; guards / nested patterns remain unadopted (see the slice ledger below).
- **Works on `Option`/`Result`** (they are builtin sum types): `match opt { Some(x) => x, None => 0 }`. `else`-unwrap and `?` remain the **ergonomic shorthands** over the general mechanism (sugar, like Rust's `?` — not a second way).
- **Representation.** `Ty::Enum(id)` interned into `Program.enums` (mirroring `Ty::Struct`/`Program.structs`); LLVM = a tagged union `{ iN tag, <bytes for the largest payload> }` — the existing `Option`/`Result` `{i8 tag, payload}` shape, generalized. Construction stores tag+payload; `match` branches on the tag, extracts the payload; rare arms can later get the cold-path treatment `Err` already has.
- **Convergence path.** With minimal generics, `Option<T>`/`Result<T,E>` become generic sum types in the general mechanism (retiring the builtin `Ty::Option`/`Ty::Result` special-case — "one way"); until then they coexist, with `match` already unifying their use.
- **Why the keystone:** replaces OOP/inheritance (a non-goal), AI-friendly via exhaustiveness, removes a "one way" exception, lower-risk than generics (no constraint model), and unblocks the **Error type** redesign (Error = a sum type of categories).
Implementation slices: **S1 DONE** — tag-only + scalar-payload enums + `Type.Variant`(`(args)`) + exhaustive `match` with positional payload bindings (no guards/nesting); the enum lowers to a non-union tagged struct `{ i32 tag, <flattened payloads> }`. **S2 DONE (struct)** plain-data struct payloads (`Circle(Point)`); a `str`-field struct payload needs enum region-tracking (deferred), and tuple payloads need a `Scalar::Tuple` (deferred); **S3 DONE** `match` on `Option`/`Result` (via a `match_variants` helper + a two-variant `IsSome`/`IsOk` branch reusing the existing unwrap rvalues); **S4 (or-patterns) DONE** — `A | B | ...` shares one arm (a new `|` token + `MatchPattern::Or`; `hir::MatchArm.variants: Vec<u32>`; MIR tests each tag in sequence into the arm block). An or-pattern lists **bare** variant names and binds nothing (a payload variant may appear, its payload unbound — binding in an or-pattern is rejected); it counts toward exhaustiveness like any arm. **Guards and recursive (boxed) enums were reviewed and NOT adopted now:** *guards* (`P if cond`) cross the settled "`match` = variants, `if` = conditions" One-Way boundary (and are expressible via an `if` in the arm body) — declined on philosophy, not difficulty; *recursive enums* (`List { Cons(i32, box<List>), Nil }`) run against the data-oriented core (pointer-chasing over arrays) and need the `box<Enum>` rejection lifted + self-referential layout + boxed-recursion drop/region — deferred as its own larger track if a concrete need (e.g. an AST) arises. (Deferred codegen optimization: a space-optimal union layout instead of flattened per-variant fields — no surface change.)
Record: `draft.md` §5 (Sum Type), `impl/07-roadmap.md`.

### Purity model
**Decision: compiler inference (no explicit marks).** Effects (Pure/Impure) are inferred from the body, and `par_map` etc. require Pure closures. **Implemented** (`align_sema` Pass 4, `check_parallelism`): a function is Impure iff it transitively performs an observable side effect — calling `print` / `io.stdout.write` / `fs.read_file`, or calling an Impure function (fixpoint over the call graph). Everything else (arithmetic, reads, builder/arena/heap, owned-value moves) is Pure. `par_map(f)` rejects an Impure `f`. (Sound for the language as it stands: a `par_map` function is `(T) -> R` with no `out` parameter, so reaching an I/O builtin is the only route to impurity.)
Record: `impl/03-types.md` §8

### Lambdas / closures — IMPLEMENTED (map/where/all reducers + capture)
**Decision: lambdas exist and are the way to pass behavior to stages/reducers; capture by value, no hidden closure environment.** Always part of the design (`draft.md` §8/§11 use `fn x { ... }`); the early implementation accepted only named functions, now lifted. **Implemented**: an inline lambda `fn params { body }` (parameter types inferred) in `map`/`where`/`reduce`/`par_map`/`scan`/`partition`/`any`/`all`/`sort_by_key` is **lifted** to a synthetic top-level function (`align_sema` `lift_lambda`), so it flows through the same `Rvalue::Call` + fused-loop lowering as a named function — optimized identically. **Capture** of enclosing locals is by value: each captured local becomes a trailing parameter passed at the call site (a loop-invariant argument the backend hoists), so there is no closure environment / allocation. Capture is wired into **every** stage and reducer (`map`/`where` + `reduce`/`scan`/`partition`/`any`/`all`/`par_map`/`sort_by_key`) for copy values; a capturing `par_map` falls back to the sequential path (the parallel thunk has no capture context). All three flow analyses (`MoveCheck`/`EscapeCheck`/`EffectScan`) walk stage and node captures. Deferred: owned-value capture, and first-class function values (see next entry).
Record: `draft.md` §8 (Function Arguments), `docs/language-spec.md`, `design-notes.md` (lambda philosophy), `impl/07-roadmap.md`.

### First-class closures + `task_group` — design SETTLED, implementation deferred (no timing pressure)
**Decision (2026-06-23): escape decides a lambda's representation; `spawn` takes a lambda; `task_group` is a structured scope.** The ideal form, chosen on merit (not legacy): a lambda that **escapes** (stored in a variable, returned, or handed to `spawn`) gets a **closure environment** holding its captured values; a non-escaping lambda (every pipeline stage/reducer) stays inlined with captures-as-parameters (zero allocation, SIMD/GPU-friendly). The compiler's **escape analysis** picks the representation — the same syntax, two representations — so first-class function values and `task_group` exist without eroding the offload-ready pipeline path. The environment is **owned by the enclosing region** (the `task_group {}` / `arena {}` scope) and freed with it — a region allocation, not a hidden `malloc`, so the visible scope is the boundary (consistent with *Nothing hidden*). (The model for a closure that escapes *every* region is part of this deferred design; the `task_group` consumer is scope-bounded.) `task_group` (`draft.md` §11) is a **structured** scope like `arena {}`: `spawn(fn { … })` takes a lambda (the deferral is then visible — *Nothing hidden* — and it is the one lambda mechanism, not a bare-call special form), returns a `Task<R>` handle; `wait()?` is the single error boundary (joins all, propagates the first `Err`); `a.get()` reads a result after the join. A spawned task **may be impure** (it does I/O — unlike a Pure `par_map`); safety comes from by-value capture (no shared mutable state). Rejected alternative: a bare-call special form `spawn(fs.read_file(p))` — it hides the deferral (against *Nothing hidden*) and is a second deferral mechanism (against *One way*); it was only attractive as a way to dodge the closure-environment work, which escape analysis handles cleanly. **Build order:** first-class closures (escape-driven) as the foundation, then `task_group` as a consumer. Rationale: [The lambda philosophy](design-notes.md#the-lambda-philosophy).
Record: `draft.md` §11 (Task Group), `design-notes.md` (lambda philosophy), `impl/07-roadmap.md`.

**Implementation plan (2026-06-23, revised), after closures ①–③ shipped.** `task_group` **does need the region-owned env** the settled design specified — a **fresh environment per `spawn`, allocated in the `task_group` region** (an arena-like bump region tied to the scope, freed at scope end). The ②b-2 frame-local env is a *single hoisted alloca slot per closure site*, so it cannot back a spawned closure: a `spawn` in a loop (or after reassigning a captured variable) reuses that one slot, and a **deferred** task (④a) would then read the final value, while a **concurrent** task (④b) would race the next iteration's overwrite. A fresh per-`spawn` allocation in the region gives each task a stable, private snapshot of its captures. (So `spawn` is the escape that triggers the region env — exactly "escape decides the representation". The frame-local env stays correct only for a closure that is *called within the frame*, never spawned.) Surface (all scalar `R` for now, matching the closure slices):
- `task_group { … }` — a block scope like `arena {}`; opens the task region + context; `wait()` (or scope end) joins, then the region is freed.
- `spawn(fn { … })` — a builtin valid inside the scope (like an arena allocation refers to its arena). Takes a `fn() -> R` value (captures by value, **snapshotted into a fresh region env**; may be impure); returns `Task<R>`. `Ty::Task(Scalar)`.
- `wait()` — joins all spawned tasks; later `wait()?` is the single error boundary (tasks returning `Result`, first-`Err` propagation).
- `t.get()` — reads the task's result `R` after the join. **`get()` before `wait()` is a compile-time error** (a flow check, like use-after-move — the result is not yet computed); it is not a runtime trap or an on-demand trigger. Symmetrically, `spawn`/`wait`/`get` are valid only inside a `task_group` scope.
Decomposition: **④a** scope + the task region + `spawn` (fresh region env per spawn) + `Task<R>` + `wait` + `get` (flow-checked), tasks run **deferred-sequential** (run at `wait` in spawn order — matches the eventual "complete by `wait`" semantics, unlike eager-at-`spawn`); **④b** real threads (reuse the `par_map` thread runtime: a thread per task, join at `wait`); **④c** the `wait()?` error boundary.

**④b memory model (2026-06-23), the load-bearing slice.** ④a shipped as the eager skeleton (`Task<R>` ≡ `R`); ④b switches to the real model, where the representation change ripples through the move/drop machinery — so it is designed before coding (the same machinery code review found ④a holes in). Model:
- **`Task<R>` becomes a pointer to a result slot** in the `task_group` region (no longer the bare `R`). The region (an arena-like bump allocator owned by the scope, freed at scope end) holds, per `spawn`: (a) a **fresh env** — the captures memcpy-snapshotted out of the frame, so concurrent/deferred tasks never share the one hoisted frame slot; (b) a **result slot** sized for `R`.
- **`spawn`** lowers to: alloc env + slot in the region, copy captures into env, register a per-spawn **trampoline** `fn(env, slot) { *slot = closure(env) }` (generated — it knows `R` for the typed store), and hand `(trampoline, env, slot)` to the runtime. The `Task<R>` value is the slot pointer.
- **Runtime IF** (`align_rt_tg_*`): `begin() -> *tg`; `alloc(*tg, size) -> *u8` (bump); `register(*tg, tramp, env, slot)` (④b-1) → in ④b-2 `register` instead spawns a `std::thread` running `tramp(env, slot)`; `wait(*tg)` runs/joins all; `end(*tg)` frees the region. ④b-1 keeps it **deferred-sequential** (run at `wait`); ④b-2 swaps the run-loop for thread-per-task + join (reusing `par_map`'s threading).
- **Owned `R` (`string`/`array<T>`)** is the subtle case: the slot holds the owned `{ptr,len}`. `get()` (consuming for a Move `R`, per ④a) moves it out — afterward the caller owns the buffer, while the slot itself stays in the region until the whole region is reclaimed at scope end. An **un-`get()`'d** owned-`R` task must still free its buffer before the region drops: codegen emits a conditional drop of each owned-`R` task at scope end, gated by a **drop flag cleared by `get()`** (the existing drop-flag-via-null pattern, applied to the slot). (Alternative under consideration: make `get()` mandatory for an owned-`R` task — a must-consume rule — so the buffer always moves out and no in-region drop is needed; decide in ④b-1.) Copy `R` needs none of this (the region free reclaims everything).

**④c-2 plan — the `wait()?` error boundary (the last task_group slice).** A task may **fail**: its closure returns `Result<R, Error>`. `wait()?` joins all, and if any task failed, propagates **an** `Err` out of the enclosing function (with parallel tasks there is no deterministic "first" — any failing task's error surfaces; documented). After `wait()?`, `get()` yields the `Ok` `R`. Implementation, in order:
- **Prerequisite — `Result`-returning spawn closures.** A `Result`-returning lambda cannot be a `Ty::Fn` value today (`FnTy.ret` is scalar-only). Since a spawned lambda is *consumed by `spawn`* (never a free first-class value), `check_spawn` should **lift the literal lambda directly** (via `lift_lambda`, whose result type may legitimately be `Ty::Result(ok, ErrCode)`) instead of routing through a `Ty::Fn` value — and the `Spawn` node carries the lifted name + captures + the `Ok` scalar + a `fallible` flag, like `Closure` does. **Infer the lambda's `Err` type from the enclosing function's return type** (no annotation needed): `wait()?` propagates the task error out of the enclosing function, so the task's `Err` must match the enclosing function's `Err` — pass that as the lambda's expected return (`Result<_, EnclosingErr>`), so `spawn(fn { fallible()? ; Ok(x) })` type-checks without a written return type.
- **`get()` requires a *successful* `wait()`.** For a fallible group, a bare `wait()` whose `Result` is ignored does **not** make `get()` safe — an `Err` task never stored its slot, so the slot is uninitialized. So the ④c-1 wait-state flag is set only by `wait()?` (or otherwise handling the `Result` such that control is on the success path) for a fallible group; a bare `wait()` does not enable `get()` there. (For an infallible group `wait()` returns `()` and enables `get()` as in ④c-1.) Thus `get()` is reachable only when `wait()` is guaranteed to have *succeeded*.
- **Per-`task_group` `fallible` flag** (a stack like `wait_state`): set when a `Result`-returning task is spawned. `wait()`'s type is `Result<(), Error>` when the group is fallible, else `()` (so infallible groups stay `()` — no spurious `Result`).
- **Error reporting via the worker's return value (no shared state).** The per-`R` trampoline returns an `i32` error code (`0` = ok): infallible → store `R`, return `0`; fallible → match the `Result`, on `Ok(v)` store `v` and return `0`, on `Err(e)` return `e`. `align_rt_tg_wait` (already `thread::scope`) collects each worker's returned code via `ScopedJoinHandle::join` and returns the first nonzero — no shared error cell, no extra aliasing.
- **`wait()?`**: codegen builds `Result<(), Error>` from `tg_wait`'s code (`Ok(())` if `0`, else `Err(code)`); `?` propagates as usual. `get()` (already `wait`-gated by ④c-1) then reads the `Ok` slot.

### `bytes` / `buffer` — design SETTLED, build deferred until a consumer
**Decision (2026-06-23): `bytes` is `slice<u8>`; `buffer` is a distinct growable owned byte container.** Resolving the two forks left by `draft.md` §12 (which names the types but specs no operations):
- **`bytes` = `slice<u8>`** — a read-only `{ptr,len}` view of `u8` elements (bytes), structurally identical to a slice of bytes (no UTF-8 invariant — that is what distinguishes it from `str`/`string`). Introducing a *separate* structural type would violate **One way** (two names for one thing), so `bytes` is the conventional spelling of `slice<u8>` in byte/I/O contexts, lowered as `slice<u8>`. `s.bytes()` yields a `slice<u8>` view of a string's UTF-8 bytes; `bytes.to_string()` is the UTF-8-validating inverse (`Result<string, Error>`). (FFI already treats `bytes` as a view handed to C by raw pointer — consistent.)
- **`buffer` = a distinct Move type**: an owned, **growable**, mutable sequence of `u8` (the byte analog of a `Vec<u8>`). It is *not* `array<u8>` (fixed length) nor `builder` (an append-only *text* writer that produces a `string`); `buffer` is random-access + growable + freezable raw bytes for the *binary* domain. Ops: `buffer()` / `buffer(cap)`, `.push(b)`, `.append(slice<u8>)`, `.len()`, `buf[i]` read/write, `.bytes()` (view), and freeze → owned `array<u8>` or `.to_string()` (UTF-8 validate). It is the first growable container.
- **Build deferred:** `bytes` largely *exists already* (it is `slice<u8>`); `buffer`'s only real consumers are binary I/O (`std`) and `core.hash`, neither built yet — so building `buffer` now, ahead of a consumer to validate the op set against, risks the wrong shape (premature). The *type* design above is settled; the build lands with its first consumer.
Record: `draft.md` §12, `impl/07-roadmap.md`.

### Ownership syntax
**Decision: ownership is a property of the type, not a keyword.** `array<T>`/`string`/`buffer`/heap are Move; primitives/small structs/`slice` (view) are Copy. No `owned` modifier is introduced. Lifetimes are inferred and lifetime syntax is not surfaced.
Record: `impl/03-types.md` §6–§7

### SIMD exposure (basic policy)
**Decision: `vec<N,T>` + auto-vectorization as the baseline.** Make mask first-class. The fused
pipeline lowers `where` / conditional reductions **branchless** (mask + `select`, not a per-element
branch — `impl/05` §5), which is what keeps hot loops vectorizable and branch-predictor-friendly.
(Whether to place explicit SIMD intrinsics in std is open, see below; **wide SIMD on a varied fleet
comes from the library layer's runtime dispatch — see "Build targets & portability".**)
Record: `draft.md` §9, `impl/04-mir.md` §4, `impl/05-backend-llvm.md` §5

### Memory layout — `soa<T>` (struct-of-arrays) — SETTLED (2026-06-26)
**Decision: the layout is chosen by an explicit type — `soa<T>` — not by automatic whole-program
inference.** Add a first-class columnar collection `soa<User>` (peer to the row-major `array<User>`);
the compiler lowers field access and pipeline stages over it to one contiguous column per field
(SIMD-aligned, `align(N)` when needed). A pipeline touching a subset of fields
(`users.where(.active).pay.sum()`) then streams only those columns — the canonical data-oriented
cache/SIMD win, and the principled form of today's hand-rolled "parallel arrays".

Why explicit over automatic: Align's safe core has no raw pointers / field-address-taking, so the
physical layout is *semantically* unobservable and a compiler **could** auto-transform — but that
hides performance, which fights "predictably fast", and needs an opaque heuristic. An explicit type
keeps the choice visible ("nothing hidden"), predictable, and AI-legible, while the *field-wise
lowering under the type* is the automatic part. This is not a "two ways to do one thing" violation:
`array` (row) and `soa` (column) are distinct tools like `array` vs `slice`. Guidance: default
`array<T>`; reach for `soa<T>` on large, hot, field-wise-processed tables.

Boundaries that assume a byte layout (FFI, `json` encode/decode, by-value pass) **materialize to AoS
explicitly** (a visible conversion). Composes with branchless `where` (masked reduce over columns).
Settles the `impl/05` §3 "automatic vs annotation" OPEN in favor of annotation. Build is M6 (uses the
`Layout::Soa` seam already reserved in `align_sema`).

**Open sub-questions (settle before the M6 build):**
- **Views/borrows of `soa<T>`.** `slice<T>` is `{T* ptr, i64 len}` — strictly AoS — so it cannot
  view columnar data without an `O(N)` materialize. A modular function taking a view of a `soa`
  table needs either a layout-parametric slice or a distinct `soa_slice<T>` (a small struct of
  per-column base pointers + len). Leaning toward `soa_slice<T>` so the AoS `slice<T>` stays a simple
  `{ptr,len}`; decide the exact shape + whether pipelines accept it directly.
- **Move fields in `soa<T>`.** If `T` has an owned field (`string`, `array<U>`), `users[i]` by value
  would move a field out of its column and leave the table invalid. Options: restrict `soa<T>` to
  Copy/plain-data structs (simplest, matches the current struct-field rule), or require explicit
  `.clone()` / return a composite read-only view for whole-element access. Leaning toward
  **Copy-only `soa<T>`** for the first cut (whole-element extraction of a Move element is the rare
  case; field-wise pipelines — the reason to use `soa` — don't need it).

**First slice DONE (2026-06-26):** `Ty::Soa(struct_id)` — a **borrowed, Copy** view of a
primitive-scalar struct, ABI = `{ptr, len}` over a **column-major** single buffer (column `i` at
`ptr + len * prefix_bytes_i`). **First cut requires uniform field width** (all fields the same byte
size), so column `i` sits at `ptr + i*len*size` — always a multiple of `size` (= the field
alignment), hence naturally aligned for any `len`. Mixed-width columns (e.g. `i8`+`i64`) would land
at unaligned offsets for some lengths (→ UB on strict-alignment archs); they need per-column
alignment padding, deferred to a later slice. `soa<T>` type syntax; field projection `ps.field` → the column's
`slice<FieldTy>` (HIR `SoaColumn`, MIR `Rvalue::SoaColumn`, codegen does the column GEP), which then
feeds the normal scalar pipeline (`ps.a.where(p).map(f).sum()`). **Measured ≈7–10× faster than an
idiomatic Rust `Vec<Struct>` field sum** on a memory-bound scan (`bench/` `col_sum`, "Align faster")
— the first place Align beats hand-written Rust. `tests/soa.rs`. The chosen design used a
dedicated `Ty::Soa` (Copy borrowed view) rather than `DynStructArray(_, Layout::Soa)` (owned/Move)
for this borrowed-param cut.

**Second slice DONE (2026-06-27) — multi-column + mixed-width:** a soa source now flows through the
**`Layout::Soa` seam** in the existing struct-array pipeline (not a single-column fold): field access
lowers to `Rvalue::IndexColumn` (`column_base(field) + index`), so a column-spanning pipeline
`rs.where(.active).pay.sum()` reads only the `active` and `pay` columns. **Mixed widths are now
allowed** — each column's start is padded to the field's alignment in codegen (`align_up` chain), so
`soa<{active: bool, pay: i64}>` is well-formed and aligned for any `len`. A whole-struct stage over
soa (`where(fn)`/`map(fn)` taking the struct) is rejected cleanly (it would gather every column —
field projection / `where(.field)` only).
Record: `draft.md` §3.4 / §9, `impl/05-backend-llvm.md` §3, `impl/04-mir.md` §3, `tests/soa.rs`, `bench/`.

### Branchless `where` (sum/count) — DONE (2026-06-27)
**Decision: a `where`/`where(.field)` feeding `sum` or `count` lowers branchless** — AND the
predicates into a `mask`, then `select` the contribution to the reduction identity
(`acc += mask ? value : 0`, count `+= mask ? 1 : 0`) instead of a per-element branch (`Rvalue::Select`
+ `accumulate_mask` in `align_mir`). `reduce`/`any`/`all`/`min`/`max` have no simple identity-masked
form, so they keep the branch. **Why it matters now (it was rightly deferred before):** the
single-column `s.where(p).sum()` over `slice<i64>` already vectorized via LLVM if-conversion — no
gain. But the **soa filtered aggregate** `rs.where(.active).pay.sum()` (bool mask column + i64 value
column) did NOT auto-vectorize — scalar, 20 branches, branch-bound, **0.93× vs Rust AoS** (parity).
After branchless lowering it vectorizes (16 vector ops, no per-element branch) and is **≈3.5× faster
than idiomatic Rust `Vec<Row>`** (`bench/` `total_pay`, "Align faster"). So the soa filtered
aggregate now beats Rust too (the plain column scan stays ~7-10×). `tests/branchless_where.rs`,
`bench/`. (Materialize via stream-compaction — `to_array`/`partition` under a `where` — stays
branchy; that is a separate slice.)

### soa construction — IMPLEMENTATION PLAN (the largest remaining soa gap; RESUME HERE for perf)

**Goal.** Make `soa<T>` usable in real Align programs. Today it is a **borrowed parameter only** — the
benchmark feeds column data from an external Rust harness; pure Align can't *make* a soa. The
winning real-world flow (chosen 2026-06-27) is direct JSON→SoA:
`users: soa<User> := json.decode(data)?` then `users.where(.active).score.sum()` — idiomatic Rust
decodes to `Vec<User>` (AoS) and drags whole records through cache; Align decodes straight to columns
and a scan reads only the touched ones.

**Key constraint (found 2026-06-27).** A JSON array's length N is unknown until parsed, but
column-major SoA needs N to compute column bases. So a *truly* transpose-free decode needs two passes;
the pragmatic correct path is **json → AoS (reuse the tested `JsonDecodeStructArray` parser) →
transpose to a column-major buffer → return the soa view**. JSON parsing dominates total time, so the
one-pass transpose is a small add-on. The **transpose (column store) is the core new primitive**, and
JSON→SoA is then a thin wiring on top.

**Sequence (each a PR, benchmark-driven):**
1. **Column store + `to_soa()` transpose primitive. — DONE (2026-06-27).** `arr.to_soa()` transposes
   an AoS `array<Struct>` (literal or local) into a column-major `soa<Struct>`. Implemented:
   `Rvalue::SoaAlloc { handle, len, struct_id }` (arena-bump the column buffer; total size = the
   per-column `align_up` offset walk to the last column + its `len*size`, buffer aligned to the
   widest field) and `Stmt::StoreColumn { base, len, index, field, struct_id, value }` (the write
   counterpart of `Rvalue::IndexColumn`, sharing a new `soa_column_offset` codegen helper). MIR
   `lower_array_to_soa` runs one fused loop reading each element's fields (`lower_field_access`, AoS)
   and scattering them into their columns; the result `{ptr,len}` view (reusing `MakeDynArray`) is
   `Ty::Soa(id)`, **arena-allocated** — so no new owned type and no per-value drop (arena bulk-frees
   it; `region_of(ArrayToSoa)=arena(depth)`, `tracks_region(Soa)=true`, so escape is checked).
   Sema `check_array_to_soa` requires an arena, an array-of-primitive-scalar-struct source, and (first
   cut) no pipeline stages before it. `tests/soa.rs` (+9): build+two-column sum (66), mixed-width
   alignment (i8+i32 → 42), built-soa→`where(.active).pay.sum()` (15), and the four rejections.
   **Deferred to a later slice:** a `bench/` runtime-data duel of multi-pass `to_soa` (the harness
   feeds AoS data + times `s := arr.to_soa(); s.a.sum()+s.b.sum()` vs re-reading AoS) — single-pass
   `arr.to_soa().a.sum()` LOSES (transpose cost), so the win is the multi-pass amortization, and the
   bench needs a no-`main` kernel taking an AoS `slice` param (the construction-from-param path).
2. **`json.decode` → `soa<Struct>`. — DONE (2026-06-27).** `s: soa<User> := json.decode(d)?` decodes
   the JSON array of objects into a temporary AoS via the tested struct-array parser (N is unknown
   until parsed), then transposes to a column-major `soa<Struct>` and frees the AoS temp. Implemented:
   new `Scalar::Soa(u32)` (so `Result<soa<T>, Error>` is representable — Copy/region-tracked like
   `Scalar::Str`, never dropped); HIR `JsonDecodeSoa { struct_id, input }`; sema arm in
   `check_json_decode` (requires an arena + an all-primitive-scalar struct, so no `str` columns ⇒ the
   soa is self-contained, region-tied to the arena not the input — `region_of(JsonDecodeSoa)=
   arena(depth)`); MIR `lower_json_decode_soa` reuses `JsonDecodeStructArray` for the AoS decode then
   the extracted `transpose_to_soa` helper on the Ok edge + `DropValue` the AoS temp. `tests/soa.rs`
   (+6): decode→`age.sum()` (75), decode→`where(.active).pay.sum()` (15), parse-error propagation,
   and the three rejections. **BENCHED 2026-06-27 (`bench/json_soa/`, vs `serde_json`) — Align
   currently LOSES ≈0.6× (a critical honest finding).** `json.decode → soa<Row> →
   where(.active).pay.sum()` (4-field records, 2 read) vs `serde_json → Vec<Row> → filter/sum`:
   Align 22.6 ms vs Rust 13.8 ms at 100k rows (0.61×), stable across 10k/100k/1M. **The workload is
   parse-bound and the parser is the bottleneck** — `align_rt_json_decode_struct_array` is a scalar
   byte-at-a-time parser vs the heavily-optimized `serde_json`, and Align additionally does
   decode-to-AoS-then-transpose (an extra pass + alloc) where Rust does one `Vec` parse. The SoA
   column-scan win is real (flat `bench/` `col_sum` ~8–10×) but here it is **swamped by the parse,
   which both sides pay in full**. **DECOMPOSED + first parser fix (2026-06-27):** the bench now also
   times Align `→array<Row>` (AoS, no transpose); soa≈aos → **the transpose is cheap, the gap is the
   PARSER**. Hand-rolling `integer()` (was `str::from_utf8(..).parse::<i64>()` — UTF-8 validation +
   generic parse + a second digit pass; now a single-pass `checked` digit accumulation, the int-field
   hot path) moved it **≈0.61× → ≈0.82–0.85×** (AoS ≈parity at 1M). Remaining path to beat serde:
   **scalar tuning is now TAPPED OUT** — the per-element zeroing memset was MEASURED (skip it via
   `set_len`: 0.80–0.81×, indistinguishable from 0.82× — ≲1%, noise; reverted, not worth `unsafe`),
   and the rest is distributed per-byte overhead with no single >5% lever. So the real remaining lever
   is **(a) a SIMD/structural JSON parser** (the big, dedicated, library-layer effort — runtime
   CPU-dispatch / simdjson-class; what it takes to actually *beat* serde's optimized scalar parser).
   Secondary: (b) **two-pass count-then-direct-column-fill** (drops the transpose — small, the
   decomposition showed it cheap; note that for a *light* single aggregate, decode→AoS is already
   ≈parity and beats decode→SoA, so SoA's transpose only pays off under heavy/repeated column scans);
   (c) **field-skip / narrow struct** (already available). Bottom line: json→SoA is a PARSER problem;
   the cheap scalar win is banked (#168), and beating serde now needs the SIMD slice.
3. **Known-schema field-skip / projection decode — DEFERRED 2026-06-27 (the perf is already
   available; the remaining delta is ergonomic-only and safety-sensitive).** KEY FINDING (verified
   2026-06-27): the runtime **already skips every JSON field not declared in the target struct**
   (`parse_object`'s `None => p.skip_value()`, `align_runtime/src/lib.rs:~675` — confirmed by a test:
   a wide `[{id,name,score,age}]` decoded into `soa<{score: i32}>` skips `id`/`name`/`age` and sums
   `score` correctly). So **the field-skip win is obtained today by declaring a narrow struct** with
   only the needed columns. What step 3 would add is skipping fields that ARE declared in the struct
   but not read by a particular pipeline (a wide canonical struct reused across pipelines) — driven by
   a sound whole-function **use+escape analysis** over the decoded local (any non-projection use, or a
   pass-to-fn / return, ⇒ decode all). The gain over "declare a narrow struct" is **ergonomic only**
   (avoid N per-pipeline structs); the perf is the same. And the analysis has a **memory-safety
   failure mode** (skip a column that is actually read ⇒ read uninitialised column bytes), so it must
   be conservatively sound. The inline-temporary form (`json.decode(d)?.where(.active)...`) is also
   **not expressible** — the decode target type can't be inferred from field names alone, and Align has
   no expression-position type ascription. Verdict: not worth a complex, safety-critical analysis for
   an ergonomic-only delta right now. Revisit if a real workload needs a wide reused struct decoded
   cheaply; until then, **document the narrow-struct technique** (done: `draft.md` §9). The next clean,
   self-contained decode win is **perfect-hash field dispatch** (below), chosen 2026-06-27.

**Perfect-hash JSON field dispatch — DONE (2026-06-27).** The runtime field lookup was a linear scan
(`descs.iter().position(...)`); now codegen bakes a **compile-time perfect-hash table** from the
(known) field names and the runtime does an O(1) `hash(key) & (m-1)` → slot → one confirming name
compare. Implemented: `build_phf` in codegen finds a collision-free `(seed, power-of-two size)` by
scanning seeds `0..4096` over sizes `next_pow2(n)..×8` (FNV-1a `phf_hash`); emits a `[i32]` slot→index
global (`jphf`, `-1` = empty) alongside the descriptor table; the two decode entry points gained
`(phf_ptr, phf_len, phf_seed)` args. Runtime `find_field` uses the table (or linear-scans when
`phf_len = 0` — empty/1-field structs, or no table found, so it degrades gracefully). The codegen and
runtime hashes are pinned to the same constant by paired tests (`phf_hash_is_pinned` /
`phf_hash_matches_codegen`) so they can't drift. ≈1.2–2.5× on wide-schema decode; sound (the confirming
compare means an unknown key colliding into an occupied slot is still skipped). `tests/soa.rs` +1
(wide struct, unknown keys, reordered fields → correct sums), codegen +3, runtime +2.

**Deferred soa / decode sub-items (after the above):**
- **bitset** bool columns (count/any/all 8–64× via popcnt; `where(.flag).sum()` only ~1.1–2× — both
  reviewers warn against over-crediting the filtered-sum case, since the value column read dominates).
- **`soa_slice<T>`** (a per-column-pointer view, so a function can take a borrowed soa slice —
  `slice<T>` is `{ptr,len}` AoS and can't); `str`/Move columns.
- **`map_into(out dst)`** pipeline terminal — the minimal construct that makes `out` `noalias`/`nonnull`
  metadata worth emitting (Sema already has the no-alias check; only the LLVM attribute is missing —
  `declare_fn`, `align_codegen_llvm/src/lib.rs:~965`). ≈1.0–1.5×, secondary to construction.
- **`arena.checkpoint()` / `rollback()`** surface API over the existing `align_rt_arena_reset`
  (`align_runtime/src/lib.rs:~1158`) — O(1) reuse of per-iteration transient allocations in a
  long-running loop. ≈1.2–3× on alloc-heavy request loops (but Rust+`bumpalo` competes — bench against
  it). Std/runtime layer.
- **Runtime CPU dispatch** (AVX2/NEON multi-versioning) for JSON scan / UTF-8 / string search — the
  std/runtime SIMD layer (after JSON→SoA), per the settled build-target policy.

**Audit (2026-06-27):** the soa hot loops are clean — `objdump` of `col_sum` / `total_pay` shows zero
`call` / `bounds_fail` in the loop (1 loop, no allocation, no bounds branch), which is why they beat
Rust. No residual-overhead cleanup is needed before construction.

### External benchmark report — Gemini on M2/arm64, Part 2 (2026-06-27, VERIFIED; one bug FIXED)
A second Gemini bench (group_by / par_map / json-decode on arm64). Verified against code:
- **group_by: Align 1.4–4.2× faster than `std::HashMap` on M2** — confirms the x86 `bench/group_by`
  result cross-arch. (A dense flat-array lookup that skips hashing is still faster — a different
  algorithm, not a hash map; expected.) Nothing to do.
- **JSON decode: Align only ~14% slower than `serde_json` (AoS), the SoA transpose adds <3%** —
  confirms the integer-parse fix (#168) landed and the transpose is cheap (matches the x86 decomposition).
- **★ Match double-free (VERIFIED by repro → FIXED 2026-06-27).** `match res { Ok(users) => … }`
  where `res: Result<array<User>, Error>` is a bound local: binding `Ok(users)` moves the array out,
  but the match lowering didn't null the scrutinee, so at scope exit BOTH `res` and `users` `Drop` →
  `align_rt_free` twice → `free(): double free detected` / SIGABRT (reproduced). **A memory-safety bug
  (worse than Gap A's leak).** Fix: `lower_match_binary`/`lower_match_enum` now call
  `null_moved_source` on the scrutinee in any arm that binds a payload (mirrors `?`/`lower_try`), and
  `finish_arm` nulls an owned local *returned* from an arm (`Ok(xs) => xs`) — a second double-free in
  the same area, found while testing, also fixed. `tests/structured_error.rs` (+3: consume / return /
  wildcard). The `?`-workaround Gemini used is no longer needed.
- **par_map thread-spawn overhead — FIXED 2026-06-27 (persistent pool).** `align_rt_par_map` spawned
  raw OS threads via `std::thread::scope` on *every* call (~20–50 µs/thread) → ~7× slower than
  sequential at N=100k. Fixed with a lazily-initialised process-lifetime worker pool (`par_pool`:
  detached workers parked on a `Mutex<VecDeque<Job>>` + `Condvar`; `par_map` submits chunks + a
  fork-join barrier, running one chunk on the caller) + a `PAR_MIN_CHUNK` floor so trivially-small
  maps stay sequential. `bench/par_map/`: 100k went **~7× slower → ≈parity** with sequential.
  **Remaining (recorded): par_map is now ≈sequential parity but still behind `rayon` (0.4–0.6×) for
  cheap work** — the ceiling is the **per-element indirect `thunk` call** (no inlining/vectorization,
  where seq/rayon inline + vectorize). par_map wins on *heavy non-vectorizable* per-element work; the
  cheap-map fix is **inlining the thunk** (same class as the builder per-write overhead — cross-object
  LTO or a specialized monomorphic emit). The shared pool can later back parallel `reduce`/`task_group` too.

**Part 3 / consolidated (2026-06-27): basics confirmed at PARITY on arm64 — no new bugs.** A third
Gemini pass added the fundamentals: **arithmetic + branches** (`math_logic` 0.99×), **recursion /
call ABI** (`recursive_fib` 1.00× — note fib is *non*-tail-recursive, so this confirms the call /
stack-frame convention matches Rust, not just TCO loops), and **struct AoS *and* SoA scanning**
(`sum_coords` 1.00× — stride/offset correctness, SoA transpose adds no scan regression). All parity →
the core codegen/ABI is solid cross-arch; nothing to fix. group_by (1.4–4.2× vs std) and JSON
(~14–17% off serde) re-confirmed. The match double-free is acknowledged **Resolved** (PR #175). The
sole remaining open item it re-flags is the **par_map OS-thread-spawn** gap above (3rd time) — still
the one perf lever in this set, std/runtime layer.

### External benchmark report — Gemini on M2/arm64 (2026-06-27, claims VERIFIED against code)
Gemini ran a 3-workload bench on Apple Silicon (arm64) and filed a gap report. Can't reproduce the
arm64 *numbers* here (linux x86), but every *code* claim was verified against the source. Not urgent
(shared for awareness); recorded so the gaps are tracked.

- **Math pipeline (`map→where→sum`): Align 1.15–1.27× FASTER than Rust on M2 — a positive confirm.**
  The branchless-`select` fusion wins on arm64 (on x86 it was parity — Rust's slice `filter` evidently
  doesn't vectorize as cleanly on arm here). Nothing to do; good signal that the flagship lowering
  holds cross-arch.
- **★ Gap A — `str + str` inside a lifted lambda silently LEAKED → OOM. FIXED 2026-06-27 (now a hard
  error).** `s.reduce("", fn acc, x { acc + x })`: the lambda lifts to a top-level fn whose `lower_fn`
  starts with `b.arenas` empty, so `str+str` (MIR ~757) got `arena = None` → `builder_finish`
  `Box::leak`d the buffer (runtime ~1196) → one leak per reduce step → OOM at N=10k. **Fix:**
  `guard_lambda_alloc_leak` (align_sema) errors on a string allocation (`str + str` / `template` /
  `json.encode` — all desugar to an arena `Template` str) inside a lifted lambda with no arena of its
  own (`capture.is_some() && arena_depth == 0`), pointing at the `builder` pattern — so the silent
  leak is now a clear compile error (Nothing-hidden restored). Legitimate cases unaffected: top-level
  / named-fn concat, the builder-reduce pattern, and a concat inside the lambda's own `arena {}`.
  `tests/lambda.rs` (+6). **Remaining sub-gap (recorded, NOT the
  reported case):** a *named* reducer fn that concats (`fn cat(a,b)=a+b` used as `reduce("", cat)`)
  leaks the same way but isn't caught (the guard is scoped to inline lambdas via `capture`); the real
  fix is **owned `string` from concat** (str+str → a heap `string` with `Drop`, freeing each
  intermediate → no leak, O(1) like Rust — also dissolves Gap B), the deferred M5 feature.
- **Gap B — `acc + x` string reduce is O(N²) arena space even if A were fixed.** Arena has no
  per-object free, so all N intermediate strings live until block exit (Rust frees each `acc`
  immediately → O(1)). Inherent; the answer is **guidance/lint: use `builder` for string
  accumulation, not `reduce(+)`** (a perf-rail lint candidate — Codex's idea). Not a core fix.
- **Gap C — `builder(capacity)` — DONE 2026-06-27 as a feature, but MEASURED *not* to be the lever.**
  Added the surface (`builder()` / `builder(capacity)`, an `i64`) + `align_rt_builder_new(arena, cap)`
  → `Vec::with_capacity`. **But `bench/string_builder/` shows `+cap` ≈ `build` (2.77 vs 2.77 ms) — the
  residual ~1.5× vs optimized Rust is NOT the realloc** (hypothesis was wrong, measured). It's the
  **per-append FFI call overhead**: 3 `align_rt_builder_*` calls per element (~300k extern calls at
  N=100k), not inlined, vs optimized Rust inlining `push_str`+itoa (~0.9 ms ≈ 300k × ~3 ns). Capacity
  is still a legitimate nothing-hidden primitive (helps *realloc-bound* building), just not this
  per-write-call-bound workload. **Real remaining lever (recorded): inline / batch the builder
  appends** (remove the per-`write` FFI boundary) — a codegen/runtime concern. (Float write still uses
  the generic formatter; `ryu` is the float analogue of the int itoa.)
- **Gap D — `align_rt_builder_write_int` used `write!(b.buf, "{v}")` — DONE 2026-06-27.** Replaced with
  a back-to-front itoa straight into the buffer (negative-magnitude accumulation so `i64::MIN` works;
  the JSON integer hand-roll #168 in reverse). **Halved the gap to optimized Rust** (Gemini Part 1 had
  the old builder ~2.8× slower; now ~1.5×) and ties/beats naive Rust. `bench/string_builder/` (new);
  runtime test `builder_write_int_matches_format`.

Remaining: **inline/batch builder appends** (the measured string-builder lever — per-write FFI
overhead, not capacity) and **Gap B** (perf-rail lint, with the broader lint work). Gaps A (leak),
C (capacity) and D (itoa) are DONE; none of the rest block current soa/analytics work.

### Column-oriented `group_by` — FIRST SLICE DONE + BENCHED (beats default Rust everywhere)
**Implemented (2026-06-27):** `s.group_by(.key).sum(.value)` over a `soa<Struct>` local → `(array<i64>,
array<i64>)` (distinct keys, per-key sums). HIR `ArrayGroupSum { base, struct_id, key_field,
value_field }`; sema detects the `X.group_by(.key).sum(.value)` chain (`as_group_by` + the `.sum(.field)`
arg), requires a soa local + i64 key/value (first cut); MIR `lower_array_group_sum` projects the two
columns (`SoaColumn`), heap-allocs two owned output buffers, calls `Rvalue::GroupSum` →
`align_rt_group_sum_i64`, then builds the result tuple (owned arrays, so it can escape). `tests/soa.rs`
(+5: aggregate-by-key 142 / 3 groups, type-check, and the rejections).
**BENCHED (`bench/group_by/`, 1M rows, vs std HashMap + ahash): Align beats the DEFAULT `std::HashMap`
(SipHash) everywhere (1.2–3.6×) and beats even `ahash` for low-cardinality grouping (1.31× at 100
groups); it loses to `ahash` at high cardinality (0.52–0.72×).** The benchmark caught a mechanism bug —
the first cut sized the table to `2·n` (row count), allocating a ~34 MB table regardless of group
count and thrashing cache (lost ~9× to ahash at 10k groups, 0.11×); fixed by **growing the table to
track the live group count** (start 16, double+rehash past 0.75 load), which is why it now beats std
across the board (the "benchmark before claiming, reconsider the mechanism" mandate paying off). **To
beat `ahash` at high cardinality (recorded, not done): a SwissTable-style layout (interleaved
key+value, SIMD control-byte probing) + a stronger/faster hash** — secondary, since Align already
beats the *default* map everywhere. **NEGATIVE result (2026-06-27): just interleaving the table into
one `{key,acc,used}` array (without SIMD control bytes) REGRESSED it** (1M: 52 → 77 ms, 0.74× → 0.49×
vs `ahash`) — for linear probing the three dense parallel arrays are better (the `used`/`key` arrays
pack many entries per cache line for probe-chain scans; a 24-byte interleaved slot packs ~2.6/line +
a bigger footprint). So the current 3-array layout stays; beating `ahash` needs the *full* SwissTable
(SIMD control-byte group probing + AES-class hash), not a naive interleave — a big, bounded-value
effort, deferred. **`min`/`max`/`count` aggregates — DONE 2026-06-27** (`group_by(.key).min/max(.value)`
and `.count()`; `ArrayGroupAgg{op}` + a monomorphized runtime `group_agg_i64` over per-op
`per_row`/`combine`, `align_rt_group_{sum,min,max,count}_i64`).
**Dense-id path — DONE 2026-06-29 (the codex P0 win; beats the *fast* baseline everywhere now).**
`group_agg_i64` now picks one of two strategies from an O(n) min/max pre-scan: when the keys span a
tight integer range (`max - min < n`, so a direct-indexed accumulator is never larger than the key
column), it aggregates by `acc[key - min]` — no hashing, no probing, keys emitted already sorted —
otherwise it falls back to the existing linear-probe hash table. The `< n` guard keeps the dense array
bounded by the input (a sparse-but-wide key set falls back rather than allocating a giant mostly-empty
array), and the pre-scan bails the instant the span reaches `n`, so sparse data pays only a partial
scan. **No surface / return-type change** — a pure runtime mechanism (one op, the runtime picks the
strategy, like an adaptive sort). **RE-BENCHED (`bench/group_by`, 1M rows, native): now beats BOTH std
SipHash (5.0–5.7×) AND `ahash` (2.06× / 2.32× / 2.74× at 100 / 10k / 632k groups)** — the previous
hash path *lost* to `ahash` at 10k/1M groups (0.52–0.72×); the dense path flips those to clean wins and
cuts the 1M-group time ~7× (≈54→7.9 ms). The bench's keys are `LCG % groups` (range `[0, groups)`),
so all three configs are dense — exactly the dense-id workload this targets. The remaining "beat ahash
on a *genuinely sparse* high-cardinality key set" case still wants the full SwissTable (deferred above).
**String-key path — DONE 2026-06-29 (the dictionary-id rail, hidden form).** `xs.group_by(.name).sum(.value)`
over an AoS `array<Struct>` (a `soa` can't hold a `str` column) yields **`(array<str>, array<i64>)`** —
the same columnar shape as the i64 path, just `K = str`, so it stays one-way (the user writes the same
`group_by(.key).sum(.value)`; no dictionary type is exposed). The runtime (`align_rt_group_sum_str`)
**interns** the `str` keys to dense ids while scanning (one string hash per row, recording the first
occurrence's view as the group representative) then aggregates by id — so the per-row work after
interning is direct-index, not per-step string hashing/probing like `HashMap<&str, Acc>`. The output key
views **borrow `base`** (region-tied; the owned key/value buffers are `Drop`-freed, their `str` elements
are not). New machinery: `ArrayGroupAgg.key_str`, MIR `GroupAggStr` (codegen derives the per-row stride +
key/value byte offsets from the struct layout via `target_data`), `PrimScalar::Str` (so `array<str>` is a
payload/tuple element). Source = AoS, `str` key + `i64` value, **`sum`/`min`/`max`/`count`** (the runtime
`group_agg_str` is generic over `value_at`/`combine`, monomorphized per op into
`align_rt_group_{sum,min,max,count}_str`; `count` reads no value column).

**A2 — the dictionary reuse rail — DESIGN + foundation 2026-06-29; SURFACE DONE + BENCHED 2026-06-29
(verdict: reuse helps vs naive, but does NOT beat fast single-pass Rust — see the bench finding below).**
Chosen surface form (user 2026-06-29): the **encoded-column** form (keeps One-Way), *not* an exposed
id-column. `e := s.dict_encode(.name)` is an explicit one-time transform (visible cost) that interns the
`.name` `str` column to a **dense id column** + a **dictionary** (`array<str>`, `dict[id] = str`),
carried on the result; then `e.group_by(.name).sum(.v)` / `.max(.w)` / … reuse the *same surface as A1*
but run on the **i64 id column** (the dense-id `align_rt_group_*_i64` from #209) and re-label results
through the dictionary → still `(array<str>, array<Acc>)`. The intent: the string interning is paid
**once** (in `dict_encode`), so repeated group-bys on the same key are integer-column work. (The
original ~19–21× projection was **wrong** — the bench below measures **2.4–3.5× vs naive Align** and a
**loss, 0.31–0.70×, to fast single-pass Rust**.) Region: the dictionary's `str` views borrow the source, so the
encoded value is region-tied to it. **Slices:** (1) **DONE (#218)** — the runtime primitive
`align_rt_dict_encode_str` (intern a strided `str` column → `out_ids[n]` dense-id column +
`out_dict[count]` dictionary; first-occurrence id order; tested). (1b) **DONE (#220)** — the label
primitive `align_rt_dict_lookup` (ids → `dict[ids]`) + a runtime integration test proving the **full
composition** (`dict_encode` → dense-id `align_rt_group_sum_i64` on the ids #209 → `dict_lookup`) equals
the one-shot A1 string `group_by`. **So the entire A2 runtime mechanism is built and validated — the
correctness is de-risked; what remains is purely the compiler surface.**
(2) **DONE — the compiler surface (`e := s.dict_encode(.name)` + reuse).** Delivered as designed
(a–d), one new type through all layers. **(a) type** — `Ty::DictEncoded(struct_id, key_field)` (two
`u32`s carried *in* the variant, like `StructArray(u32,u32)` — no side table needed); a Move,
region-tracked value laid out as **three `{ptr,len}` slices** `{ source (borrowed AoS), ids (owned i64
column), dict (owned str dictionary) }`. First cut = a local used immediately by `group_by` (no `Scalar`
variant). A `Scalar::DictEncoded` stays the follow-up, needed the moment a `DictEncoded` is **returned or
wrapped** (`Result<DictEncoded, Error>`, or returning one whose AoS source is a parameter) — Align
restricts `Option`/`Result` payloads to `Scalar`s. **(b) sema** — `check_dict_encode(recv: array<Struct>
AoS, .key: str field)` → `Ty::DictEncoded`; HIR `ExprKind::ArrayDictEncode { base, struct_id, key_field }`;
region = source's; threaded through the 4 HIR walkers (effect / escape / movecheck / finalize) +
`region_of` + the Move/drop predicates (`is_owned_droppable`/`ty_is_move`/`tracks_region`). **(c)
MIR/codegen** — `lower_dict_encode` loads the AoS, `HeapAllocBuf`s ids (i64×n) + dict (str×n), calls
`align_rt_dict_encode_str` (codegen derives stride + key byte offset via `target_data`), and assembles the
3-slice value (`MakeDictEncoded`); **Drop** frees fields 1+2 (ids, dict), never field 0 (the borrowed
source). **(d) `group_by(.name)` on `DictEncoded`** — a third `GroupSource::Encoded` arm in
`check_group_agg` (validates the group key == the encoded key); `lower_array_group_encoded` extracts the
three slices (`DictField`), gathers the chosen i64 value column out of the borrowed AoS into a contiguous
buffer (`align_rt_gather_i64`, the one tiny new runtime plumbing — see below), runs the dense-id
`align_rt_group_*_i64` over `(ids, vals)`, then `align_rt_dict_lookup` labels the distinct ids → result
`(array<str>, array<i64>)` (same shape as A1). Covers `sum`/`min`/`max`/`count`, `str` key + `i64` value,
AoS source. End-to-end test `dict_encode_reuse_matches_a1_string_group_by` proves reuse across three
aggregates equals the one-shot A1 str group_by. (New runtime: `align_rt_gather_i64` — gather a strided i64
column to contiguous; the value projection of an encoded group_by. Trivial plumbing, unit-tested.)
**(e) bench — DONE (`bench/group_by_reuse`, 1M rows, 4 aggregates `sum a/sum b/max c/min d`).** Result
(native): **a1/a2 = 2.4–3.5×** (a2 reuse beats Align's naive 4× str group_bys — the reuse is real and
widens with cardinality), a2 also beats *naive* Rust (4× `HashMap<&str>`), **but a2 LOSES to fast
single-pass Rust (`HashMap<&str,[i64;4]>`, one hash + 4 accumulators): `smart/a2` = 0.31–0.70×** (Rust is
1.4–3.2× faster). Per the mandate (only a win over the *fast* baseline is honest), **A2 as a batch of
separate group_bys does not beat idiomatic fast Rust.** Why: smart Rust makes **one pass** (hash once,
update 4 accumulators); a2 hashes once via `dict_encode` but then makes **four more passes** (gather +
dense-id aggregate + label, each with a malloc) — reuse removes the re-*hashing*, not the re-*scanning*.
**Root cause (understood, marked — not chased now):** it is structural (pass count × allocation), not
hashing. Three culprits, in impact order: (1) **N passes vs 1** — a2 = `dict_encode` (1 hash pass, ≈ all
of smart Rust's work) + 4×(gather pass + aggregate pass), while smart Rust does one pass; (2) **per-call
`malloc`/`free` of n-sized scratch** (gather buf + out_ids + out_vals + labels, ~3–4 × 8 MB per
aggregate); (3) **the gather pass is pure waste** — it materializes the strided value column to
contiguous only to feed the contiguous-input `group_i64`; a fused design reads the value inline. The
cardinality trend confirms it's fixed overhead: `smart/a2` worsens to 0.31× at 100 groups (overhead
dominates) and eases to 0.70× at 632k (hashing dominates). Fixes map 1:1 to deferred items — fuse the K
aggregates (cause 1+3), arena-allocate the scratch (cause 2).
**Roadmap consequence (the bench's job): the real lever is "multiple aggregates in one pass"** — fuse K
aggregates into one scan filling K result columns. **FIRST CUT DONE 2026-06-29** — the fused
`group_by(.key).agg(sum(.a), max(.b), count(), …)` surface (parser interprets `sum(.f)`/`count()` args;
sema `check_group_agg_multi` → `hir::ExprKind::ArrayGroupAggMulti`; MIR `Rvalue::GroupAggMultiStr`;
runtime `align_rt_group_multi_str` does one pass — intern key once, fold K accumulators — with a fast
FxHash-class hasher, not SipHash). Result: bench `a3` **beats a1 (naive) 3.2–3.7× and beats a2
(dict_encode reuse) everywhere**, but **still loses to smart single-pass Rust (0.42–0.77×)**: hashing is
no longer the cause (FxHash finalizer ≈ ahash), the remaining gap is the per-call `malloc` of `n`-sized
output columns + the dense-id `acc[id*K+j]` indirection vs smart Rust's inline `[i64;K]` map value. So
fusion landed the structural win (cause 1: N passes → 1); **still open** to beat fast Rust: right-size /
arena the output columns (cause 2), an inline-value accumulator layout, plus the deferred non-headline
sources (i64-key soa / precomputed `dict_encoded` multi-aggregate), a `group_by(.key)` lambda key, and
the `Scalar::DictEncoded` (return/wrap) follow-up. A2's honest niche stays **sequential/interactive**
reuse (aggregates arriving over time, not fusible into one pass). Design ↓.
**Surface positioning — DECIDED 2026-06-29 (Codex overreach review).** `dict_encode` is an **advanced
explicit escape-hatch**, NOT the way users learn `group_by`. The one-way user story stays
`xs.group_by(.key).sum(.value)`. What is **decided** is the *positioning* (dict_encode = escape-hatch);
the **intended** (not-yet-ratified) primary multi-aggregate surface is a fused
`xs.group_by(.key).agg(sum(.revenue), max(.score), count())` (one pass, K result columns — the "multiple
aggregates in one pass" lever above, given a user-facing form; the exact `.agg(...)` grammar is a
proposal, not settled syntax). `dict_encode` then remains a lower-level
reuse rail for the sequential/interactive niche, not a general dictionary/id-column API. Guardrails
(Codex): keep first-class `group_by` narrow — columnar result `(array<K>, array<V>)` / small tuple of
arrays, no exposed hash/table-strategy knobs, no arbitrary user aggregate lambdas; add multiple
aggregates **before** arbitrary key/agg lambdas. `dict_encode` is **not** promoted in `draft.md` (the
spec's group_by story is the clean `group_by(.key).sum(.value)`) — keep it that way; it stays an
implementation-tracker rail until the `.agg(...)` surface lands.

### Column-oriented `group_by` — DESIGN / runway (the next analytics headline)
The next "Align beats idiomatic Rust on a realistic workload" pillar after json→soa: grouped
aggregation. Idiomatic Rust reaches for `HashMap<K, Acc>` (SipHash by default, generic, per-entry
churn, cache-unfriendly); Align can lower a **column-oriented group-aggregate** fed by sequential
soa column reads. `group_by` is in the `draft.md` op list; the roadmap (`impl/07` #5) says **design
the return type first** — done here.

- **Return type = columnar, NOT a map.** `xs.group_by(.key).sum(.value)` yields **`(array<K>,
  array<Acc>)`** — two parallel owned arrays (distinct keys, per-key aggregate), reusing the
  `partition` tuple-of-two-owned-arrays result machinery (`Ty::Tuple` of two `DynArray`s). This is the
  data-oriented form (no general `HashMap` in the surface; Codex agreed "not a general HashMap") and
  sidesteps the "groups as a first-class container" problem (which would need generic containers,
  deliberately not built).
- **Surface.** `xs.group_by(.key).sum(.value)` — `group_by(.key)` takes a field-shorthand like
  `where(.active)`; the following reduction names the value field. **`sum`/`min`/`max(.value)` and
  `count()` are implemented** (one key field, one aggregate). (Later: multiple aggregates in one pass
  → more result columns, a `group_by(.key)` with a lambda key, string keys.)
- **Mechanism = open-addressing hash-aggregate.** A primitive-key, no-boxing, linear-probing table
  (the win lever vs std HashMap): hash the key, probe, insert or accumulate. Inputs are soa columns
  read sequentially. Runtime helper `align_rt_group_sum_i64(keys_ptr, vals_ptr, len, out_keys,
  out_vals, cap) -> count` for the first slice (i64 key + i64 sum); emits distinct keys + sums into
  two caller arrays. **Table allocation:** the first-slice primitive uses an internal heap `Vec`
  (one `malloc` per call, amortized over all elements) to stay self-contained + unit-testable;
  allocating the table in the caller's arena (to drop that one `malloc` when `group_by` runs in a hot
  loop) is a **refinement** for once the wiring threads an arena — secondary to the aggregate itself.
- **First slice scope:** `i64` key + `i64` value + `sum`, source = `soa<Struct>` or `array<Struct>`
  (read the key + value columns). Output `(array<i64>, array<i64>)`. Requires an arena (the hash
  table is arena-allocated, like `to_soa`); the result arrays are owned (heap, `Drop`-freed) so they
  can escape.
- **BENCHMARK-DRIVEN (the json→soa lesson):** the "beats Rust" is a CLAIM until measured. Bench vs
  Rust **both** `std::collections::HashMap` (SipHash) AND a fast idiomatic baseline (`ahash`/`FxHashMap`)
  — only a win over the *fast* baseline is honest. Measure right after the first slice; if the
  specialized table doesn't beat `ahash`, reconsider the mechanism (radix partition? two-pass?) before
  building more.
- **Deferred within group_by:** the *exposed* dictionary-encode / id-column reuse rail (the ~19–21×
  multi-aggregation reuse — needs a new id-column/dictionary data model), multiple aggregates in one
  pass, lambda keys, and parallel (per-chunk partial tables + merge). (`min`/`max`/`count` for i64
  keys, the **dense-id fast path**, and **string keys (hidden dictionary-id form,
  `sum`/`min`/`max`/`count`)** are DONE — see above.)
- **Why design-first, not rushed:** per "ideal form or defer" + roadmap #5 — the return-type and
  mechanism are the load-bearing decisions; the above fixes them so implementation PRs are mechanical.

### Additional perf levers — own code-grounded review (2026-06-27, empirically checked)

Beyond the JSON→SoA / field-skip thrust (which both external reviews converged on), two orthogonal,
*cheap* levers that neither external review surfaced — found by reading the codegen + disassembling:

- **Emit the LLVM function attributes Align can soundly assert.** The function-level generalization
  of the out-param `noalias` idea — broader, since it applies to *every* function.
  - **`nounwind` on all Align functions — DONE (2026-06-27).** Align functions never unwind (errors
    are `Result` values; a fatal fault `abort`s, it does not unwind — settled "no unwinding"; codegen
    emits plain `call`, never `invoke`). `mark_nounwind` (`align_codegen_llvm`) tags every
    **Align-generated** function — program fns (`declare_fn`), the C `main` wrapper, and the fn-value
    / closure thunks — but **not** the external `align_rt_*` runtime declarations (ordinary Rust fns,
    not promised nounwind here). Lets LLVM drop exception edges / unwind tables and inline more
    aggressively. Verified in IR (`attributes #0 = { nounwind }`); test
    `align_functions_are_marked_nounwind`.
  - **`memory(none)` / `readonly` on pure functions — DEFERRED (purity ≠ readonly).** Align's
    inferred purity (`EffectScan`) means only **"no observable I/O side effect"** — it *explicitly*
    counts arena/heap allocation, builder use, and reads/writes through args as **pure** (see the
    `check_parallelism` doc-comment). So a "pure" Align fn may allocate and touch arg-pointed memory →
    asserting LLVM `readnone`/`readonly` would be **unsound** (LLVM could CSE/DCE a call that really
    allocates). A sound version needs a *stricter* analysis ("allocation-free + no arg writes, reads
    only through args" → `readonly`; "scalar args only, no alloc" → `readnone`). Worth it only for
    non-inlined pure calls with loop-invariant args — pipeline stage fns are inlined by fusion, so the
    attr is usually moot. Deferred until that stricter analysis exists.
  - Remaining sound-but-unbuilt: `noalias`/`nonnull`/`dereferenceable`/`align` on pointer args —
    blocked the same way (`nonnull` is false for an empty `{null,0}` slice; aggressive `noalias` wants
    the `map_into(out)` write-construct, deferred above).
- **Compile-time pipeline evaluation = zero-cost lookup tables.** Verified: a pipeline over literal /
  const data **constant-folds entirely** (`[1..16].sum()` → `mov $136`, no loop). So a declarative
  `[...].map(f)` that builds a lookup table (CRC/hash/codec/math LUT) costs **zero at runtime** (a
  const global), where idiomatic Rust needs `const fn` (float/alloc-limited) or a build script. **Gap
  /prerequisite:** top-level constants (PR #145) are scalar/string only — **aggregate (array)
  constants don't exist yet**, so a top-level const *table* can't be expressed; that is the
  prerequisite slice. Confidence: high (folding observed). Win is for table-driven code only.

**Audit — ruled out a risk (2026-06-27):** Align has no loops (map/reduce + recursion), so tail
recursion *must* match a Rust loop. Verified: `fn sum_to(n, acc) = if n==0 {acc} else {sum_to(n-1,
acc+n)}` compiles to a **call-free 14-instruction tight loop** (`run(1e6)` correct) — LLVM converts
the tail recursion to a loop at O2. So the loop-less design is not a perf liability for tail-recursive
algorithms.

### External idea-generation review — Gemini (2026-06-27, UNVERIFIED candidates)
Gemini was asked for Rust-beating perf/architecture ideas (advanced-model pass). Treated as
idea-generation, vetted against the code + settled decisions; **not yet independently benchmarked**.
Verdict per idea (most are already shipped/planned or conflict with a core invariant — the one new
convergent signal is the function-attributes lever above):

- **Function attributes (`noalias`/`nounwind`/`dereferenceable`/`align`).** ✓ Converges with the
  "Additional perf levers" item above (codegen emits zero attributes today). Strengthens that lever's
  priority. `nounwind` + pure-fn `memory(none)`/`memory(read)` are the cheap, sound first cut; aggressive
  `noalias` still needs the `map_into(out)` write-construct (deferred above). **Best actionable item.**
- **Bitset bool / `Option` columns.** Already a deferred soa sub-item above. Real but bounded
  (popcnt `count`/`any`/`all` 8–64×; `where(.flag).sum()` only ~1.1–2× — value-column read dominates).
- **Tagged-array dispatch (batch a sum-type array by variant).** FUTURE / speculative. Note: Align
  has **no `dyn`/vtable** (grep = 0; OOP + generics are non-goals/CLOSED), so this solves a
  non-problem today; the underlying "SoA-for-sum-types, tag-partition then batch" is a possible far
  future idea only if a real polymorphic-array workload appears.
- **Evaluated and NOT pursued (recorded so they aren't re-proposed):**
  - *Hidden default arena allocator.* ✗ Violates **Nothing-hidden** + predictable performance (and
    the settled memory-model v2). Arena is correct but stays **explicit** (`arena {}`, already
    ergonomic); the request/task-scoped pattern is expressible today.
  - *Chunked / tiled SoA (AoS-of-SoA), auto.* ✗ Premise (row-access L1 thrashing) doesn't fit
    Align's access pattern — soa pipelines are **column streams** (`s.field.sum()`), where plain SoA
    is optimal (max bandwidth + HW prefetch); chunking helps only same-row multi-column access (the
    AoS case). Also conflicts with the settled "layout chosen by explicit type, not whole-program
    inference." Revisit only if a real row-wise soa workload appears.
  - *Transpose-free one-pass JSON→SoA.* ✗ Not possible for arrays — N is unknown until parsed, so
    column bases can't be computed up front (the AoS→transpose path, shipped #161, is the correct
    form; the perfect-hash #162 covers the parse-speed angle).
  - *Blanket `if`→`select` predication for all branches.* ✗ `select` evaluates both arms — wrong for
    side-effecting / expensive / early-exit (`return`/`?`, the settled cold-path Err) branches; LLVM
    already if-converts profitable branches at O2. The **targeted** branchless `where` (#156) is the
    right scope.

(Codex's parallel report, when shared, gets the same treatment — record useful candidates here as
unverified, verify later; current soa/decode work takes priority.)

### External idea-generation review — Codex (2026-06-27, UNVERIFIED candidates)
Codex's parallel "how Align beats Rust" pass — **code-grounded** (cites real `file:line`, knows the
shipped state), so weighted higher than a feature catalog. Recorded as idea-generation; verify later.

**Guiding framing (worth adopting):** the win is not "a stronger optimizer than Rust" (flat scalar
loops hit ~parity — same LLVM) but **"a language where the slow form is hard to write"** — naturally
steering AI-written code to SoA / fusion / arena / zero-copy / sink-first I/O instead of Rust's
default `Vec<Struct>` / `serde_json` / owned `String` / unbuffered output. The reason to use a minor
language. This is the existing one-way / nothing-hidden / data-oriented stance, sharpened.

- **Converges with already-recorded items (raises their priority):**
  - **LLVM attributes** (`nounwind`, pure-fn `memory(none)`/`memory(read)`, `noalias`, `nonnull`, `align`,
    cold-path-Err edge metadata). NOW THREE INDEPENDENT REVIEWS converge (own code-review + Gemini + Codex)
    → the strongest-supported next perf slice. See "Additional perf levers" above. (codegen still emits
    zero — verified again 2026-06-27.)
  - **Bitset bool columns** (popcnt `count`/`any`/`all`). Also 3-way convergent; deferred soa sub-item.
  - **`map_into(out)` / surface `noalias`** — already a deferred soa sub-item; Codex endorses as the
    SIMD scaffold.
  - **Runtime CPU dispatch** (AVX2/NEON for JSON/string/hash, baseline-binary-safe) — already SETTLED
    in the build-target policy (library layer).
  - **Narrow-struct field skip already works** — Codex independently confirms the "declare only the
    fields you need" experience (verified + documented in `draft.md` §9; the auto known-but-unused
    version stays deferred). And **no hidden auto-SoA** — Codex agrees it must stay explicit `soa<T>`
    + lint guidance (matches the Gemini-review rejection above).

- **New candidates worth carrying (unverified):**
  - **★ Performance-rail lints + "missed performance rail" diagnostics.** The concrete mechanism for
    "hard to write the slow form": the compiler *suggests* (not errors) the fast Align shape — e.g.
    `array<Struct>` field-scanned more than once → `to_soa()`; many decoded fields unused → narrow
    struct; `io.stdout.write(x.to_string())` → pass the builder directly. Distinctive and highly
    on-philosophy; pairs with the formatter below.
  - **★ Column-oriented `group_by` + aggregate** — the next headline after json→soa:
    `json → soa → group_by → aggregate`. Primitive-key-specialized (radix/hash), arena-allocated,
    string keys interned/dictionary-encoded — *not* a general `HashMap`. The data-processing-language
    win. Big-ticket; design slice of its own.
  - **View-first / sink-first std + buffered I/O.** `print` locks+flushes stdout every call
    (`align_runtime/src/lib.rs:~19`) — it's the debug path; the fast path is `builder →
    io.stdout.write(builder)` / a buffered writer (the no-`to_string()` API is already right, make it
    standard). Std should be `read_file_view`/`mmap`, `json.decode(view)`, `json.write(out, value)`,
    `csv.scan(view)`, `io.copy`/`writev` — never materialize an owned string in the hot path. (Std
    layer — after core; records the *direction*.)
  - **Two-pass JSON→SoA (count then direct column fill).** The eventual form of json→soa: a structural
    count pass for N, allocate columns, then fill columns directly — dropping the AoS intermediate +
    transpose (the shipped #161 path). `str` columns via an offset+len column borrowing the input, or
    a string arena. Refinement, not a redo.
  - **Formatter (implement).** CONFIRMED not implemented (only `format_diagnostics`; the settled
    source-formatter that normalizes spacing/`;`/trailing-comma is absent). Needed to converge
    AI-generated code / docs / tests. Pairs with the perf-rail lints.
  - **FFI "borrow-engine" wrapping for heavy libs** (zstd / sqlite / simdjson-class) — don't reimplement
    in pure Align; wrap via FFI as borrow engines (FFI is the library layer per `non-goals`/memory).
  - **Expand `bench/`** beyond flat / col_sum / total_pay: AoS-vs-SoA, json→soa,
    fs→json→aggregate→write, par_map, task_group — each vs a Rust baseline.
  - **Build robustness — runtime-archive staleness (CONFIRMED, fix later).** `runtime_archive()`
    (`align_driver/src/lib.rs:~149`) path-locates `libalign_runtime.a` near the exe with **no cargo
    artifact-dependency edge**, so a runtime-source change not followed by a full `cargo build` links a
    stale archive. Codex flagged it as recurring. Fix candidate: an artifact dep / build.rs
    `rerun-if-changed` / a source-vs-archive mtime assertion in the driver.

- **Anti-recommendations (all align with existing non-goals):** don't chase Rust trait/generic/async
  (generics CLOSED; async = a far future `task_group`-first story); no early *general* async runtime
  (task_group + fast blocking batch I/O first); don't write all of std in pure Align (FFI-wrap the
  heavy engines).

(Both external reports are idea-generation; the convergent + on-philosophy items above are recorded
as unverified candidates. Current soa/decode work takes priority; benchmark before shipping any.)

### Codex perf / I/O / LLM research sweep (2026-06-28, BENCHMARKED) — verifies prior candidates + new rails
A second Codex pass that **ran probes** (host: AMD Ryzen 9 5950X, 32 logical CPUs, x86_64 AVX2),
upgrading several previously-UNVERIFIED candidates above to measured numbers, and adding new ones.
Raw memos + probe sources live under `work/` (gitignored; the durable signal is captured here). Each
number is a Rust micro-probe, not yet an Align `bench/`; treat as direction + magnitude, re-bench in
Align before shipping.

**Independently re-run on this host (2026-06-28) — claims reproduced, NOT just transcribed.** The
Align-vs-Rust `bench/` suite (both sides pinned to the same `--target-cpu=native`, alternating-min
timing) and the `work/` probes were re-executed here; magnitudes vary run-to-run (cache warmth /
frequency scaling) but every conclusion held:
```text
Align-vs-Rust (bench/, head-to-head):
  sum_sq_pos (flat pipeline)        1.00x  = parity (same LLVM; not the win lever)
  col_sum  soa vs Rust Vec AoS     ~11-12x Align faster (0.084-0.093 ratio)
  total_pay soa where().sum() AoS   ~3.5x  Align faster (native; 7x seen only at baseline tier)
  group_by vs std HashMap           4.4x / 1.4x / 1.9x  (100 / 10k / 1M groups) — beats std everywhere
  group_by vs ahash                 1.8x / 0.59x / 0.93x — wins low-card, loses high-card (→ SwissTable)
  json decode soa vs serde_json     ~0.89x (parse-bound; SoA transpose loses, AoS ~parity at 1M)
  par_map heavy vs Rust seq         2.1x / 8.4x / 15.9x — heavy fn wins; cheap fn LOSES to seq (0.2-0.9x)
Rust-only runtime probes (justify the runtime-level levers):
  skip_number lexical               3.13x   mmap view 12.3x   stdout buffered 374x
  fs.read_file direct read 1.84x    AVX2 structural scan 6.6x   dictionary-id reuse ~21x
  I/O overlap (task_group) 17x
```
So the numbers below are verified on this machine, not transcribed from a memo. **License/patent posture:** the references checked (Arrow, simdjson, Abseil
SwissTables, Velox, io_uring, GGUF/llama.cpp) are **design references only** — implement any adopted
idea from scratch; do not vendor their code; keep compression/codec choices pluggable and conservative.

- **SHIPPED from this sweep:**
  - **JSON unknown-numeric lexical skip — DONE (#191).** `skip_value` parsed unknown numeric fields
    to `f64` only to discard them; now `number_span` is shared and `skip_number` advances without
    parsing. **~3.1×** unknown-number skip (87.6→28.1 ms / 1M records, 6M skips); makes narrow /
    projected struct decode reliably faster. (Closes the "narrow-struct field skip" follow-up.)

- **Upgraded to BENCHMARKED (raises priority of items already recorded above / in Future):**
  - **`fs.read_file` extra copy → direct read — ~1.8×** (150.8→83.9 ms / 128 MiB). Runtime does
    `std::fs::read` then `copy_nonoverlapping` into an `align_rt_alloc` buffer (`align_runtime/src/
    lib.rs:~219`); allocate the owned buffer first and `read_exact` into it. Small bounded next slice
    (the natural #191 follow-up). Zeroing was not a measurable cost on this host.
  - **Buffered / sink-first stdout — ~355×** vs flush-each-line (30.1 ms→0.085 ms / 100k lines; one
    big write 8000×; `writev` 120×). Confirms the "view-first / sink-first std + buffered I/O" Codex
    candidate above: `print` is the debug path (locks+flushes every call); the fast rail is
    `builder → io.stdout.buffered() → write → flush`. Std-layer, M5+.
  - **Scoped `mmap` view — ~13×** vs owned read+scan (195→14.7 ms / 256 MiB). Directly validates the
    **Transparent zero-copy I/O (std.io)** Future entry; the mapping handle must dominate all views
    (region model). Biggest single I/O lever measured.
  - **Runtime-dispatched AVX2 structural scan — ~5×** vs scalar (34.1→6.85 ms / 128 MiB JSON-ish).
    Confirms the already-SETTLED "wide SIMD in runtime-dispatched library, baseline binary stays
    portable" policy. First targets: JSON structural scan, `memchr`-class find, UTF-8 / quote /
    backslash masks. (NEON/SVE expected to win too per 2024–2025 SIMD-parsing papers; AVX-512 untested
    — CPU lacks it.)
    - **Runtime CPU-dispatch *architecture* (codex advice 2026-06-28, explicitly "do not implement
      immediately").** A `RuntimeFns` table behind a `OnceLock`, populated once by
      `is_x86_feature_detected!`/`is_aarch64_feature_detected!`, selecting per-CPU backends for
      hot std/runtime functions (scalar / AVX2 / NEON). Rules (all consistent with this repo's
      stance): generated user code stays portable-baseline; `--target-cpu native|x86-64-v3` is the
      only whole-program opt-in; **never call a `#[target_feature]` fn without the matching detect**;
      detect once, not per inner loop; **every SIMD path tested for scalar-equivalence + benched
      before adoption**; NEON is first-class on arm64/Apple Silicon (no Apple-private accel
      dependency); AVX-512 only later with real hardware. Priority: P0 JSON/string scan → P1
      bitset count/any/all + SwissTable control-byte probing / dictionary-id grouping → P2 LLM
      primitives (tokenizer scan, quantized CPU matvec fallback, KV-cache copy/scan).
      - **Timing assessment (build-deferred-until-a-consumer):** the scaffold's *only* current
        candidate, `find_quote_or_escape`, is **already runtime-dispatched by the `memchr` crate**
        (its own AVX2/NEON detection), so wrapping it in a `RuntimeFns` table is architecture ahead
        of a real consumer. The scaffold earns its place with the **first hand-written SIMD function
        not covered by a crate** — `json_structural_scan` or `bitset_count` — and should be built
        *together with* that function (so the dispatch + a scalar backend + the scalar-equivalence
        test all land with a measurable win). That first hand-SIMD consumer in turn wants the
        simdjson-style two-stage parser (a large, separately-deferred rewrite — the current
        recursive-descent parser has no structural-scan stage to accelerate). So: **record now,
        build with the first crate-uncovered SIMD kernel, not standalone.** Full advice in
        `work/runtime-cpu-dispatch-advice-for-claude-2026-06-28.md` (gitignored scratch).

        **JSON two-stage SIMD decode — Mison speculation IMPLEMENTED 2026-06-29 (wins the projection
        rail; full-decode at parity; remaining bottleneck = the walk).** The speculative decoder
        (lean decode-index `{ } [ ] :` + Mison pattern: learn each declared field's colon ordinal from
        the first record, then jump+verify per record — no `find_field` — falling back to a
        `find_field` scan + relearn on a structure miss) ships in `align_rt_json_decode_struct_array`.
        **`bench/json_decode` (1M rows, vs serde_json): proj 1.16–1.61× (was ≈1.09×), full 0.88–1.06×
        (≈parity, was ≈1.03×)** — a real win on the **projection rail** (declare only the fields you
        read, the Align idiom; the unqueried fields' colons are skipped entirely), parity when every
        field is decoded. It does **not** reach the probe's 3.4× — an autopsy pinned the remaining cost
        to the **walk** (index-build 18 ms for the lean 24 MB/6M-token index, down from 72 MB/47 ms with
        the quote-heavy #213 index; + a 41 ms stage-2 walk = per-token `src[idx[k]]` gather + `rec_cols`
        collection + key scan-back + per-value `JsonParser` parse), which the general decoder pays and
        the probe's inlined positional sum did not. The lean index (vs #213's full structural index)
        was the autopsy-identified first fix (idx-build 47→18 ms). **Strict semantics preserved** (62
        tests green): missing/duplicate fields error via the fallback; one narrow documented relaxation
        — a duplicate of a declared field at a position the learned pattern treats as unqueried is not
        re-detected on the speculative path (no test covers it; a dup at a *declared* position trips the
        colon-count gate → fallback → error).
        **Duplicate-key semantics — DECIDED (SETTLED) 2026-06-29 (Codex overreach review).** The
        `json.decode` field contract is **strict and exactly-once**: every declared field appears exactly
        once; a missing *or duplicated* declared field is a `decode` `Err` (never a silent last-wins);
        undeclared keys are skipped. This formalizes what the implementation already does on the fallback
        path and is now written into the surface spec (`draft.md` §9 + `language-spec.md`). **Pre-freeze
        gap to close:** the speculative path's narrow relaxation above (a duplicate of a declared field at
        a position the learned pattern treats as *unqueried* is not re-detected) is now a known deviation
        from the stated contract — it must be closed (or the contract re-decided) before JSON behavior is
        frozen. Closing it costs a key-check on unqueried colons (partly against the projection win), so
        it is its own slice, not bundled here. (Why strict, not serde-style last-wins: duplicate keys into
        a fixed struct are a data error, and strict-reject matches Align's "nothing hidden / one error
        model" — a malformed shape surfaces as a value, never a silent partial decode.)
        **Walk-optimization probe (2026-06-29) → NOT worth forcing.** Before pushing `proj` higher, a
        probe added each walk cost to the inline-positional floor and measured the delta (1M rows):
        `rec_cols` two-pass **+2 ms**, key-verify scan-back **+4 ms**, AoS materialize **+2 ms** — all
        small. So removing `rec_cols` (inline speculation) saves ~2 ms (not worth the fallback/nesting
        complexity), and the verify is intrinsic to speculation (it's how `find_field` is skipped). The
        rest of the gap to the probe's floor is diffuse, correctness-tied overhead (overflow-checked
        value parse, descriptor-driven writes) with no single removable hotspot. Conclusion: `proj`
        (1.16–1.61×) is good as-is; the better future lever — if pursued — is **soa-column direct
        decode** (the probe's 3.6× path; materialization itself is cheap, so writing the projected
        fields straight into columns is the real headroom), a separate slice, not walk micro-tuning.
        **ARM64 NEON decode-index — IMPLEMENTED 2026-06-29 (closes the aarch64 SIMD gap; projection
        rail now wins on Apple Silicon too).** The lean decode-index was AVX2-only (`json_decode_index`
        fell back to the scalar walk on aarch64), so on Apple Silicon stage-1 index build was scalar
        and the whole decode ran ~2× *slower* than serde_json. Added `json_decode_index_neon`: 64 bytes
        per block as four 16-byte vectors, a 16-bit movemask per vector via bit-weight `vand` +
        across-lane `vaddv` (no x86 `movemask` equivalent on NEON), combined into the same 64-bit masks
        the AVX2 path uses, then **sharing the arch-independent `find_escaped`** and a baseline
        shift-XOR `prefix_xor_portable` (Kogge-Stone, 6 `u64` ops) **in place of `pclmulqdq`** — chosen
        over PMULL (`vmull_p64`) deliberately: PMULL is the *optional* `aes` crypto feature, not ARMv8-A
        baseline, and the prefix-XOR is not the hot cost (the per-byte movemask dominates), so a
        branch-free baseline ladder keeps the whole NEON path detection-free (NEON *is* baseline → no
        `is_aarch64_feature_detected!`, no scalar-fallback branch on aarch64). Same scalar-oracle +
        exhaustive-fuzz differential test as the AVX2 path (`json_decode_index_simd_matches_scalar_oracle`,
        green). **`bench/json_decode` on Apple Silicon (M-series), before→after: full 0.49–0.50×→0.75–0.79×
        serde (1.55–1.57× faster), proj 0.62–0.63×→1.15–1.16× serde (1.85–1.86× faster — now BEATS
        serde, matching the x86 projection win).** The residual full-rail ~1.3× gap is the same
        per-field key-matching/walk cost x86 pays (autopsy above), not the index — the ARM64 index
        bottleneck is closed. (Found while wiring this up: the existing `json_structural_index` AVX2
        test named `is_x86_feature_detected!` cross-arch, which is a hard compile error on aarch64 — so
        the runtime test suite had never built on aarch64; fixed by moving the detect inside the
        `#[cfg(target_arch = "x86_64")]` block. `json_structural_index` itself stays scalar-only on
        aarch64 — it is still dead code, "wired in a later slice", so a NEON port waits for that
        consumer.)
        **Speculation key-verify fused — IMPLEMENTED 2026-06-29 (full 0.80→0.90×, proj 1.25→1.35×
        serde on Apple Silicon).** A sampling profile (`sample`, via the new
        `crates/align_runtime/examples/profile_decode.rs` harness that loops the raw
        `align_rt_json_decode_struct_array`) **refuted the static guess that the NEON index build is the
        ARM bottleneck**: full and proj build the *identical* index, yet proj beats serde and full lost,
        so the index can't be why full lags. Leaf self-time (1M-row full): walk ~37%, value-parse
        (`write_field_indexed`) ~32%, **key-verify ~27%** (`key_before_colon` 16% + `memcmp` 11%), index
        build only ~14%, memset/memmove ~4%. The largest *addressable* waste was the key-verify: the
        speculation path already knows the expected field name, but it was scanning the key back to its
        opening quote (`key_before_colon`) and then doing a generic slice `==`/`memcmp`. Replaced with
        `key_matches_before_colon(src, cpos, name)` — computes the opening-quote position from
        `name.len()` (no backward scan) and matches the bytes against the known `name` inline. In the
        profile `key_before_colon` vanished from the hot leaves; full→0.90× (0.95× at 10k/100k, ≈parity),
        proj→1.35×. **Tried and reverted**: lowering the per-byte value-write loops to constant-width
        `copy_nonoverlapping` stores — perf-neutral (the write is ~4% of `write_field_indexed`; the cost
        there is `integer()`, already lean), so not shipped (ideal-form-or-defer). Remaining full-rail
        gap to serde is now the intrinsic walk + value-parse, the same x86 pays; the SoA-column direct
        decode (above) is still the real lever if pursued.
        The historical investigation that led here ↓. Built the
        **stage-1 structural index** (PR #213: AVX2 + `pclmulqdq` prefix-XOR string mask + odd/even
        backslash-run escapes, block-carried, scalar oracle + exhaustive fuzz; runtime-dispatched,
        baseline-binary-safe) and a `bench/json_decode/` harness (PR #212; recursive-descent baseline
        ≈ ties `serde_json`: full ≈1.03×, proj ≈1.09×). A `work/json_simd_probe` validated the
        **mechanism**: a SIMD structural index + a *projecting* two-stage decode beats `serde_json`
        **3.4–4.1×** (≈3.2–3.9× materializing into soa columns), correctness-checked. **But two
        integration attempts into `align_rt_json_decode_struct_array` both REGRESSED** (0.67–0.93×):
        a probe diagnostic (all building the SIMD index + materializing + projecting `active`+`pay`)
        isolated why — **positional + soa-columns = 3.6×, positional + AoS-struct = 3.3×, but
        name-match (`find_field`) + columns = 2.4×**. **An absolute-ms autopsy (1M rows) pinned the
        cost precisely:** stage-1 index build alone = **10.5 ms**; + positional stage-2 + materialize
        (soa columns) = **23 ms** (3.4× serde's 84 ms); and materializing into an **AoS struct with
        `buf.resize`-zero per element + a final whole-buffer copy adds only +1.6 ms** — so
        materialization is **NOT** the cost (correcting an earlier guess). The dominant avoidable cost
        is **per-field key matching (`find_field`), paid even for the unqueried fields** (positional
        3.6× → name-match 2.4×, and the runtime's *perfect-hash* `find_field` is heavier than the
        diagnostic's two `==`), plus the per-field machinery (`SeenSet`, per-value-`JsonParser` dispatch) and a
        **quote-heavy index** (the runtime emits key+value quotes, ~2× the probe's punctuation-only
        index — projection needs only colons + the queried fields). `integer()`/etc. are already lean,
        so value parsing is not the gap.
        **The literature confirms the path (papers consulted):** *Mison* (Li et al., VLDB 2017,
        `vol10/p1118-li.pdf`) gets 3.6× with a structural index and **10.2× with speculation** — a
        pattern tree predicting each queried field's colon ordinal so it **jumps to the value and
        verifies the key, skipping `find_field` and unqueried fields**; *simdjson* (Langdale &
        Lemire, arXiv 1902.08318) and *Pison* (VLDB 2021, `vol14/p694-zhao.pdf`, leveled colon/comma
        index construction). **To actually win, attack the measured cost (per-field key matching),
        not materialization:** (1) **speculation/positional** field access — the Mison lever —
        predicting each queried field's colon ordinal and verifying the key, so perfect-hash
        `find_field` and the unqueried fields are skipped (the +1.2–1.5× the diagnostic showed, the
        bulk of the gap); (2) a **leaner index** emitting only what projection needs (colons + the
        queried fields' delimiters, not every key+value quote — ~½ the index size); (3) ideally
        **column (soa) output** (Align's selling point; the diagnostic's fastest path).
        Materialization is cheap (+1.6 ms), so a two-pass exact alloc is *not* needed. A careful,
        benchmark-driven effort with residual uncertainty — **deferred as a focused track**; the
        stage-1 index (#213) + harness (#212) are the merged foundation, and the
        recursive-descent decoder (≈serde parity) stays in place meanwhile. (Probe + diagnostics:
        `work/json_simd_probe/`, gitignored scratch.)
      - **★ `core.string` byte-first APIs (codex string-processing advice 2026-06-28) — the
        actionable consumer.** The string *model* is judged directionally right (`str` = `{ptr,len}`
        UTF-8 view, `string` owned, `builder` construction, byte `len`, byte-equality, memchr scan,
        run-copy escape #197). The gap is `core.string`: `find_byte` / `find_any` / `split_byte`
        (return **borrowed `str` views**, never owned) / `trim_ascii` / `contains` / `starts_with`
        / `ends_with`, plus a UTF-8 validator. Rule: **UTF-8 is the representation, but hot scans are
        byte-oriented** — `chars()` is the *wrong* default for protocol/delimiter scanning (probe:
        newline count via `chars()` 52.7 ms vs byte 11.4 ms (4.6×) vs AVX2 4.6 ms (11.6×); JSON
        structural AVX2 6.4×; escape run-copy 3.0×, already shipped; UTF-8 ASCII fast-path only 1.28×
        and the naive mixed fallback *loses* at 0.93× — a real SIMD validator is needed, not a
        double-scan fallback). **This is the first *real consumer* of the dispatch table** (P0: ship
        byte-first APIs **backed by `memchr`/`memmem` now** — no scaffold needed; P1: move them
        behind the dispatch table + AVX2 `find_any`/structural classifier + NEON + UTF-8 validator,
        reused across JSON/HTTP/CSV/HTML/tokenizers since they share one byte-classifier). Keep
        Unicode (`chars`/grapheme/normalization/case-fold) explicit and mostly package-level, out of
        core v1. Builder is ~0.55× of optimized Rust — batching adjacent static/template appends into
        fewer runtime calls (a `write_many` internal ABI) is the lever. Probe:
        `work/string_processing_probe.rs`; advice `work/string-processing-findings-2026-06-28.md`.
      - **LLVM-version gap + upgrade as a perf-roadmap item (codex modern-CPU advice 2026-06-28).**
        Align is pinned to **LLVM 19** (inkwell 0.9, `llvm19-1`); rustc 1.96 already rides **LLVM 22**,
        so current Rust *sees* newer target features than Align's backend (x86 `avx10.1/.2`, `apxf`,
        `amx-*`; aarch64 `sve2`, `sme2`, `i8mm`, `bf16`, `fp8`). Division of labor: **LLVM** does
        instruction selection / new ISA legalization / vectorizer + cost model (so APX is "free" once
        the backend targets it — just keep emitting clean optimizable IR); **the runtime** does
        feature-detect + function-multiversioning like Rust crates. Plan: short-term AVX2+NEON runtime
        dispatch on LLVM 19; **mid-term schedule an LLVM/inkwell upgrade checkpoint** before targeting
        AVX10/APX/SME2 seriously (guarded by the existing bench + IR/behavior tests, since an LLVM
        bump can shift codegen); long-term treat LLVM upgrade as part of the *performance* roadmap,
        not just maintenance. Model **capabilities, not feature-names**, in the dispatch table (vector
        width / mask / byte-permute / VNNI-int8) so fixed-width SIMD, scalable vectors (SVE/RVV), and
        matrix engines (AMX/SME2, which stay behind the LLM/tensor backend, never core syntax) all
        fit later. Advice `work/modern-cpu-features-align-2026-06-28.md`.
  - **SoA column scan / filtered aggregate** re-confirmed: col_sum **9.4–12.2×**, `where(.active).
    pay.sum()` **3.7–7×** vs Rust `Vec<Struct>` AoS. The shipped headline; unchanged.
  - **Bitset bool/Option columns** re-confirmed with the **caveat already recorded**: `count`/`any`/
    `all` **45–48×** (popcnt), but dense `where(.flag).value.sum()` **0.36–0.67× (LOSES)** — value
    loads dominate. So generate *different* kernels: bitmap+POPCNT for cardinality terminals;
    byte/select masks for dense filtered value sums; sparse bit-iteration only at low selectivity.
  - **CAUTION — hand-SIMD is not a free win.** int8 dot (64M elems): scalar Rust 6.31 ms, manual
    unroll **0.54× (worse)**, AVX2 intrinsics only **1.35×**. LLVM `-O2` already vectorizes the scalar
    loop well. Lesson: every hand-SIMD path must earn its place against the O2 baseline with a bench —
    do not assume Align-native kernels beat mature backends. (Reinforces "bind backends via FFI
    first.")

- **New candidates worth carrying (unverified-in-Align / future):**
  - **★ Dictionary-id rail for string-key analytics.** Intern a string column to integer ids, then
    `group_by(id)`: **3.0×** first use, **~19–21× when ids are reused** across multiple aggregations
    (vs `HashMap<&str,_>×3`). The first aggregation pays for dictionary construction; repeats become
    integer-column work. Fits `json/csv decode selected str field → id column → group_by`. Strong fit
    for the column-oriented `group_by` runway; output needs an id→string map. Distinct from the
    SwissTable lever (which is for *high-cardinality* primitive-key grouping).
  - **★ Streaming / projected scanner terminals** (a typed scanner bound to its row schema, then a
    fused terminal: `rows: csv.scanner<Row> := csv.scan(view); rows.where(.active).pay.sum()?`;
    likewise NDJSON `json.scan`). The row type comes from the **binding annotation**, never an
    expression-position `scan<Row>(…)` turbofish (Settled "no turbofish"); the scanner's schema is in
    neither args nor result, so it is exactly the open **schema-selector** residual noted there.
    Streaming projected scan beat materializing all rows **2.7–2.9×** at 1–5M rows; if the terminal
    is a single aggregate, beats even building columns. A `line` must be a borrowed `str` view into a
    chunk (region-bounded, cannot escape). Pairs with mmap views; the "don't materialize
    `array<string>`" rail. Std-layer.
  - **Network std rails — connection/batching shape dominates.** Local 20k-request probe: connect-
    per-request 1.0×, keepalive 1.48×, **pipelined write-then-read 19.1×**. The network analogue of the
    stdout-flush result: the std `http`/`socket` API should reuse connections by default, expose
    batched/pipelined send-receive + bounded-concurrency `get_many`, and **lint connect-per-request
    loops to a static host**. `task_group` + blocking pool hides I/O wait (earlier probe: 64 reqs
    ×10 ms → **12.8×**) — structured concurrency first, **not** a general async runtime; `io_uring` is
    a later *Linux backend*, not the semantic model.
  - **Cache-aware shaped ops.** 512² f32 matmul: naive `i-j-k` vs `i-k-j` loop order = **33.8×** (a
    simple tile was 8–15×). Lesson is not "always tile" but "traversal/layout is a first-order semantic
    rail": offer shaped ops (`tensor.matmul(..., policy: .cache_aware)`, `rows.chunks(tile)`) and a
    diagnostic for strided hot loops over row-major data, rather than asking AI to hand-pick loop order.
    Future / tensor-kernel territory.
  - **Velox-style string layout** (short string inline-or-prefix, long string in region-owned backing
    buffers, compare by length+prefix before full bytes). Feeds the Open **String representation (SSO)**
    item; columnar string views want this.
  - **Data-oriented error accumulation** (`ok, errs := rows.validate_all(rule)`) — batch parse/validate
    wants "process all rows, collect bad rows into a column", complementing fail-fast `Result`/`?`. Keep
    explicit (no exception-like hidden accumulation).
  - **Deterministic parallel-reduce modes** (`xs.par_sum()` vs `xs.par_sum(deterministic)`) — make the
    reproducibility/perf tradeoff visible for float/log/analytics reductions. Start with integers (order
    unobservable under wrapping).
  - **Profile-guided perf lints** (`alignc run --profile` → diagnostics like "this field scan ran 10M
    times; consider `soa<T>`") — runtime evidence reduces false positives for the perf-rail lints; must
    improve *diagnostics*, never *semantics*, and never be required for good performance.
  - **`io.copy` zero-copy transfer** (`sendfile`/`copy_file_range`/`splice`) — already folded into the
    Transparent zero-copy I/O Future entry; the network/static-file-serving probes reinforce it.
  - **Deadlines / cancellation as structured scope** (`deadline(200.ms) { task_group { … } }`) — bound
    runaway I/O without a general async model; std-layer, after the structured-concurrency I/O slice.

- **Anti-recommendations (consistent with existing non-goals):** general async/await as the first I/O
  story (task_group + blocking batch pool first); hidden auto-SoA / hidden per-request arenas (explicit
  type/scope + lint); a general `HashMap` as the headline (columnar/dictionary/group_by rails); a
  hand-written SIMD library before the O2 baseline is measured; chasing load *alignment* before data
  shape + copy elimination (unaligned AVX2 loads were within ~0.95–1.0× on this host).

- **Recheck + sharpened conclusions (codex re-run 2026-06-28, three new probes verified on this host).**
  A second pass re-ran the Align-vs-Rust suite (parity zone, SoA, JSON, group_by, builder, and par_map,
  all of which reproduced) and added three focused probes. The new durable conclusions, beyond the
  bullets above:
  - **Builder: the lever is *inlining*, not a batched ABI — so the ideal form is cross-runtime LTO,
    deferred (NOT a `write_many` call).** `work/builder_batch_probe.rs` (verified): folding three
    `write` calls into one batched call is only **~1.2–1.6×** here (codex host: 2.4–3.2×), and
    **pre-sized capacity is confirmed irrelevant** — the *fully-inlined* append column is what reaches
    optimized Rust. Each `align_rt_builder_write*` is a non-inlinable FFI call across the
    `libalign_runtime.a` boundary (no LTO today), so the per-element cost is the call, not the copy. A
    `write_many`/template-fusion ABI would be **a second mechanism for something `write` already does**
    (violates "One way") and still tops out at ~1.5×. The mechanism that actually closes the gap —
    *and helps every `align_rt_*` call, not just the builder* — is link-time inlining of the runtime
    (ship `align_runtime` as LLVM bitcode / link the hot module under lld LTO). One mechanism, nothing
    hidden, reaches the LLVM ceiling. **Per "ideal form or defer", builder batching is deferred behind
    the LTO infra slice**; the earlier "`write_many` is the lever" note is superseded by this.
  - **`par_map` cost-threshold lint (P0 diagnostic).** `work/par_map_chunk_probe.rs` (verified):
    cheap per-element `par_map` *loses* to sequential (**0.24–0.81× vs seq inline**; Rayon-style
    scheduling only wins at ~1M+ elems / heavier bodies). Function indirection alone is a **~9–10×**
    penalty for trivial bodies (seq inline vs seq indirect). So the rail is: lint a cheap `par_map`
    toward sequential/vectorized, and (P1) specialize the chunk body in MIR/codegen so the per-element
    thunk disappears. Reinforces the "make the fast shape the normal rail, warn when it falls off"
    direction (and the Profile-guided perf-lints bullet above).
  - **group_by wants *three* strategies, not one hash table.** `work/group_sort_probe.rs` (verified,
    1M rows): **dense-id array aggregation 5.8 ms vs std HashMap 63 ms (~11×)** when keys are a dense
    integer range; **sort-group (24 ms) beats hash (63 ms) at 1M distinct** (high cardinality / already
    sorted). So the columnar `group_by` runway is: dense-id/dictionary path → SwissTable for general
    high-cardinality primitive keys → sort-group for very-high-cardinality or pre-sorted, with
    diagnostics ("key is a dense integer range — use dense group_by"; "string key in a hot group_by —
    dictionary id"). Extends the Dictionary-id rail + SwissTable bullets with the sort-group third leg.
  - **Codex's handed-over priority order** (for sequencing, not commitment): (1) builder inline/LTO,
    (2) JSON SIMD structural scan + projected/column decode, (3) dense-id/dictionary group_by, (4)
    `core.string` byte-first APIs + runtime CPU dispatch, (5) buffered/view-first I/O *(buffered stdout
    shipped #198/#200)*, (6) cheap-`par_map` lint/threshold, (7) high-cardinality SwissTable/sort-group.
    Reading: 1/2/3/7 are deep infra slices (LTO, simdjson-style two-stage rewrite, new aggregate
    strategies); **4 and 6 are the clean bounded ideal-form wins to ship first** — byte-first string
    predicates next, then the par_map cost lint. Probes are gitignored scratch under `work/`.

(All probes are Rust micro-benchmarks under `work/`; the convergent + on-philosophy items are recorded
for later. Re-bench in Align (`bench/`) before shipping any. The local-LLM-inference direction these
memos also explore is recorded in the Future section, "Resource-oriented north star + local LLM
inference".)

### Build targets & portability (cloud / Docker) — SETTLED (2026-06-26)
**Decision: the default build targets a safe, portable, per-architecture baseline; anything more is
opt-in; wide SIMD on a varied fleet comes from runtime dispatch in the library, not a fixed high
baseline.** Driven by the real deployment model — cloud VMs and containers are *build-once, run on an
unknown/varied fleet* (Intel/AMD/Graviton, feature-masked or live-migrated hosts), so a binary baked
for the build host's CPU (or a high fixed baseline like AVX2) would `SIGILL` somewhere.

- **Default baseline (portable):** `x86-64-v2` (SSE4.2; universal across cloud x86 since ~2010) for
  amd64; `armv8-a` (NEON is mandatory in the base ISA) for arm64. One binary runs across the fleet.
- **Opt-in, never default:** `--target-cpu native` (fastest on the build host, non-portable — for
  source-build-on-host) and higher baselines (`x86-64-v3`/AVX2, v4) for those who control their fleet.
- **Wide SIMD for the varied fleet = runtime CPU-feature dispatch in the library layer**: one binary
  detects the host CPU and picks the best path (AVX2/NEON), falling back safely. Mechanism = function
  **multi-versioning** (`#[target_feature]` variants selected via `is_x86_feature_detected!`), most
  cheaply by leaning on crates that already do it (`memchr` etc.). `std::simd` alone is *not*
  runtime-adaptive — it writes each variant's body portably; the per-feature variants + selector stay
  explicit (`impl/06` §1). **No hand-written per-architecture intrinsics**; x86-64 + aarch64 from one
  source. Heavy SIMD work (JSON/UTF-8/string scan, bulk copy) lives here. AOT-generated pipeline loops stay at
  the safe baseline (128-bit) for portability; runtime-multiversioning generated loops is a possible
  future refinement (this settles the `impl/05` §5 / `04` §9 "target width W + multi-ISA" OPEN item).
- **Multi-arch containers:** cross-build per arch+baseline into one image manifest (`linux/amd64` +
  `linux/arm64`); the driver gains a target (arch + baseline) selector. Implementation lands with the
  std / runtime layer (core-first); the policy is fixed now.

**Codegen baseline + opt-in — DONE (2026-06-26):** the codegen half is implemented. `BuildTarget`
(`align_codegen_llvm`) = `Baseline` (default: `x86-64-v2` on amd64, `generic`/`armv8-a` on arm64),
`Native` (host CPU + features), or `Cpu(name)` — an explicit LLVM CPU passed through. The recommended
portable performance tier is **`--target-cpu x86-64-v3`** (AVX2/FMA/BMI2; runs on any such host —
the server/container "fast" build, ≈1.5× the baseline on compute-bound work per `bench/run.sh v3`).
One `create_target_machine` picks the CPU/feature string for both the data-layout and the emission
machine; the driver threads `--target-cpu baseline|native|<cpu>`. `tests/build_target.rs`. **Still
pending (with the std/runtime layer):** the library's runtime CPU-feature dispatch (multi-versioning)
and explicit cross-compile triples.

Style: one good portable default + visible opt-in for more (nothing hidden).
Record: `draft.md` §3.4, `design-notes.md`, `impl/05-backend-llvm.md` §5, `impl/06-runtime-std.md` §1

### Reflection
**Decision: none.** Only the feasibility of limited compile-time reflection is considered for the future.

### Database ecosystem
**Decision: delegated to packages.** No SQL abstraction in core/std. Foundational parts (bytes/buffer/json/reader-writer etc.) are placed in core/std.
Record: `draft.md` §18.3

### String representation (SSO)
**Decision: `string` is `{ ptr, len }` (16 bytes), heap-owned. Small-String Optimization (an inline `{ ptr, len, cap }` header with a length-tag bit) is NOT adopted.**
Rationale: SSO adds a branch to every `ptr`/`len` access and breaks FFI pointer stability (an inline string cannot hand a stable address to C without first materializing it). Align's arena-centric model already avoids the small-`malloc` churn SSO targets, so the win is marginal while the cost lands on "predictable performance" + "nothing hidden". Revisit only if profiling on real workloads justifies it (digested from `work/proposals/string-optimization.md` §1).
Record: `impl/08-memory-model-v2.md` (slice 7a, owned `string`), `design-notes.md`.

### Panic / unwinding (CFG shape)
**Decision: no unwinding; immediate abort.** Fatal errors (div-by-zero, OOM) abort the process; there is no catch/recover boundary. The compiler emits plain LLVM `call` (never `invoke` + landing pads), so the MIR→LLVM CFG stays exception-free. (Promotes the prior "currently: immediate abort" detail to a locked decision — committing now keeps the CFG-generation stage from ever needing landing-pad support.) The *build-level* `panic=abort` + strip-`.eh_frame` step that drops the Rust-std unwinder is a separate, opt-in binary-size/startup lever (see Future "Hardware & backend optimization backlog").
Record: `impl/04-mir.md` (CFG), `non-goals.md`.

### Memory model v2 (borrow-region propagation + owned heap/drop) — IMPLEMENTED
**Decision: one inferred region lattice + owned heap collections with per-binding drop; views are region-tied and escape is checked; a value that must outlive its source is cloned explicitly (the compiler never inserts a copy on escape).** The phase that unified the old point solutions and lifted the M3/M4/M5 ownership deferrals. Concretely settled and shipped:
- **One region lattice** `Static ⊐ Frame ⊐ Arena(k)` (regions stay *inferred* — no lifetime syntax). Every view producer (`slice`, `str` borrow, struct field, a `json.decode`-d struct or `array<Struct>`, a call re-borrowing an argument) carries a region; `EscapeCheck` forbids a view outliving its source. Replaces the three unrelated mechanisms (arena depth for `box`/`str`, slice "local-backed", struct `str` region-0).
- **Owned (Move) heap collections + drop**: free-standing owned `string` / `array<T>` / `array<Struct>` (AoS) / `builder`, freed by per-binding MIR `Drop` (null-on-move drop flags) outside an arena, or arena bulk-free inside one. Owned payloads inside `Option`/`Result` are dropped / moved-out as a unit.
- **Explicit `.clone()` over hidden copy-on-escape**: a zero-copy decoded view that must escape its input is cloned explicitly (Nothing hidden + Predictable performance; supersedes the old `draft.md` auto-buffer wording). An in-arena clone is a bump allocation, so escaping is not a sudden heap cost.
- **`json.decode`**: `str` and `array<Struct>` decode are zero-copy views region-tied to the input (a struct's `str` fields borrow it); `array<scalar>` is copied into a fresh buffer (owned / `Static` / returnable, not region-tied). Together → **`draft.md` §19 runs end-to-end except the `fs`/`io` std boundary**.
SSO is **not** adopted (its own Settled entry above). Element indexing is implemented: `recv[index]` (array/slice/owned array → scalar; **struct array → whole struct by value**, a Copy load region-tied to the array via `region_of`) and `arr[index].field` (a struct-array element's field), both bounds-checked. Since-implemented on separate tracks: tuples / multi-value returns → `partition`; `array<slice<T>>` → `chunks` (`Ty::DynSliceArray`); `out` params + the no-alias check. Still open: `array<Struct>.clone()`, and emitting LLVM `noalias` (below).
Record: `impl/08-memory-model-v2.md` (full model + slice ledger §11), `design-notes.md` ("one region lattice, explicit copies"), `draft.md` §6/§7/§14, `impl/07-roadmap.md` (Memory Model v2 — DONE).

### Tuples / multi-value returns
**Decision (2026-06-22): first-class anonymous tuples `(T, U, …)`; multi-value return is just
returning a tuple — no separate Go-style multi-value mechanism.** A Go-style "multiple return
values" feature would be a second way to produce several values that is *not itself a value*
(can't be stored, nested, or put in an array) — exactly the special-casing Align avoids. A tuple
is the anonymous, positional companion of the keyword-less named struct: use a named struct for a
domain type, a tuple for an ad-hoc "two things" result. Syntax: type `(T, U)`; literal `(a, b)`;
destructure `(a, b) := expr` (parens required — mirrors the literal — with `_` to ignore an
element); positional access `t.0` / `t.1`. Arity ≥ 2 (`()` is unit, `(e)` is grouping). Ownership
is derived from the elements (Move if any element is Move; region-tied if any is a view), reusing
the MMv2 owned-aggregate/region machinery — no new ownership rule. Represented as `Ty::Tuple(id)`
into an interned tuple table (the dual of the struct table), lowered to an anonymous LLVM struct.
**Implemented:** the type + literal + destructure + `.N` + tuple params/returns for primitive
scalars, `str` (region-tracked), **and owned `string`/`array<T>` elements** (a Move tuple). An owned
tuple may be **bound to a variable** (`t := split()`) and **passed as a parameter** — codegen drops
each owned element at scope exit (`Drop`/`DropFlagInit` over the tuple aggregate), and a
destructure/return/call that moves it nulls the slot; an owned-tuple parameter the callee never
consumes is dropped at the callee's exit (the same drop set as an owned array param). **Partial
field moves** are supported: `a := t.0` (a bound tuple) moves that owned element out, leaving the
other elements usable; MoveCheck tracks moves per field (`MovedKey::Field`), forbids re-moving a
field or using the tuple as a whole afterwards, and a borrowing read (`t.0.sum()`) does not move.
MIR nulls the moved field (`NullTupleField`) so the tuple's exit `Drop` frees null there. Indexing
an owned element out of a *temporary* tuple (`f().0`) is rejected (it would orphan the other owned
elements) — bind it first. A Copy element reads fine in any position. The first consumer
**`partition`** (`(array<T>, array<T>)`) is implemented. The remaining potential consumer is
`min_with_index` (`(value, index)`). Record:
`draft.md` §5 (Types → Tuple), `impl/02-frontend.md`
§8, `impl/03-types.md`, `impl/07-roadmap.md`.

### Type-argument syntax: no turbofish (expression position)
**Decision (2026-06-22): there is no expression-position type-argument syntax.** A call's type parameters are recovered by inference — from a value argument (`json.encode(u)`) or from the expected type propagated from context, including back through `?` (`u: User := json.decode(d)?`). When neither supplies the type it is a hard error directing the user to annotate the binding; an explicit `f<T>(x)` / `f::<T>(x)` form is **not** adopted. Rationale: keeps "one way" (the binding annotation is the single place a type is written), removes the `<` vs comparison parse ambiguity at expression position outright (the reason Go uses `f[T](x)` and Rust `::<>`), and is friendlier to generate. The headline case — `draft.md` §19's `json.decode<array<User>>(data)` — therefore becomes `users: array<User> := json.decode(data)?`; the checker already takes `decode`'s target from the expected `Result<T,_>` and emits an annotate-the-binding error otherwise (no code change needed — only the spec/comment caught up). **Residual (still open):** a *schema-selector* builtin whose type appears in neither arguments nor result (`json.validate<T>`, `json.field_table<T>`); narrow, unimplemented, and may fold into `decode`. This rule scales to general generics (below): a return-only type parameter is supplied by the binding annotation, never a turbofish. Record: `impl/02-frontend.md` §8 (generics `<` vs comparison), `draft.md` §18 (core.json), `language-spec.md` (JSON).

---

## Open (to be decided)

Each item is tagged with a target milestone for resolution (`impl/07-roadmap.md`).

### Module / import system — design SETTLED (2026-06-25), implementation in progress
**The last big language-core gap.** Today `module`/`import` are *parsed* into `File.module`/`File.imports` but otherwise **ignored** (single-file compilation; `core.*`/`std.*` are compiler builtins). Decided:

- **core stays builtin (language-intrinsic), and so does std for now.** core members are intrinsically compiler-magic — `core.json`/`core.template` need compiler-generated static field tables (`non-goals.md`: "compile-time story is builtin-driven static data only"), `map`/`where`/`reduce` fuse into one MIR loop, `core.vec`/`core.mask` lower to SIMD. They are language semantics wearing a library name, not hand-writable library code. **std** bottoms out in `align_rt_*` calls today; it becomes real Align-over-FFI library code only **after FFI** (post-M8), so it stays builtin until then.
- **`import` is REQUIRED + verified for the prefix-accessed builtin namespaces** — exactly `json` (`core.json`), `fs` (`std.fs`), `io` (`std.io`) today (the only builtins called through a module-name prefix; everything else in core is method/operator/keyword syntax). Using `json.decode` / `fs.read_file` / `io.stdout.write` without the matching `import` is a compile error; an `import` naming an unknown module is a compile error; an unused `import` is a lint. This makes a file's capability surface (touches JSON / filesystem / stdout) visible in its header — "Nothing hidden." The **language-syntactic core** (`Option`/`Result`/`?`/`else`, `arena`, the array pipeline `.map`/`.where`/`.reduce`/`.sum`/…, `x.abs()` math methods, `template "…"`) needs **no import** — requiring one would be requiring an import for syntax.
- **User-authored modules are load-bearing** — `module foo` names a file's module; `import myproj.foo` resolves to another source file; `pub` controls cross-module visibility; names are mangled per module. This is the genuinely new machinery (multi-file discovery + resolution + visibility + cross-module name/type identity).

Implementation slices: **A — builtin import validation — DONE (2026-06-25).** `collect_imports` validates every `import` against the `BUILTIN_MODULES` table (unknown / duplicate → error); the imported set threads into each `Checker`; `require_import` enforces `core.json` / `std.fs` / `std.io` at the `json.*` / `fs.read_file` / `io.stdout.write` dispatch sites (once per source function, skipped for monomorphs). Syntactic core needs no import. `tests/imports.rs` (7) + corpus updated (every existing json/fs/io program/example now carries its import). (Unused-import lint was deferred here until user modules existed — now **DONE**, see B-lint below.) **B — real multi-file user modules.** Resolution scheme decided (2026-06-25): **filename convention** — `import geom` → `geom.align` in the entry file's directory (its `module` decl must match the filename); chosen for simple+fast+predictable (no directory scan, only imported files are read) over scan-by-`module`-decl or a CLI file list. **B1 DONE (2026-06-25):** driver loads the entry + transitively-imported user modules (BFS, dedup, cycle-safe); sema's `check_file` → `check_program(&[Module])` checks them together; functions are **per-module mangled** (`module$fn`, entry module unmangled so single-file programs are byte-identical); bare calls resolve in the caller's module, `mod.fn(...)` resolves cross-module with **`pub` visibility**; the capability-import rule applies per file. `tests/modules.rs` (8), `examples/modules/`. **B2 (nested paths) DONE (2026-06-25):** `import util.math` → `util/math.align` (declaring `module util.math`), called `util.math.fn(...)`; the driver walks the directory tree, sema flattens the dotted receiver (`flatten_module_path`) to resolve the call. **B-types (cross-module type export) DONE (2026-06-26):** types are now **per-module namespaced** like functions — a non-entry module's type `T` has canonical name `module$T` (entry module unmangled, so single-file programs stay byte-identical), two modules may reuse a type name, and `type_table` (module → bare → canonical + `pub`) drives resolution. `pub` on a struct/enum exports it; an importer names it qualified (`geom.Point`, resolved with import + `pub` checks via `canonical_type_name`); a bare type resolves in the current module (so an imported type **must** be qualified). `StructLit.name` became a `Path` (the parser detects a dotted `Path { ident :`); `resolve_type` routes qualified paths through the table. **B-variant-ctor (qualified variant construction) DONE (2026-06-26):** an imported `pub` sum type's variant is constructed qualified — `pal.Color.Green` (tag-only, via `check_field_access`) and `pal.Color.Code(40)` (payload, via `check_call`). A unified `resolve_type_receiver` resolves a `Type.Variant` receiver as a bare type (current module) or `mod.Type` (imported `pub` type), used by both the tag-only and payload paths; a private cross-module type emits one clean error (3-state `Ok(Some)`/`Ok(None)`/`Err`, no cascade). So an exported sum type is now **fully** usable across modules (construct + hold + return + match). `tests/modules.rs` (now 19). **Still deferred:** cross-module **field/payload types** (a field `f: other.T`) — but note this is mostly blocked on **nested struct/enum fields not existing yet** (`is_field_ok` allows only scalar/str), not on module plumbing; the only live slice is an enum payload of an imported struct (passes 0b/0c resolve with `no_imports` — would need the import table built before the type passes). **B-lint (unused-import lint) DONE (2026-06-26):** an `import` never referenced in a file is a **warning** (tidiness, not a hard error — unlike unhandled `Result`, which is a correctness error). Detection is a syntactic AST walk (`collect_refs` → `walk_expr`/`walk_type`/`walk_block`) collecting every qualified reference's dotted prefix, independent of the resolution code so signatures / bodies / constants are covered uniformly; an import is used iff some prefix equals it or starts with it + "." (a builtin `core.json` matches its `json.*` namespace). The walk over-approximates "used" (a local shadowing a module name still counts), so the lint never wrongly fires. `tests/unused_import.rs` (7). Still deferred: project-root config (entry dir is the root). Record: `draft.md` §17, `impl/02-frontend.md`, `tests/modules.rs`, `tests/imports.rs`, `tests/unused_import.rs`.

### Generics (minimal system) — DONE / CLOSED (4c)
**This feature is complete and closed.** Generics is deliberately a *minimal*, supporting feature
(`CLAUDE.md`: "approach minimally", "no Rust-trait complexity", "AI-friendliness is a constraint —
avoid complex generics"). Align is **data-oriented** — arrays/slices are the protagonist, not
generics. The implemented surface below (generic functions + builtin bounds + generic structs +
generic sum types) is the intended scope; **do not keep extending it.** The items once listed as
"later 4c slices" are not generics work and have moved to their real homes (see "Out of generics —
moved to their own tracks" at the end of this entry).

**Settled & built — 4c-1 (the unconstrained walking skeleton) DONE.** A function may declare type
parameters `fn f<T, U>(...)` and is **monomorphized** per distinct concrete instantiation. Decisions
made and implemented:
- **Monomorphization unit = the function, specialized per concrete type-argument tuple**, generated
  *before* the flow analyses / MIR (so MoveCheck/EscapeCheck/drop and codegen only ever see concrete
  types — a Move `T` moves, a Copy `T` copies, all "for free"). `Ty::Param(i)` represents a type
  parameter inside a template; it never reaches MIR. Mangled symbol = `name$arg$arg…` (`id$i32`).
  Instantiations are discovered transitively (a generic calling a generic) to a fixpoint.
- **Type arguments are inferred, never written** (reaffirms the no-turbofish decision): from a value
  argument, or the expected type via the binding annotation. Uninferable → annotate-the-binding error.
- **A type parameter is opaque (no constraints yet)**: in the template body `T` may only be passed /
  returned / stored / moved; an operation needing a capability (arithmetic, field access) is rejected
  (the template is checked abstractly with `T = Param`). An **uninstantiated** generic is not
  type-checked (C++-template-like; only its instances are).
- **Skeleton cut**: a type parameter appears only in a **bare** position (a whole parameter / return),
  never nested (`array<T>` / `Option<T>` / a tuple of `T` are rejected — `Scalar` can't hold a
  `Param`); and a generic function may not contain a lambda / pipeline yet (its lifted helper would
  collide across instances). (`crates/align_driver/tests/generics.rs`, `examples/generics.align`.)

**Settled & built — 4c-2 (the constraint model) DONE.** A type parameter may carry a **builtin
bound** — `fn f<T: Ord>` — from a small fixed hierarchy **`Num` ⊃ `Ord` ⊃ `Eq`**: `Num` grants
arithmetic + ordering + equality (the numerics), `Ord` grants ordering + equality (numerics +
`char`), `Eq` grants equality (numerics + `char` + `bool` + `str`). The bound gates which operations
a `Ty::Param` value allows in the template body (an op needing a capability the bound doesn't grant
is rejected — `x + x` needs `Num`, `a > b` needs `Ord`, `a == b` needs `Eq`), and at instantiation a
concrete type argument is checked against the bound (`max<T: Ord>(true, false)` → "bool does not
satisfy Ord"). **No user-defined trait-style bounds** (avoids Rust-trait complexity; AI-friendly;
*one way*). Structural inference of bounds-from-usage was considered and set aside (implicit, harder
error messages). (`FnSig.bounds` + `Checker.param_bounds`; gated in `check_binary`; instantiation
check in `finalize_expr`. Closes a 4c-1 hole where `==`/`>` on an unconstrained `T` were wrongly
allowed.)

**Settled & built — 4c-3 (type parameters in `Option`/`Result` positions) DONE.** A type parameter
may appear **nested** in an `Option<T>` / `Result<T, E>` payload (parameter or return position) —
generic combinators `fn unwrap_or<T>(o: Option<T>, d: T) -> T`, `fn ok<T>(x: T) -> Result<T, Error>`.
`Scalar::Param(u32)` makes a parameter representable as an Option/Result payload (var-free invariant
relaxed only inside the abstract template check — never reaches MIR/codegen, like `Ty::Param`).
Inference is **structural** (`match_param`): a type argument is matched against the declared type,
binding `Param` bare or nested; a return-only param is seeded from the expected type
(`o: Option<i32> := wrap(x)`). A *nested* parameter is finalized eagerly at the call (a `Scalar`
can't hold an inference variable), while a *bare* parameter stays deferred (keeps 4c-1's
return-context inference). `box<T>` / `slice<T>` / `array<T>` / tuple positions are still rejected
(only Option/Result are wired).

**4c-4 (decl syntax groundwork) + 4c-5 (generic structs) DONE.** Generic struct declarations
`Pair<T> { a: T, b: T }` work end to end: the **resolver refactor** landed — `resolve_type` takes a
`TyCx` bundling the interners, the concrete `structs` table grows *during* resolution (a `&mut Vec`,
like `tuples`/`fn_types`), and a `Pair<i32>` type interns a concrete monomorph `StructDef` on demand
(deduped by mangled name via `struct_mono`; templates with `Param` fields live in a separate
`struct_templates` registry, kept out of codegen). Concrete struct ids get reserved slots so
monomorphs (appended after) never shift them. A **generic struct literal** (`Pair { a: 1, b: 2 }`)
infers its type arguments from the field values (`match_param`, no turbofish) then monomorphizes;
`Pair<i32>` is also a parameter/annotation type. A field must be Copy after substitution.

**4c-6 (generic sum types) DONE.** `Opt<T> { Some(T), None }` works end to end — the enum analogue
of generic structs: an `enum_templates` registry, the concrete `enums` table grows during resolution
(reserved slots + `enum_mono` dedup), `resolve_type` interns a monomorph `EnumDef` for `Opt<i32>`,
and variant construction (`Opt.Some(7)`) infers the type arguments from the payload (`match_param`)
then monomorphizes. A no-payload variant (`Opt.None`) is uninferable on its own (no expected-type
decomposition yet). Payloads are scalars / plain structs (same as a non-generic enum).

**Generics is closed — the surface above is the whole feature.** The minimal-generics goal is met:
generic functions, builtin bounds (`Num`/`Ord`/`Eq`), generic structs, and generic sum types, all
monomorphized, no turbofish, no user trait bounds. That covers ordinary generic code; further
extension is explicitly **not** pursued, to keep generics minimal and Align data-oriented.

**Out of generics — moved to their own tracks (NOT generics todo):**
- **Generic containers** (`Stack<T>`, an `array<T>`/`slice<T>` field/param) belong to the
  **data-oriented core / `group_by` track** (roadmap #5), not here. They need the fused-pipeline
  machinery to carry a generic element (and `PrimScalar` to hold a `Param`) — a perf-core change,
  pursued *if and when* a concrete consumer (e.g. `group_by`) needs it. Align already ships builtin
  `array`/`slice`/`Option`/`Result`/`Error`/tuples, so the language is complete without generic
  containers.
- **Value generics `vec<N, T>`** — part of **M6 (SIMD)**, not generics.
- **A generic def used inside a generic function** (`fn mk<T> -> Pair<T>`) and expected-type
  decomposition for `Opt.None` — small optional refinements, rejected cleanly today; only revisit
  if real code demands them. Not required for the language to be complete.

### Error type design — Open (next after sum types; the i32 is an M2 placeholder)
Today `Error` is the M2 `Ty::ErrCode` (an i32 code). **Leaning (2026-06-24, validated by external review):** build the real `Error` **on the sum-type mechanism** — `Error` is a **sum type of categories** (the variant carries a lightweight payload: a `str` view + position for a parse error, a code for an OS error, …). Constraints from the philosophy:
- **An explicit value, nothing hidden:** no exceptions, no unwinding, no implicit stack-trace allocation. (The cold-`Err`-edge treatment stays.)
- **No implicit `?` conversion — explicit `map_err` instead (4b-3 DONE).** `?` requires the same `E` (an implicit `E → E'` coercion would be *hidden* — Align has no `From`-trait to point at, unlike Rust). To change a result's error type, use `result.map_err(f)` (`f: fn(E) -> E'`), then `?`: `inner().map_err(to_error)?`. Explicit, visible, closure-based; lowers to a branch over the `Result` reusing the existing unwrap rvalues + an indirect call.
- **Context is structured, not free-form (revised 2026-06-25, see 4b-4):** the Align way of attaching context to an error is **structured data in a sum-type payload** — a variant that carries the relevant fields (a `Pos`, a code, a name) — not a free-form appended string. Free-form `.with_context("…")` string-chaining is the dynamic / allocating / unstructured anti-pattern (Rust `anyhow`-style); it cuts against the data-oriented + AI-friendly grain and would force either `str`/owned-`string` payloads in the error (making `Error` Move, rippling through `?`/drop) or recursive `box<Error>` wrapping (deferred with recursive enums). So **`.with_context` is not adopted**; structured errors are the mechanism. (Reconsider only if a concrete need appears *and* `str`-in-error-payload region tracking lands — the same deferral as S2's `str`-field struct payloads.)
- **Structured errors carry position — DONE (4b-4):** a user error enum whose variant carries a plain-data struct payload models a parse/validation error that carries its position (`ParseError { BadToken(Pos), Eof }` with `Pos { line, col }`), constructed, `?`-propagated, and read back with `match` — end to end. No new mechanism: it falls out of user error enums (4b-1) + plain-struct variant payloads (S2). (Tests: `structured_error.rs`; example: `examples/structured_error.align`.)
- **Exit-code mapping** at the `main` boundary stays as today (`clamp(1,255)`).
So this entry **waits on sum types** (4a) and then defines `Error` as a concrete sum type + the `?` conversion + exit mapping (`impl/03-types.md` §5, `impl/06-runtime-std.md` §9).

**4b-1 DONE (the foundation): errors can be user-defined sum types.** `Scalar::Enum(u32)` was added (a sum type is a Copy composite payload, like `Scalar::Struct`), so an enum is now a first-class `Option`/`Result` payload — most importantly **`Result<T, MyError>`** with a user error enum: construct `Err(MyError.Variant(…))`, `match` the `Result` then the error enum, and `?`-propagate it (same `E`). `option_struct_type`/`result_struct_type` (and `scalar_type`/`abi_type`) thread the enum-type table so the aggregate can hold an enum field.

**4b-2 DONE: the canonical `Error` is a builtin sum type.** `Error { NotFound, Invalid, Denied, Code(i32) }` — a real enum registered as a reserved type name (resolved via `enum_ids` like any sum type). `Error.NotFound` / `Error.Code(c)` construct it (`error(c)` is sugar for `Error.Code(c)`); `match` discriminates the categories; `?` propagates. Every fallible builtin (`fs.read_file`, `json.decode`, `io`, `task_group`) now returns `Result<_, Error>`, wrapping its runtime i32 status as `Error.Code(code)`. The **`main` exit mapping**: `Code(c)` → exit `clamp(c)`, a category → `tag + 1` (a small distinct nonzero code). The **task_group** fallible path was reworked to carry the full `Error` across threads: each task gets an `err_slot`, the trampoline writes its `Err` value there and returns 0/1, `tg_wait` returns the first errored `err_slot` (null if none), `wait()?` builds the `Result` from it. (`Ty::ErrCode`/`Scalar::ErrCode` are now vestigial — only an i32-status alias in the builtin lowerings; removable in a follow-up.) **4b-3 DONE** the explicit **`?` `E → E'` conversion** via `result.map_err(f)` (no implicit coercion). **4b-4 DONE (structured errors) / `.with_context` not adopted** — position-bearing structured errors already work on the 4b-1 + S2 foundation (a variant carrying a `Pos` struct, `?`-propagated, `match`-read); free-form `.with_context` string-chaining was reviewed and dropped as off-philosophy (structured sum-type payloads are the context mechanism — see the bullet above). **So the Error type (4b) is complete** for the planned surface: `Error` is a builtin sum type, user error enums work, `map_err` converts, structured payloads carry context. (Richer `str`-carrying error payloads remain deferred with S2's `str`-field payloads — enum region tracking.)

### Arena with explicit allocator — partially settled (M3)
**M3 decision: anonymous `arena {}` only.** Nested arenas use region = arena nesting
depth; a box's region is the depth at which it was allocated, and escape = reaching a
shallower depth (`impl/03-types.md` §7, `impl/07-roadmap.md` M3). Still **open**: a
named/explicit-allocator form like `arena a {}` and cross-arena chunk sharing.

### Exposing SIMD intrinsics in std
In addition to auto-vectorization, whether to place explicit intrinsics in std (`impl/04-mir.md` §9).

### SoA (struct-of-arrays) layout — design now, implement ~M6
**Leaning: an explicit `soa array<T>` modifier (annotation), not auto-detection.** A column-oriented array lowers `users[i].field` to an index into the matching column array instead of an AoS GEP. **Retrofit-sensitive**: this changes AST/HIR/MIR field-access resolution and the array ABI, so the array / struct-array type representation and field-access lowering should stay **layout-parametric** (treat AoS vs SoA as a property of the array type) *now*, while the array machinery is still being built — even though the `soa` surface + SoA codegen ship at M6 (its payoff is SIMD auto-vectorization of column scans). Still open: whether to also allow auto-SoA under a heuristic. (Digested from `work/proposals/next-draft.md` §1.2, `optimization-milestones.md` §1.1.)
**Groundwork landed (pre-M6):** `Ty::DynStructArray(id, Layout)` now carries a `Layout` (only `Aos` today; `Soa` joins at M6) — layout is a property of the array *type*, so adding `Layout::Soa` makes every site that must handle it a compile error (it can't be silently forgotten). All struct-array element-field addressing is funneled through one MIR seam (`lower_field_access`), where the SoA column-index branch will hook in — localized, not a cross-cutting retrofit. (`Scalar::DynStructArray` stays layout-free — an SoA array as an Option/Result payload is a later concern.)
Record: `impl/05-backend-llvm.md` §2, `design-notes.md` (hardware-friendly).

### Struct/array alignment attribute `align(N)` — design reserved, implement ~M6
A type/allocation alignment attribute (`align(256) Node { … }`, `align(4096) data := …`) for GPU/DMA/page-aligned zero-copy interop. **Retrofit-sensitive**: it modifies struct field-offset math and the arena bump allocator's alignment, so reserve room in the layout model now; the surface + LLVM `align N` emission + arena honoring it can land at M6 alongside SoA. (Digested from `work/proposals/next-draft.md` §1.1.)
**Groundwork landed (pre-M6):** `StructDef` carries `align: Option<u32>` (always `None` today — no surface syntax), and codegen routes all allocation alignment through one seam, `type_align(ty)` (natural ABI alignment today; a struct's custom `align` if set). M6 work is then "parse `align(N)` → set `StructDef.align`" + the seam returns it — the stack-slot alloca already calls the seam; the arena bump allocator already takes an explicit `align` argument. (Retrofit risk was low — a custom alignment is largely *additive* at the alloca/global/alloc sites — so this groundwork is a light reservation, unlike the SoA field-access seam.)

### `out` parameters + `noalias` — write mechanism + no-alias check DONE; LLVM metadata is the follow-up
`out` params (`draft.md` §7) are a no-alias optimization. **Implemented:** (1) the write mechanism —
`out dst: slice<T>` is a writable output buffer and `place[i] = v` (bounds-checked) writes a `mut`
array local or `out` slice (primitive elements); (2) the **no-alias check** — at a call site an
`out` argument must not alias another argument, compared by **root buffer**: a slice local's
provenance is tracked back to the array it borrows (`s: slice := a`), so `fill(a, s)` and
`fill(s1, s2)` (two slices of `a`) are both rejected, not just `fill(a, a)`. (Residual for the
noalias-emission follow-up: a slice returned from a function has unknown provenance and is treated
as its own root — sound for today's direct-borrow slices, but the emission gate may need to
conservatively reject unknown-provenance `out` args.) **What remains is emitting the LLVM `noalias`** so loop vectorization can skip
runtime overlap checks — blocked on the slice ABI: a slice is passed **by value** as a `{ptr,len}`
aggregate, so its buffer pointer is not a standalone pointer parameter to attribute. Needs either a
by-pointer `out`-slice ABI or scoped `!noalias` metadata on the buffer stores. The no-alias *check*
is the soundness precondition for that emission. (Digested from
`work/proposals/optimization-milestones.md` §1.2, `toolchain-optimizations.md` §5; see also
`08-memory-model-v2.md` §11 "out parameters".)

### SoA conversion trigger
Whether to automate the decision to lay out `array<T>` as SoA, or use annotation. Impact on the array ABI (`impl/05-backend-llvm.md` §2). (Subsumed by "SoA layout" above; kept as the open auto-vs-annotation sub-question.)

### Tuples / multi-value returns — design SETTLED (see Settled); implementation in progress
The *design* is settled (first-class anonymous tuples; multi-value return = returning a tuple —
see "Tuples / multi-value returns" under Settled). The **foundation is implemented**: the
`(T, U, …)` type, literals, destructuring `(a, b) :=`, positional `.N`, tuple params/returns, for
primitive scalars, `str` (region-tracked), and **owned `string`/`array<T>`** elements (a Move
tuple — including **bound to a variable**, with per-element `Drop` in codegen), and the first
consumer **`partition`** (`(array<T>, array<T>)`), and **partial field moves** (`a := t.0` moves
one owned element out of a bound tuple, per-field move tracking). What remains is purely additive
*implementation*, not design: one more potential consumer — `min_with_index`-style
`(value, index)` reductions.

### Arena checkpoint / rollback — std arena API, after MMv2
A lightweight `cp := arena.checkpoint()` / `arena.rollback(cp)` for `O(1)` bulk-free of everything allocated since a checkpoint, for long-running loops (event loops, packet/stream parsers) that must keep a flat memory footprint while reusing the same blocks. The runtime arena already bump-allocates; this exposes a reset-to-mark on top. (Digested from `work/proposals/library-foundations.md` §3; used by the streaming-parse story in `http-optimization.md` §5.)

### Build system / package layout
Visibility (`pub`), import, and module are decided (`impl/02-frontend.md`). What remains is the design of the build system, package layout, and dependency resolution.

### FFI (foreign function interface) — after M8 (keystone for the library strategy)
Detailed design of C / Rust / Zig interoperability. Because Align is AOT-via-LLVM with no GC, an external C call is a direct LLVM `call` at native speed (no pinning / stack-switch / marshaling), and an Align `slice`/`str`/`bytes` hands its raw pointer straight to C. **This gates a deliberate library strategy: "own the memory wrappers, borrow the mathematical engines"** — `std.compress` wraps `libzstd`/`zlib-ng`, `pkg` DB drivers wrap `libpq`/`sqlite`, etc., rather than re-implementing assembly-tuned algorithms in Align. So FFI's design should land before those `std`/`pkg` libraries are built, even though it stays out of the v1 *language* core. (Digested from `work/proposals/ffi-optimization.md`, `compression-strategy.md`, `rdb-optimization.md`.)

### Details (settled during implementation)
```text
- default-type lint (warn when the i64 default is wasteful in large arrays; no literal *suffix* — `as` covers expression-position typing, see "Numeric literal typing" Settled)
- match exhaustiveness algorithm / guards / | multiple patterns
- struct size threshold dividing Copy/Move
- Determining vector width W (vec<N> fixed vs target ISA)
- Scope of the LLVM optimization pipeline adopted
- Whether to allow {expr} in string interpolation (or only {ident})
- Thread pool lifetime / floating-point reproducibility of par reduce
- Whether to provide a panic-catch boundary (currently: immediate abort)
```
Details correspond to the "Open issues" section in each `impl/*.md`.

---

## Future (out of v1 scope)

```text
GPU backend
distributed execution
incremental compilation
self-host
```
Keeping MIR backend-agnostic does not impede future additions (`impl/00-overview.md`).

### Transparent zero-copy I/O (std.io)

CLI use (pipes, redirects) is a primary target (`draft.md` §2). The aim: a uniform
`std.io` surface — `reader` / `writer` and a `copy(reader, writer)` — where the user
writes ordinary code and the implementation picks the fastest transfer path **without
the caller knowing**, while staying memory-bounded. This is the proven `io.Copy`-style
capability-dispatch pattern (Go selects splice/sendfile via `ReaderFrom`/`WriterTo`,
else a fixed-buffer fallback).

Deterministic dispatch on file-descriptor kind:

```text
file → socket/pipe   sendfile / splice   (Linux)
pipe ↔ pipe/fd       splice              (Linux)
scan a file          mmap + madvise, returning bytes/str views
otherwise / other OS fixed-buffer streaming copy (portable default)
```

Why this is allowed under the core invariants: "Nothing hidden" governs allocation /
errors / effects / parallelism / unsafe — **not which syscall is used**, so hiding the
*mechanism* is fine. The line to hold is "Predictable performance": the abstraction
must not silently change cost class.

Guardrails (a build is only "problem-free" if these hold):
```text
- The portable fixed-buffer copy is the reference; fast paths must match it exactly
  and are validated against it. Streaming keeps memory O(buffer), never full-file read.
- Fast paths add edge cases: handle partial transfer, EINTR/EAGAIN, EPIPE/SIGPIPE,
  short writes. mmap: gate to regular files via fstat; handle SIGBUS (truncation);
  avoid zero-length / /proc / character-device files.
- "Predictable" is per-platform: Linux uses splice/sendfile, mac/Windows fall back —
  the result is identical, only performance differs (acceptable, unavoidable).
- Zero-copy views keep their backing alive; bound that lifetime with region/arena
  (`draft.md` §6.4/§15) so a small view cannot pin a huge mapping unnoticed.
- This is a std-layer optimization (not core, not the walking skeleton). Add it after
  measurement; do not let it leak into core or block earlier milestones.
```
Placement: `std.io` (OS boundary, `draft.md` §18.2), implemented in the Rust runtime
with a portable fallback; cross-platform mmap via a crate (e.g. `memmap2`). Revisit
around the string/JSON milestone (M5) and std build-out.

### Fast startup (non-functional goal)

CLI tools are invoked repeatedly (in scripts/pipes), so startup latency is a primary
quality. Rough scale: Python ~30ms, Go ~1–2ms, static C ~0.2ms; sub-millisecond is the
target. Most of this is structural — Align wins by *not having* things rather than by
optimizing them:
```text
- Static link + thin runtime: no dynamic-loader resolution; output carries no LLVM, no GC.
- No hidden global init: "nothing hidden" means no startup-time global constructors /
  lazy statics to run.
- Thread pool is created on demand at block scope, not at process start (06-runtime §5);
  a CLI that uses no parallelism stays single-threaded and exits immediately.
- Small binary + hot-code locality (DCE / strip / LTO / section ordering or PGO) to cut
  page faults on cold start.
- Lazy resource touch: argv / env / locale / timezone DB only when used.
```
Promote to `draft.md` §2/§3 as a non-functional goal once committed. Per-platform and
opt-in only: `-march=native`, PGO, non-PIE (a few µs, security tradeoff) must not be the
default — they break "predictable performance".

### Performance levers (data / build-time)

Forward-looking levers beyond what the spec already bakes in (fusion §9, SIMD/mask §9,
arena §6.4, cold error path §10, scan-once / const string pool / JSON field table §12/§14,
SoA §05-backend §2):
```text
- Limited const-eval: precompute lookup tables at build time instead of at startup
  (also feeds "fast startup"). Distinct from reflection (which stays "none").
- SIMD numeric parse/format (fast atoi/itoa): CLIs convert numbers <-> text constantly.
  Lives in core.str / core.math.
- Perfect hashing for static keys: compile-time perfect hash for JSON fields / keyword
  lookup (an extension of the field table).
- Embedding read-only data in the binary as const (no startup load).
- Niche / opt-in: huge pages (madvise), prefetch, io_uring batched I/O (Linux; same
  "hidden fast path + portable fallback" rule as zero-copy I/O above).
- Out of core/std: zero-parse formats (capnproto/flatbuffers-style mmap-and-access)
  belong in pkg (`draft.md` §18.3).
```
Line-drawing (to preserve the core invariants): default-on only when predictable
(fusion / arena / SIMD / cold path / small static binary); mechanism-hidden-but-cost-
predictable fast paths go in std with a portable fallback; environment-dependent or
footgun techniques stay opt-in / isolated.

### Hardware & backend optimization backlog (deferrable; no front-end change)

A consolidated home for the performance proposals that are **pure backend lowering,
driver settings, or library internals** — none touch parser / type checker / IR
*semantics*, so they are safe to add after the language core, enabled by the
"backend-agnostic MIR" invariant (an alternate lowering, not a redesign). Digested from
`work/proposals/` (kept there as raw drafts); listed here so the drafts can be discarded
without losing the backlog.

**Status note (the foundational lever): the LLVM middle-end optimization pipeline is now run.**
`write_object` runs the default **`-O2`** pipeline (`module.run_passes("default<O2>", &tm,
PassBuilderOptions)`) before `TargetMachine::write_to_file`, so the inliner / LICM / loop-vectorizer
/ SLP all run. The lifted lambdas are inlined and the fused `map`/`where`/`reduce` loops vectorize:
e.g. `xs.map(dbl).sum()` lowers to one SSE2 loop (`movdqu` + `paddq`, two `i64` per instruction,
the `dbl` call inlined) with a horizontal-reduction tail — verified via `objdump`, and all
end-to-end tests stay correct under `-O2` (no miscompile from latent IR UB). `emit-llvm` still
prints the *un*-optimized IR (it is for inspecting codegen output). This was the prerequisite for
every vectorization lever below; the remaining ones (the explicit `vec`/`mask`/SoA surface, VLA,
non-temporal, fast-math, `-march=native`) are **M6** proper, alongside the LLVM-version upgrade.

```text
Backend / codegen lowering (MIR -> LLVM, source unchanged):
- Cold Err edge metadata: the `?` / Result Err edge is the designed cold path (§10), but codegen
  emits a plain branch with no branch-weight / cold hint (verified; align_mir notes it deferred).
  Needs a cold-hint on the MIR Result/`?` branch (Term representation) + codegen emitting
  `!prof branch_weights` (or llvm.expect), so the optimizer lays the Err path out of line and the
  predictor assumes Ok. NOT a few lines — it touches the MIR Term, hence backlog not a quick fix.
- Scalable-vector (VLA) loops: emit <vscale x N x T> + predication for ARM SVE /
  RISC-V V, eliminating the scalar remainder loop. (Baseline = fixed-width vec<N> at M6.)
- Non-temporal stores: tag large materializing writes with !nontemporal to bypass cache.
- Fast-math flags on float ops (opt-in): unlock float reassociation / autovectorization.
- -march=native / host CPU feature detection (opt-in; breaks portable "predictable").
- Cross-language LTO: build the Rust runtime to bitcode so align_rt_* helpers inline into
  user loops across the language boundary.
- GPU codegen for pure par_map/reduce: compile the closure to PTX / SPIR-V / MSL, embed as
  a blob, runtime device-dispatch with a length heuristic + unified-memory zero-copy.
  (GPU backend is already listed Future, above.)
- panic=abort build + strip .eh_frame: drop the Rust-std unwinder (cleaner I-cache, marginally
  smaller). The no-unwind CFG itself is already Settled; this is the build-flag half.
  NOTE (2026-06-26): the earlier "~5 MB + libgcc_s" concern is stale — a built example is now
  ~16 KB (14 KB stripped), dynamically linked to libc + ld only (no libgcc_s in `ldd`). Binary
  size / startup is already good; this lever is now only marginal polish, not a real problem.

Runtime / std internals (API unchanged, fast path swapped in):
- SIMD-accelerated runtime: JSON structural scan, str find/split/trim, UTF-8 validation,
  zero-alloc itoa/atoi (an extension of the existing fast atoi/itoa lever).
- Perfect hashing for static keys (already a lever above; JSON field tables / keywords).
- core.bitset (POPCNT/TZCNT/LZCNT) and a default SIMD non-crypto hash (core.hash).
- Buffered, optionally-unlocked stdout (ring buffer; flush on full/newline-to-TTY/exit).
- Zero-copy I/O: mmap+madvise file views, io_uring/GCD async — see "Transparent zero-copy
  I/O (std.io)" above; same hidden-fast-path + portable-fallback rule.

Library architecture principle (record before std is built, applies to all of std):
- Read-oriented std APIs take/return views (str / slice / bytes), not owned copies
  (fs.read_file_view, path.base, env.get). Output APIs write into a caller-provided
  "mut builder" sink (write_json(out: mut builder, …)) rather than returning a fresh
  string. This makes zero-allocation pipelines the default and is painful to retrofit, so
  it is a design rule for std, not an afterthought. (Digested from library-foundations.md,
  api-server-db.md; consistent with design-notes "string philosophy".)
```

### Domain libraries belong to `std`/`pkg`, not core (placement note)

The proposals' application domains are **not core-language work** and must not pull
framework concerns into the core (per `non-goals.md` and `draft.md` §18 layering):

```text
- std (OS boundary): std.fs / std.net / std.io fast paths, std.regex (RE2-style linear-time
  NFA/DFA; a compile-time `rx"…"` literal is a *language* add tracked separately if pursued),
  std.compress (FFI wrappers over libzstd/zlib-ng — gated on FFI).
- pkg (frameworks/ecosystem, kept out of core/std): HTTP/3 client+server, socket tuning
  (TFO/REUSEPORT/thread-per-core), RDB drivers (Postgres/MySQL/SQLite), the API-server
  blueprint. DB ecosystem delegation is already Settled above.
```
These ride on the core capabilities (arena, views, FFI, task_group, zero-copy I/O); they
are downstream consumers, scheduled after the core + std foundations, and are recorded here
only so the vision is not lost when `work/proposals/` is discarded.

### Resource-oriented north star + local LLM inference (Future / direction, not a v1 commitment)

The headline long-term instance of the resource-oriented north star (design-notes "The
resource-oriented north star"). Recorded 2026-06-28 from the `work/` LLM memos; a *direction*
that must not distort the language into a GPU-only ML framework, and must not become a core
dependency. **License posture:** GGUF / llama.cpp / vLLM / FlexGen / FlashAttention are design
references only — no kernel / scheduler / quant / format code vendored; model licenses are
separate from engine licenses.

The bet: not "beat a datacenter GPU" but **"given the CPU/GPU/RAM/SSD/power the user already has,
find the largest useful model and the least-bad execution plan, and say so honestly."** Align owns
the *systems layer* of inference, not the math kernels (a local int8-dot probe beat scalar by only
1.35× — hand-SIMD does not beat mature backends; bind them via FFI first).

```text
Where Align fits (all are existing core strengths, not new language surface):
- model file as a scoped mmap view + typed tensor descriptors (GGUF is mmap-designed)   [zero-copy I/O]
- memory planner: tensor sizes x quant x VRAM/RAM/disk/PCIe -> inspectable placement plan [the strongest native fit]
- KV cache as a first-class planned region resource (paged blocks, prefix sharing)
- tokenizer / sampling / streaming server I/O (mmap vocab, SIMD UTF-8, sink-first token output, task_group)
- diagnostics that tell the truth: "fits VRAM" / "18 GPU layers, slow tokens" / "32k context impossible, try 8k"

Probe evidence (work/kv_cache_planner_probe.rs, 100k mixed-length requests):
  naive contiguous next-pow2   +38.9% memory waste
  paged block 16 / 64          +0.08% / +0.36% waste
  shared-prefix block          -5.2% (below per-request exact sum; shared prompt stored once)
  -> KV cache should be a planned resource, not a hidden vector allocation.

Honest positioning (no overclaim): full VRAM = fastest; partial offload = usable; RAM+mmap+prefetch =
large-model fallback; disk paging during hot decode = last resort unless heavily batched/pipelined.
Main memory cannot replace VRAM bandwidth — each decoded token touches many weights.
```

Realistic milestone shape (far future, after core + std + FFI): a `pkg` that wraps a llama.cpp/Ollama-
class backend, adds zero-copy GGUF metadata inspection + a memory planner, and a local inference server
written in Align (requests, streaming, batching, scheduling). Align-native kernels only for bounded,
benchmarked components (tokenizer, sampling, KV-cache manager, quantized CPU matvec, planner). The
probe-backed std prerequisites (mmap views, buffered/`writev` sink-first I/O, runtime-dispatched SIMD
scan, `task_group` I/O overlap, network pipelining) are exactly the P0/P1 rails above — so this
direction does not add core work, it *consumes* it.

**Mining-adjacent tooling** (profiler / autotuner / energy-aware scheduler / pool client) shares the
"make cost visible" instinct but is a **weaker** north star: ASIC/electricity economics dominate, mature
GPU miners are hard to beat, and the hot loop is tiny/specialized. Acceptable as a side experiment;
**not** a core language driver. Do not optimize the language around speculative profitability.
