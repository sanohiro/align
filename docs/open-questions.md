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
