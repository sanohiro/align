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

### `core.hash` + `core.bitset` (roadmap #6) — design SETTLED (2026-06-29)
The roadmap pairs these two as "#6", but they split cleanly by their prerequisites:

- **`core.bitset` stays deferred to M6** (no new decision — this re-confirms the Bitwise-operators
  ruling above). The `bitset` type is "large, SIMD-friendly", so its layout *is* the M6 `vec`/`mask`/
  SoA/`align(N)` model. Designing it before that model exists is exactly the premature design that
  ruling parked. → not built in #6; it rides M6 (roadmap #7). Nothing to do now but record the split.

- **`core.hash` is the buildable half of #6, and it is the forcing function that settles the
  long-deferred "canonical non-crypto hash" question** (raised in the `group_by` perf notes: FxHash
  vs `ahash`(AES dep) vs hand-rolled AES, "best decided once, applies to all str group paths").
  **Decision: one dependency-free strong mixer — `wyhash` (final v3) — is Align's canonical
  non-crypto hash.** Rationale: keeps the minimal/zero-dep runtime identity (no `ahash`/AES-NI
  dependency, no cross-arch fallback), small (~40 lines), battle-tested (Zig std, V8-adjacent), strong
  avalanche (good enough to expose as a public `hash64`, unlike FxHash whose weak avalanche is fine
  only as a private bucketer). rapidhash (wyhash's successor) was considered and **not** adopted —
  marginally faster but larger/newer for no identity gain. **No user-facing `Hash` trait** (the
  "no trait complexity" non-goal): hashing is over a **byte view** only.
  - **Surface** (`draft.md` §18.1): `hash64(data) -> u64` and `hash128(data) -> (u64, u64)` (Align has
    no `u128`; the 128-bit result is a tuple — the data-oriented spelling). `data` is a byte view:
    `str` or `slice<u8>` (`bytes`). Both are `{ptr,len}` at the ABI, so one runtime entry per width
    serves both input types.
  - **Guarantees:** deterministic for a given input within a build (fixed seed); **non-crypto** — not
    DoS-resistant, not a stable on-disk/wire format, not for security (crypto hashes live in
    `std.crypto`). Documented at the call site.
  - **Convergence (One way) — DONE 2026-07-03:** the *public* `hash64` and `group_by`/`dict_encode`'s
    *internal* hasher (was FxHash) and the JSON PHF (was FNV-1a, with the codegen↔runtime byte-match
    constraint) now all route through the **one** `wyhash`. The single source of truth is a new
    dependency-free crate **`align_hash`** (`crates/align_hash`, `#![forbid(unsafe_code)]`), depended on
    by both `align_runtime` (`hash64`/`hash128`, `group_by`/`dict_encode`, the runtime PHF probe) and
    `align_codegen_llvm` (the compile-time PHF table builder). This makes the PHF byte-match
    **structural, not test-guarded**: codegen's `build_phf` and the runtime's `json_phf_hash` call the
    same `align_hash::wyhash` with the same seed convention, so they *cannot* compute a different slot
    for a field name. The pinned canary (`wyhash(b"score", 0) == 0x1300a50cfadb78d9`) is asserted on all
    three sides (`align_hash::phf_pinned_vector`, codegen `phf_hash_is_pinned`, runtime
    `phf_hash_matches_codegen`); an end-to-end differential test decodes an 8-field struct from JSON with
    keys in reverse order and checks every field routes correctly
    (`align_driver/tests/m5.rs::json_decode_phf_two_end_match_all_fields`). The internal string-interning
    maps use a `WyKey` (pre-hashed with `wyhash`) + pass-through `IdentityHasher`, so each key is hashed
    exactly once with the canonical hash. **Perf (measured before/after, `bench/`):** the JSON PHF path
    (`bench/json_decode`) is neutral (±1.5%, within run-to-run noise — short field names hit wyhash's
    ≤16-byte fast path); the string-keyed `group_by`/`dict_encode` path (`bench/group_by_reuse`) got
    **~1.5–1.8× faster** (wyhash is a cheaper per-lookup hash than the old FxHash finalizer; the win is
    largest when the group map fits in cache and hashing dominates). Integer-key `group_by`
    (`bench/group_by`) is untouched (its dense-id direct-index path never hashes).
  - **Build plan:** runtime `align_rt_hash64`/`align_rt_hash128` (`{ptr,len}` → `u64` / `{u64,u64}`),
    sema builtins `hash64`/`hash128` (like `print`/`error`), MIR rvalue + codegen call, `tests`,
    `examples/hash.align`. Record on build: `draft.md` §18.1, `docs/language-spec.md`,
    `docs/design-notes.md`, `examples/hash.align`, `tests/hash.rs`.

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

### Out-of-range compile-time integer literals — hard error (SETTLED 2026-07-02)
**Decision: a *value* literal whose value provably does not fit the type it is given by context is a
compile error, not a silent two's-complement wrap.** When both the value and the type are known at
compile time (`x: u8 := 300`, an argument, a field initializer, an array element, a return value), a
provably-out-of-range literal is hidden data corruption — at odds with "nothing hidden" — and the
compiler can reject it at zero runtime cost. This is symmetric with `as`'s zero-UB design and with
rejecting a negative literal given an unsigned type. **Runtime arithmetic overflow is unchanged**
(still defined wrap; see "Integer overflow" above) — this is a *static* check on literals only.
Implemented in `align_sema` at the `finalize_expr` seam (after inference resolves each literal's
concrete type): `check_int_lit_range` rejects a bare literal outside `[min, max]`; a negated literal
(`-lit`) is checked at its **effective** value in the `Unary` arm, so `-128` is a valid `i8` while the
positive `128` is not (and a negative literal into an unsigned type still reports only the existing
unsigned-`-` error, not a duplicate). A too-wide **`match` pattern** literal is deliberately *not*
affected — it truncates to the scrutinee's type by the defined wrap rule (`draft.md` §5), since a
pattern is a comparison, not a stored value (integer-literal patterns are not implemented yet, so
this is a spec reservation). Record: `draft.md` §5 ("Integer Literals"), `docs/language-spec.md`
digest; tests in `crates/align_driver/tests/literal_range.rs`.

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

### `bytes` / `buffer` — design SETTLED; minimal `buffer` BUILT with M9 std.io (2026-07-03)
**Decision (2026-06-23): `bytes` is `slice<u8>`; `buffer` is a distinct growable owned byte container.** Resolving the two forks left by `draft.md` §12 (which names the types but specs no operations):
- **`bytes` = `slice<u8>`** — a read-only `{ptr,len}` view of `u8` elements (bytes), structurally identical to a slice of bytes (no UTF-8 invariant — that is what distinguishes it from `str`/`string`). Introducing a *separate* structural type would violate **One way** (two names for one thing), so `bytes` is the conventional spelling of `slice<u8>` in byte/I/O contexts, lowered as `slice<u8>`. `s.bytes()` yields a `slice<u8>` view of a string's UTF-8 bytes; `bytes.to_string()` is the UTF-8-validating inverse (`Result<string, Error>`). (FFI already treats `bytes` as a view handed to C by raw pointer — consistent.)
- **`buffer` = a distinct Move type**: an owned, **growable**, mutable sequence of `u8` (the byte analog of a `Vec<u8>`). It is *not* `array<u8>` (fixed length) nor `builder` (an append-only *text* writer that produces a `string`); `buffer` is random-access + growable + freezable raw bytes for the *binary* domain. Ops: `buffer()` / `buffer(cap)`, `.push(b)`, `.append(slice<u8>)`, `.len()`, `buf[i]` read/write, `.bytes()` (view), and freeze → owned `array<u8>` or `.to_string()` (UTF-8 validate). It is the first growable container.
- **Build (was deferred until a consumer) — the minimal `buffer` landed with its first consumer, M9 std.io Slice 1 (2026-07-03).** `Ty::Buffer` is an owned Move handle to a growable heap `Vec<u8>` (`Drop`-freed); the shipped ops are the subset `reader.read` needs — `buffer(cap)` (a read window), `.bytes()` (the `slice<u8>` view, region-tracked to the buffer so it can't escape), `.len()`. The rest of the settled op set (`.push`/`.append`/`buf[i]` read/write/freeze → `array<u8>`/`.to_string()`) is still deferred to its next consumer (`core.hash` / binary parsing) — same "build ahead of a consumer risks the wrong shape" rationale, now applied per-op. `bytes` remains `slice<u8>` (no separate structural type).
Record: `draft.md` §12/§18.2, `impl/07-roadmap.md` M9 Slice 1, `crates/align_driver/tests/m9_io.rs`.

### Ownership syntax
**Decision: ownership is a property of the type, not a keyword.** `array<T>`/`string`/`buffer`/heap are Move; primitives/small structs/`slice` (view) are Copy. No `owned` modifier is introduced. Lifetimes are inferred and lifetime syntax is not surfaced.
Record: `impl/03-types.md` §6–§7

### SIMD exposure (basic policy)
**First slice DONE (M6 slice 1) — explicit `vecN<T>`.** The fixed-width vector type
`vec2`/`vec4`/`vec8`/`vec16` of a numeric scalar (`Ty::Vec(Scalar, N)`, Copy/`Static`, LLVM
`<N x T>`). Two design points were **settled here** (the spec was silent on them):
- **Construction = an array literal under a `vecN<T>` annotation** (`a: vec4<f32> := [1.0, 2.0, 3.0,
  4.0]`), not a separate constructor/splat. Rationale: `[…]` is already the language's fixed-sequence
  literal; the annotation picks the SIMD representation, exactly as a literal int's width comes from
  context — one way, nothing hidden. (A scalar broadcast `vecN<T>(x)` is a later, additive form.)
- **Lane read = `v[i]` with a constant index** (extractelement). A SIMD lane is a fixed position, so
  the index must be a compile-time constant in `0..N` (a dynamic lane would risk an out-of-range
  poison value); lane *assignment* `v[i] = x` is deferred.
Elementwise `+`/`-`/`*`/`/` lower to one lane-wise hardware instruction each. The `vec4<f32>`
N-in-name spelling needs no lexer/parser/AST change. (`crates/align_*`, `tests/vec_simd.rs`,
`examples/vec_simd.align`.)

**Slice 2 DONE — `mask` + comparison + `select`.** A `vecN<T>` comparison (`==`/`!=`/`<`/`<=`/`>`/
`>=`) is elementwise and yields a **`mask`** — `Ty::Mask(N)` → LLVM `<N x i1>`, one bool lane per
vector lane. Settled here: the mask is **width-only / element-agnostic** (a width-`N` mask blends any
two `vecN<T>`) and **produced/consumed inline** — no written `mask<T>` annotation yet (the surface
spelling `mask<T>` carries no width, so the annotation is deferred until a use needs it).
`select(mask, a, b)` (a `core.vec` builtin) is the consumer: lane `i` is `a[i]` where the mask is set,
else `b[i]` (so `select(a > b, a, b)` is elementwise max). Comparisons reuse `ExprKind::Binary`
(codegen `gen_bin` routes a vec operand + comparison op to `gen_vec_cmp` → vector `icmp`/`fcmp`);
`select` is `hir::Select` lowering to the existing `Rvalue::Select`, **extended to accept a vector
cond** (reused from branchless `where`'s scalar select). Width is checked between the mask and the two
vectors. (`examples/vec_mask.align`.)

**Slice 3 DONE — scalar broadcast + `sum_where`.** A **scalar on the right** of a vector op
broadcasts across the lanes (`a + 5`, `scores > 80` — the draft §9 spelling). Settled here: broadcast
is **implicit in `vec OP scalar`** (a cheap, lossless splat implied by the operand types — not a
hidden allocation or a lossy coercion, so it stays within "nothing hidden"), and the **vector must be
on the left** (scalar-on-the-left and a vector-literal right operand are deferred — they need
bidirectional inference the one-pass checker doesn't do cleanly yet). The scalar's type unifies with
the element (`vec4<i32> + 2.0` is rejected — int vector, float scalar). `vec.sum_where(mask)` is the
**masked horizontal sum** (the first vec→scalar reduction): `select(mask, vec, 0)` then add all lanes
→ the element scalar, so `scores.sum_where(scores > 80)` runs (draft §9). Codegen splats via an
all-lane insertelement chain (`operand_as_vector`) that folds to a hardware broadcast; `sum_where` is
`hir::VecSumWhere` → `Rvalue::VecSumWhere`. (`examples/vec_sum_where.align`.)

**Slice 4 DONE — `dot`.** `dot(a, b)` is the dot product of two `vecN<T>` → the element scalar
`sum(a[i] * b[i])`. Settled here: the vector `dot` is the **free-function** form `dot(a, b)` (the
draft §9 spelling, the vector sibling of `select`), kept **distinct from the array pipeline terminal
`xs.dot(ys)`** (a method — a fused loop over arbitrary-length arrays). They are different operations
(a fixed-width register reduction vs an array pipeline) on different types, spelled differently, and
never collide at parse time (a free call vs a method call) — so this is not a One-Way violation, the
same way `select` (a vec primitive) coexists with `where` (a pipeline stage). Lowers to a vector
multiply then a shared `horizontal_sum` lane reduction (the multiply dual of `sum_where`); int +
float. (`examples/vec_dot.align`.)

**Slice 5 DONE — `min` / `max`.** `v.min()` / `v.max()` — the horizontal min/max of a `vecN<T>` →
the smallest/largest lane, as the element scalar. Settled here: it shares the **array-reduction
surface** `arr.min()`/`arr.max()` (a no-arg method, "one way"), disambiguated by a **non-destructive
receiver peek** — `is_vec_local_recv` checks whether the receiver is a *local of vector type* without
`check_expr`-ing it, so a vector local routes to the SIMD reduction while an array source / pipeline
(`xs.where(p).min()`) still routes to the array path (which `check_expr`-ing the receiver would have
broken — a pipeline-without-terminal is an error). Lowers (`hir::VecMinMax` → `Rvalue::VecMinMax`) by
folding the lanes with the **same `llvm.{s,u}{min,max}` / `llvm.{minimum,maximum}` intrinsics as the
`core.math` scalar `a.min(b)`/`a.max(b)`**, so the reduction matches that semantics exactly (incl. the
IEEE `minimum`/`maximum` NaN/signed-zero behavior for floats); int / unsigned / float. The receiver
is generalized to **any vector value** (not just a local): the dispatch routes to the array reduction
only for a syntactically pipeline-shaped receiver (`is_array_pipeline_recv` — a `.map()`/`.where()`
stage or a `.field` projection), and type-checks every other receiver to detect a vector. (`examples/vec_minmax.align`.)

**Slice 6 DONE — bare `v.sum()`.** `v.sum()` — the horizontal sum of a `vecN<T>` → the sum of all
lanes, as the element scalar (the unmasked sibling of `sum_where`). Same dispatch shape as `min`/`max`
(a vector receiver → the SIMD reduction; an array pipeline `xs.map(f).sum()` → the fused array path).
`hir::VecSum` → `Rvalue::VecSum`, reusing the shared `horizontal_sum`; int + float. **The vector
reduction surface (`sum`/`sum_where`/`dot`/`min`/`max`) is now complete.** Still deferred:
scalar-on-the-left broadcast, array load/store, the generic `vec<N,T>` spelling, lane assignment, a
written `mask<T>` annotation, and a SIMD-unit **tree reduction** (the reductions extract-and-fold
today — semantics-exact and -O2-reshaped, but a shuffle tree would keep it on the vector units).
(`examples/vec_sum.align`.)

**Slice 7 DONE — array load/store (the array ↔ vector bridge).** `s.load(i) -> vecN<T>` reads `N`
consecutive elements of a `slice<T>` from runtime index `i` into a vector (`N`/`T` from the target
annotation, like a vector literal); `s.store(i, v)` writes a vector's lanes into a **writable**
(`mut`/`out`) `slice<T>` at `i..i+N`. Settled here: the surface is **method-form on a `slice<T>`**
(`s.load(i)` / `s.store(i, v)`), with the width from the annotation and a runtime offset — a fixed
array is loaded/stored by passing it where a slice is expected (the array→slice borrow; nothing
hidden). Both are **bounds-checked** (`0 <= i && i + N <= len`, reusing the range-fail path); the
store reuses the `out`-slice writability rule (`place[i] = v`). Codegen GEPs `&buf[i]` and emits the
`<N x T>` load/store **at the element alignment** — the GEP yields only an element-aligned pointer, so
assuming the wider vector alignment would be UB on strict-alignment targets (an unaligned-but-valid
vector access). `hir::VecLoad`/`hir::VecStore` → `Rvalue::VecLoad` / `Stmt::VecStore`.
(`examples/vec_load_store.align`.)

**Slice 8 DONE — lane assignment `v[i] = x`.** Writes one lane `i` (a constant in `0..N`) of a `mut
vecN<T>` local to the scalar `x` — the write counterpart of the lane read `v[i]`. A vector is a
register value (not memory), so it lowers to `v = insertelement(v, x, i)`: a new `Place::VecLane`
(detected in `check_place` when the index target is a vector local) → `hir::Stmt::AssignVecLane` →
`Rvalue::VecInsert`, which re-stores the updated vector into the local. Reuses the mutable-place
writability rule (a `mut` local; an immutable vector, or a dynamic / out-of-range lane, is rejected,
matching the lane read). (`examples/vec_lane_set.align`.)

**Slice 9 DONE — scalar-on-the-left broadcast.** A scalar on the **left** of a vector op broadcasts
too (`10 + a`, `2 < scores`), completing the broadcast symmetry (slice 3 settled implicit `vec OP
scalar`; this lifts the "vector must be on the left" cut). The operand order is preserved for the
non-commutative ops (`20 - a` = `[20 - a0, …]`). Settled mechanism: the one-pass checker handles the
ambiguity with a **speculative rhs check + diagnostic rollback** (`check_binop_rhs`) — the rhs is
hinted with the lhs type as usual, but if the lhs is a scalar and the rhs is a vector, that hint
mis-constrains, so its diagnostics are rolled back (`Diagnostics::truncate`) and the rhs re-checked
unhinted, letting the scalar broadcast. This regresses nothing: a scalar+scalar or generic-call rhs
still gets the lhs hint (no rollback). `vec_binop` gained the `(scalar, vec)` case; codegen detects
the vector in either operand and `operand_as_vector` splats the scalar. (`examples/vec_broadcast.align`.)

**Slice 10 DONE — written `maskN<T>` annotation.** A comparison mask is now a **nameable type**, so it
can be a `let` annotation, a function parameter, or a return type (threading a mask through code).
Settled here: the spelling is **`maskN<T>`** — N-in-name like `vecN<T>`, with the same width and
element as the compared vectors (`mask4<i32>` = the result of comparing `vec4<i32>`s). This amends the
spec's `mask<T>` (draft §13) exactly as `vec<N,T>` → `vecN<T>`: the **width must be in the type**, and
the spec's lone `<T>` left it ambiguous. `Ty::Mask(u32)` became `Ty::Mask(Scalar, u32)` (element +
width) so the type is fully meaningful and type-safe — `select`/`sum_where` now require the mask's
**element and width** to match the vectors (operationally a mask is still `<N x i1>`, element-
independent; the element is part of the *type*, not the repr). `resolve_type` gained the `maskN<T>`
arm (`parse_mask_name`). The decision to make the mask element-aware (vs the previous width-only
`Ty::Mask(u32)`) is the type-safe choice and matches the spec's element-parameterized intent; the
minor flexibility loss (an `i32`-comparison mask can no longer select `f32` vectors) is acceptable and
arguably more correct. Still deferred: the generic `vec<N,T>` / numeric-type-arg spelling, an aligned-
load fast path, the SIMD-unit tree reduction. (`examples/vec_mask_annot.align`.)

**Decision: `vec<N,T>` + auto-vectorization as the baseline.** Make mask first-class. The fused
pipeline lowers `where` / conditional reductions **branchless** (mask + `select`, not a per-element
branch — `impl/05` §5), which is what keeps hot loops vectorizable and branch-predictor-friendly.
(Whether to place explicit SIMD intrinsics in std is open, see below; **wide SIMD on a varied fleet
comes from the library layer's runtime dispatch — see "Build targets & portability".**)
Record: `draft.md` §9, `impl/04-mir.md` §4, `impl/05-backend-llvm.md` §5

**Addendum (2026-07-02, internal review — MIR width-agnostic invariant):** amends the above. **MIR
carries vectorization-*enabling properties*** — element independence, `Effect=Pure`, `out`-derived
noalias, trip count, a reduction's monoid (identity + associative op), and the access plan
(contiguous/strided) — **and never bakes in a vector width.** Width is permanently a *backend*
decision: fixed-width + scalar remainder on NEON/AVX-class ISAs, scalable + predication on SVE/RVV.
(Was: MIR shapes a fused loop as width `W` + remainder, per `impl/04-mir.md` §4 / `impl/05-backend-
llvm.md` §5 as originally written — that baked a fixed-width assumption into the backend-agnostic IR
and is now understood to be wrong once scalable ISAs are in view; corrected at the documentation
level before M6 locks the lowering in.) **Two-tier SIMD positioning, stated explicitly:**
`vecN<T>`/`maskN<T>` stay the **fixed-width kernel escape hatch** (hand-tuned dot/FMA/FIR-style code,
always a compile-time-constant width, never scalable) while the **pipeline** (`map`/`where`/`reduce`)
is the **width-agnostic main path** — it names no width in source, so scalable ISAs live there
invisibly, the same way choosing AVX2 vs NEON is already a hardware detail, not a semantic one.
Opus and Codex, asked the same question independently, converged on this exact conclusion. Record:
`impl/04-mir.md` §4, `impl/05-backend-llvm.md` §5 (doc update landed), this file's Future →
"Hardware & backend optimization backlog" (scalable-vector / matrix-engine entries).

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

**Whole-element gather slice DONE — `s[i]`.** Indexing a `soa<Struct>` now gathers a **whole struct
value** from the columns at `i` (`check_index` gains a `Ty::Soa(id) => Ty::Struct(id)` arm; MIR
`lower_index` → `Rvalue::SoaGather`, which loads every column's element via the shared
`soa_column_offset` and builds the struct via insert-value). This resolves the **"Move fields in
`soa<T>`" sub-question for the Copy case**: a soa is primitive-only, so the gather **copies** — the
result is a free `Static` Copy value (`region_of` special-cases a soa `Index` to `Static`, not the
soa's borrowed region), so it can escape the arena the soa was built in. The whole-struct pipeline
*stage* over a soa (`map(fn)`/`where(fn)` taking the struct) stays rejected — that would gather every
column per element; for one field use `s.field[i]` (project then index) or gather then read
(`r := s[i]; r.field`). Still deferred: owned/nested columns, `soa_slice<T>`
sub-views, bitset/bool packed columns. (`tests/soa.rs`, `examples/soa.align`.)

**`str` columns in `soa<T>` — DONE (2026-07-01).** A `soa<Struct>` may now hold `str` columns. A
`str` field decodes (via `json.decode → soa`) as a column of 16-byte `{ptr,len}` views borrowing the
JSON input — the whole runtime/codegen path was **already str-aware** (`scalar_bytes(Str)=16`, the
descriptor `tag`'s `(3<<8)|16`, the `write_field_indexed` `kind==3` AlignStr write feeding `SoaDst`,
the `soa_column_offset`/`soa_layout` width-as-alignment walk, and the `IndexColumn`/`SoaGather` loads
that go through `scalar_type`/`abi_type` and so load the 16-byte aggregate). The slice was therefore
**sema-only**: relax the primitive-only guards on the `soa<T>` type and the `json.decode → soa` decode
(both now accept `Ty::Str`), and — the soundness core — the **region tie**. A str-bearing soa's
columns borrow the input, so it is no longer arena-self-contained: `region_of(JsonDecodeSoa)` becomes
`region_of(input).shorter(arena(depth))` when the struct has a str field (a new `struct_has_str`
predicate gates it), `s[i]` gather inherits the soa's region instead of `Static`, and the `SoaColumn`
projection inherits its base's region (closing a `slice<str>` escape hole). A primitive-only soa is
unchanged — still arena-regioned and free to escape the input (`s[i]` gather returns a Copy POD).
Escape-checked end to end (`str_column_view_cannot_escape_the_arena`; `primitive_soa_stays_self_contained`
guards the non-regression). `tests/soa.rs`, `examples/soa_json_str.align`, `draft.md` §9.

**`str` column WRITES — DONE (2026-07-03).** `s[i].name = v` (single column) and `s[i] = value`
(whole-element scatter) on a str-bearing soa now type-check and run. A `str` view is a **Copy**
16-byte `{ptr,len}` — a str-bearing soa is a view-Copy aggregate (owns no buffer, needs no per-field
drop), so both writes ride the *existing* store machinery: the per-field `StoreColumn` scatter is
already str-capable (it built the `to_soa` / decode columns), and the store's escape is already
guarded by the `AssignElemField` / `AssignElem` region rule
(`region_of(value).outlives(region_of(base_soa))`). This is the exact **dual** of the read escape
check: a stored view that does not outlive the soa (an inner-arena view scattered into an outer-arena
soa — directly, via a gather, or via a struct literal whose `StructLit` region folds to the shorter
field) is a compile error. The only code change was a one-predicate sema gate relax (`str_view`:
every field a Copy scalar incl. `str`, soa-only) on the whole-element store; the single-field
`s[i].name = v` was already reachable (the `AssignElemField` gate only restricts the *dynamic-array*
pointer-store path) and is now locked by tests. MIR/codegen needed nothing. Tests:
`str_column_single_field_write`, `str_column_field_write_cannot_store_shorter_lived`,
`str_column_whole_elem_write_scatters`, `str_column_whole_elem_write_cannot_store_shorter_lived`,
`str_column_whole_elem_write_via_literal_cannot_store_shorter_lived` (`tests/soa.rs`).

**Owned columns (`string`/`array<T>`) — still deferred; this is the remaining "Move fields in
`soa<T>`" open item above.** An owned column is *owned per element*, so it is a real slice (drop +
move wiring, not a new analysis mechanism): (a) a write `s[i] = value` / `s[i].name = v` must **drop
the overwritten element's owned field** before storing and **move** the RHS in (null its source,
like the fixed-array Move element path) — `StoreColumn` has no drop today; (b) dropping the whole
soa must **free every owned element of every owned column** (no per-column drop exists); (c)
`region_of` must treat an owned column as **self-contained** (arena/frame, not a borrow of the
input), and a gather `s[i]` of an owned column stops being a free Copy (it would deep-copy or move a
field out of the column — the invalidation the "Move fields in `soa<T>`" note warns about). Defer
until pursued.

**`.to_soa()` with str columns — DONE (2026-07-01).** The transpose analogue: `arr.to_soa()` over an
AoS `array<Struct>` with a `str` field now copies each element's `str` view into a view column. The
MIR transpose (`transpose_to_soa`: a fused loop of `lower_field_access` reads + `StoreColumn` writes)
and its codegen (`StoreColumn`/`SoaAlloc` via the str-aware `soa_field_sizes`/`soa_column_offset`, a
16-byte aggregate store) were **already str-capable** — same as the json path — so this too is
sema-only: relax the `check_array_to_soa` guard to accept `Ty::Str`, and tie the region to the
**source** (not the input): `region_of(ArrayToSoa)` becomes `region_of(source).shorter(arena(depth))`
when the struct has a str field (a primitive-only `to_soa` stays purely arena-regioned). Reads only,
like the decode path. `tests/soa.rs` (`to_soa_transposes_a_str_column`,
`to_soa_str_column_view_cannot_escape_the_arena`, `to_soa_with_a_nested_field_struct_is_rejected`).

**str-key `group_by` over a `soa<Struct>` — DONE (2026-07-01).** `s.group_by(.name).{sum,min,max}(.pay)`
/ `.count()` now works when the key is a **`str` column** (previously a soa keyed only on an i64
column; a str key required an AoS `array<Struct>`). Since a soa can hold str columns, this is the
natural columnar counterpart of the AoS str-key rail. Same surface, same result `(array<str>,
array<i64>)`. Implementation: a new `hir::GroupSource::SoaStr` (chosen in `check_group_agg` when the
soa key field's type is `Ty::Str`, else `SoaI64`); MIR `lower_array_group_str_cols` extracts the key
column (`slice<str>`) + value column (`slice<i64>`) via `SoaColumn` (like the i64-key soa path) and
emits a new `Rvalue::GroupAggStrCols`; codegen extracts the two column base pointers and calls a new
runtime `align_rt_group_{sum,min,max,count}_str_cols(key_col, val_col, n, out_keys, out_vals, cap)`.
The runtime **shares one core** with the AoS str path: `group_agg_str` was refactored to take
`key_at`/`value_at` **index closures** (mirroring `group_agg_i64`'s `per_row`), so the AoS wrapper
feeds strided-record closures and the soa wrapper feeds two-contiguous-column closures — one interning
implementation, two column layouts. **Region**: the str keys borrow the soa's string storage, so
`region_of(ArrayGroupAgg{SoaStr})` inherits `base`'s region (added to the same arm as `AosStr`) —
escape-checked (a str-key result can't leave the arena; an i64-key result's owned keys still can).
`tests/soa.rs` (`soa_str_key_group_by_all_aggregates`, `…_type_checks_and_selects_by_key_column`,
`…_result_cannot_escape_the_arena`), runtime `group_str_cols_aggregates_two_separate_columns`,
`draft.md` §9. **Deferred:** fused multi-aggregate (`.agg(...)`) over a soa str key (still AoS-only).

**Scalar-accessor slice DONE — `s.len()` + `s[i].field`.** A soa now answers `s.len()` (its row
count — the `{ptr,len}` length, via `ExprKind::Len` → `SliceLen`) and `s[i].field` (one column's
element directly, the column-major analogue of AoS `arr[i].field`). `s[i].field` reuses the fused
`check_index_field` / `lower_index_field` path: a soa receiver sets `struct_view = (id, Layout::Soa)`,
so the shared `lower_field_access` seam emits `IndexColumn` (one column read, **not** a whole-struct
gather — verified in MIR). soa fields are scalar, so the field path is always length 1 and the leaf is
Copy (no region/move concern). (`tests/soa.rs`, `examples/soa.align`.)
**Column-windowing slice DONE — `s.field[a..b]` (+ a `SoaColumn` offset bug fix).** A projected
column `s.field` is an ordinary `slice<FieldTy>`, so it **windows** with the existing slice sub-range:
`s.pay[1..3].sum()` scans rows `1..3` of one column. No new type, no sema arm — the SubSlice path
applies as-is once the column base is correct. Fixing that base was the real work: `Rvalue::SoaColumn`
(the **value-materialization** path — `c := s.field`, passing a column, or sub-ranging it) computed
the column byte offset as a **flat `len * prefix_bytes`**, while the per-element
`IndexColumn`/`StoreColumn`/`SoaAlloc` paths use the `align_up`-padded `soa_column_offset`. The
mixed-width `align_up` work (the "Second slice" note above) had only been applied to the per-element
path, so a materialized column after a *narrower* one (`i64` after `bool`) pointed mid-padding and read
garbage — a **silent wrong answer** that the example/tests missed because they only used the
pipeline-source (`IndexColumn`) path. `SoaColumn` now calls the same `soa_column_offset`, so all four
soa addressing sites agree. Regression + window tests in `tests/soa.rs`; `examples/soa.align`.
**Multi-column `soa_slice<T>` (`s[a..b]` over *every* column) stays deferred** (and remains the open
shape from the "Views/borrows of `soa<T>`" sub-question above): unlike a single column, a multi-column
sub-view cannot reuse the `{ptr,len}` repr, because each column's stride is `align_up(total_rows *
prefix, …)` — a function of the **original** row count, not the window length. A correct view needs
`{ptr, total_len, start, count}` (threaded into `soa_column_offset` + a `+start` element bias at every
access site, plus a 4-field runtime `json.decode → soa` out-write) — a cross-stage view-repr change of
the same weight class as the deferred `bitset`. The single-column window covers the primary use
(windowed column reduction) with none of that cost, so the multi-column view waits until a concrete
need (e.g. a function taking a windowed multi-field view) justifies the repr change.

**Design finalized (2026-07-03) — repr = unify, not a distinct type; implementation still deferred.**
When picked up, do it as the **degenerate-form unification**, not a separate `soa_slice<T>` type:

- **Repr decision: widen the *one* `soa<T>` view to 4 words `{base_ptr, total_rows, start, count}`.**
  A full soa is the degenerate `{ptr, rows, 0, rows}`; `s[a..b]` is `{ptr, rows, start+a, (b-a)}`.
  `soa_slice<T>` is then **spec-level sugar for a windowed `soa<T>`, not a new `Ty`** — exactly how
  AoS `s[a..b]` is a view-adjustment of `slice<T>`, never a new type. This is forced, not optional: a
  function parameter typed `soa<T>` must accept both a full and a windowed soa, so the two **must share
  one ABI** → both carry the window state. Rejected alternatives: (B) a distinct `Ty::SoaSlice` with
  `soa<T>` staying 2-word — duplicates *every* column-addressing site (a second mechanism for the same
  thing, violates "one way") and needs a soa→soa_slice coercion; (C) per-column base pointers `+ len`
  (the old "small struct of per-column pointers" lean) — variable-width repr per field count, non-
  uniform LLVM type, precomputes all columns even for a one-column pipeline. (A) is fixed-width,
  uniform (extend `slice_struct_type` from 2 to 4 fields), and keeps single-column projection cheap:
  `s.field` still lowers to a plain 2-word `slice<FieldTy>` = `{base + col_off(total_rows) + start*sz,
  count}`, so the whole downstream scalar pipeline is unchanged.

- **Why still deferred:** the widening is the named defer trigger (large ABI ripple). It changes the
  soa view from 16→32 bytes, which crosses the SysV register→memory boundary for by-value soa params
  (internal-only, so still self-consistent, but every existing soa call site re-lowers), and it must be
  landed *atomically* across all consuming sites + re-green the whole `tests/soa.rs` suite (162 fns).
  With no in-tree consumer yet (the single-column `s.field[a..b]` window already covers windowed column
  reduction), this stays gated on a concrete "function taking a windowed multi-field view" need.

- **Consuming sites to touch (complete map), all currently assuming `total_rows == count == start-0`:**
  1. `abi_type`/`llvm_type` `Ty::Soa` arm (×3) → a new 4-word `soa_view_type` (not `slice_struct_type`).
  2. codegen `Rvalue::IndexColumn` (pipeline element read + `s[i].field`): stride from `total_rows`,
     element at `(start + index)`.
  3. codegen `Rvalue::SoaGather` (`s[i]` whole-struct gather): same, per column.
  4. codegen `Rvalue::SoaColumn` (`s.field` projection → `slice<FieldTy>`): `{base + col_off(total_rows)
     + start*size, count}` (result is a plain 2-word slice — the bridge that keeps pipelines unchanged).
  5. `Stmt::StoreColumn` (`s[i].field = v` + `to_soa`/decode construction): add `total_rows` + `start`
     (today it carries a single `len` operand; construction uses `start = 0`).
  6. `Rvalue::SoaAlloc` — unchanged (allocates `total_rows` rows; stride math already uses that `len`).
  7. `soa_column_offset` — signature unchanged (already takes `len` = `total_rows`); callers pass
     `total_rows` and add the `+start` element bias themselves.
  8. view construction: `transpose_to_soa` builds the view via `MakeDynArray {ptr,len}` today → needs a
     4-word build (`{ptr, rows, 0, rows}`) — add a `Rvalue::MakeSoaView` (or extend the constructor).
  9. `Rvalue::JsonDecodeSoa`: keep the runtime writing a 2-word `{ptr,len}` into a scratch slot, then
     **codegen expands** it to `{ptr, len, 0, len}` in the out slot — **no runtime ABI change needed**
     (cheaper than the "4-field runtime out-write" the note above imagined).
  10. sema `check_slice_range`: add a `Ty::Soa(id) => Ty::Soa(id)` arm (currently the `other =>` reject);
      `s[a..b]` reuses the existing `SliceRange` HIR/AST — **no grammar change** (same surface as AoS).
  11. MIR `lower` `SliceRange` — add a `Ty::Soa` arm building the windowed 4-word view.
  12. region/escape: **nothing** — `region_of(SliceRange) = region_of(recv)` already ties the sub-view
      to the parent soa (str-bearing soas already carry the input-tied region); soa is Copy so no move
      concern. This is the one part already done.
  13. `s.len()` on a windowed soa → `count` (field 3, not field 1). `group_by` consumes columns via
      `SoaColumn` (site 4), so it needs no direct change once the projection is window-aware.
  Spec: `draft.md` §9 gains a windowed-view paragraph (result type `soa<T>`; `soa_slice<T>` named only
  as the conceptual term for a windowed soa). Estimated ~330 LoC across sema/MIR/codegen + tests
  (sub-view projection / gather / pipeline-source / `.len()` / escape).

**In-place element-field write slice DONE — `s[i].field = v` (+ AoS `arr[i].field = v`).** The write
counterpart of the `c[i].field` read, closing the read/write symmetry: you could read a struct-array /
soa element's field but not store it (`invalid assignment target`). One surface — `c[i].field = v` —
over both layouts, dispatched by the receiver local's type: a `soa<Struct>` lowers to a column store
(`Stmt::StoreColumn`, the `align_up` column offset), a fixed `array<Struct>` to a slot element-field
store (`Stmt::StoreElemField`, a `[0,index,field]` GEP). Both store ops already existed (emitted by
`.to_soa()` construction); this slice just makes them reachable from a user assignment. New
`hir::Stmt::AssignElemField` + `Place::ElemField`; the `check_place` `FieldAccess{ Index{ local, i },
field }` branch resolves it, `mut`-gated (writing through a soa view requires a `mut` view binding, the
slice-mutability precedent). Bounds-checked at the write (same `index_fail` path as a read). The stored
value is a **scalar** field, so MoveCheck/EscapeCheck treat it exactly like `AssignIndex` (Copy value +
index, base is a use) — no move/region/drop concern, so the new Stmt needed no new analysis logic, only
an or-pattern next to `AssignIndex` at each exhaustive `Stmt` match (the compiler forced all five).
**Deferred: the dynamic `array<Struct>` (`DynStructArray`) element-field write** — its read uses the
pointer-based `Rvalue::IndexFieldPtr`, so the write needs a `StoreElemFieldPtr` dual that does not yet
exist (the fixed `StructArray` and `soa` both had a store op already, which is why they ship now).
Tests: `tests/struct_index.rs` (AoS), `tests/soa.rs` (soa); `examples/soa.align`.

**Whole-element write slice DONE — `s[i] = value` (+ AoS `arr[i] = value`).** The write counterpart of
the `s[i]` gather / `arr[i]` whole-element read, completing the element read/write matrix (read whole /
read field / write field / **write whole**). One surface — `c[i] = structval` — over both layouts via
`hir::Stmt::AssignElem` + `Place::Elem`: a `soa<Struct>` materializes the value into a temp slot and
**scatters** each field into its column (`StoreColumn` per field; columns are non-contiguous, so no
single store), a fixed `array<Struct>` does **one aggregate `StoreIndex`** into the element (`[0,index]`
GEP). `mut`-gated, bounds-checked. **First cut is plain-old-data structs** — the sema gate requires
every field to be a flat numeric/bool/char scalar (not `str`, not nested, not owned), so the value is a
Copy aggregate with **no region/move/drop**: the new `Stmt` again rides the `AssignIndex` or-pattern at
every exhaustive `Stmt` match (index + value walked, base is a use). A `str`-bearing struct would store
a borrowed view into the element (an escape concern) — deferred with the nested/owned cases. The
plain-data gate matches what soa already enforces on its columns, so `soa<Struct>` always qualifies;
the restriction only bites AoS arrays of `str`/nested structs. Tests: `tests/struct_index.rs` (AoS:
literal value, struct-local value, `mut`-required, `str`-field rejected), `tests/soa.rs` (soa: scatter,
gather→scatter `s[0]=s[1]`, `mut`-required); `examples/soa.align`.
Record: `draft.md` §3.4 / §9, `impl/05-backend-llvm.md` §3, `impl/04-mir.md` §3, `tests/soa.rs`, `bench/`.

### Default struct layout: field reordering — SETTLED + DONE (2026-07-02)
**Decision: a non-`layout(C)` struct has an *unspecified* field order; the compiler reorders fields
by descending alignment (ties keep declaration order) to eliminate padding** (Rust's default).
`{ a: i8, b: i64, c: i8 }` occupies 16 bytes, not 24. Source access is by name, so the reorder is
invisible and free; it packs hot structs tighter — a direct cache-density win, the language's center
of gravity. `layout(C)` is the escape hatch: it keeps declaration order + natural alignment + no
reordering (the FFI / `raw` / `json`-encode / by-value byte-layout boundary, unchanged).
**Implementation:** the reorder + a **logical→physical field-index map** (`field_perm[struct_id]`)
live in *one* place — the struct `set_body` in `align_codegen_llvm`. Every field-index consumer routes
through it: struct-field GEPs (`field_path_ptr`, `elem_field_ptr`, AoS `IndexFieldPtr`,
`NullStructField`, `DropElemField`, the `drop_struct_fields` walk), byte-offset sites
(`offset_of_element` for the `json.decode` field table, `group_by`/dict key & value offsets,
`GatherColumnI64`), and the `soa` gather's struct-aggregate insert. `sizeof`/alignment follow for free
(read back from the built LLVM struct type). `layout(C)` structs use the identity map. `soa` column
order stays in declaration order (a separate, self-consistent column layout independent of the AoS
field order). Tests: `tests/struct_field_reorder.rs` (padding elimination via the emitted LLVM type,
all-width round-trip, field writes, struct-array element fields, json decode, `to_soa`, `layout(C)`
unchanged, nested structs); the differential struct fuzzer (`tests/fuzz_differential.rs`) sums *all*
mixed-width fields back against an oracle. All ~1057 workspace tests green.
Provenance: surfaced by an external idea review (2026-07-02); adopted + implemented same day.
Record: `draft.md` §9 (memory layout) + §15 (`layout(C)`), `docs/language-spec.md`,
`docs/design-notes.md`, `impl/05-backend-llvm.md` §2, `tests/struct_field_reorder.rs`.

### Branchless `where` (all reducing terminals) — DONE (sum/count 2026-06-27; extended to min/max/any/all/reduce 2026-07-02)
**Decision: a `where`/`where(.field)` feeding *any* reducing terminal lowers branchless** — AND the
predicates into a `mask`, then `select` each masked-out lane to the reducer's identity instead of a
per-element branch (`Rvalue::Select` + `accumulate_mask` in `align_mir`). Fixed identities:
`sum`/`count` → `0` (`acc += mask ? value : 0`, `count += mask ? 1 : 0`), `min` → `+∞` / `max` → `−∞`
(the `extreme_of` fold seed), `any` → `false` / `all` → `true`. Generic `reduce` has no identity for
its user `f`, so it uses the **accumulator-select** form `acc = mask ? f(acc,v) : acc` (a masked-out
lane leaves the accumulator unchanged). `min`/`max` also moved from a compare-and-branch update to
the `select(cur `cmp` acc, cur, acc)` idiom, so the plain (no-`where`) path is branch-free too — one
lowering, no dual mechanism. Semantics are byte-identical to the branch form: same ordered comparison
(NaN elements still skipped by `min`/`max`), same empty-selection result (`min`/`max` → the extreme
seed, `reduce` → `init`, `any` → `false`, `all` → `true`). `dot` is out of scope — `a.dot(b)` is a
two-array kernel with no `where`, already branch-free. NB: the branchless form runs a reducer's own
`f`/predicate (and any post-`where` stage) on masked-out elements too, its contribution discarded —
the deliberate cost of a vectorizable, predication-ready loop (pipeline functions are pure, so this
cannot differ observably); this already held for `sum`/`count`. **Why it matters (deferred before):**
the single-column `s.where(p).sum()` over `slice<i64>` already vectorized via LLVM if-conversion — no
gain. But the **soa filtered aggregate** `rs.where(.active).pay.sum()` (bool mask column + i64 value
column) did NOT auto-vectorize — scalar, 20 branches, branch-bound, **0.93× vs Rust AoS** (parity).
After branchless lowering it vectorizes (16 vector ops, no per-element branch) and is **≈3.5× faster
than idiomatic Rust `Vec<Row>`** (`bench/` `total_pay`, "Align faster"). So the soa filtered
aggregate now beats Rust too (the plain column scan stays ~7-10×). `xs.where(p).min()` over a
`slice<i32>` now emits `pminsd`/`pcmpgtd` (verified via `objdump`) where the branch form was scalar
with 10 branches. `tests/branchless_where.rs`, `tests/optimizer.rs`, `bench/`. (Materialize via
stream-compaction — `to_array`/`partition`/`scan` under a `where` — stays branchy: it must not
*append* a masked-out element, which is not an identity op; that is a separate slice.)

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
   **UPDATE — secondary (b) SHIPPED (#228, 2026-06-29):** the two-pass count-then-direct-column-fill
   (`align_rt_json_decode_soa`) replaced decode→AoS→transpose. This flipped the SoA rail **≈0.82× →
   ≈1.03× of serde** at 1M rows (now beats serde, and edges the AoS decode-only path which still
   heap-materializes) — so lever (b) is done and the transpose penalty is gone. Lever (a) — the
   SIMD/structural parser to reach the probe's 3.4–4.1× — remains the big open perf item (see the
   Mison/two-stage record below); (c) narrow-struct field-skip is available as documented.
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
scanning seeds `0..4096` over sizes `next_pow2(n)..×8` (`phf_hash` = the canonical `wyhash`, since
2026-07-03; originally FNV-1a); emits a `[i32]` slot→index
global (`jphf`, `-1` = empty) alongside the descriptor table; the two decode entry points gained
`(phf_ptr, phf_len, phf_seed)` args. Runtime `find_field` uses the table (or linear-scans when
`phf_len = 0` — empty/1-field structs, or no table found, so it degrades gracefully). Codegen's
`build_phf` and runtime's `json_phf_hash` now call the **same** `align_hash::wyhash`, so the byte-match
is structural; the paired pinned tests (`phf_hash_is_pinned` / `phf_hash_matches_codegen`, plus
`align_hash::phf_pinned_vector`) are a canary against an accidental algorithm edit. ≈1.2–2.5× on wide-schema decode; sound (the confirming
compare means an unknown key colliding into an occupied slot is still skipped). `tests/soa.rs` +1
(wide struct, unknown keys, reordered fields → correct sums), codegen +3, runtime +2.

**Deferred soa / decode sub-items (after the above):**
- **bitset** bool columns (count/any/all via popcnt; `where(.flag).sum()` only ~1.1–2× — both
  reviewers warn against over-crediting the filtered-sum case, since the value column read dominates).
  **Investigated + deferred (2026-06-30).** A bit-packed bool column (1 bit/elem) is a larger,
  higher-risk change than it looks, and the win is **density-only**: the existing byte-column count is
  *already* compute-optimal — the branchless `count` = `sum(select(mask,1,0))` over a byte column
  auto-vectorizes to `psadbw` (popcnt-of-bytes) on x86. Packing buys 8× memory **bandwidth**, not
  compute. The cost: the packed layout must agree **bit-for-bit across two languages** — the LLVM
  codegen helpers (`soa_field_sizes`/`soa_column_offset`/`IndexColumn`/`SoaGather`/`StoreColumn`/
  `SoaAlloc`) **and** the Rust runtime (`align_rt_json_decode_soa`'s `soa_layout(widths, n_rows)` + its
  column writes), because `json.decode → soa` is a single runtime call (not the codegen transpose) and
  `json → soa` **with a `bool` field is already a tested path** (`soa.rs`), so it can't be scoped out.
  Plus a popcnt pattern-match in `lower_array_reduce` and a rejection of explicit `s.boolfield`
  projection (a packed bitset can't be a byte-addressed `slice<bool>`). This is Gate-4 (cross-stage
  ABI) territory, cross-*language* — defer until the density win is actually needed, and ideally design
  it as a first-class `bitset` *type* (draft §13) so the bool-column projection becomes a `bitset`
  view rather than an outright rejection.
- **`soa_slice<T>`** (a per-column-pointer view, so a function can take a borrowed soa slice —
  `slice<T>` is `{ptr,len}` AoS and can't); `str`/Move columns.
- ~~**`map_into(out dst)`** pipeline terminal — the minimal construct that makes `out` `noalias`
  metadata worth emitting.~~ **DONE** — the terminal, the alias-soundness gate, and scoped
  `!alias.scope`/`!noalias` emission all landed (verified the `-O2` overlap guard drops 3 → 0). See
  "`out` parameters + `noalias`" above.
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

### First-party arm64 benchmark — Apple Silicon, in-repo harness (2026-06-30)
The authoritative `bench/` numbers had been x86 (linux); arm64 was only external (Gemini, below) +
spot-checks. Ran the in-repo harness natively on **Apple Silicon (aarch64-apple-darwin, `--target-cpu
native`, alternating-min timing)** — so arm64 is now a first-party tracked tier. Ratios are
Align/Rust unless noted (< 1 = Align faster). The documented x86 wins **hold on arm64**:

```text
math pipeline  sum_sq_pos (map→where→sum)   0.80×    (Align 1.25× faster)   ~ x86 parity / M2 1.15–1.27×
SoA col scan   col_sum (soa vs Rust AoS)    0.127×   (Align 7.9× faster)    ← the SoA flagship win, arm64
filtered agg   total_pay (soa where.sum)    0.32×    (Align 3.1× faster)
group_by       .sum(.v), 1M rows            vs std HashMap 4.0–6.0× · vs ahash 2.2–2.4×   (Align faster)
par_map        vs Rust seq / rayon          vs seq 0.47×(small)→1.39×(1M) · vs rayon 3.45×→0.41×(1M)
json full      decode all fields            0.86× — serde a touch faster (full decode; matches x86)
json projected decode only needed fields    1.28× (Align faster) — the projection/SoA advantage
string builder reduce-append                vs naive Rust String 1.6–1.8× · vs hand-tuned Rust 0.58×
```

Reading: the columnar/data-oriented wins (SoA scan, group_by, projected decode, math fusion) are
**large on arm64** — the SoA column scan is 7.9× here. The not-wins are the same as x86 (serde beats
full JSON decode; a hand-tuned Rust `String` with `with_capacity` beats the builder; rayon wins
`par_map` at scale). SIMD parity audit (2026-06-30): every **live** hand-written SIMD routine has an
x86 (AVX2/`pclmulqdq`) **and** an arm64 (NEON/PMULL) path plus a scalar fallback — `json_decode_index`,
the carry-less in-string fold, and (newly) `json_structural_index` now have NEON; `memchr`/`memmem`
dispatch per-arch via the crate; auto-vectorized loops and the `vec`/`mask` surface go through LLVM
for the target arch. No x86-only SIMD remains on a live path.

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
(dict_encode reuse) everywhere**, but **still loses to smart single-pass Rust (0.42–0.77×)**. Fusion
landed the structural win (cause 1: N passes → 1).
**Why a3 still trails smart Rust — measured 2026-06-29 (corrects the earlier guess).** Two probes:
- **Output-buffer right-sizing is a *no-op* — NOT the lever the earlier note claimed.** A prototype
  moved the K+1 output buffers from MIR-allocated `n`-sized (row count) to runtime-allocated, exactly
  group-count-sized; the benchmark was unchanged (within noise) at every cardinality. Reason: the
  over-allocated buffers are **lazily paged** — only the `count` written entries ever fault in, so the
  oversize was already nearly free. (Don't re-try this in isolation.)
- **The hasher *is* a real lever.** Swapping the dependency-free FxHash for `ahash` (AES) moved
  `smart/a3` **0.77× → 0.92×** at 632k groups (244 ms for smart vs 264 ms for a3) and **0.41× → 0.61×**
  at 100 groups — so
  the FxHash↔ahash gap was material, not negligible. But even with `ahash`, a3 does not fully beat
  smart Rust at low cardinality, and `ahash` is a **new dependency on the minimal runtime** (a tradeoff
  to weigh, applies to all str group paths).
- **The smart baseline reads pre-extracted columns.** The bench's `rust_single` reads `gidx[i]` +
  contiguous `cols[j][i]`, while a3 reads the **AoS struct array strided** (key + K values per row).
  Part of the low-cardinality gap is this columnar-vs-AoS advantage, not the aggregation itself.
**So beating smart Rust is a cross-cutting "smart" pass, deferred** (we trail smart in other benches
too — best decided once): pick the hash strategy (`ahash` dep vs hand-rolled AES, applied to **all**
str group paths incl. `dict_encode`), an inline-value accumulator layout (vs the dense-id `acc[id*K+j]`
indirection), and possibly an AoS-reading (fair) smart baseline. Plus the deferred non-headline sources
(i64-key soa / precomputed `dict_encoded` multi-aggregate), a `group_by(.key)` lambda key, and the
`Scalar::DictEncoded` (return/wrap) follow-up. A2's honest niche stays **sequential/interactive** reuse
(aggregates arriving over time, not fusible into one pass). Design ↓.
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
  - **Two-pass JSON→SoA (count then direct column fill) — SHIPPED (#228, 2026-06-29).** The eventual
    form of json→soa landed: a structural count pass for N, allocate columns, then fill columns
    directly (`align_rt_json_decode_soa`) — dropping the AoS intermediate + transpose of the earlier
    #161 path. Result: full decode+aggregate ≈0.82× → ≈1.03× of `serde_json` at 1M rows (now beats
    serde). Still open here: **`str` columns** via an offset+len column borrowing the input (or a
    string arena) — the sema gate still rejects non-primitive-scalar soa fields. Refinement, not a redo.
  - **Formatter (implement).** In progress (M8). The *policy* was always settled (`draft.md` §4/§16:
    normalize only meaningless variation — spacing / `;` placement / trailing comma / alignment — and
    **preserve the author's one-line ↔ multi-line choice**). The **mechanism** is now settled too
    (2026-06-29): a **hybrid token-reprint + AST-assist** formatter, crate `align_fmt`.
    - **Why hybrid, not pure-AST or pure-token.** The lexer discards comments entirely and drops every
      non-statement-terminating newline (continuation-line `\n`s leave no token), so neither the token
      stream nor the AST alone can round-trip comments / blank lines / the author's line breaks. But
      **spans + `SourceMap` retain the full source**, so the gap `src[prev_tok.hi .. cur_tok.lo]`
      between adjacent significant tokens recovers exactly the comments, newlines, blank-line runs, and
      any `;` that the lexer dropped. So the formatter **walks the significant tokens and re-emits each
      token's text verbatim from its source span** (literals/escapes/radix preserved byte-for-byte),
      deciding only the *whitespace* between tokens from canonical rules, and recovering trivia from the
      gaps. The **AST is consulted only to disambiguate the few context-sensitive spacing cases**:
      `<`/`>` are always `Lt`/`Gt` tokens (a type-arg bracket inside a `Type::Named{args}` span / a decl
      generic-param list → no surrounding space; a comparison → spaced), and unary `-`/`~`/`!` (offset
      == a `Unary` expr's `span.lo` → no trailing space) vs binary. (Getting these wrong is only
      *cosmetic* — spaces around `<` or after unary `-` re-lex identically — so the AST assist is for
      polish, not safety.)
    - **Rules:** indent = 2 spaces × brace depth (matches the examples); a line starting with `.` or a
      binary operator is a continuation (+1 indent); preserve line breaks; collapse 2+ blank lines → 1;
      drop a `;` that is immediately followed by a newline (redundant terminator), keep it only when
      cramming statements on one line; preserve `//` line comments (trailing vs own-line by whether a
      newline precedes them in the gap). Format only parse-clean input; on any lex/parse error pass the
      source through unchanged (never emit from a partial AST).
    - **Deferred to a follow-up slice:** trailing-comma *insertion* on multi-line bracketed lists (needs
      the AST to tell a comma-list `{}` from a block `{}`); for now an existing trailing comma is kept,
      none is added. Block `/* */` comments (the lexer has none; only `//`). `--write` in-place / a
      `--check` mode (slice 1 prints to stdout).
    - This **supersedes the earlier impl-doc hint** that "the formatter uses the AST" (`01-pipeline.md`,
      `02-frontend.md`): it is token-driven with AST assist, which is strictly more faithful (verbatim
      token text, real line breaks). Those docs are updated to say so.
    - Pairs with the perf-rail lints (next M8 item).
  - **`unsafe {}` + `raw.*` — first slice DONE (2026-07-01).** The M8 unsafe escape hatch (draft.md
    §6.5 / §15). `unsafe {}` is a block **expression** modeled on `arena` but strictly simpler — a
    plain marker block (no region, no runtime effect); the only new mechanism is an `unsafe_depth`
    counter that gates the `raw.*` ops (exactly like `arena_depth` gates `heap.new`). Shipped:
    `unsafe {}` + `raw.alloc(size)` (→ `Ty::Raw`, an opaque byte pointer: Copy, `Static`, never
    auto-dropped, LLVM `ptr` like `ArenaHandle`) + `raw.free(p)`, calling the existing flat
    `align_rt_alloc`/`align_rt_free`; plus **`raw.store(p, off, v)` / `raw.load(p, off)`** — typed
    flat load/store at a byte offset (an i8 GEP + a scalar load/store, element-aligned). **No
    turbofish** (settled convention): the stored type follows the value, the loaded type the expected
    annotation (`x: i64 := raw.load(p, 0)`, like `json.decode`). Primitive scalars only (int/float/
    bool/char) — `str`/struct through raw memory is deferred. draft.md §15 was respelled off the old
    `raw.ptr_cast<T>` turbofish example to this inference form. A `raw.*` op outside `unsafe` errors; a function containing
    `unsafe` is inferred **impure** (reusing the single Pure/Impure `EffectScan` flag → never a
    `par_map` callee; "unsafe is visible/traceable"). `raw` is a nameable type (`fn f(p: raw)`).
    **Soundness note (Gate 1):** `unsafe {}` opens no region, but `region_of(Unsafe)` returns the
    block's tail-value region (NOT the `Static` wildcard) so an arena value returned through an unsafe
    block is still escape-checked; `null_moved_source` also treats an unsafe block's tail like a plain
    block (move-null through it). `raw` is Copy so no Drop/Move analysis needed. **Design flag (first
    cut):** the effect model is binary, so `unsafe` is conflated with I/O-impure — fine for now (both
    are par_map-ineligible); a distinct "unsafe" effect is a second flag if ever needed.
    **Pointer arithmetic — `raw.offset(p, n)` DONE (2026-07-01):** advances a `raw` by `n` bytes →
    a new `raw` (a plain, non-`inbounds` i8 GEP, so out-of-bounds arithmetic stays well-defined — the
    same GEP the load/store address uses). `hir::ExprKind::RawOffset` / `mir::Rvalue::RawOffset`.
    **FFI first slice — DONE (2026-07-01):** `extern "C" fn name(params) -> ret` (and the braced group
    `extern "C" { fn … }`) declares a bodyless foreign function bound to the C symbol; a call is only
    valid inside `unsafe {}` (reuses the `unsafe_depth` gate + `unsafe`→impure inference, exactly like
    `raw.*` — decided over Zig-style always-allowed because foreign code can violate every invariant).
    FFI-safe types = int/float scalars + `raw`, plus a `()` return; libc/libm resolve with no extra
    `-l`. Threaded as a bodyless `hir::ExternFn`/`mir::ExternFn` list; codegen declares each under its
    C symbol (mirroring the `align_rt_*` external decls), so a `Rvalue::Call` resolves to a direct
    native `call`. `TokKind::Extern`, `ast::Item::Extern(ExternBlock)`, `FnSig.is_extern`.
    (`tests/ffi.rs`, `examples/ffi.align`.) **Remaining (widen):** `raw.ptr_cast<T>` (unchecked cast)
    is still deferred — with only `raw` (opaque bytes) a typed cast has nothing to reinterpret to; it
    earns meaning once FFI adds typed/external pointers (ideal-form-or-defer). Later FFI slices:
    `layout(C)` struct ABI, `str`/`slice`/`bytes` as pointer+len, an explicit `-l<lib>` link
    directive, `bool`/`char` params. `Ty::Raw`,
    `hir::ExprKind::{Unsafe,RawAlloc,RawFree,RawLoad,RawStore,RawOffset}`,
    `mir::{Rvalue::{RawAlloc,RawLoad,RawOffset}, Stmt::{RawFree,RawStore}}`.
    (`tests/unsafe_raw.rs`, `examples/unsafe_raw.align`, `impl/07-roadmap.md` M8.)
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
        was the autopsy-identified first fix (idx-build 47→18 ms). **Strict semantics preserved**:
        missing/duplicate fields error via the fallback, and — since 2026-07-02 — also on the speculative
        path (see the gap closure below), so both paths enforce the same exactly-once contract.
        **Duplicate-key semantics — DECIDED (SETTLED) 2026-06-29 (Codex overreach review).** The
        `json.decode` field contract is **strict and exactly-once**: every declared field appears exactly
        once; a missing *or duplicated* declared field is a `decode` `Err` (never a silent last-wins);
        undeclared keys are skipped. This formalizes what the implementation already does on the fallback
        path and is now written into the surface spec (`draft.md` §9 + `language-spec.md`). **Pre-freeze
        gap — CLOSED (fixed) 2026-07-02 (`fix/json-duplicate-key-fast-path`):** the speculative path's
        narrow relaxation (a duplicate of a declared field at a colon position the learned pattern treats
        as *unqueried* was not re-detected) now conforms to the contract. Method: `json_speculate` no
        longer skips an unqueried colon blindly — it delimits that colon's key (`key_before_colon`) and
        checks it against the declared set (`find_field`); on a declared hit (or a key that can't be
        cleanly delimited, which the fallback also rejects) it returns `false`, so `json_fallback`
        re-scans and surfaces the duplicate/missing/malformed as a decode `Err`. Chosen over (a) a
        per-record seen-bitmap on the fast path (the duplicate sits at an *unwritten* unqueried slot, so
        a write-time bitmap never sees it unless the unqueried key is resolved anyway) and (b) demanding
        a full key-set match (fallback on any extra key — that disables the projection win outright). Cost
        lands only on records carrying undeclared extra colons (the projection rail) and is the minimal
        key check for soundness — one PHF probe per unqueried colon that misses (empty/mismatched slot),
        so an ordinary undeclared key still speculates and fast-path usage is preserved (no spurious
        fallback on undeclared-key variation). Covered by the `align_runtime` test
        `json_struct_array_speculative_duplicate_key_is_strict` (repro of the unqueried-slot duplicate +
        queried-position duplicate + no-duplicate projection/full-decode regressions). (Why strict, not
        serde-style last-wins: duplicate keys into a fixed struct are a data error, and strict-reject
        matches Align's "nothing hidden / one error model" — a malformed shape surfaces as a value, never
        a silent partial decode.)
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
        gap to serde is now the intrinsic walk + value-parse, the same x86 pays.
        **SoA-column direct decode is SHIPPED, and the SoA projection rail is now MEASURED (2026-07-01).**
        Verified in code: `align_rt_json_decode_soa` already runs the lean `json_decode_index` + Mison
        `json_speculate`/`json_fallback` over a `SoaDst` (direct-to-column write, no AoS intermediate) —
        i.e. the "soa-column direct decode" the notes above called "the real lever if pursued" is not a
        pending slice, it landed with #228 + the `FieldDst` generalization. What was genuinely missing was
        a **measurement**: `bench/json_soa` declared all 4 fields (full decode, no skip). Added an
        `agg_proj` variant — the same 4-field JSON decoded into a narrow `soa<Row2 {active, pay}>` vs a
        fair `serde_json::<Vec<Row2>>` baseline (both skip the two unknown keys). **Result (native): soa
        projection = 1.29–1.61× serde** (vs ≈1.12× full), matching the AoS `json_decode` proj number; the
        profile shows the columnar scan is ~free (agg delta 0.2–0.4 ms) so the win is almost entirely
        **decode-projection** — skipping the unqueried columns' colons saves ~25 ms / ~30% of the
        4-column decode at 1M. It does **not** reach the probe's 3.4–4.1×: that gap is the inlined,
        descriptor-free, verify-free single-pass positional walk (the `rec_cols` two-pass + `FieldDst`/
        `JsonParser` indirection + intrinsic key-verify), whose pieces were each measured small (+2/+4/+2
        ms) and judged not-worth-forcing. `bench/json_soa` is now the instrument to revisit that with data.
        Note (dead code): the heavier `json_structural_index` (#213/#254 AVX2+NEON, quote+comma) was
        **removed 2026-07-01** — it never had a live consumer (the live decode uses the lean
        `json_decode_index`, which emits only `{ } [ ] :`). The shared bit-twiddling helpers it used —
        `prefix_xor` (x86 pclmulqdq), `prefix_xor_portable` (NEON), `find_escaped` — stay: the lean
        index's AVX2/NEON paths use them. If a future full-structural pass ever needs the quote+comma
        index, it is in git history (#213/#254). The differential SIMD-vs-scalar-oracle test now covers
        only the live lean index (`json_decode_index_simd_matches_scalar_oracle`).
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

### String concatenation via `+` — SETTLED 2026-07-02: hard error, builder is the one way
**Decision: `str`/`string` do not support `+`; it is a compile-time error naming `builder` as the
alternative.** `draft.md` §12 previously left this a two-way "forbidden or linted" note. Resolved in
favor of the hard error: a lint is opt-out-able and a silent per-call hidden allocation is exactly
what "Nothing hidden" + "One way" rule out (concatenation already leaked when reached through a
lifted lambda with no arena — see "External benchmark report — Gemini on M2/arm64" Gap A above, fixed
2026-06-27 for that specific path; this decision generalizes the fix into the actual rule rather than
a lambda-only guard). `builder` (`.write`/`.finish()`) is the one way to build a string incrementally.
Record: `draft.md` §12 (doc update landed), `impl/06-runtime-std.md`.

### Unconstrained literal defaults + `&&`/`||` evaluation order — now explicit in the spec (2026-07-02)
Two implementation-notes-only facts are promoted to explicit spec text: **an unconstrained integer
literal defaults to `i64`, an unconstrained float literal to `f64`** (previously only stated in
`impl/02-frontend.md` / this file's "Numeric literal typing" entry above, now stated in `draft.md`
§5 directly — user-visible, since it affects overflow/precision); and **`&&`/`||` evaluate
left-to-right with short-circuit semantics** (`a && b` never evaluates `b` if `a` is false), now
given its own evaluation-order note in `draft.md` rather than being implied by "logical operators."
This is a **spec-documentation** settlement, not a claim that the short-circuit *implementation* is
verified end-to-end — track that separately (External soundness audit item **3-1** above records
`&&`/`||` lowering to a strict, non-short-circuiting `Rvalue::Bin` in MIR as of that audit; confirm
it is actually fixed before relying on the spec text here as also describing current codegen).
Record: `draft.md` §5 (doc update landed).

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

### External soundness audit — multi-agent (2026-07-02, VERIFIED; fixes in progress)

A 7-agent audit on another machine (frontend / sema-types / sema-flow / MIR+codegen / runtime+driver / docs / perf), cut short by a token budget. Every finding below was **reproduced by compiling + running** on this machine (Linux/glibc) before any fix. The unifying diagnosis (audit §6.1, confirmed): the escape / effect / move analyses are **per-`ExprKind` hand-written traversals with fail-open defaults** (`_ => Region::Static`, `_ => false`) — every hole was a syntax node someone forgot to add an arm for. `If` was handled; `Match` (and the fn-value / element-assign forms) repeatedly were not.

**Confirmed soundness holes — FIXED (in the analysis-coverage sema PR #270, not this docs-only entry):**
- **1-2** arena `str` escapes through a `match` arm (`region_of` lacked `Match`).
- **NEW-1** (found here) arena `str` escapes through an indirect call `g(t)` (`region_of` lacked `CallFnValue` — the fn-value sibling of 1-4).
- **1-5** `return xs[0..2]` over a local array returns a dangling slice (`slice_is_local` lacked `SliceRange`; fixed-array locals weren't marked frame-local).
- **1-6** arena `str` stored into an outer array element via `arr[i] = t` (element/field stores skipped the region check that `Assign`/`AssignField` do).
- **1-4** an impure fn laundered through a fn value (`g := loud; g(x)`) bypassed `par_map` purity (`EffectScan` had `FnValue(_) => {}`).
- **NEW-3** (found here) a *false* "use of moved value" when mutually-exclusive `match` arms consume the same value (`MoveCheck` shared one moved-set across arms instead of clone+join like `if`/`else`).

**Confirmed — still open (tracked in the Open section / their milestones):**
- **1-3** `arena { mut xs := […].to_array(); xs = make() }` double-frees. Reproduced as `free(): double free detected in tcache` once the arrays are large enough to trip glibc's tcache (a small case survives silently; macOS aborts immediately). The `to_array` arena-bump result and the reassignment `drop_old` / arena bulk-free don't reconcile. **Highest-priority remaining bug.**
- **3-1** `&&` / `||` are **not short-circuit** — MIR lowers them as a strict `Rvalue::Bin`, so `i < len && arr[i]` still evaluates `arr[i]` and can trap. (Confirms the audit's "requires-verification" item.)
- **2-1** a type-annotated `let` at an `if`-body head (`if flag { x: i32 := 5 … }`) misparses as a struct literal (no `no_struct_literal` context flag on the condition).
- **2-2** `x as u32 < 5` won't parse (`parse_type` greedily eats `<` as generic args; a cast target is always a concrete primitive).
- **2-3** two statements with no separator (`{ x := 1 return x }`) are silently accepted (weak statement-boundary check).
- **2-4** deep nesting (50k parens / 100k unary `-`) overflows the parser stack (exit 134); needs a recursion-depth limit that errors.
- **2-5..2-8** diagnostic-quality: `1e999` silently becomes `inf` (no diagnostic); a non-ASCII identifier reports byte-wise garbage; the internal `enum#0` leaks into a type-mismatch message instead of the source name; a trailing `\` before EOF emits a doubled/misleading error.

**1-1 (found + FIXED after all):** the `-5` → `4294967291` sign loss reproduces when a **negative literal is given an unsigned type by context** — `x: u32 := -5`, or `g(-5)` into a `u32` parameter — which `check` silently accepted, wrapping `-5` to `4294967291`. Root cause matches the audit's "finalize-without-bind" guess: unary negation's signedness was never validated against the (later-inferred) unsigned type. Fixed by rejecting **unary `-` on an unsigned type** at finalize time (a negative value cannot have an unsigned type; cast explicitly for the wrapped pattern). Unsigned *subtraction* `a - b` stays a defined wrap; `(-5) as u32` stays a sanctioned conversion. `tests/numeric_cast.rs`, `draft.md` §3.

**Structural follow-ups (design-level, from audit §6):**
- Move escape/region checking off per-`ExprKind` enumeration onto a **MIR dataflow pass** (like `MoveCheck`'s shape), so a new syntax node can't silently open a hole. This is the `align-self-review` "new IR variant skips an analysis pass" gate recurring at the *language-design* level. Until then, prefer **failing closed** (a region-tracked value reaching an unhandled node → conservative shortest region / error, not `Static`).
- Record purity as an **effect bit on the function type**, not a name-based propagation result, so fn-value / closure / FFI-pointer indirection can't dodge it (keeps "purity is inferred"; only stores the result in the type).
- A spec table of **value-carrying control structures** (block / `if` / `match` / `else`-unwrap / `?`): for each, how the region is composed and how an owned value moves/drops — with 1:1 tests. Every hole above is a blank cell in that table.
- ~~Stand up **fuzzing** (parser / JSON / fmt, with a depth cap) and a **negative-test corpus**~~ **(DONE, #286–#290)** — dependency-free fuzz + property suite in `crates/align_driver/tests/` (SplitMix64 + `catch_unwind`, seeds printed, runs as `cargo test`): `fuzz_frontend.rs` (lexer/parser/sema never panic, incl. non-ASCII), `fuzz_fmt.rs` (formatter never panics + idempotent + parse-preserving on all examples), and `fuzz_differential.rs` — a **generate-program-with-its-oracle** differential fuzzer that catches *miscompiles* (the array-garbage class) across scalars, all integer widths + cross-width casts, the call ABI, struct/array aggregates, and (wave 2, #326 → this wave) `map`/`where`/reduce pipelines, `vecN<T>` lane arithmetic, **Option `else`-unwrap + Result `?`-propagation chains** (both the Some/Ok and None/Err arms forced across the seed range), **enum + exhaustive `match`** (mixed tag-only / scalar-payload variants, per-variant base so a mis-tag or mis-read payload both surface), and **nested-struct read/write chains** (depth-2..3 towers with a randomly-positioned nested field, exercising #307 field reordering at every nesting level). A single `wrap(v, ITy)` models both arithmetic wrapping and integer casts; a per-test mutation check (deliberately `+1` the oracle) proves the harness isn't a vacuous pass. No miscompile found. The negative-test corpus is `tests/analysis_coverage.rs` + the audit repros. Still open: the MIR-dataflow / purity-as-effect-bit structural refactors above.

Record: `crates/align_sema` (the analyses), `tests/analysis_coverage.rs`, `align-self-review` Gate 1.

### 2026-07-02 internal review (multi-agent: 4 deep-dive tracks + independent Opus/Codex design passes)

Same-day, separate from the external soundness audit above (no overlap): 4 parallel deep-dive tracks
(frontend soundness / MIR+LLVM codegen / runtime+library / language-design evaluation), plus the
design-evaluation question put to Opus and Codex **independently and cross-checked** (both converged
on the same conclusions, folded into the Settled addendum above and the Future entries below).
**Status: open**, except the two items marked **fixed** below (`MoveCheck`'s `AssignField` gap and
`align_rt_arena_alloc`'s raw cast); the rest are recorded here so they aren't lost. Confidence follows
the same convention as the external audit: **CONFIRMED** = read against the code (or reproduced),
**PLAUSIBLE** = strong code-reading suspicion, not yet reproduced.

**Confirmed bugs:**
- **Division has no zero / `INT_MIN ÷ -1` guard — immediate LLVM UB, not a clean abort.**
  **(status: FIXED — `fix/div-guard`.)** `align_codegen_llvm/src/lib.rs` (~:3797) emitted raw
  `sdiv`/`udiv`/`srem`/`urem` straight from MIR, with no guard branch anywhere upstream. `sdiv`/`srem`
  by zero and `sdiv INT_MIN, -1` are LLVM-level *undefined behavior*, not a trap — the O2 optimizer is
  entitled to assume the divisor is nonzero and can delete a surrounding `if b != 0` check, or delete
  the division itself if the quotient is dead (so it wouldn't even SIGFPE). This **directly violated**
  the Settled "division by zero ... is never silent, always an error" decision (see "Panic / unwinding"
  above and `draft.md` §5). **Fix:** MIR `lower_int_div` (align_mir) now guards every integer `/`/`%`
  with a *runtime* divisor, the same shape as `emit_bounds_check`: `divisor == 0` branches to a new
  `align_rt_div_fail` (`-> !`,
  cold edge, aborts with "division by zero"); the signed `INT_MIN / -1` overflow is folded away with a
  `select` (divide by `1` in place of `-1` so the raw sdiv/srem never sees the UB case, then select the
  wrapped result `0 - x` for `/` or `0` for `%`) so it wraps to `INT_MIN` per the defined
  two's-complement overflow rule. A *constant* non-zero divisor (`x / 2`, `x % 10`, `x / -1`) is the
  common case and needs no guard — both UB cases are decidable at compile time — so it is lowered
  straight to the raw op (or, for `-1`, folded to `0 - x` / `0`), keeping the MIR lean. `float`
  division (IEEE) and `vecN<T>` (SIMD, out of scope) are
  untouched. The differential fuzzer's oracle (which forced positive divisors) now also generates
  negative divisors incl. `-1`, exercising the wrap at every width; direct integration tests in
  `crates/align_driver/tests/div_guard.rs` cover the abort + `INT_MIN/-1` cases.
- **Status: fixed.** **`json.decode` silently truncates/sign-wraps out-of-range integers.**
  `align_runtime/src/lib.rs` (`parse_object`; same pattern in `write_field_indexed` — AoS/columnar —
  and `align_rt_json_decode_array` — scalar arrays). `JsonField.tag` packed `(kind<<8)|width` with
  **no sign bit**, so it structurally could not range-check: `{"n": 300}` into `u8` silently became
  `44`, `{"n": -1}` into `u32` became `0xFFFFFFFF`, `{"n": 200}` into `i8` became `-56`. Hidden
  corruption from untrusted input, in a language whose flagship consumer is JSON. **Fixed** by adding
  bit 16 to the tag as the int sign flag — `tag = (signed<<16)|(kind<<8)|width` — an **ABI change
  applied to codegen (`decode_field_table` + `gen_json_decode_array` emit the flag) and runtime (the
  decoder reads it) together**; the bit sits above the existing kind/width bytes so their decoders are
  unchanged. Every integer write path now range-checks the parsed `i64` against the field's
  `(width, signed)` `[min, max]` via `int_in_range` and routes an out-of-range value through the
  existing bad-value path (`None` → decode error). **Follow-up (fixed):** the earlier remaining
  limitation — `JsonParser::integer` parses into `i64`, so a `u64` field accepted only `[0, i64::MAX]`
  and rejected a representable JSON value in `(i64::MAX, u64::MAX]` — is now closed. `JsonParser`
  gained `integer_unsigned` (full-range unsigned accumulate + `checked_*`) and `integer_field(w,
  signed)`, which routes a width-8 *unsigned* (`u64`) field to the full `[0, u64::MAX]` path and every
  other width / any signed field to the `i64` path + `int_in_range` (unchanged negative / overflow /
  `i64::MIN`-edge handling). All three integer write sites (`parse_object` / `write_field_indexed` /
  `decode_array`) call `integer_field`, so the routing is consistent everywhere. Tests:
  `int_in_range_covers_widths_and_signs`, `integer_unsigned_parses_full_u64_range`,
  `json_decode_range_checks_integer_fields`, `json_decode_array_range_checks_integers`,
  `json_decode_soa_u64_full_range` (runtime); `json_decode_rejects_out_of_range_integers`
  (driver, `crates/align_driver/tests/m5.rs`).
- **Status: fixed.** **Parser depth guard doesn't cover iteratively-parsed chains — sema
  stack-overflows (ICE).** `align_parser/src/lib.rs` capped `MAX_EXPR_DEPTH=256`, but that budget is
  spent only by *recursive* parsing; the left-associative binary-operator loop and the postfix-chain
  loop build arbitrarily deep ASTs **iteratively**, consuming no depth budget. A ~1000-term chain
  (`x := 1+1+1+...`, ~2KB source — a plausible size for machine-generated code, this project's target
  authorship mode) parsed cleanly and then blew the native stack in a downstream recursive walk
  (`align_sema` `check_binary`/`MoveCheck`/`EscapeCheck`/`EffectScan`, then MIR lowering — the
  heaviest) — a process abort, not a diagnostic. **Fixed** with a post-parse pass, `cap_expr_depths`
  (`align_parser/src/lib.rs`): after `parse_file` it walks the finished AST and truncates any
  expression nested deeper than the ceiling to a `Unit` placeholder, emitting the same "expression
  nests too deeply" diagnostic the recursion guards use (one clean error per over-deep chain — a
  leaf that lands one past the ceiling is left alone). The walk recurses at most `MAX_EXPR_DEPTH`
  levels (it stops at a truncation point), so it is itself stack-safe. `MAX_EXPR_DEPTH` was lowered
  256 → **128**, chosen from measured debug-build stack limits: the heaviest downstream pass, MIR
  lowering, overflows at depth ~275 on the 8 MB main thread (where full builds run) and sema
  overflows ~235 on a 2 MB worker/test thread — 128 leaves ~2x headroom on both. (Note: the
  recursion guard's old 256 was itself unsafe on a 2 MB stack; 128 fixes that too.) Tests:
  `crates/align_driver/tests/expr_depth.rs` (over-limit `+`/postfix chains rejected cleanly not by
  ICE, deep parens still guarded, within-limit expressions still accepted + compiled/run).
  **Residual (recorded, not blocking):** MIR-lowering/codegen frames are very stack-hungry in debug,
  so the deeper long-term fix — as `rustc`/`clang` do — is to run the compile pipeline on a
  dedicated large-stack thread, which would let the ceiling be far more generous; deferred.
- **Status: fixed.** **`MoveCheck`'s `Stmt::AssignField` doesn't check `whole_moved(root)`.**
  `align_sema/src/lib.rs` (~:3141) — writing into a field of an already-moved-out struct (`take(u);
  u.name = "x".clone()`) is silently accepted, while *reading* a moved struct's field is correctly
  rejected. `Stmt::AssignIndex` already has the matching `whole_moved(base)` check (~:3145-3151) —
  this is a one-line fix mirroring it. MIR (~:935-947) drops the old value and stores the new one, but
  the struct stays flagged moved and is excluded from `drop_locals`, so the freshly-stored value
  **leaks** (confirmed no double-free under `MALLOC_CHECK_=3` — a leak, not UB, today). Fixed by adding
  the same `whole_moved(root)` check to `Stmt::AssignField` (rejecting the write at compile time, so
  the MIR leak path is unreachable for valid programs); see `field_assign_after_whole_move_rejected` /
  `field_assign_without_move_still_checks` in `align_sema/src/lib.rs` tests.
- **Status: fixed.** **`chunks` over a frame-local scalar array infers `Region::Static`.**
  `align_sema/src/lib.rs` (~:2529) — `region_of(Local)` falls back to `Static` for an unregistered
  local; `tracks_region` returns `false` for scalar arrays (~:2376), so a local scalar array is never
  registered in the first place. `local_backed_slice` (~:2609-2637), the guard that would normally
  catch this, only covers `Ty::Slice`, not the `DynSliceArray` that `chunks` produces. Not reachable
  today only because "array elements are scalar-only" prevents writing `array<slice<T>>` — i.e. it is
  **shielded by an unrelated restriction, not a correct check**:
  `cs := arena { xs := [1,2,3,4]; xs.chunks(2) }` already type-checks with no escape error, and would
  be a real use-after-free the moment that scalar-only restriction lifts — and it was **also present
  for `array<str>`** (which *is* reachable today: `str` is a valid `chunks` element via `PrimScalar`).
  There it is worse: `array<str>` is region-tracked, so its `Let` stores the array's *element* region
  (`Static` for `str` literals) in the region map, and `region_of(chunks)` inherited that `Static` —
  `cs := arena { xs := ["a","b","c","d"]; xs.chunks(2) }` and the outer-assign form both type-checked
  with no escape error (confirmed). **Fixed** by binding `region_of(ArrayChunks)` to the source's
  **storage** region (new `chunks_source_storage_region`), *distinct from* the element/value region
  `region_of` returns: a fixed stack `array<T>`/`array<Struct>` bound as a `Let`-local owns a frame
  slot scoped to the arena it was declared in (`Frame.shorter(arena(decl_depth))`), a fixed-array
  parameter borrows the caller (`Static`, returnable), an array literal is a frame temporary, a
  frame-backed slice (`local_backed_slice`) re-borrows frame storage; any other source keeps its
  `region_of` — the chunks region is the shorter of the storage region and `region_of(source)` (so an
  `array<str>` of arena strings is bounded by both). This is the key distinction: the storage region
  is *not* the element region — an element read (`xs[0]`, a `str` view of static data) stays
  returnable while the whole-array borrow (`chunks`) is frame-bound. Chosen over touching the region
  map at the `Let` (would clobber the element region and wrongly reject `return xs[0]`) or extending
  `local_backed_slice` (a parallel `Ty::Slice`-only mechanism that guards only *returns*, not the
  arena-block-value / outer-assign escapes `region_of` already covers). A companion drop-set fix
  always drops a `DynSliceArray` local even at `Arena(k)` region (its header buffer is always
  heap-`malloc`'d by `align_rt_chunks`, never arena memory — region tracks the borrowed source, not
  the container's storage), so a chunks bound inside an arena is freed, not leaked. Tests:
  `chunks_of_arena_local_cannot_escape_as_block_value` / `…_via_outer_assign` (scalar + `str`),
  `chunks_used_in_same_scope_ok`, `chunks_bound_in_arena_used_locally_ok`,
  `chunks_of_local_cannot_be_returned`, `chunks_of_str_array_cannot_be_returned`,
  `str_array_element_read_still_returnable`, `chunks_of_struct_array_rejected` (`align_sema/src/lib.rs`).
  Related (**noted, not an active hole**): `region_of` also returns the *element* region (not the
  storage region) for `ArrayToSlice` / `SliceRange` over a fixed `str` array — but the only genuine
  use-after-free there (returning such a slice) is caught by the orthogonal type-driven
  `local_backed_slice`/`slice_is_local` return check, and the arena-escape it under-reports is
  conservatively safe (the frame slot outlives the arena, matching the existing slice leniency). A
  future *region-only* consumer of those producers must not assume the region reflects storage.
- **Status: fixed.** **`align_rt_arena_alloc` uses a raw `as usize` cast, unlike every other FFI entry
  point.** `align_runtime/src/lib.rs` (~:3495-3498) — every other runtime FFI boundary normalizes an
  incoming size via `usize::try_from(...)`; this one does `size as usize` directly, so a negative input
  becomes a huge `usize` and `off + need` (~:3471) could wrap in a release build. Not reachable today
  (codegen always passes a sound value) — but it is exactly the `i64 as usize` bug class this repo's
  own past audits keep flagging (`align-self-review` Gate 1). Fixed via
  `usize::try_from(...)` on both `size` and `align`, returning null on failure (matching the
  null-on-invalid-input convention of `align_rt_alloc`/`align_rt_chunks`); see
  `arena_alloc_rejects_negative_or_oversized_size_and_align` in `align_runtime/src/lib.rs` tests.

**Perf backlog (non-blocking; recorded so none of it is re-discovered from scratch):**
- **Top lever: no-alias information never reaches LLVM**, even though the language guarantees it (see
  "`out` parameters + `noalias`" above). The slice ABI passes `{ptr, i64}` **by value**, so there is
  no standalone pointer parameter to attach a `noalias` *attribute* to; the workable form is **`!alias.
  scope`/`!noalias` metadata** on the fused loop's element loads/stores. **Investigated 2026-07-02 —
  DEFERRED (see the "`out` parameters + `noalias`" section for the full finding):** the metadata was
  proven to remove the runtime overlap guard on a two-slice-param loop, but **no source construct
  generates such a loop today** (`map_into(out)` deferred; whole-slice `dst = a + b` unimplemented;
  the pipeline store-loops write fresh allocations LLVM already disambiguates — zero memchecks across
  the whole example corpus at `-O2`), and the no-alias *check* has an untracked `SliceRange` provenance
  hole that must be closed before any emission. Belongs with the `map_into(out)` slice, not now.
- **Status: fixed.** **`task_group` spawned one OS thread per task** (`align_runtime/src/lib.rs`,
  `align_rt_tg_wait`, via `thread::scope` + a `spawn` per task) instead of reusing the **persistent**
  `ParPool` that `par_map` already built for exactly this cost. `tg_wait` now routes tasks through
  `ParPool` with a **caller-participating work-claiming** loop: the tasks live in a shared claim-once
  list (`TgTasks`, `Send + Sync` by construction — each index is claimed exactly once via an atomic
  cursor, each `env`/`slot`/`err_slot` is a private disjoint region allocation) with a join barrier
  (`TgBarrier`: done-count + first-panic + first-errored-slot by lowest index). `wait()` dispatches
  `min(workers, n-1)` runners onto the pool **and runs the same claim loop on the calling thread**,
  then blocks until every task is done (so the join still precedes the region free at `tg_end`). The
  panic-collecting behaviour is preserved (a worker panic is re-raised on the caller — defensive: a
  real Align task is `extern "C"` and *aborts* on panic rather than unwinding). **Nesting/deadlock
  analysis (the crux):** a spawned closure is lifted to an ordinary fn, so its body may open its own
  `task_group` — a pool worker *can* re-enter `tg_wait`. A finite pool would deadlock under a naive
  "submit-all-then-wait" scheme (nested waits on busy workers wait for jobs no free worker can take).
  The caller-participates loop removes that hazard: **every `wait()`'s calling thread drains its own
  group to completion itself if no worker is free**, so an N-deep nest just runs sequentially (one
  level per blocked thread) — no `wait()` ever waits on the pool for its *own* tasks. Late-scheduled
  runner jobs that a worker picks up after the group drained find the cursor past the end and exit
  without touching the (possibly-freed) region. Tests: `tg_wait_runs_all_tasks_pool_backed`,
  `tg_wait_returns_first_errored_slot_by_index`, `tg_wait_nested_task_groups_do_not_deadlock` (the
  last would hang on a deadlock) in `align_runtime`, plus the existing `align_driver` `task_group`
  suite. (The `par_map` "still behind rayon" note above is unrelated and stands.)
- **Status: fixed.** **Allocator-family runtime declarations lacked return/function attributes** in
  codegen's declarations. Each attribute was verified against the function's *actual* Rust body
  (over-declaration is a miscompile, so this split matters):
  - `noalias` (return) on all of them — every one returns a *fresh* allocation disjoint from any
    live pointer (compatible with the null `align_rt_alloc`/`arena_alloc` may return).
  - `nounwind` on all of them — they `abort` on OOM, and a panic (e.g. `Vec` capacity overflow)
    can't escape the `extern "C"` boundary, so no unwind ever leaves the call.
  - `nofree` on the **single-shot** allocators only (`align_rt_alloc` = one `malloc`; the `*_begin`
    handle allocators + `align_rt_builder_new` = one `Box::new`) — they never free. The **bump**
    allocators `align_rt_arena_alloc` / `align_rt_tg_alloc` are pointedly **excluded**: growing the
    region `Vec::push`es the chunk-index list, which can reallocate (free) before-the-call memory, so
    `nofree` would be unsound there (the returned bump pointer is still `noalias` — only the index
    vector moves, never the chunk buffers). `align_rt_par_map` gets `noalias` (fresh output buffer)
    alone (it may `resume_unwind` and it invokes the user thunk).
  - **Deliberately NOT added: `willreturn`/`mustprogress`** — the OOM `abort` path means asserting
    they always return would be a miscompile.
  Helpers: `mark_alloc_like` (single-shot) / `mark_bump_alloc` (region) / `mark_alloc_common`. Test:
  `allocator_declarations_carry_noalias_and_hygiene_attrs` asserts `noalias` on each, that the bump
  allocators are NOT `nofree` while a single-shot one is, and that the IR never claims `willreturn`.
- **Status: fixed.** **`emit-llvm` output set a data layout but no target triple.**
  `build_module` (`align_codegen_llvm/src/lib.rs`) now also calls `module.set_triple(&tm.get_triple())`
  so emitted IR is self-describing: an external `opt`/`llc` uses the right cost model / vectorizer
  instead of a generic one. The driver's own `write_object` path was unaffected either way. Test:
  `emitted_ir_is_self_describing`.
- **Low priority, deliberate design: `print` does a flushing `write(2)` per call.** An option is
  process-lifetime buffered stdout flushed via `align_rt_start` — the runtime's existing
  `BufferedWriter` already does this shape elsewhere, so it would be reuse, not new machinery. Noted
  in passing so it isn't "fixed" by accident: the arena chunk's 64KiB zero-fill looks like waste but
  is **load-bearing** — `json.decode` depends on the zeroed-out contract — don't remove it
  independently of touching that contract.

Record: none yet (all open); this session's design-facing conclusions (MIR width-agnostic invariant,
two-tier SIMD positioning, the string-concatenation/literal-default/short-circuit spec gaps) are
folded into the Settled/Open/Future entries above, and were also landed the same day in
`draft.md`/`docs/impl/*` (see those files' history, not duplicated here) and in `HANDOFF.md`.

### M9 std design (2026-07-03)

Settled ahead of any `std.io`/`std.fs`/`std.path`/`std.env`/`std.time` implementation
(`impl/07-roadmap.md` M9; full API shape in `draft.md` §18.2):

1. **`reader`/`writer` are concrete, builtin Move types (own an fd, `Drop` closes it) — not a
   trait.** Align has no traits/comptime, so "one type, many constructors" (`fs.open`, `io.stdin`,
   `io.stdout.buffered()`) is the only way to get polymorphism without a second mechanism.
2. **Time is one `i64` nanosecond timeline — no `Duration` type.** `time.now()`/`time.instant()`/
   `time.sleep(ns)` all take/return a plain `i64`; one representation, one way.
3. **One fixed errno→`Error` mapping table, shared by every `std` fn.** `ENOENT`→`NotFound`,
   `EACCES`/`EPERM`→`Denied`, `EINVAL`→`Invalid`, else `Code(errno)` — a per-module ad hoc mapping
   would be a second error-translation mechanism.
4. **A view-returning std fn (`fs.read_file_view`, `path.base`/`dir`/`ext`) requires an enclosing
   arena; escaping the view is `.clone()`.** Same region rule as M3's `heap.new` requiring an arena
   — one escape/region mechanism for the whole language, not a new one for I/O views.
5. **Implementation stays the `core.json` pattern: Rust runtime `align_rt_*` + sema builtin
   dispatch + required `import`.** "`std` as a real Align-over-FFI library" remains a Future item —
   not reopened for M9.

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

### Error type design — Settled 2026-07-02 (built on sum types; the exit-code residual is now closed)
Today `Error` is the M2 `Ty::ErrCode` (an i32 code). **Leaning (2026-06-24, validated by external review):** build the real `Error` **on the sum-type mechanism** — `Error` is a **sum type of categories** (the variant carries a lightweight payload: a `str` view + position for a parse error, a code for an OS error, …). Constraints from the philosophy:
- **An explicit value, nothing hidden:** no exceptions, no unwinding, no implicit stack-trace allocation. (The cold-`Err`-edge treatment stays.)
- **No implicit `?` conversion — explicit `map_err` instead (4b-3 DONE).** `?` requires the same `E` (an implicit `E → E'` coercion would be *hidden* — Align has no `From`-trait to point at, unlike Rust). To change a result's error type, use `result.map_err(f)` (`f: fn(E) -> E'`), then `?`: `inner().map_err(to_error)?`. Explicit, visible, closure-based; lowers to a branch over the `Result` reusing the existing unwrap rvalues + an indirect call.
- **Context is structured, not free-form (revised 2026-06-25, see 4b-4):** the Align way of attaching context to an error is **structured data in a sum-type payload** — a variant that carries the relevant fields (a `Pos`, a code, a name) — not a free-form appended string. Free-form `.with_context("…")` string-chaining is the dynamic / allocating / unstructured anti-pattern (Rust `anyhow`-style); it cuts against the data-oriented + AI-friendly grain and would force either `str`/owned-`string` payloads in the error (making `Error` Move, rippling through `?`/drop) or recursive `box<Error>` wrapping (deferred with recursive enums). So **`.with_context` is not adopted**; structured errors are the mechanism. (Reconsider only if a concrete need appears *and* `str`-in-error-payload region tracking lands — the same deferral as S2's `str`-field struct payloads.)
- **Structured errors carry position — DONE (4b-4):** a user error enum whose variant carries a plain-data struct payload models a parse/validation error that carries its position (`ParseError { BadToken(Pos), Eof }` with `Pos { line, col }`), constructed, `?`-propagated, and read back with `match` — end to end. No new mechanism: it falls out of user error enums (4b-1) + plain-struct variant payloads (S2). (Tests: `structured_error.rs`; example: `examples/structured_error.align`.)
- **Exit-code mapping** at the `main` boundary stays as today (`clamp(1,255)`).
So this entry **waits on sum types** (4a) and then defines `Error` as a concrete sum type + the `?` conversion + exit mapping (`impl/03-types.md` §5, `impl/06-runtime-std.md` §9).

**4b-1 DONE (the foundation): errors can be user-defined sum types.** `Scalar::Enum(u32)` was added (a sum type is a Copy composite payload, like `Scalar::Struct`), so an enum is now a first-class `Option`/`Result` payload — most importantly **`Result<T, MyError>`** with a user error enum: construct `Err(MyError.Variant(…))`, `match` the `Result` then the error enum, and `?`-propagate it (same `E`). `option_struct_type`/`result_struct_type` (and `scalar_type`/`abi_type`) thread the enum-type table so the aggregate can hold an enum field.

**4b-2 DONE: the canonical `Error` is a builtin sum type.** `Error { NotFound, Invalid, Denied, Code(i32) }` — a real enum registered as a reserved type name (resolved via `enum_ids` like any sum type). `Error.NotFound` / `Error.Code(c)` construct it (`error(c)` is sugar for `Error.Code(c)`); `match` discriminates the categories; `?` propagates. Every fallible builtin (`fs.read_file`, `json.decode`, `io`, `task_group`) now returns `Result<_, Error>`, wrapping its runtime i32 status as `Error.Code(code)`. The **`main` exit mapping**: `Code(c)` → exit `clamp(c)`, a category → `tag + 1` (a small distinct nonzero code). The **task_group** fallible path was reworked to carry the full `Error` across threads: each task gets an `err_slot`, the trampoline writes its `Err` value there and returns 0/1, `tg_wait` returns the first errored `err_slot` (null if none), `wait()?` builds the `Result` from it. (`Ty::ErrCode`/`Scalar::ErrCode` are now vestigial — only an i32-status alias in the builtin lowerings; removable in a follow-up.) **4b-3 DONE** the explicit **`?` `E → E'` conversion** via `result.map_err(f)` (no implicit coercion). **4b-4 DONE (structured errors) / `.with_context` not adopted** — position-bearing structured errors already work on the 4b-1 + S2 foundation (a variant carrying a `Pos` struct, `?`-propagated, `match`-read); free-form `.with_context` string-chaining was reviewed and dropped as off-philosophy (structured sum-type payloads are the context mechanism — see the bullet above). **So the Error type (4b) is complete** for the planned surface: `Error` is a builtin sum type, user error enums work, `map_err` converts, structured payloads carry context. (Richer `str`-carrying error payloads remain deferred with S2's `str`-field payloads — enum region tracking.)

**Exit-code residual — SETTLED 2026-07-02: `main`'s `E` is restricted to the builtin `Error`.**
The `main` wrapper's exit-code lowering (`align_codegen_llvm/src/lib.rs`, the `align_main` wrapper)
reads the payload as the builtin `Error` enum's specific `{ i32 tag, i32 code }` shape
(`Code(c)` → `clamp(c)`, category → `tag + 1`); a user-defined error enum at `main`'s `E` position
has a different layout and no defined exit-code mapping — previously this fell through to codegen and
surfaced as an internal "aggregate extract index out of range" lowering failure (undefined behavior
at the `main` boundary, not merely unimplemented sugar). **Decision (owner-approved): restrict**
`main`'s `E` to the builtin `Error`; a user-defined error type there is now a clean sema diagnostic
("main's error type must be the builtin `Error`; user-defined error types in main's return will be
allowed once the full Error design lands"). The check is in `align_sema` alongside the other `main`
signature checks (the return-type validation now runs for both the no-arg and the `args: array<str>`
forms). Convert a domain error to `Error` at the boundary with `map_err(to_error)?`. **This will be
revisited when the general enum→exit-code mapping is designed** (the deferred alternative — e.g. tag
index + 1 for any sum type at that position); that is the only remaining piece of the broader Error
type design (see the section body above), so this section is otherwise complete.

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

### Struct/array alignment attribute `align(N)` — struct + scalar-array-binding form DONE (M6)
**DONE (struct form, M6):** `align(N) Name { … }` over-aligns a struct's storage to `N` bytes (a
validated power of two). Parsed as a prefix attribute (`parse_align_attr` → `StructDecl.align`,
threaded to `StructDef.align`, carried through generic monomorphs via `StructTemplate.align`); honored
at the one `type_align` codegen seam (the slot alloca / AoS struct-array element), which now returns
`max(declared, natural)` so a too-small `align(N)` can never *under*-align (UB). A non-power-of-two /
too-large value, or `align(N)` on a sum type, is a clean error. `draft.md` §9 documents it;
`tests/align_attr.rs`, `examples/align_attr.align`.

**Fixed-array stride padding DONE (2026-07-03).** An over-aligned struct's LLVM type is now
**size-padded** up to its alignment (an `[K x i8]` tail appended at the one struct-layout seam,
`set_struct_body`), so `round_up(size, align)` holds — a fixed `[align(64) S]` array has a tight,
over-aligned element **stride** (every element stays `align(N)`, since the array's stack slot is
already over-aligned via `type_align`). This is the rule the proposal named made concrete: an array
element's stride is always `round_up(element_size, element_align)`, and `align(N)` is simply the only
case that raises it above the natural size. The over-alignment is applied only at the storage seam
(alloca/global), never as a member alignment, so the aggregate type's own ABI alignment stays natural
(the padding field is `align 1`); `align_sema::struct_size_align` reports `(padded_size,
natural_align)` to match, pinned by the `sema_and_codegen_struct_layout_agree` parity test (now
including over-aligned cases). Composes with `layout(C)` (matches C's `__attribute__((aligned(N)))`,
which also pads `sizeof`). A *fixed* `[S{…}, …]` array literal of an `align(N)` struct now compiles;
`draft.md` §9 documents the stride rule.

**DONE (binding form + aligned load, M6):** `align(N) data := […]` over a fixed array of a **numeric**
scalar (int/float — the only element a vector load can target; `int` covers every `u8..u64` byte-buffer
/ DMA case) over-aligns its stack storage — the prefix flows `ast::Stmt::Let.align` → `hir::Local.align`
→ `mir::Function.slot_align`, and codegen over-aligns the alloca via the same `max(declared, natural)`
rule as the struct form (a scalar, a `str`/`bool`/`char`-element array, or a struct array is a clean
error; `N` is the parser-validated power of two). The **aligned vector-load fast path** rides on it: a `data[..].load(i)` on a whole
borrow of the binding is emitted as an *aligned* `<n x T>` load when `(start+i)*sizeof(elem)` is a
compile-time `N`-multiple (`proven_vec_load_align` in MIR, computed from the HIR receiver before it
becomes an opaque slice temp). Everything else — a runtime/non-const index, a non-`N` offset, or a
slice that crossed a function boundary (a `slice<T>` parameter, which carries no alignment
provenance) — stays the always-safe element-aligned load; the alignment is **never over-stated**
(that would be UB). `tests/aligned_binding.rs`, `examples/aligned_load.align`, `draft.md` §9.

**Still deferred:** arena/heap-buffer over-alignment — and, tied to it,
a **dynamic** `array<align(N)Struct>` (the stride is now correct, but its heap buffer can't be
over-aligned yet, so it stays a clean error); an `align(N)` struct as a **struct field** (honoring
it needs the aggregate type's ABI alignment to actually be `N`, which LLVM can't express for a struct
type — it lives at the alloca, not the type — so field embedding stays rejected); and the
**cross-function** aligned-load path — a *fat* `slice<T>` that carries an alignment through a call
would let a callee (`fn f(s: slice<T>)`) prove `s.load(i)` aligned, but that is a slice-type redesign
kept out of this increment. Original design note follows.

A type/allocation alignment attribute (`align(256) Node { … }`, `align(4096) data := …`) for GPU/DMA/page-aligned zero-copy interop. **Retrofit-sensitive**: it modifies struct field-offset math and the arena bump allocator's alignment, so reserve room in the layout model now; the surface + LLVM `align N` emission + arena honoring it can land at M6 alongside SoA. (Digested from `work/proposals/next-draft.md` §1.1.)
**Groundwork landed (pre-M6):** `StructDef` carries `align: Option<u32>` (always `None` today — no surface syntax), and codegen routes all allocation alignment through one seam, `type_align(ty)` (natural ABI alignment today; a struct's custom `align` if set). M6 work is then "parse `align(N)` → set `StructDef.align`" + the seam returns it — the stack-slot alloca already calls the seam; the arena bump allocator already takes an explicit `align` argument. (Retrofit risk was low — a custom alignment is largely *additive* at the alloca/global/alloc sites — so this groundwork is a light reservation, unlike the SoA field-access seam.)

### `out` parameters + `noalias` — DONE (write mechanism + no-alias check + `map_into` + scoped `!noalias` emission)
`out` params (`draft.md` §7) are a no-alias optimization. **All three layers landed:**
1. **Write mechanism** — `out dst: slice<T>` is a writable output buffer and `place[i] = v`
   (bounds-checked) writes a `mut` array local or `out` slice (primitive elements).
2. **No-alias check** — at a call site an `out` argument must not alias another argument, compared
   by **root buffer**: a slice local's provenance is tracked back to the array it borrows
   (`s: slice := a`), so `fill(a, s)` and `fill(s1, s2)` (two slices of `a`) are both rejected, not
   just `fill(a, a)`. `expr_root_local`/`arg_root_local` see through `SliceRange` (`recv[a..b]`), so
   an inline sub-slice argument `fill(xs, xs[0..2])`, two overlapping sub-slices via bindings
   (`s1 := xs[0..2]; s2 := xs[1..3]; fill(s1, s2)`), and nested sub-slices (`xs[0..4][1..2]`) all
   resolve to the shared root buffer and are rejected (conservative: sub-slices of one array are
   rejected whether or not their ranges actually overlap — range analysis is a separate follow-up).
   **Conservatized for the `noalias` precondition (fix for a confirmed miscompile):** the check now
   also requires each root to be a *known* backing buffer (`slice_root_is_known` — a slice/array
   parameter or a real array local) and **rejects** an argument it cannot resolve (a fn-call / `if` /
   block result) or one bound to a slice of unknown origin — instead of the earlier silent skip that
   let `scale(ident(ys[0..4]), ys[1..5])` (an aliasing fn-returned view) through, whereupon the
   callee's `map_into` `noalias` was a miscompile. A fresh array-literal argument is allowed (stack
   storage); scalar arguments are not compared. Tests: `crates/align_driver/tests/out_params.rs`.
3. **`map_into(out dst)` + scoped `!noalias` emission** — the first materializing terminal that
   writes a pipeline into a caller buffer (`src.map(f).map_into(dst)`), and the reachable target that
   makes the metadata worth emitting. The fused loop's source load and `dst` store carry the loop's
   disjoint `in`/`out` alias scopes (`MIR SliceIndexNoalias`/`PtrStoreNoalias` → codegen
   `!alias.scope`/`!noalias`; one fresh domain + `in`/`out` scope pair per `map_into` loop, named
   `fn.mapinto.id` so distinct loops never collide). **Verified:** at `-O2 -force-vector-width=4`
   the loop's runtime overlap guard drops **3 → 0** `diff.check`/`or.cond` instructions vs. the same
   IR with the metadata stripped, both still vectorizing. (`map_into` v1 is length-preserving —
   `map`/field-projection stages, `dst.len() == src.len()` or abort; a filtering `where` before it,
   which writes a variable prefix, is deferred.) Tests: `crates/align_driver/tests/map_into.rs`.

**Soundness gate (the precondition for emission), now closed.** `noalias` on the `map_into` loop
asserts `dst` is disjoint from the source, so the emission gate rejects anything it cannot prove
disjoint: (a) the destination and the source root must be a *known* backing buffer — a slice/array
**parameter** (distinct by the caller's `out` no-alias contract) or a real array local; (b) a slice
**local of unknown origin** (bound to a fn-returned slice, a `soa` column `s.col`, or a struct-field
slice — all `expr_root_local == None`, so their root is themselves and would falsely read as
"distinct") is rejected, as is such a form used directly as the source; (c) a fixed array-literal
source (fresh stack storage, provably disjoint from any caller slice) is allowed but its loads are
**not** scope-tagged (no over-emission). A slice-typed local carries a new `hir::Local::is_param`
flag so the gate can tell a parameter from an unknown-origin `let`. This closes the earlier hole
where two sub-slices of one array (or a fn-returned view that aliases the `out`) slipped past.

**Encoding that shipped** (proven by the 2026-07-02 investigation, then implemented): tag only the
loads/stores whose base is a slice — the source `SliceIndex` load `!alias.scope !{in}, !noalias
!{out}` and the `dst` store `!alias.scope !{out}, !noalias !{in}`, with `in` and `out` two scopes in
a fresh per-loop domain (a scope node's operand[1] is its domain, so both report the same domain and
the AA proves the store/load never overlap). Only **one** input scope and **one** output scope per
loop — inputs never claim noalias against each other, so no over-emission. Fresh-alloc pipeline
loops (`to_array`/`scan`/`to_soa`) write a freshly allocated buffer disjoint from the source and
LLVM already vectorizes them with no overlap check, so they are left untagged; the
`noalias`/`willreturn`/`nofree` return attrs on the allocator-family runtime decls are the
orthogonal lever there.

(Digested from
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

### FFI (foreign function interface) — v1 COMPLETE (keystone for the library strategy)
Detailed design of C / Rust / Zig interoperability. Because Align is AOT-via-LLVM with no GC, an external C call is a direct LLVM `call` at native speed (no pinning / stack-switch / marshaling), and an Align `slice`/`str`/`bytes` hands its raw pointer straight to C. **This gates a deliberate library strategy: "own the memory wrappers, borrow the mathematical engines"** — `std.compress` wraps `libzstd`/`zlib-ng`, `pkg` DB drivers wrap `libpq`/`sqlite`, etc., rather than re-implementing assembly-tuned algorithms in Align. So FFI's design should land before those `std`/`pkg` libraries are built, even though it stays out of the v1 *language* core. (Digested from `work/proposals/ffi-optimization.md`, `compression-strategy.md`, `rdb-optimization.md`.)

**First slice SHIPPED (2026-07-01):** `extern "C"` bodyless declarations + `unsafe`-gated direct calls; FFI-safe scalars (int/float) + `raw` + `()` return; libc/libm resolve with no extra `-l`. See the `unsafe`/`raw` Settled entry above for the full record.

**`layout(C)` struct ABI — slice 1 SHIPPED (2026-07-01):** a `layout(C)` attribute (composes with `align(N)`) pins a struct to a stable, C-compatible flat layout (decl order, natural alignment, no reordering — Align's default, which the marker *locks* and opts into FFI). Only a `layout(C)` struct may be moved through a `raw` pointer — `raw.store`/`raw.load` widened to accept a struct value (no new IR variant; the existing `Scalar::Struct` flows through `RawLoad`/`RawStore`, codegen does an unaligned aggregate load/store). Fields must be int/float. This is the **pointer-based** FFI pattern (hand C a buffer, read/write structs in it).

**FFI views — SHIPPED (2026-07-01):** a `str`/`slice`/`bytes` view is FFI-safe as an extern **parameter**, lowered to its data pointer (C `char*`/`void*`); the length is passed separately by the caller (`s.len()`) — the C `(ptr, len)` idiom, no hidden arg (`is_ffi_safe_param`; codegen `ffi_param_type` + an `extern_params` map that coerces the `{ptr,len}` arg to element 0). Slice element must be an int/float scalar (`slice<str>`/`slice<Struct>` rejected — no settled C element layout). Not a valid return type (a bare pointer has no length → returns stay scalar-only); not NUL-terminated (length-based C fns only).

**External library linking — SHIPPED (2026-07-01):** an `extern "C" link("name")` clause names a library to link (`-lname`); sema validates + dedupes into `hir`/`mir::Program.link_libs`, and the driver's `link_executable` appends `-l<name>` after the objects/runtime (libc/libm stay auto-linked). The name is charset-validated (`[A-Za-z0-9._+-]`) and passed as a single `-l<name>` argv (no flag/shell injection). `ast::ExternBlock.link`.

**FFI v1 — COMPLETE (2026-07-01).** The shipped surface: `extern "C"` decls + `unsafe`-gated calls; scalar/`raw`/`()` signatures; `layout(C)` struct-by-pointer (`raw.load`/`store`); `str`/`slice`/`bytes` views (data-pointer + separate length); `link("name")` external libraries. That is a coherent, tested v1 — the `std`/`pkg` C-engine wrapper strategy (zstd/sqlite/…) can be built on it (own the memory wrappers, borrow the engines, pass buffers by pointer+len).

**Deliberately out of FFI v1** (draft §15 "Not in FFI v1", decided 2026-07-01 — defer over ship-half-right):
- **A struct by value** — SHIPPED for **x86-64 SysV (Linux) only** (`feat/ffi-byvalue-sysv`). A `layout(C)` struct ≤ 16 bytes is passed/returned in registers via the SysV AMD64 classification (each eightbyte INTEGER→`i64` slot / SSE→`double` slot; a two-register value returns as an `{T0,T1}` aggregate). The compiler emits exactly clang's coerced IR, so a call is binary-compatible with a real C callee — proven by a compiled-C-helper harness (`crates/align_driver/tests/ffi_byval.rs`) that links a `cc`-built by-value callee and round-trips every eightbyte pattern ({i32,i32}/{i64,i64}/{f64,f64}/{f32,f32} packed/{i32,f32} merge/mixed {i64,f64} return). This is the one FFI corner a wrong per-target rule *silently miscompiles*, so it is structurally fenced: **codegen refuses on any non-SysV target** (diagnostic: pass by pointer instead) rather than guessing; a **> 16-byte MEMORY-class struct is rejected** (redundant with struct-by-pointer); and — the subtle one — a struct argument that would **fall to memory under register pressure** is rejected too. SysV's all-or-nothing rule passes a struct in registers only if every eightbyte fits in the class registers left after preceding args, else the whole struct goes `byval` on the stack; clang implements that reclassification in its frontend, and a flattened `{i64,i64}` at the exhaustion boundary makes LLVM split the struct across the last register and the stack (verified round-trip corruption vs a clang `byval` callee), so those signatures are refused rather than miscompiled (reorder the struct earlier, or pass by pointer). In every accepted case the struct fits in registers and per-eightbyte flattening is byte-identical to clang's own flattened parameter form. Still deferred: AAPCS64 (other arches), and the MEMORY-class `byval`/`sret` path (added only when a concrete wrapper needs a large by-value struct).
- **`bool` / `char` as FFI types** — use the integer types (C `_Bool` = `u8`, `char` = `i8`/`u8`, `char32_t` = `u32`; a `wchar_t` is platform-sized — pick the matching integer width). Align `char` is a 32-bit Unicode scalar (**not** a C `char`), so admitting it would invite the wrong mapping; `bool` stays out for the same one-unambiguous-way reason (and dodges the `i1`-`zeroext` ABI subtlety). Note: there is no `bool as int` cast today, so a `bool` reaches C as `if b { 1 } else { 0 }`.
- **`raw.ptr_cast<T>`** — a *typed* reinterpret has nothing to reinterpret to while `raw` (opaque bytes) is the only pointer type; it earns meaning once FFI grows typed/external pointers.

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
Concrete Linux mechanisms (external idea review, 2026-07-02; verified): **io_uring**, including
SQPOLL polling mode, and **Direct I/O into huge-page-backed arenas** are candidate fast paths behind
the dispatch table above. The API-shaping constraint is unchanged either way: `std.fs`/`std.io`
buffers are caller-owned (arena), so a zero-copy path drops in without an API change.

Placement: `std.io` (OS boundary, `draft.md` §18.2), implemented in the Rust runtime
with a portable fallback; cross-platform mmap via a crate (e.g. `memmap2`). Revisit
around the string/JSON milestone (M5) and std build-out.

**Status update (2026-07-03, M9 std design):** the v1/reference portable fixed-buffer loop for
`io.copy` is scheduled as `impl/07-roadmap.md` M9 Slice 2 (memory-bounded, `O(buffer)`, tested).
The fast paths above (`sendfile`/`splice`/mmap/`io_uring`/Direct I/O) stay **post-M9**, added later
without an `io.copy` signature change.

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
  Reservation (2026-07-02, internal review, Opus+Codex independently agreed): when this ships,
  give scalable vectors their OWN spelling — e.g. svec<T>/spred<T>, still unused/undecided — never
  a runtime-variable-N vecN<T>. A scalable type is register-only with no stable byte layout, so it
  must be PROHIBITED (not just "not yet supported") in: struct fields, array/tuple elements,
  layout(C), raw.load/raw.store, extern "C" signatures, and any constant layout computation (soa
  column stride, sizeof). vecN<T>/maskN<T> stay fixed-size forever — this is a second, sibling type,
  not a generalization of the first. The pipeline (map/where/reduce) stays the width-agnostic path
  scalable ISAs actually live in (see the SIMD-exposure Settled addendum above); vecN<T> is only the
  fixed-width kernel escape hatch.
- Matrix engines — ARM SME/SME2 AND x86 AMX (deferred; the migration foundation is the point, not the
  implementation; the foundation is cross-ISA — same shaped-op surface lowers to SME, AMX, or a
  scalar fallback, picked by the capability dispatch above, never named in source). Taking SME as the
  worked example. SME is NOT another wider NEON — it is a 2D tile accumulator (ZA register,
  outer-product-accumulate → matmul) requiring streaming-SVE mode (PSTATE.SM) and a streaming-
  function ABI, so it lands in codegen's ABI layer, not as a loop-vectorization tweak. Hard
  constraints that keep the door open WITHOUT building it now: (1) never expose SME (or any fixed
  width) in source — the only surface is a high-level shaped op (`tensor.matmul`, batched
  outer-product/reduce) that lowers to SME with a NEON fallback, per "SIMD comes from map/reduce
  lowering well, not intrinsics" + Nothing hidden; the language stays width-/engine-agnostic.
  (2) Keep MIR free of baked-in vector width / NEON-128 assumptions so the same IR can target
  NEON today and SME/SVE2 later (already the "capabilities, not feature-names" dispatch rule above).
  (3) The prerequisite is a 2D/tensor abstraction — design the M4 array model so a 2D extension +
  reduce-over-2D is reachable, don't make `array`/pipeline fundamentally 1D-only. Trigger: a tensor
  surface lands AND SME hardware is testable (Apple M4+ has SME but no SVE; cloud Graviton/A64FX for
  SVE2 — none testable on the M1 dev host, so verification is rent-cloud-briefly, not a blocker).
  Needs the LLVM/inkwell upgrade checkpoint first (LLVM 19 predates serious `sme2` codegen).
  Reservation (2026-07-02, internal review, Opus+Codex independently agreed): reserve mat<R,C,T> as
  the fixed-shape 2D sibling of vecN<T> for this (tiles are naturally fixed-shape, matching SME/AMX
  fixed tile registers). A tile is an OPAQUE accumulator — never a byte-layout type, never a soa/
  array element or struct field, same rule as the scalable-vector reservation above. matmul/contract
  is a builtin over contiguous or soa columns (the 2D sibling of `dot`), NOT a pipeline stage —
  2D reduction doesn't fit the 1D map/where/reduce shape without a magic special case. The natural
  input shape is already available: group_by's columnar `(array<K>, array<V...>)` result is the
  right shape to feed a GEMM; only an explicit conversion to a future 2D/tensor view is allowed, no
  implicit tiling. No new type is needed yet — this is a reservation, not a build item.
- APX (x86, 32 GPRs instead of 16): fully backend-transparent, essentially zero language work
  (2026-07-02, internal review, Opus+Codex independently agreed). LLVM handles the new encoding once
  it targets APX; Align exposes no register constraints, no inline-asm, no fixed calling convention
  (FFI is layout(C) + by-pointer, not register-pinned) — nothing in the surface assumes a GPR count.
  The only guardrail: keep it that way — never let a spec passage assume 16 GPRs or fix a register
  ABI, and keep struct-size-related lints (e.g. "this struct is cache-unfriendly") anchored to cache
  line size, not register count. If anything, Align's shape (multi-accumulator reductions, wide
  group_by) benefits more from extra GPRs than typical code. Implementation (LLVM/inkwell upgrade,
  --target-cpu apx) rides the same LLVM-upgrade checkpoint as AVX10/SME2 above; nothing to do now.
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

### Niche optimization for `Option` payloads (external idea review, 2026-07-02)
Represent `Option<box<T>>` (and future non-null-pointer-like payloads) with the null niche: the tag
occupies zero bytes, `None` = null. Semantically invisible (still plain `Option<T>`/`match`/`else`),
FFI-explainable (a null-or-valid pointer is exactly what C already does), proven in Rust. **Must be
decided before the ABI/layout freeze** — like the field-reordering item in Open above, it is a
one-time representation choice. Does **not** extend to general pointer-tagging / NaN-boxing for other
payloads — those stay rejected (arch-dependent, breaks layout predictability).
**Blocked on target type — deferred (2026-07-02):** the sole single-pointer payload this decision
targets, `box<T>`, is **not currently expressible as an `Option` payload**. `Ty::Option` carries a
`Scalar`, and there is no `Scalar::Box` — `ty_to_scalar` returns `None` for `Ty::Box`, so
`Option<box<T>>` is rejected at type resolution ("Option payload must be a scalar (composite payloads
are not supported yet)"). The same holds for `Ty::Task` and `Ty::ArenaHandle` (also non-`Scalar`, and
arena handles aren't even user-writable type names). The niche has no expressible target, so
implementing it now would mean first widening the type system to admit a pointer-payload `Option` —
out of scope for a representation-only change. **Revisit when `Option<box<T>>` becomes writable**
(a `Scalar::Box` / pointer-payload `Option`): at that point add an `is_niche_option(scalar)` predicate
and route Option type-lowering + the tag read/write sites (codegen `option_struct_type`,
`Rvalue::OptionIsSome`/`OptionUnwrap` lowering in `align_mir`, and the `else`-unwrap / match-decompose
paths) through it (Some = pointer, None = null, tag = 0 bytes). Note: the *fat*-pointer Move payloads
that **can** already be `Option` payloads (`Scalar::String`, `Scalar::DynArray`, `Scalar::DynStructArray`
— `{ptr,len}`) admit a related null-`ptr`-niche, but that is a **separate** design (a fat pointer is
not the "single pointer, None = null" form decided here) and is intentionally left out of this item.
Provenance: surfaced by an external idea review (2026-07-02); verified. Target-type block recorded
after implementation attempt (2026-07-02).

### `f16` / `bf16` scalar and vector element types (external idea review, 2026-07-02)
Add half-precision scalars (`f16` IEEE binary16, `bf16` brain float16) usable as `vecN<T>` element
types, mapping to AVX-512 FP16/VNNI and NEON/SVE FP16/BF16. Needs one semantic decision before
building: native f16/bf16 arithmetic vs. widen-to-f32-compute with narrow storage (most hardware
converts on load/store rather than computing natively). Motivated by LLM/signal-processing workloads
(ties to "Resource-oriented north star" below). Belongs after M6's SIMD layer, before any
tensor/matrix backend — a scalar-width prerequisite for feeding the `mat<R,C,T>`/matrix-engine
reservations in "Hardware & backend optimization backlog" above. Kept as its own entry rather than
folded bodily into that backlog: a new scalar type touches the frontend/type-checker (a new `Scalar`
variant), outside that backlog's stated "pure backend lowering" scope.
Provenance: surfaced by an external idea review (2026-07-02); verified.

### SIMD string search for `str` ops (external idea review, 2026-07-02) — Status: done (2026-07-02)
`str.contains`/`find`/`rfind` are `memchr::memmem`-backed (since #203/#207), which already ships the
AVX2 (x86_64) + NEON (aarch64) + scalar-fallback triple path with runtime feature detection — the
reference form of the memchr-style first-byte-scan + verify this item asked for, satisfying the
arch-parity rule by delegation. Re-implementing a hand-rolled parallel SIMD substring search was
rejected as a strictly-worse duplicate mechanism (a second search path, more `unsafe`, no perf gain
over the shipping ~29× vs naive-scalar throughput) — against "one way / ideal form". `starts_with`/
`ends_with` stay scalar `==`/`memcmp` (bounded to the needle length; no worthwhile SIMD lever).
The item's stated specific contribution — the **differential-oracle test discipline** — is now in
place: `str_search_simd_matches_scalar_oracle` locks whichever SIMD path the host CPU selects against
an independent scalar oracle across a 64-byte-boundary padding sweep, prefilter decoys, needle lengths
0/1/large, multibyte UTF-8, overlapping repeats, tail matches, a multi-KB haystack, and a
deterministic randomized cross-check (the JSON-index `json_decode_index_simd_matches_scalar_oracle`
discipline). Converges with the `core.string` byte-first-APIs plan above (P0 memchr/memmem-backed).
Provenance: surfaced by an external idea review (2026-07-02); verified.

### Relative (offset) pointers inside arenas (external idea review, 2026-07-02)
When recursive/pointer-linked types are eventually designed (recursive enums are currently deferred —
see the Sum-types Settled entry), the first-choice representation for intra-arena links should be a
**32-bit self-relative offset**, not an 8-byte absolute pointer — halves node size and composes with
zero-copy mmap of arena images. Record now so the recursive-types design starts from this default.
Provenance: surfaced by an external idea review (2026-07-02); verified.

### M8 lint candidates (consolidated, gathered across reviews)
The formatter is M8's first deliverable (in progress, see "Additional perf levers" above); these are
the lint candidates that have accumulated around it, gathered here so they aren't scattered across
individual review entries. None block anything; pick up when the lint suite is actually built.
```text
- Wasteful i64/f64 default on a large array/soa/pipeline literal: an unconstrained-width literal
  defaults to i64/f64 (Settled, "Numeric literal typing"), which is fine for a scalar but doubles
  memory bandwidth for a big data-oriented buffer that didn't need 64 bits. Flag it where it's most
  likely to matter — array/soa element types and pipeline literals — not every scalar `x := 1`.
  DONE (2026-07-02, lint batch 1): a warning on a literal array of >= 64 elements whose element type
  is left to the i64/f64 default (`check_array_lit`; threshold `DEFAULT_ELEM_LITERAL_ARRAY_LEN`).
  Silent below threshold, when a context/annotation constrains the element, or when the element type
  comes from a concrete value. `tests/lint_default_elem_array.rs`.
- Lossy/saturating `as` diagnostic: `as` is the one conversion operator and deliberately covers
  lossless, truncating, and saturating conversions alike (Settled, "Numeric conversion — as"); a lint
  distinguishing narrowing / float→int / char<->int casts (silently lossy or saturating) from
  lossless ones gives back the visibility without adding a second conversion mechanism.
  DONE (2026-07-02, lint batch 1): a warning on a narrowing int->int, float->int (saturating),
  wide-int->float (past the mantissa), narrowing float->float, or char narrowing `as` (`cast_loss`
  in `check_cast`). Same-width / widening / same-width sign-change and unconstrained-literal sources
  stay silent. `tests/lint_lossy_cast.rs`.
- Unnecessary heap: a `box<T>` that is allocated but never needs a heap identity — its scalar is only
  read back and it never escapes — should be a stack value.
  DONE (2026-07-03, lint batch 2, narrow slice): a warning on the inline form `heap.new(x).get()` — a
  box allocated only to immediately read its scalar straight back (a `box<T>` payload is a scalar in
  M3, so `.get()` is a plain copy-out). Detected purely locally in `finalize_expr` (a `BoxGet` whose
  receiver is the allocating `HeapNew` itself); reuses no escape-analysis state, is profile-independent
  (structural, like huge-struct-copy — NOT in the deferred frequency-dependent allocation-lint bucket),
  and never false-positives. `tests/lint_unnecessary_heap.rs`.
  DONE (2026-07-03, lint batch 2, broad slice): the common shape `p := heap.new(x); … p.get()` (box
  bound to a local, only ever `.get()`-ed, never moved/stored/returned/cloned) is now flagged by a new
  whole-function box-use scan (`UnnecessaryHeapScan` in `align_sema`, run from `check_fn` after
  finalize). It collects every box local (a `Let` whose init is `HeapNew`), then in one linear pass
  over the body classifies every *occurrence* of every local: a `BoxGet` whose receiver is the local is
  a "get", anything else is an "other". It fires for a box local iff it has ≥1 get and **no** other
  occurrence (any move / store / return / `.clone()` / call-arg / capture / reassignment target
  suppresses it) — sound and conservative, exactly as proposed. The scan's `ExprKind` match is
  exhaustive (no wildcard), so a future IR variant carrying a box use forces a compile error rather than
  silently escaping the classification. Disjoint from the narrow slice (its `.get()` receiver is a
  `HeapNew`, the scan's is a `Local`), so the two never double-report. The didactic `examples/arena.align`
  did fire (a true positive: both boxes were only `.get()`-ed) — rather than distort the firing
  condition, the example was rewritten to demonstrate `box<T>`'s defining trait, the *move* (`q := p`),
  which is a genuine non-`.get()` box use and keeps the example warning-clean. `tests/lint_unnecessary_heap.rs`.
  (Latent bug found while building this, orthogonal to the lint — **FIXED** (2026-07-03): the inline
  `v: i32 := heap.new(7).get()` used to **miscompile**. `heap.new`'s payload scalar resolved eagerly
  from the literal — defaulting to `i64` — the outer `v: i32` annotation did not flow back into it, so
  an `i64` box was read into an `i32` slot (exit 160, not 7) and the `let`'s `i64`→`i32` mismatch was
  not caught. Two root causes, both closed: **(1) soundness** — `check_expr` had *no single point*
  reconciling a value's concrete type with its `expected` context type. Literal/constructor arms thread
  `expected` inward (`constrain`), but value-producing arms — a call, a `box.get()`/`box.clone()`, an
  `as` cast, a reduction terminal — return a *fixed* type and ignored `expected`, so the binding site
  (`let` / assignment / struct field / `return` / call arg) silently took the annotation type while
  codegen stored the value's real type. A `check_expr` wrapper now `constrain`s `result.ty` against
  `expected` at the boundary — the single reconciliation point — gated on "this subtree reported no
  error of its own" (via `Diagnostics::error_count`) so a terminal that already enforces its own result
  type is not double-reported; a warning does not gate it, and skipping only defers a diagnostic since
  any error halts codegen. This closed the whole class uniformly (verified across let/assign/struct-
  field/return/call-arg and the sibling builtin results `box.clone`/`str.len`/`array.sum`/`task.get`),
  and surfaced several latent i64→i32-through-i32-`main` returns in the test corpus (made type-correct
  with a visible `as i32` narrowing — *Nothing hidden*). **(2) inference quality** — the expected type
  now flows into a `heap.new(...)` receiver of `.get()`/`.clone()` (scoped to that receiver via
  `is_heap_new_call`, so a box-typed *variable* is not double-constrained), so `v: i32 :=
  heap.new(7).get()` infers `box<i32>` and *works* rather than merely erroring. Tests: `align_sema`
  `heap_new_payload_infers_from_binding_annotation` / `box_get_result_width_mismatch_is_caught_once` /
  `value_result_width_mismatch_is_caught_across_contexts`; e2e `m3::inline_heap_new_get_infers_payload_width`.)
- Prefer-pipeline-over-vecN for bulk data: nudge bulk/array-shaped code from a hand-tuned fixed-width
  vecN<T> kernel toward the width-agnostic pipeline (map/where/reduce) when the data is a plain bulk
  scan — vecN<T> is the escape hatch for genuinely hand-tuned kernels, not the default, and pipeline
  code is exactly what stays portable to scalable ISAs (see the SIMD-exposure Settled addendum and
  the scalable-vector reservation in the Hardware backlog above). Reserved 2026-07-02 (internal
  review, Opus+Codex independently agreed) specifically to guard against AI-generated code defaulting
  to a fixed 128/256-bit vecN<T> loop and losing SVE/RVV portability for no reason.
  DEFERRED (lint batch 2, 2026-07-03): no firing surface exists yet. The lint's target — a *hand-written
  `vecN<T>` loop* (a counted loop doing vec-load → arith → vec-store per iteration) — cannot be written
  in Align today: there is **no loop construct** (iteration is only `map`/`reduce`/… pipelines), and a
  bare `vecN<T>` expression (`a + b` over `vec4<i32>`, or a single `.load(i)`/lane op) is the *correct*
  hand-tuned-kernel use, not a convert-to-pipeline candidate — flagging it would be a false positive
  against `vecN<T>`'s reason to exist. Any purely-mechanical single-expression trigger would be wrong;
  the heuristic "bulk scan expressed as vecN" is only meaningful once a loop/kernel form exists.
  Firing-condition proposal for then: a counted loop over an array whose body is *exactly* {vec-load a
  contiguous chunk → one elementwise arithmetic op → vec-store the chunk}, with no other statements and
  no cross-lane/reduction op — that mechanical shape is a portable `map`; anything richer (shuffles,
  masked lanes, hand-tuned reductions) is a genuine kernel and stays silent.
- ~~Out-of-range compile-time integer literal (`x: u8 := 300`): candidate lint~~ — **done as a hard
  error instead** (SETTLED 2026-07-02; see "Out-of-range compile-time integer literals — hard error"
  in Settled). No lint needed.
- par_map cost-threshold lint / cheap-par_map-loses-to-sequential (already recorded above under
  "Codex perf / I/O / LLM research sweep" — listed here only so it isn't missed in a lint-suite pass).
- connect-per-request-to-a-static-host lint for the future std `http`/`socket` layer (already
  recorded above under the same sweep).
- Hot/cold field-split suggestion (external idea review, 2026-07-02, verified): when a struct mixes
  hot (scanned) and cold (rarely-read) fields under array/pipeline access, suggest `soa<T>` or a
  manual struct split — suggestion only, never an automatic layout change (explicit-layout is
  Settled).
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
