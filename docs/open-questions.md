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
references *inside* an initializer are deferred; **scalar/`str`-array aggregate constants landed
2026-07-17 — see the extension below**; struct constants and `as` in a constant stay deferred). A
constant's type is **fixed at the definition** (unlike a local it does not infer
from a use site — it must be stable across modules), so an unannotated integer defaults to `i64` /
a float to `f64`. Constants are **per-module namespaced like functions/types** (`module$NAME`
canonical, entry unmangled so single-file programs stay byte-identical): `pub` exports one, an
importer names it qualified (`mod.NAME`), and a name may not be both a function and a constant in
one module. Overflow wraps (defined two's-complement); division by zero, a cyclic definition, and a
type mismatch are compile-time errors. Folded values feed the const string pool (`draft.md` §12).
Record: `draft.md` §3/§4, `docs/language-spec.md`, `impl/02-frontend.md` §3, `examples/constants.align`, `tests/constants.rs`

**Extension — aggregate (array) constants (S1, DONE 2026-07-17).** An initializer may be an **array
literal** (`PRIMES := [2, 3, 5]`, `SCALE: slice<f64> := […]`, `DAYS := ["Mon", "Tue"]`). Its type is
**`slice<T>` with `Region::Static`, never `array<T>`** — ownership is a property of the type, so a
top-level array constant owns nothing; it is the exact analogue of a `str` literal (a static view of
rodata). The folded elements become one **per-unit private read-only global** and the constant is a
`{ptr,len}` view of it — shared, never copied, returnable, never dropped. Because it is a `slice<T>`,
indexing, `.len()`, slicing, and pipelines flow through the existing borrowed-view paths with **no
allocation** and no array-constant-as-value seam; a **constant index folds to the element** (no load),
a dynamic index reads the table. Elements are S1 scalars / `str`, each folded by the same const-eval
as a scalar (so element positions accept the `AREA := W*H` capability); the element type is inferred
(`[1,2,3]` → `slice<i64>`) or taken from a `slice<T>` annotation. An `array<T>` annotation is rejected
with guidance (a top-level array constant is a static `slice<T>` view). **Read-only enforcement:** the
view is `Static` rodata, so writing through it (`TABLE[i] = v`, or an `out slice<T>` argument) is
rejected even through a `mut` binding / sub-slice / rebind — a `readonly_locals` provenance set,
grown at binding and slice reassignment (insert-only → sound-conservative), checked at `check_place`
and the `out`-argument site; the same rule covers a string literal's `.bytes()` view. Cross-unit: a
`pub` aggregate constant exports its initializer source (`IConst.value_src`, already folded into
`interface_hash`), so each consumer rematerializes it against its own rodata and an edit invalidates
dependents for free — no `FORMAT_VERSION` bump. A `pub` constant's value is part of the exported
interface, so its initializer may reference only `pub` constants — enforced **producer-side** at the
defining unit (Pass 0d-2) so whole-program and per-unit builds reach the same verdict (this fixed a
pre-existing D1 divergence for the scalar `pub A := SECRET` shape too). **Deferred (recorded S1.5):**
struct constants and struct elements; in an element position — function calls, `as` casts, nested
arrays, and references to other aggregate constants (all fail-closed). **S3 SHIPPED 2026-07-17
(implementation-only, no language surface):** a function-local all-constant array literal binding is
pooled into the same S1 rodata via one memcpy (LLVM elides it to a direct rodata read) when it is
non-`mut`, non-`align(N)`, a fixed `array<T>` of a scalar, all elements fold to constants, and its
length is ≥32 (measured crossover). The binding **keeps its fixed `array<T>` type** — the observable
type is unchanged, so no program is re-typed or rejected (the tempting `slice<T>`-rewrite was
rejected precisely because it would change the type and a single-pass checker cannot prove every use
is slice-compatible). `ALIGN_CONST_POOL=off` reverts. Deferred: `mut`/`align(N)` bindings (memcpy
template is sound but a smaller win), `str`/struct elements, and folded-*expression* elements
(fail-closed). See `impl/13-…` §8.4 for the measured cutoff table and gates.
Record: `draft.md` §3 (Constants) / §12, `docs/language-spec.md`, `docs/design-notes.md` (Memory model v2),
`impl/02-frontend.md` §3, `impl/13-…` §8.4, `tests/constants_aggregate.rs`, `tests/per_unit.rs`, `tests/cache_codegen.rs`, `align_interface/tests/summary.rs`

**Open follow-up (pre-existing, exposed here):** the read-only-view write check flags *compile-time
constant* provenance (`ConstArray`, a string literal's bytes) traced within a function. The broader
analogue — a slice viewing a **non-writable arena `mmap` view** (`fs.read_file_view`), or a constant
laundered through a plain (non-`out`) `slice<T>` parameter across a call — is not yet flagged (it needs
whole-program / buffer-writability provenance). Neither is introduced by aggregate constants; both are
pre-existing holes. Record here rather than blocking S1.

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
  **Reference pointer (recorded 2026-07-04, external design-note review adoption):** Roaring Bitmaps
  (compressed/sparse bitset representation) as prior art when `core.bitset`'s design resumes.

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

### Ordinary sequential pipeline effects and evaluation order
**Decision (2026-07-13): Impure is allowed; guarded source order is normative.** Sequential
`map`/`where`/`reduce`/`scan`/`partition`/`any`/`all` callables run in input-index order and stage
order, exactly once for each element that reaches them. A false `where` suppresses every later
stage/reducer for that element; `any`/`all` do not short-circuit. Inferred effects constrain
reordering, speculation, erasure, duplication, and parallelization instead of rejecting ordinary
sequential code. Pure is not a proof of total/non-trapping execution. Explicit `par_map` remains
Pure-required. `sort_by_key` is excluded pending its separate key-evaluation settlement below.
**Implemented:** reducing MIR branches around every general callable after `where`, while safe field
operations plus builtin `sum`/`count`/`min`/`max` retain the mask/identity-select vector shape.
Record: `draft.md` §8, `docs/language-spec.md`, `impl/12-pipeline-closure-memory-io-simd-audit.md` §3

### Lambdas / closures — IMPLEMENTED (map/where/all reducers + capture)
**Decision: lambdas exist and are the way to pass behavior to stages/reducers; capture by value, no hidden closure environment.** Always part of the design (`draft.md` §8/§11 use `fn x { ... }`); the early implementation accepted only named functions, now lifted. **Implemented**: an inline lambda `fn params { body }` (parameter types inferred) in `map`/`where`/`reduce`/`par_map`/`scan`/`partition`/`any`/`all`/`sort_by_key` is **lifted** to a synthetic top-level function (`align_sema` `lift_lambda`), so it flows through the same `Rvalue::Call` + fused-loop lowering as a named function — optimized identically. **Capture** of enclosing locals is by value: each captured local becomes a trailing parameter passed at the call site (a loop-invariant argument the backend hoists), so there is no closure environment / allocation. Capture is wired into **every** stage and reducer (`map`/`where` + `reduce`/`scan`/`partition`/`any`/`all`/`par_map`/`sort_by_key`) for copy values; a capturing `par_map` falls back to the sequential path (the parallel thunk has no capture context). All three flow analyses (`MoveCheck`/`EscapeCheck`/`EffectScan`) walk stage and node captures. First-class function values and task-group capture have since shipped as recorded next; fully escaping function values and the remaining owned-value capture shapes stay deferred.
Record: `draft.md` §8 (Function Arguments), `docs/language-spec.md`, `design-notes.md` (lambda philosophy), `impl/07-roadmap.md`.

### First-class closures + `task_group` — SETTLED + IMPLEMENTED (fully-escaping fn values deferred)

> **Current status:** first-class closures ①–③ and `task_group` ④a–④c are shipped, including real
> threads, scoped capture environments, `spawn`/`wait()`/`get()`, and fallible `wait()?`. The plan
> below is retained as the implementation record. Only function values that escape every enclosing
> region (return / struct field / array element) remain deferred pending a heap-owned environment
> and Drop model with a real consumer.
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
  **Reference pointer (concurrent arena, Future, recorded 2026-07-04, external design-note review
  adoption):** Mimalloc free-list sharding as prior art if/when this region's bump allocator needs to
  serve concurrent `spawn`/`alloc` calls across the ④b-2 real threads without a single global lock.
- **Owned `R` (`string`/`array<T>`)** is the subtle case: the slot holds the owned `{ptr,len}`. `get()` (consuming for a Move `R`, per ④a) moves it out — afterward the caller owns the buffer, while the slot itself stays in the region until the whole region is reclaimed at scope end. An **un-`get()`'d** owned-`R` task must still free its buffer before the region drops: codegen emits a conditional drop of each owned-`R` task at scope end, gated by a **drop flag cleared by `get()`** (the existing drop-flag-via-null pattern, applied to the slot). (Alternative under consideration: make `get()` mandatory for an owned-`R` task — a must-consume rule — so the buffer always moves out and no in-region drop is needed; decide in ④b-1.) Copy `R` needs none of this (the region free reclaims everything).

**④c-2 plan — the `wait()?` error boundary (the last task_group slice).** A task may **fail**: its closure returns `Result<R, Error>`. `wait()?` joins all, and if any task failed, propagates **an** `Err` out of the enclosing function (with parallel tasks there is no deterministic "first" — any failing task's error surfaces; documented). After `wait()?`, `get()` yields the `Ok` `R`. Implementation, in order:
- **Prerequisite — `Result`-returning spawn closures.** A `Result`-returning lambda cannot be a `Ty::Fn` value today (`FnTy.ret` is scalar-only). Since a spawned lambda is *consumed by `spawn`* (never a free first-class value), `check_spawn` should **lift the literal lambda directly** (via `lift_lambda`, whose result type may legitimately be `Ty::Result(ok, ErrCode)`) instead of routing through a `Ty::Fn` value — and the `Spawn` node carries the lifted name + captures + the `Ok` scalar + a `fallible` flag, like `Closure` does. **Infer the lambda's `Err` type from the enclosing function's return type** (no annotation needed): `wait()?` propagates the task error out of the enclosing function, so the task's `Err` must match the enclosing function's `Err` — pass that as the lambda's expected return (`Result<_, EnclosingErr>`), so `spawn(fn { fallible()? ; Ok(x) })` type-checks without a written return type.
- **`get()` requires a *successful* `wait()`.** For a fallible group, a bare `wait()` whose `Result` is ignored does **not** make `get()` safe — an `Err` task never stored its slot, so the slot is uninitialized. So the ④c-1 wait-state flag is set only by `wait()?` (or otherwise handling the `Result` such that control is on the success path) for a fallible group; a bare `wait()` does not enable `get()` there. (For an infallible group `wait()` returns `()` and enables `get()` as in ④c-1.) Thus `get()` is reachable only when `wait()` is guaranteed to have *succeeded*.
- **Per-`task_group` `fallible` flag** (a stack like `wait_state`): set when a `Result`-returning task is spawned. `wait()`'s type is `Result<(), Error>` when the group is fallible, else `()` (so infallible groups stay `()` — no spurious `Result`).
- **Error reporting via the worker's return value (no shared state).** The per-`R` trampoline returns an `i32` error code (`0` = ok): infallible → store `R`, return `0`; fallible → match the `Result`, on `Ok(v)` store `v` and return `0`, on `Err(e)` return `e`. `align_rt_tg_wait` (already `thread::scope`) collects each worker's returned code via `ScopedJoinHandle::join` and returns the first nonzero — no shared error cell, no extra aliasing.
- **`wait()?`**: codegen builds `Result<(), Error>` from `tg_wait`'s code (`Ok(())` if `0`, else `Err(code)`); `?` propagates as usual. `get()` (already `wait`-gated by ④c-1) then reads the `Ok` slot.

**Closure-captured arena-view escape — FIXED (2026-07-10, PR #406).** The gap the A1 adversarial
review recorded here (a lambda capturing an arena-backed view escaped the arena unchecked →
use-after-free; `f := arena { v := fs.read_bytes_view(p)?; fn { v[0] } }` passed `check` and
SIGSEGVed at `f()`) is closed: `Ty::Fn` is now `tracks_region`, and `region_of` folds a closure's
region over its captures, so that program is rejected at `check`. Zero-capture closures stay
`Static`; closures used entirely within the arena stay legal. When fully-escaping fn values
(return / struct-field / array-element) land, this capture-region fold is the machinery they
must preserve.

### `bytes` / `buffer` — design SETTLED; minimal `buffer` BUILT 2026-07-03; `str.bytes()` BUILT 2026-07-15
**Decision (2026-06-23): `bytes` is `slice<u8>`; `buffer` is a distinct growable owned byte container.** Resolving the two forks left by `draft.md` §12 (which names the types but specs no operations):
- **`bytes` = `slice<u8>`** — a read-only `{ptr,len}` view of `u8` elements (bytes), structurally identical to a slice of bytes (no UTF-8 invariant — that is what distinguishes it from `str`/`string`). Introducing a *separate* structural type would violate **One way** (two names for one thing), so `bytes` is the conventional spelling of `slice<u8>` in byte/I/O contexts, lowered as `slice<u8>`. `s.bytes()` yields a `slice<u8>` view of a string's UTF-8 bytes; `bytes.to_string()` is the UTF-8-validating inverse (`Result<string, Error>`). (FFI already treats `bytes` as a view handed to C by raw pointer — consistent.)
- **`buffer` = a distinct Move type**: an owned, **growable**, mutable sequence of `u8` (the byte analog of a `Vec<u8>`). It is *not* `array<u8>` (fixed length) nor `builder` (an append-only *text* writer that produces a `string`); `buffer` is random-access + growable + freezable raw bytes for the *binary* domain. Ops: `buffer()` / `buffer(cap)`, `.push(b)`, `.append(slice<u8>)`, `.len()`, `buf[i]` read/write, `.bytes()` (view), and freeze → owned `array<u8>` or `.to_string()` (UTF-8 validate). It is the first growable container.
- **Build (was deferred until a consumer) — the minimal `buffer` landed with its first consumer, M9 std.io Slice 1 (2026-07-03).** `Ty::Buffer` is an owned Move handle to a growable heap `Vec<u8>` (`Drop`-freed); the shipped ops are the subset `reader.read` needs — `buffer(cap)` (a read window), `.bytes()` (the `slice<u8>` view, region-tracked to the buffer so it can't escape), `.len()`. The rest of the settled op set (`.push`/`.append`/`buf[i]` read/write/freeze → `array<u8>`/`.to_string()`) is still deferred to its next consumer (`core.hash` / binary parsing) — same "build ahead of a consumer risks the wrong shape" rationale, now applied per-op. `bytes` remains `slice<u8>` (no separate structural type).
  - **The string-side view shipped 2026-07-15.** `str.bytes()` and owned `string.bytes()` are a descriptor-only `{ptr,len}` retype to `slice<u8>`: no allocation, copy, MIR operation, or runtime call. Owned receivers auto-borrow, and region/owner provenance follows the source so a byte view cannot outlive or survive mutation of its backing string.
  - **Binary parsing arrived (2026-07-10, align-LLM runway A2, branch `runway-a2-binary-codec`).** The consumer settled the byte-level op shape as **typed binary decode/encode** rather than the originally-listed raw `.push(b)` / `buf[i]` ops: reads `bytes.<scalar>_<le|be>(off)` and writes `buffer.put_<scalar>_<le|be>(v)` (the endian-explicit typed pair — see the Open → "align-LLM runway" A2 record for the full design), plus `buffer.append(bytes)` for a raw blob. `put_u8` **supersedes** the settled `.push(b)` (a typed single-byte append), and `.append(slice<u8>)` shipped as specified. Still deferred (no A2 consumer): `buf[i]` random-access read/write and `freeze → array<u8>` / `.to_string()` — a growable-then-freeze output is the `loop` / growable-`array<T>` story, not binary parsing.
Record: `draft.md` §12/§18.2, `impl/07-roadmap.md` M9 Slice 1, `crates/align_driver/tests/m9_io.rs`.

### `str` I/O UTF-8 validation — SETTLED + BUILT (2026-07-04)
**Decision: every I/O boundary that produces a `str`/`string` validates its bytes as UTF-8; invalid content fails rather than yielding a malformed `str`.** `draft.md` §7/§12 make "a `str` is always valid UTF-8" a load-bearing invariant (it lets `str` APIs — `chars`, slicing, `find`, display — assume well-formedness), but the M9 std.fs / core.json surfaces produced `str` values directly from raw file/mmap/JSON bytes **without checking**, so a binary file → a malformed `str` → broken invariant (external report, 2026-07-03).
- **Fixed at every `str`-returning entry point:** `fs.read_file` (both the fast `read_exact` path and the copy fallback), `fs.read_file_view` (both the mmap path — validated before the view is registered on the arena, `munmap` on failure — and the arena-copy fallback), and `json.decode` (validate the **whole input once** at the head; a decoded `str` field is a zero-copy substring view into that input, so one pass covers every field — the same one-shot check simdjson does). Invalid → `Error.Invalid` for the `fs.*` errno-mapped surfaces; a decode error for `json.decode` (whose error channel is `Error.Code`).
- **Binary is a separate path:** `bytes`/`buffer` carry no UTF-8 invariant, so binary reads use `reader.read(buffer)` — no validation, no change. Stated in `draft.md` §18.2.
- **`read_dir` non-UTF-8 filenames** (the M9 known caveat): a name that is not valid UTF-8 cannot be a `string`, so the entry is **excluded** from the listing (not an error that fails the whole enumeration, and not lossy retention). Chosen because enumeration is a discovery tool and such a file is unreachable through a `str` path anyway; recorded so the shorter-than-on-disk count is intended, not a bug.
- **Implementation:** Lemire's range/lookup UTF-8 validator (simdjson `utf8_lookup4`) as AVX2 / NEON / scalar paths, dispatched at runtime, differentially fuzzed against `std::str::from_utf8` (isolated continuations, truncated/overlong sequences, surrogates, out-of-range 4-byte leads, block-boundary straddling). Cost is memcpy-class: ~14.8 GB/s SIMD vs ~15.3 GB/s memcpy (97%), ~4× the scalar 3.7 GB/s — so the decode/read paths degrade a few % at most, and the SIMD path is the main one.
Record: `draft.md` §18.2, `crates/align_runtime/src/lib.rs` (`validate_utf8` + `utf8_tbl` + the `fs`/`json` call sites and their tests).

### Ownership syntax
**Decision: ownership is a property of the type, not a keyword.** `array<T>`/`string`/`buffer`/heap are Move; primitives/small structs/`slice` (view) are Copy. No `owned` modifier is introduced. Lifetimes are inferred and lifetime syntax is not surfaced.
Record: `impl/03-types.md` §6–§7

### Owned `mut` cleanup is path-local — SETTLED + DONE (2026-07-15)
**A resource-owning local may be reassigned across heap and arena regions when the normal lifetime
target check passes; MIR drops it only on paths that currently hold an individually owned value.**
Audit item **1-3** originally exposed a double-free: a function-wide exit-drop classification could
mark an `arena → heap` local for `Drop` even on a bypass path that still held the arena pointer. The
2026-07-10 fail-closed gate temporarily pinned owned locals to their initialization region. #463
replaces that restriction with the final path-local rule:
- **Owned Move locals** — checked HIR classifies every produced value as individually owned or
  arena-managed. MIR maintains an internal boolean flag per resource-owning slot, transfers it on
  moves and destructuring, updates it on assignment, and branches around each `Drop` unless the flag
  is true. Region-changing reassignment is therefore legal without a leak or double-free.
- **Copy region-bearing views** (`str`/`slice`/`bytes`) — no drop flag is needed; escape analysis
  still joins paths conservatively and keeps the shortest region the binding may hold, preventing a
  bypassed arena borrow from being returned or stored into a longer-lived target.
Initialization, return/early cleanup, loops and `break`, tuple destructuring, `match` payloads,
direct moves, and owned self-assignment are regression-gated. Mixed-region value expressions retain
their runtime ownership bit, so taking an individually owned branch no longer leaks merely because
the joined escape region is shorter.
Record: `crates/align_hir`, `crates/align_sema` (`EscapeCheck`, `Stmt::Assign::drop_new`),
`crates/align_mir` (owned-slot drop flags), `tests/reassign_drop.rs`, `draft.md` §4.

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

### Masked reducing `where` — vector shape + callable correctness SHIPPED (2026-07-13)

**Audit correction shipped:** the 2026-07-02 implementation established a valuable identity-select
vector shape, but overextended it to user callables and post-`where` stages. A Pure callable can
still divide by zero, fail a bounds check, allocate/OOM, or fail to terminate. Reducing MIR now keeps
mask/select only for safe field operations plus builtin `sum`/`count`/`min`/`max`; a general
post-`where` callable is control-flow guarded. Sequential Impure callables are normatively allowed
with exact guarded source order. Full reproduction, legality split, and regression gate:
[`impl/12-pipeline-closure-memory-io-simd-audit.md` §3.1](impl/12-pipeline-closure-memory-io-simd-audit.md#31-fixed-2026-07-13--where-guards-later-callables-and-callable-reducers).

**Historical shipped shape:** a `where`/`where(.field)` feeding a reducing terminal lowers by ANDing the
predicates into a `mask`, then `select` each masked-out lane to the reducer's identity instead of a
per-element branch (`Rvalue::Select` + `accumulate_mask` in `align_mir`). Fixed identities:
`sum`/`count` → `0` (`acc += mask ? value : 0`, `count += mask ? 1 : 0`), `min` → `+∞` / `max` → `−∞`
(the `extreme_of` fold seed), `any` → `false` / `all` → `true`. Generic `reduce` has no identity for
its user `f`, so it uses the **accumulator-select** form `acc = mask ? f(acc,v) : acc` (a masked-out
lane leaves the accumulator unchanged). `min`/`max` also moved from a compare-and-branch update to
the `select(cur `cmp` acc, cur, acc)` idiom, so the plain (no-`where`) path is branch-free too — one
lowering, no dual mechanism. For the builtin identity operation itself, results are byte-identical to
the branch form: same ordered comparison
(NaN elements still skipped by `min`/`max`), same empty-selection result (`min`/`max` → the extreme
seed, `reduce` → `init`, `any` → `false`, `all` → `true`). `dot` is out of scope — `a.dot(b)` is a
two-array kernel with no `where`, already branch-free. Generic reducers and `any`/`all` now execute
only on surviving elements. **Why the safe identity-select shape matters:**
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

**Historical scope note (superseded 2026-07-02):** Gap A below records the earlier lambda-only guard,
not the current language contract. The later settled rule rejects string `+` everywhere and uses
`builder.to_string()` as the one construction path. Audit 13 records the sema/MIR hard cutover and
test migration completed on 2026-07-15.

- **Math pipeline (`map→where→sum`): Align 1.15–1.27× FASTER than Rust on M2 — a positive confirm.**
  The branchless-`select` fusion wins on arm64 (on x86 it was parity — Rust's slice `filter` evidently
  doesn't vectorize as cleanly on arm here). Nothing to do; good signal that the flagship lowering
  holds cross-arch.
- **★ Gap A — `str + str` inside a lifted lambda silently LEAKED → OOM. FIRST GUARD 2026-06-27;
  UNIFORM CONTRACT FIX 2026-07-15.** `s.reduce("", fn acc, x { acc + x })`: the lambda lifts to a top-level fn whose `lower_fn`
  starts with `b.arenas` empty, so `str+str` (MIR ~757) got `arena = None` → `builder_finish`
  `Box::leak`d the buffer (runtime ~1196) → one leak per reduce step → OOM at N=10k. **Fix:**
  `guard_lambda_alloc_leak` (align_sema) historically errored on a string allocation (`str + str` / `template` /
  `json.encode` — all desugar to an arena `Template` str) inside a lifted lambda with no arena of its
  own (`capture.is_some() && arena_depth == 0`), pointing at the `builder` pattern — so the silent
  leak became a clear compile error. That first guard deliberately left named/top-level and
  inner-arena concatenation untouched. Arena-free dynamic templates gained hidden scoped owners on
  2026-07-15, so locally consumed templates in lifted lambdas are now safe; returning their
  frame-bounded views remains rejected. The settled One-way rule later chose `builder` instead of an
  owned-concat surface; on 2026-07-15 sema made the rejection uniform and the obsolete MIR
  concatenation lowering was removed. The named-reducer sub-gap is therefore closed by the same
  hard error, while builder-reduce remains valid. `tests/lambda.rs` pins all three former contexts.
- **Gap B — `acc + x` string reduce would use O(N²) arena space; MOOT under the hard error.** Arena has no
  per-object free, so all N intermediate strings live until block exit (Rust frees each `acc`
  immediately → O(1)). The uniform compile-time rejection makes this shape unrepresentable;
  `builder` accumulation is the single supported path, so no separate lint remains.
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
  **Reference pointers (dict/std.collections, recorded 2026-07-04, external design-note review
  adoption; the string rail already rides `std::collections::HashMap`'s hashbrown Swiss table
  indirectly — this integer rail's linear-probe table is the replacement candidate):** Swiss
  Tables/F14 (SIMD metadata probing) + Eytzinger branchless search + FAST + AMAC prefetch chaining +
  cache-oblivious algorithms + the 7-dimensional hashing-scheme analysis (L1-resident metadata).
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
  const global), where idiomatic Rust needs `const fn` (float/alloc-limited) or a build script.
  **Prerequisite now met (2026-07-17):** scalar/`str`-array **aggregate constants** shipped (S1 above),
  so a top-level const *table* is expressible directly (`TABLE := [ … ]` → `slice<T>` over a private
  rodata global; a constant index folds to the element). Remaining: a **struct**-element table (S1.5)
  and folding a `[…].map(f)` *initializer* into the table (element positions today take folded scalar
  expressions, not a pipeline). Confidence: high (folding + rodata lowering observed). Win is for
  table-driven code only.

**Audit — ruled out a risk (2026-06-27):** Align has no loops (map/reduce + recursion), so tail
recursion *must* match a Rust loop. Verified: `fn sum_to(n, acc) = if n==0 {acc} else {sum_to(n-1,
acc+n)}` compiles to a **call-free 14-instruction tight loop** (`run(1e6)` correct) — LLVM converts
the tail recursion to a loop at O2. So the loop-less design is not a perf liability for tail-recursive
algorithms. (Superseded 2026-07-09: sequential control is now the `loop` expression and the spec
guarantees no TCO — see Settled → "Sequential control"; this audit stays as the record that LLVM
*did* optimize the old tail-recursive form.)

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
    layer — after core; records the *direction*.) Reference pointer (csv, recorded 2026-07-04,
    external design-note review adoption): simdcsv-style structural scan.
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
        reused across JSON/HTTP/CSV/HTML/tokenizers since they share one byte-classifier).
        **Reference pointers (recorded 2026-07-04, external design-note review adoption):**
        Parabix-class parallel bit streams (transpose bytes into per-bit SIMD streams, then boolean
        ops) as prior art if a regex/XML-class engine ever outgrows the shared byte-classifier;
        for UAX #29 grapheme segmentation, pure SIMD cannot handle ZWJ sequences — the
        state-of-the-art shape is SIMD as an ASCII/fast-path prefilter over a scalar general path.
        Keep
        Unicode (`chars`/grapheme/normalization/case-fold) explicit and mostly package-level, out of
        core v1. Builder is ~0.55× of optimized Rust — batching adjacent static/template appends into
        fewer runtime calls (a `write_many` internal ABI) is the lever. Probe:
        `work/string_processing_probe.rs`; advice `work/string-processing-findings-2026-06-28.md`.
      - **LLVM-version gap + upgrade as a perf-roadmap item (codex modern-CPU advice 2026-06-28).**
        Align rode **LLVM 19** (inkwell 0.9, `llvm19-1`) through M13; the post-M13 upgrade checkpoint
        moved it to **LLVM 22** (inkwell `llvm22-1`, llvm-sys 221) — now matching rustc 1.96's LLVM,
        so the backend sees the newer target features (x86 `avx10.1/.2`, `apxf`,
        `amx-*`; aarch64 `sve2`, `sme2`, `i8mm`, `bf16`, `fp8`). Division of labor: **LLVM** does
        instruction selection / new ISA legalization / vectorizer + cost model (so APX is "free" once
        the backend targets it — just keep emitting clean optimizable IR); **the runtime** does
        feature-detect + function-multiversioning like Rust crates. Plan: short-term AVX2+NEON runtime
        dispatch; **the LLVM/inkwell upgrade checkpoint is DONE** (LLVM 19 → 22, post-M13, 2026-07-12
        — guarded by the M13 bench + IR/behavior tests, which caught two vectorizer-behavior shifts and
        one attribute-spelling change; all re-pinned to verified-equivalent shapes) so the backend can
        now target AVX10/APX/SME2 seriously; long-term treat LLVM upgrade as part of the *performance* roadmap,
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
  shape + copy elimination (unaligned AVX2 loads were within ~0.95–1.0× on this host); **blanket
  branchless-ification of scalar control flow** (recorded 2026-07-04, external design-note review
  adoption: TAGE-class predictors make predictable branches nearly free, while CMOV/select chains add
  data dependencies that stall — matches the measured dense-bitset `where` value-sum loss above;
  branchless is for vectorized kernels and `std.crypto` constant-time code, not a general style).

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
    direction (and the Profile-guided perf-lints bullet above). **Reference pointer (recorded
    2026-07-04, external design-note review adoption):** Futhark's defunctionalisation (Hovgaard
    et al., 2018) — compile higher-order functions to static data + inlined dispatch so pipelines
    stay vectorizable; prior art for the thunk-erasing specialization (Align's non-escaping lambdas
    already take the same shape: inlined, captures-as-parameters).
  - **group_by wants *three* strategies, not one hash table.** `work/group_sort_probe.rs` (verified,
    1M rows): **dense-id array aggregation 5.8 ms vs std HashMap 63 ms (~11×)** when keys are a dense
    integer range; **sort-group (24 ms) beats hash (63 ms) at 1M distinct** (high cardinality / already
    sorted). So the columnar `group_by` runway is: dense-id/dictionary path → SwissTable for general
    high-cardinality primitive keys → sort-group for very-high-cardinality or pre-sorted, with
    diagnostics ("key is a dense integer range — use dense group_by"; "string key in a hot group_by —
    dictionary id"). Extends the Dictionary-id rail + SwissTable bullets with the sort-group third leg.
    **Reference pointer (sort, recorded 2026-07-04, external design-note review adoption):** VQSort
    (VLA-portable vectorized sort) as prior art for a vectorized sort-group implementation.
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
a lambda-only guard). `builder` (`.write`/`.to_string()`) is the one way to build a string incrementally.
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
**Decision (2026-06-22): there is no expression-position type-argument syntax.** A call's type parameters are recovered by inference — from a value argument (`json.encode(u)`) or from the expected type propagated from context, including back through `?` (`u: User := json.decode(d)?`). When neither supplies the type it is a hard error directing the user to annotate the binding; an explicit `f<T>(x)` / `f::<T>(x)` form is **not** adopted. Rationale: keeps "one way" (the binding annotation is the single place a type is written), removes the `<` vs comparison parse ambiguity at expression position outright (the reason Go uses `f[T](x)` and Rust `::<>`), and is friendlier to generate. The headline case — `draft.md` §19's `json.decode<array<User>>(data)` — therefore becomes `users: array<User> := json.decode(data)?`; the checker already takes `decode`'s target from the expected `Result<T,_>` and emits an annotate-the-binding error otherwise (no code change needed — only the spec/comment caught up). **Residual — CLOSED 2026-07-18 (the JSON-completeness design):** the schema-selector builtins `json.validate<T>` / `json.field_table<T>` are **deleted from the catalog** (validate folds into decode-and-discard; field_table is compiler-internal), so no `<T>`-only surface remains there; the one surviving schema-selector is the streaming scanner (`rows: json.scanner<Row> := json.scan(view)`), whose type comes from the binding annotation exactly like `decode` — consistent, no new syntax. This rule scales to general generics (below): a return-only type parameter is supplied by the binding annotation, never a turbofish. Record: `impl/02-frontend.md` §8 (generics `<` vs comparison), `draft.md` §18 (core.json), `language-spec.md` (JSON).

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
- **1-3 — FIXED (2026-07-10), restriction relaxed by #463 (2026-07-15).** `arena { mut xs := […].to_array(); xs = make() }` double-freed because the `to_array` arena-bump result and function-wide `drop_old` / arena bulk-free classification did not reconcile. The first gate pinned an owned `mut` binding's region; #463 replaced that temporary restriction with path-local MIR drop flags, so region-changing owned reassignment is now legal when its lifetime target is valid. Copy views still join to the shortest possible region. See Settled "Owned `mut` cleanup is path-local".
- **3-1** `&&` / `||` are **not short-circuit** — MIR lowers them as a strict `Rvalue::Bin`, so `i < len && arr[i]` still evaluates `arr[i]` and can trap. (Confirms the audit's "requires-verification" item.)
- **2-1** a type-annotated `let` at an `if`-body head (`if flag { x: i32 := 5 … }`) misparses as a struct literal (no `no_struct_literal` context flag on the condition).
- **2-2** `x as u32 < 5` won't parse (`parse_type` greedily eats `<` as generic args; a cast target is always a concrete primitive).
- **2-3** two statements with no separator (`{ x := 1 return x }`) are silently accepted (weak statement-boundary check).
- **2-4** deep nesting (50k parens / 100k unary `-`) overflows the parser stack (exit 134); needs a recursion-depth limit that errors.
- **2-5..2-8** diagnostic-quality: `1e999` silently becomes `inf` (no diagnostic); a non-ASCII identifier reports byte-wise garbage; the internal `enum#0` leaks into a type-mismatch message instead of the source name; a trailing `\` before EOF emits a doubled/misleading error.

**1-1 (found + FIXED after all):** the `-5` → `4294967291` sign loss reproduces when a **negative literal is given an unsigned type by context** — `x: u32 := -5`, or `g(-5)` into a `u32` parameter — which `check` silently accepted, wrapping `-5` to `4294967291`. Root cause matches the audit's "finalize-without-bind" guess: unary negation's signedness was never validated against the (later-inferred) unsigned type. Fixed by rejecting **unary `-` on an unsigned type** at finalize time (a negative value cannot have an unsigned type; cast explicitly for the wrapped pattern). Unsigned *subtraction* `a - b` stays a defined wrap; `(-5) as u32` stays a sanctioned conversion. `tests/numeric_cast.rs`, `draft.md` §3.

**Structural follow-ups (design-level, from audit §6):**
- ~~Move escape/region checking off recursive per-`ExprKind` re-evaluation onto a **flow-sensitive sema CFG pass**~~ **DONE through #461–#464 (2026-07-15).** The dependency audit corrected the older "MIR pass" placement: safety diagnostics and cleanup provenance stay at the checked-HIR boundary. #461 made expression/type provenance classification exhaustive; #462 introduced the finite joined `EscapeState`; #463 added path-local MIR ownership flags; #464 now lowers the exhaustive HIR walk into compact basic blocks and explicit `if`/`match`/`else`/loop/`break` edges. One shared worklist owns all joins and fixpoints, while diagnostics replay once in syntax order from fixed block inputs. New control-flow syntax must choose its CFG edges in the exhaustive match and cannot silently inherit a recursive fallback.
- ~~Record purity as an **effect bit on the function type**, not a name-based propagation result, so fn-value / closure / FFI-pointer indirection can't dodge it (keeps "purity is inferred"; only stores the result in the type).~~ **DONE in #465 (2026-07-15).** Concrete `FnTy` entries carry `Pure` / `Impure` / `Unknown`; a least fixpoint refines named functions, lifted closures, mutable local joins, imported summaries, and FFI pointers. Indirect calls and `map_err` consume the type bit, unresolved HOF parameters stay fail-closed, and signature equality excludes the inferred mutable fact.
- ~~A spec table of **value-carrying control structures** (block / `if` / `match` / `else`-unwrap / `?`): for each, how the region is composed and how an owned value moves/drops — with 1:1 tests.~~ **DONE as #466, 2026-07-15.** `draft.md` §6.3 is the exhaustive 5×2 table and `tests/value_control_flow.rs` pins one region cell plus one owned-cleanup cell per form. The audit found four real cleanup gaps: `if`, `match`, `else`-unwrap, and `?` selected the value but lost its runtime individual-vs-arena bit at the join, so a heap-owned selected path could leak when another path was arena-owned. Checked HIR now records static allocation provenance per expression, and MIR carries a parallel bit through the same result CFG slot; direct local moves still load their path-local flag. `block` and all five region cells were already correct.
- ~~Stand up **fuzzing** (parser / JSON / fmt, with a depth cap) and a **negative-test corpus**~~ **(DONE, #286–#290)** — dependency-free fuzz + property suite in `crates/align_driver/tests/` (SplitMix64 + `catch_unwind`, seeds printed, runs as `cargo test`): `fuzz_frontend.rs` (lexer/parser/sema never panic, incl. non-ASCII), `fuzz_fmt.rs` (formatter never panics + idempotent + parse-preserving on all examples), and `fuzz_differential.rs` — a **generate-program-with-its-oracle** differential fuzzer that catches *miscompiles* (the array-garbage class) across scalars, all integer widths + cross-width casts, the call ABI, struct/array aggregates, and (wave 2, #326 → this wave) `map`/`where`/reduce pipelines, `vecN<T>` lane arithmetic, **Option `else`-unwrap + Result `?`-propagation chains** (both the Some/Ok and None/Err arms forced across the seed range), **enum + exhaustive `match`** (mixed tag-only / scalar-payload variants, per-variant base so a mis-tag or mis-read payload both surface), and **nested-struct read/write chains** (depth-2..3 towers with a randomly-positioned nested field, exercising #307 field reordering at every nesting level). A single `wrap(v, ITy)` models both arithmetic wrapping and integer casts; a per-test mutation check (deliberately `+1` the oracle) proves the harness isn't a vacuous pass. No miscompile found. The negative-test corpus is `tests/analysis_coverage.rs` + the audit repros. The value-carrying-control-flow matrix above is now complete.

Record: `crates/align_sema` (the analyses), `tests/analysis_coverage.rs`, `align-self-review` Gate 1.

**Borrow-liveness gap — FIXED as #460 (2026-07-15).** Region analysis already tracked *where* a borrow
may point, but did not invalidate a frame-local view when its owning source generation ended. The
result was a use-after-free/use-after-close after moving or reassigning a `string`, `buffer`, CLI
`parsed`, `tcp_conn`, HTTP `response`, or `array<response>` source; a stale socket reader/writer was
especially dangerous because an fd could be reused for an unrelated connection. `MoveCheck` now
has one shared, flow-sensitive borrow state alongside its move state: borrow-producing expressions
flatten their provenance to owner locals, every consuming/replacing operation invalidates dependent
views, borrower reassignment establishes a fresh generation, branches join only fallthrough states,
and loop heads compute a finite may-state fixpoint. Buffer operations that may reallocate
(`append`, scalar `put`, and `read_line`) invalidate existing byte/string views as well. Provenance
distinguishes a view of an owned container's slots from copied view elements in a materialized
array/SoA, avoiding both the `rs[0].body()` hole and false invalidation when a primitive SoA has
already copied its source. Regression coverage spans direct/chained/call/pipeline-produced views,
aggregate fields and response arrays, branch/loop joins, re-borrowing, diverging paths, reallocating
mutation, diagnostics, and safe materialization cases in `tests/borrow_liveness.rs`. The broader
escape-flow structural refactor subsequently completed at the checked-HIR boundary through
#461–#464; this fix supplied the neighboring borrow-state dataflow rather than that migration.

**Borrow liveness ends at MOVE, not at scope-end DROP — FIXED (found 2026-07-21 adversarially while
implementing `ctx.headers()`; fixed 2026-07-22).** #460's shared borrow state invalidated a view when
its owning source was **moved or reassigned** (`invalidate_storage` / `invalidate_owner`). It did
**not** notice the other way a source's storage ends: an owned local bound inside a `loop` body being
**dropped when the iteration closes**. `Region::Frame` cannot help, because it does not distinguish
"this frame" from "this iteration". So a view assigned out to a longer-lived local survived into the
next pass and read freed memory — no `unsafe`, no std handle required:

```align
mut keep: str := "start"
loop {
  print(keep)              // read the PREVIOUS iteration's freed buffer (printed heap garbage)
  owned := mk("hello")     // dropped at end of iteration — never moved
  keep = owned
}
```

**The fix mirrors MIR's actual drop points, and an iteration frees TWO kinds of storage — the second
was missed by the first cut and caught by the adversarial review.** `emit_drop_if_live` is emitted
at: function exit (nothing follows), an `Assign`'s drop-of-old (already covered by the move path),
a hidden owner after a **scalar-only** consumer (a view-returning consumer keeps that owner alive),
and the `loop` per-iteration set at the **back-edge** and at **every `break`**. That set is *not*
just `drop_locals ∩ body_locals`: `Builder::new_synthetic_owner` also pushes the hidden owner of
every **unbound Move temporary** into the innermost loop frame, and `lower_loop` deliberately
re-reads the final frame at the back-edge for exactly that reason. A temporary has no `LocalId`, so a
rule keyed on locals cannot see it:

```align
mut keep: str := "start"
loop {
  print(keep)
  keep = "AAAA…".clone()   // the clone is a temporary — freed at the back-edge and at `break`
}
```

`MoveCheck` therefore ends **both** at every iteration edge. `BorrowRoots` became a set of
`BorrowRoot::Local(id) | IterTemp(depth)`: the named half comes from `iteration_drops` via the shared
`needs_drop_flag` predicate that builds `Fn::drop_locals`, and the anonymous half from
`temp_owner_root`, which applies MIR's own materialization condition — `needs_drop_flag` **+
`may_need_synthetic_owner`**, the latter moved into sema so both stages share one definition. Depth
is all the identity a temporary needs: every temporary at a given loop depth is freed by that loop's
two edges. Outside any loop the hidden owner lives to function exit, so no root is recorded there.

**Where that root is attributed matters as much as the condition, and getting it wrong cost a false
positive** (caught by the second adversarial round). MIR mints the hidden owner only where a fresh
Move value is *borrowed* (`lower_borrowed_owned`); a value **moved** into a local that owns it
transfers its storage to that named local instead, and nothing joins the loop's drop set. Attributing
the root in `borrow_sources` — reached by every consumer — therefore rejected the ordinary
`names = src.map(up).to_array()` rebuild-each-pass idiom, whose `array<str>` is owned by `names` and
whose element views point at an outer `src`. The root now comes from `storage_roots`, which *is* the
borrowing position: every borrow producer (a `str`/slice borrow, a view-producing method receiver, a
`str`/slice call argument — the coercion node wraps those — and a `json` input) routes its operand
through it, while a materializing consumer recurses through `borrow_sources` and gets nothing. The
two functions now carry that distinction explicitly, and the json arms were routed to `storage_roots`
for the same consistency (behaviour-neutral, verified: every json operand is already `str`-typed or
coercion-wrapped).

**The paired invariant, and the hole that proved it: `storage_roots` must be transparent through
exactly the constructs `may_need_synthetic_owner` is transparent through.** That predicate recurses
into a `{ }` / `unsafe { }` block's value — a block whose value is a bound place borrows the place
and mints no hidden owner — but `storage_roots` had no block arm, so it fell to a `borrow_sources`
tail that short-circuits on an owned, non-borrowing type. A block therefore recorded **no root at
all**: not `IterTemp` (correctly not a temporary) and not the place's `Local`. `keep = { inner }`
walked past the entire rule and printed freed heap bytes, two characters from the rejected
`keep = inner` — and it had slipped every earlier revision of this fix. The decision is now
**single-sourced** in `borrow_transparent_value`, which both consumers call, so adding a wrapper
updates them together instead of relying on a comment. `arena {}` / `task_group {}` are transparent
in *neither*, so they stay covered by the `IterTemp` path. **Remaining structural weakness, recorded
not fixed:** both functions still end in a `_` catch-all, so a future variant that *should* be
transparent compiles fine while being neither — the "new IR variant skips a pass" class. Making
`storage_roots` dispatch bound places through an exhaustive, wildcard-free helper would compile-force
it; that belongs with the structural follow-up below, not bolted onto this slice.

**A third over-rejection, pinned** (`over_rejects_a_control_flow_borrow_over_outer_bound_places`):
`may_need_synthetic_owner` is conservatively `true` for the wrappers whose *runtime* value can still
be a bound place — `if`, `match`, `else`-unwrap, `arena {}`, `task_group {}` — so
`keep = if c { a } else { b }` over sources declared outside the loop mints a spurious `IterTemp`.
`emit-mir` shows the owner's temporary flag stored `false` on every bound-arm path, so no drop is
emitted at either edge. Same family as the arena and chunks pins: a static shape predicate in sema
against a per-path runtime flag in MIR that `MoveCheck` cannot see. The workaround (borrow the arms
as `str` views first) is accepted and runs.
The invalidation runs on the state that reaches the loop head (probe pass + every fixpoint round) and
on each `break` snapshot — the latter to the snapshot only, since the statements this pass still
walks after a `break` belong to the iteration that has not dropped yet. `BorrowState::invalid`
records *how* each generation ended (`BorrowEnd::Consumed` / `Dropped`) so the diagnostic says which,
and names the source when it has a name; the join merges per root by `Ord` (`Consumed` < `Dropped`),
keeping it commutative for the fixpoint.

**Two claims in the original write-up were wrong, and the corrections matter more than the bug.**
① "The inner scope need not be a loop — an `arena {}` block does it too" is **false**, in both
directions: a heap-owned local bound inside `arena {}` is *not* freed at the block's end (MIR drops
it at function exit — `emit-mir` shows `drop _3` after `arena_end`, and the shape prints the correct
string, i.e. nothing was freed), while storage that genuinely *is* arena-allocated is already
rejected by the region rule's `decl_depth` check ("this value is bound to an arena block and cannot
escape it"). The `arena` shape "printing a plausible answer" was not allocator luck — it was a
program with no use-after-free in it. ② The `http_headers` case was described as louder than the
`str` case; both are the same shape- and allocator-dependent UB, and the `str` case reproduces on a
plain `string` with no std handle anywhere.

**The one cost, pinned as a test rather than hidden: an over-rejection on arena-owned loop locals.**
The rule keys on the *type* predicate `needs_drop_flag`, because `MoveCheck` runs **before**
`EscapeCheck` and so cannot see the individual-vs-arena ownership bit. An array allocated inside an
enclosing `arena {}` is arena-owned — its drop flag is never set, MIR's back-edge drop folds away,
nothing is freed until `arena_end` — yet a view of it assigned out of the loop is now rejected:

```align
arena {
  mut keep: slice<i64> := [7, 7, 7][..]
  loop { xs := [1, 2, 3].map(…).to_array(); keep = xs[..]; … }   // safe, but rejected
  print(keep[0])
}
```

The same shape with a **heap**-owned source (a `string`, malloc'd even inside an arena — verified by
running it) is a genuine use-after-free that must stay rejected, and the two are indistinguishable to
a type-level predicate. Conservative is the right side to err on (inconvenience, not breakage), and
approximating the ownership bit inside `MoveCheck` would be a second mechanism for something that
already has one. **The real fix is the structural follow-up recorded above**: borrow liveness belongs
in the checked-HIR escape CFG (#461–#464), which already carries regions, allocation provenance, and
loop fixpoints. Pinned as `over_rejects_a_view_of_an_arena_allocated_loop_local` in
`tests/borrow_liveness.rs`; flip it when the analysis moves.

**A second, pre-existing over-rejection the loop rule widened**, also pinned
(`over_rejects_a_view_into_the_source_of_a_dropped_chunks_header`): `ch[0]` is a `{ptr,len}` view
into the *source* array, not into the chunks header, yet `local_owns_view_storage` counts a
`DynSliceArray` local as owning its elements' storage — so ending the header's generation invalidates
the element views. Rejected on plain reassignment long before this change; the loop rule only made
the common shape hit it. The fix belongs with `local_owns_view_storage` (which owned containers
actually own the bytes their elements view — a type-class question), not with borrow liveness.

The variant where the loop **consumes** the handle (`ctx.respond(rb)?` — the pkg.web shape, so every
shipped serve loop was always safe) was already rejected by the move path. Coverage: the flipped
`a_view_of_a_handle_dropped_at_the_end_of_an_iteration_is_rejected` (was
`known_hole_scope_end_drop_…`) in `tests/http_headers_view.rs`, plus eleven tests in
`tests/borrow_liveness.rs` — back-edge, `break` edge, all four temporary-materializing shapes (a
call, a view-returning call *over* a temporary, `?` on one, a materialized array sliced in place),
a `json` view over a temporary and over a dropped loop-body input, six block-laundering shapes
(bare / declaration-inside / `unsafe` / through a call / nested / a struct field reached through
one), and the controls that keep the rule from over-rejecting: a block over a source that outlives
the loop, a fresh value **moved into an owning local** (array and Move-struct forms),
same-iteration use of a local and of a temporary, a temporary outside any loop, a source declared
outside the loop, an inner `break` that must drop only the inner body's locals, and an owned local
*moved out* by `break` (its flag is cleared, so it is not a freed source). Mutation-checked in six
places — neutering `invalidate_iteration_drops`, `temp_owner_root`, the `IterTemp` arm of the edge,
the `storage_roots` attribution, or the block-transparency arm, and *restoring* the attribution to
`borrow_sources` — each fails exactly the corresponding tests and nothing else.

**Borrow PROVENANCE was fail-open where borrow LIVENESS was complete — FIXED as #621 (2026-07-23).**
The rule above was right and still never fired for three expressions, because `borrow_sources_inner`
ended in `_ => BorrowRoots::new()`: a form it did not name reported "borrows nothing", and a rule can
only invalidate what provenance reports. ① **`template`** is the one expression whose value views
storage the expression *itself* allocates — MIR mints a hidden owned `string` at the node while the
value's type is `str`, so `temp_owner_root`'s `needs_drop_flag(e.ty)` half was structurally blind to
it, and `region_of(Template) = Frame` blocks a `return` but is not provenance. It is also the one
hidden owner minted **unconditionally** rather than only in a borrowing position, so its root belongs
in `borrow_sources`, not in `storage_roots` where every other temp root lives. The condition is now
single-sourced in sema's `owns_hidden_string(e, in_arena)`, which MIR's lowering calls, and
`MoveCheck` mirrors MIR's arena stack — inside an `arena {}` no owner is minted, and without that
mirror the idiomatic arena-scoped accumulator (the correct way to write the rejected loop) would be
rejected too. ② **`EnumValue`** — the one aggregate constructor that did not forward its payload's
provenance, unlike `StructLit` / `Tuple` / `OptionSome`. ③ **`RandSample`** — a sampled `array<str>`
holds views into its source, the `.to_array()` shape, which does forward. `json.encode` needed no
rule: it desugars to `Template`. Neither ② nor ③ had been reported; **both fell out of closing the
tail**, which is the argument for closing a fail-open classification rather than its instances.
`borrow_sources_inner` is now exhaustive over all 216 `ExprKind` variants with no wildcard
(compiler-forced: deleting one variant yields `E0004`), the 145 previously-swallowed variants
classified in two documented groups. `Loop` is in the safe group *for a reason worth keeping*: its
value is provably `Static` because `check_break_escape` rejects breaking a view of local storage out
of a loop. `emit-mir` over all 213 repo `.align` files is byte-identical, and the legal cases —
same-iteration use, literal-only holes, `t := template "{h}"` then `h = …` (a template COPIES its
holes), builder writes, nested loops, arena scoping — are pinned as tests, each mutation-checked.

The adversarial review added three hardening items worth recording as *rules*, not incidents.
**(a) An exhaustive variant list still leaves its justification fail-open.** The ~130 arms justified
by "this result type never borrows" are right today — verified by replacing that arm's body with
`assert!(false)`: the whole suite stays green and no corpus file panics, so they are reached zero
times — but nothing would go red if `ArrayBuilderBuild`'s element rule, `RandShuffle`,
`EncodingDecode`, or an `HttpResponseBuilder` type later became borrow-capable. They now end in a
`debug_assert!(!ty_may_borrow(…))`, which turns exactly that future change into a test failure.
**(b) `MoveCheck::arena_depth` counts `arena` ONLY**, deliberately unlike the two identically-named
region counters in the same file, which also count `task_group`: MIR keeps task groups on a stack
separate from `Builder::arenas`, so a `template` inside one still gets its hidden owner and still
dies on the enclosing loop's edge. Harmonizing the three counters would silently re-open the
use-after-free with only the region rule left to catch it — pinned by a `task_group` row.
**(c) `borrow_sources` recurses through an `Arena` node without entering that depth** — the single
place the sema/MIR mirror is not lexical. It errs strict (a root can only reject) and the escape
check rejects those programs anyway, so it is left as-is with a comment, because the "obvious fix"
is the unsound direction. The temporary-root diagnostic also gained the two escapes it was missing:
it used to say only "bind the owned value to a local declared outside the loop", which a `template`
user cannot do (its owner is hidden), and now names `.clone()` and an enclosing `arena` as well —
still one message per fact.

**Wrapper-hidden local-slice escape through a function return — FIXED as #459, 2026-07-15 (found in the
#406 review).** `fn f() -> Result<slice<i64>, Error> { xs := [1, 2, 3]; return Ok(xs[..]) }`
previously passed `check`: a frame-local array's slice escaped inside a `Result`/`Option` wrapper,
creating a use-after-free. The bare form was already rejected via `slice_is_local`, but that check
was not wrapper-transparent. The fix keeps local-storage provenance separate from `region_of` and
makes it type-transparent at `check_return_escape` / `check_break_escape`: `Option`/`Result`, tuple,
struct, call, and value-carrying control-flow forms recurse to their slice-bearing payloads. A local
bound or reassigned to such a wrapper retains the provenance; tuple destructuring and `match`
payload bindings propagate it to slice-bearing locals. This deliberately does **not** fold
frame-local slices into `region_of`: that alternative over-rejects a safe slice of an arena-local
array that leaves the inner arena but remains within the function because of the existing
`arena(0)`-vs-`Static` distinction. Negative tests cover direct `Ok(xs[..])`, a wrapped local, and a
`match` payload; a caller-provided slice wrapped in a local `Result` remains returnable.

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
  in passing so it isn't "fixed" by accident. **Arena-zeroing correction (audit 2026-07-13):** the
  earlier claim that JSON missing fields make the full 64 KiB zero-fill load-bearing is stale; the
  current known-schema decoder rejects missing declared fields and writes every semantic field on
  success. Blanket zeroing still must not simply be deleted: move a callsite to uninitialized storage
  only after proving every semantically readable/copied/FFI/drop byte or field is initialized first,
  while unwritten capacity/padding is never covered by an initialized bulk view. Full gate in
  `impl/12-pipeline-closure-memory-io-simd-audit.md` §6.2.

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

### M10 scope decision (2026-07-04)

Settled ahead of any `std.encoding`/`std.rand`/`std.cli` implementation (`impl/07-roadmap.md` M10;
full API shape in `draft.md` §18.2):

1. **Scope = `std.encoding` / `std.rand` / `std.cli`.** All three are pure Align surface over
   already-existing mechanisms — `str`/`bytes`/`buffer`, `mut` slice, `main(args: array<str>)`'s
   `array<str>` — with **zero new Move types, zero new effects, no concurrency, and no FFI engine**.
   `rand.seed`'s OS `getrandom` call is the only new runtime primitive this milestone adds.
2. **`std.net` / `std.http` / `std.process` / `std.compress` / `std.crypto` are explicitly deferred
   to M11+.** Each needs a new Move type (socket / child-process handle), an FFI engine (TLS,
   `libzstd`/`zlib-ng`), or an unsettled design question (`process.exit`'s Drop semantics,
   constant-time verification for crypto) — heavier ground than a scope-closing milestone should
   carry. encoding/rand/cli establish the std-module footing first, individually, before those.

### `process.exit` Drop semantics — SETTLED + BUILT (M11 std.process Slice 1, 2026-07-06)

Design settled in `docs/impl/std-design/process.md` and now **shipped** (M11 std.process Slice 1,
branch `feat/m11-process-slice1-exit`): **`process.exit(code)` runs the current function's pending
cleanup first — Drops for live owned locals (buffered writers flush + close), arena / `task_group`
ends — the exact emission a top-level `return` uses, THEN calls libc `exit(code)`.** "One way": exit
is not a second, Drop-skipping shutdown mechanism; it reuses `emit_exit_cleanup`. The immediate
hard-exit that skips all cleanup is the separately-named `process.abort()` (`_exit(1)`, no Drops/
flushes) — the default is the *safe* one (no silently lost buffered output), the dangerous one is
named. There is no `Never` type yet, so both are typed `()` (they diverge in MIR — cleanup + runtime
call + `Unreachable` — but the type system does not model the divergence, so `exit`/`abort` cannot be
the tail value of a non-unit-returning function; use them as statements). **v1 gap (recorded in
`process.md`):** only the *current* frame's cleanup runs — a full multi-frame stack unwind running
every caller's Drops is the documented ideal, deferred. (Was Open, target M11; the two candidates —
run-Drops-then-exit vs. an immediate hard-exit API — both landed: the first as `exit`, the second as
`abort`.)

---

### Sequential control — the `loop` expression (SETTLED 2026-07-09; IMPLEMENTED 2026-07-10)

**Decision: one narrow `loop` expression; no `for`, no `while`; recursion is not iteration.**
Surfaced by an external design discussion (Codex) on loop elimination: the spec banned counting
loops in one parenthetical and said nothing else — sequential control was a design *vacuum*, masked
because every sequential loop lived inside the Rust runtime (`io.copy`'s pump, the HTTP client's
read-until-EOF, `getrandom`'s EINTR retry). M11's `net.tcp_conn` made it milestone-real: a user
protocol client cannot be written in Align source without it.

- **Surface:** `loop { ... }` is an expression; `break expr` yields the loop's value (bare `break`
  = `()`); breaks unify like `match` arms; a `loop` with no `break` diverges. No `continue`, no
  labels (an `if` covers skip; a two-level exit is a function waiting to be extracted — both
  revisitable on real-code evidence). `?`/`return` exit the function, `break` is the only loop
  exit; `break` cannot cross a lambda boundary. Per-iteration locals drop each pass; a `break`
  value must not borrow from one (block-value escape rule). Normative text: `draft.md` §4 "Loop".
- **Boundary:** the pipeline owns the data path; `loop` owns the control path (EOF pumps, retry/
  backoff, protocol drivers, convergence). Walking an array by index inside a `loop` is a lint
  ("write it as a pipeline"). `loop` finally gives the deferred frequency-dependent M8 lints
  (allocation-in-loop, branch-in-hot-loop, `prefer-pipeline-over-vecN`) their firing surface.
- **Recursion-with-TCO was rejected on structure, not taste** — it conflicts with four settled
  pillars: scope-end drops/region frees kill tail position (the Rust reason); `?` kills tail
  position (the one error model makes loops fallible); TCO-or-not is invisible in source (a hidden
  O(1)→O(n) stack failure mode, against Nothing hidden); and a back-edge CFG is compiler-friendly
  while loop-reconstruction-from-recursion is the fragile inverse, with accumulator-threading a
  known human/LLM bug source. Recursion stays legal for recursive *problems* (parsers, trees);
  **the spec now explicitly guarantees no TCO.** Full rationale: `design-notes.md` → "The loop
  philosophy".
- **Docs updated 2026-07-09** (design-first, implementation deferred — no timing pressure, per
  ideal-form-or-defer): `draft.md` §4/§7, `language-spec.md`, `design-notes.md`, `history.md`,
  guide ch00/02/06/13/17, little-aligner ch11 (rewritten — it taught recursion-as-iteration and
  overclaimed TCO), + `ja/` mirrors. Implementation is a future slice (lexer `loop`/`break` +
  HIR/MIR back-edge + break-type unification + escape/drop wiring); not scheduled inside M11.
  Implementer notes from the design review: loop-carried `mut` Move state needs drop-on-reassign
  each iteration (the existing `drop_old` machinery), and a per-iteration owned local carried out
  by `break` needs path-sensitive move-vs-drop (move on the break edge, drop on the back edge) —
  both reuse existing mechanisms, neither needs new spec text.
- **Implemented 2026-07-10** (the design above shipped as code). Lexer `loop`/`break` keywords;
  parser `loop` expression + `break` statement; `for`/`while`/`continue` rejected at statement
  position with a pointer to `loop`/pipelines. Break-type unification reuses the `match`-arm
  running-unify (a `LoopCtx` stack seeded from the loop's expected type; a break-less loop diverges
  via `hir_expr_diverges`, like an all-diverging `match`). MIR lowers a header/back-edge/exit CFG;
  per-iteration owned locals (body-declared ∩ `drop_locals`) are `Drop`+null-reset at the back-edge
  and at each `break` (the moved-out break value is nulled first, so it is not double-freed). The
  loop-back `MoveCheck` is a two-pass fixpoint: a suppressed probe pass finds the fall-through
  (back-edge) moves, then the real pass runs from `entry ∪ back-edge` so a 2nd-iteration use of an
  enclosing owned local moved by the 1st is caught; the post-loop state is the union of the `break`
  snapshots. `break` cannot cross a lambda (the loop stack resets at each lambda body).
  **Two deliberate deferrals, each cleanly rejected/conservative, not half-measures:** (1) a `break`
  lexically inside an `arena`/`task_group` nested in the loop is rejected with a clear diagnostic
  (the scoped region-unwind-on-break wiring is a separate slice); (2) the `break`-value escape rule
  is enforced as "must be `Static`" (identical to the return-escape rule — a `break` leaves the loop
  as a `return` leaves the function), which soundly rejects a view of a per-iteration owned local
  but conservatively also rejects breaking an enclosing-arena / outer-frame view out of the loop
  (`.clone()` to copy out) — loosening that is a future refinement. New tests:
  `crates/align_driver/tests/loop_expr.rs`. The `expr_depth` headroom lesson recurred: the new
  `lower_expr` arm is bindings-free and delegates to an out-of-line `#[inline(never)]` `lower_loop`,
  and `MoveCheck`'s loop code is an out-of-line helper, so neither bloats its recursive frame.

### Spec-vacuum sweep — five settlements (2026-07-09)

**Trigger:** the `loop` decision exposed a failure pattern — the guides teaching semantics the
authoritative spec never states. A two-track audit (recorded-items inventory + adversarial vacuum
hunt over `draft.md`) found 12 unrecorded holes. The five that were ripe are settled here
(normative text landed in `draft.md`); the remainder is recorded under Open → "Unrecorded spec
vacuums — remainder". Each settlement codifies or refines what is already shipped/taught;
implementation deltas are noted.

1. **`print` + value display** (`draft.md` §4 "print and Value Display"): primitives only;
   per-type display contract (floats = shortest round-trip; `bool` = `true`/`false`; strings
   verbatim); printing an aggregate is a compile error (no magic deep-formatter); the contract is
   shared with template interpolation; `print` is Impure. Matches the guide and shipped behavior.
2. **Literals & escapes** (§12 "Literals and Escapes"): string literals single-line; `char` = one
   Unicode scalar (surrogates rejected); escape set `\n \t \r \0 \\ \" \' \u{...}`; unknown
   escape = compile error. Shipped today: `\n \t \" \\`; the lexer still owes `\r \0 \u{}` + the
   explicit single-line / unknown-escape errors.
3. **Equality = scalars + strings only** (§5 "Equality and Ordering"): no structural `==` on
   struct/tuple/array/sum — explicit field comparison / `match` / pipeline instead (nothing
   hidden; the `match`-is-for-variants boundary). **Implementation bug found while settling
   — FIXED:** sema used to let struct `==` through to codegen, which **panicked** (ICE —
   `align_codegen_llvm` "expected the IntValue variant"). sema now rejects every non-scalar /
   non-string comparison operand up front with a clean diagnostic, via a *positive* allow-list
   (the `Eq` / `Ord` bound predicate — numbers / `bool` / `char` / `str`, never a fail-open
   pass-through): struct / tuple / array / slice / sum / `Option` / `Result` / owned `string`
   `==` `!=` `<` `<=` `>` `>=` are all compile errors, not ICEs (`bool` ordering too — ordering
   is numbers + `char` only). Owned `string` equality stays deferred (its own "not directly
   comparable yet" message; only the `str` view is comparable today).
4. **No shadowing** (§4 Variables): a name binds once per scope chain — same-scope re-`:=` and
   inner-scope shadowing of a visible binding/parameter are compile errors; disjoint sibling
   blocks may reuse a name. Rationale: rebinding hides a state change (a known human/LLM bug
   source) and `mut`/a-new-name cover every need. **Implemented:** sema's `check_shadow` rejects a
   binding whose name is already visible — a local/parameter in scope, an enclosing binding a lambda
   could capture, or a top-level constant of the module (also catches duplicate parameters and
   `(a, a) := …`); disjoint sibling blocks / `match` arms stay legal.
5. **Float semantics = IEEE 754, total, never aborts** (§5 "Float Semantics"): `x/0.0` → `±inf`,
   `0.0/0.0` → NaN, NaN ≠ everything incl. itself, scalar `frem` unguarded like the vector form;
   only *integer* division aborts. Same zero-cost/never-blocks-SIMD rationale as wrap-on-overflow.
   Matches shipped behavior (`1.0/0.0` prints `inf` today).

### `Ord(str)` + `else` on `Result` — SETTLED (2026-07-09, owner-directed follow-up)

Two more settlements from the same session, decided under the owner's re-affirmed criteria: no
freedom that blocks optimization, no complexity, no soundness breaks; inconvenience is acceptable
(most code will be AI-written) but the language must stay human-*understandable*.

1. **`str`/`string` join `Ord`** (`draft.md` §5 "Equality and Ordering" + Generics bounds):
   `<`/`<=`/`>`/`>=` on strings, and string keys in `sort_by_key`, with **byte-lexicographic**
   order (= Unicode scalar order for valid UTF-8). Deterministic, locale-free, one `memcmp` —
   dictionary/locale collation is a `pkg` concern, never the operator. Motivation: a
   data-oriented language must sort by a name column; `Eq(str)` was already byte-based, so this
   is the consistent completion. **IMPLEMENTED** (`Bound::Ord.satisfied_by` accepts `str`; a
   runtime `align_rt_str_cmp` returns -1/0/1 backing the four ordering operators and the `sort`/
   `sort_by_key` `str`-key comparator — `str_eq` keeps its own length-fast-path for `==`/`!=`).
   Owned `string` ordering stays deferred with its existing "take a `str` view" diagnostic (the
   `str` view is the only comparable string form).
2. **`else` works on `Result`** (`draft.md` §5 Result; guide ch04 rewritten): `v := f() else
   fallback` yields `Ok`'s value or deliberately discards the error — visible handling, so the
   unhandled-`Result` error never fires on it; no error binding (needing the error *is* the
   signal to `match`). Completes the intent triangle **`?` propagates / `else` falls back /
   `match` inspects**, symmetric for `Option` and `Result`. This **overturns a guide-invented
   doctrine** ("else is Option-only — don't paper over errors"; the spec was silent — the same
   vacuum pattern as `loop`): that doctrine conflated *accidental* ignoring (still impossible)
   with *deliberate* fallback (legitimate; without one visible form, users wrap fallible APIs in
   `Option` helpers and the culture splits). **IMPLEMENTED** (sema's `check_else_unwrap` accepts a
   `Result` scrutinee and yields `Ok`'s type; MIR's `lower_else_unwrap` reuses the two-way Option
   shape on the `ResultIsOk`/`ResultUnwrapOk` discriminant). The discarded `Err` must be a **Copy**
   scalar — every `Result` error today is (the `Error` enum / a user error enum) — so there is
   nothing to drop on the fallback path; a **Move** error (`Result<T, string>`) is rejected with a
   clear "not yet" (its discarded buffer would leak) and lands when enum/Result Move payloads gain
   discard-drop support.

### Separate compilation + ThinLTO — SHIPPED (M15 + ThinLTO S0–SV; design SETTLED 2026-07-14)

Recorded 2026-07-12; owner-mandated. "One `Program` → one whole-program object" is no longer Align's
only compilation model. **Design SETTLED 2026-07-14** by the mandated two-lens review
(language/soundness + driver/artifacts/cache); the full settlement + slice plan (S0–SV) is the "M15
design SETTLED" block in the roadmap M15 section (implementation source of truth). Key decisions:
unit = one module/file, driver-discovered DAG (cyclic imports = hard error, no other new syntax); the
unit interface is COMPLETE (escape/Move/MoveCheck need no body summaries; purity = a 3-valued
per-`pub`-fn effect bit, fail-CLOSED — missing/Unknown ⇒ Impure ⇒ rejected at parallel boundaries);
generics = instantiate-in-consumer with serialized template ASTs (duplicate internal monomorphs
accepted in v1); visibility = `{main} ∪ --export ∪ pub` external, everything else internal;
incremental cache on the doc-10 contract with an interface-vs-impl hash split (interface hash
INCLUDES effect bits + generic template bodies); hard cutover (N=1 IS whole-program, byte-identical).
**SHIPPED 2026-07-15: M15 separate compilation COMPLETE through SV** (per-unit
interfaces/sema/codegen/link, the default-on incremental object cache, parallel unit codegen, the
doc-10 §7 verification bundle).

**Cross-module optimization — scope of the resolution:** the recorded "multi-file loses cross-module
inlining" trade is RESOLVED **only under the opt-in `--thin-lto` flag on the `release`/`fast`
profiles.** Default builds and every `debug` multi-file build still run with ZERO cross-unit
optimization by design — each unit is compiled in isolation, and `--thin-lto` is never folded into a
profile in v1. Under that flag the ThinLTO arc (S0 spike → S1 serial → S2 cache/parallel → SV
verification) is CLOSED (2026-07-17); the roadmap "ThinLTO design SETTLED" / "ThinLTO S1/S2 SHIPPED" /
"ThinLTO SV SHIPPED" paragraphs are the record, and `impl/10-cache-first-optimization.md` §7 is the
invalidation-matrix source of truth. **S2 correction 2026-07-16:** ThinLTO composes via separate
`prelink`/`thinbackend` phase keys + CAS namespaces, so the M15-reserved empty `cross_unit_opt_digest`
codegen-key field was removed outright rather than populated. The genuinely-open follow-ups (cross-unit
`pub` internalization, precise-vs-conservative digest evolution, ThinLTO-aware
`explain-opt`/`emit-llvm --stage`, `extern "C"` export-of-body / fully-escaping cross-unit fn values)
stay findable as a slim "Separate compilation / ThinLTO — remaining deferrals" entry in the Open
section, alongside the parallel/pipeline/cache companion-audit records.

### `sort_by_key` key effects and evaluation count — SETTLED 2026-07-17

`sort` and `sort_by_key` are stable. A `sort_by_key` key callable may be Impure and is evaluated
exactly once per surviving element, in input order, before any reordering. The implementation's
decorate step records those keys; comparisons never invoke the callable again. This makes the
shipped behavior normative without fixing the internal sorting algorithm. Full context:
`impl/12-pipeline-closure-memory-io-simd-audit.md` §3.2.

## Open (to be decided)

Each item is tagged with a target milestone for resolution (`impl/07-roadmap.md`).

### Unit-returning `fn main()` yields a nondeterministic exit code — FIXED as #450, 2026-07-14

Found by the M15 S2 adversarial gate; reproduced on the untouched whole-program path, so
pre-existing (not an S2 regression — the per-unit object was byte-identical; the garbage was a
runtime return-register artifact). A `fn main()` with Unit return produced a different exit
code per run of the SAME binary (observed 88/216/168/120/104 across five runs);
`fn main() -> i32` with `return 0` was clean. **Fix:** a `Unit`-returning `main` is now renamed
`align_main` and gets the same generated C `main` wrapper a `Result`-returning `main` already
had (`crates/align_codegen_llvm/src/lib.rs`, `symbol_name`/`emit_main_wrapper`) — the wrapper
calls `align_main` then always emits `ret i32 0`, so the C ABI's return register is never left
undefined. Pinned by a same-binary, run-5-times determinism test on both the whole-program and
per-unit (`build_per_unit`) codegen paths (`crates/align_driver/tests/unit_main_exit_code.rs`).

### By-name fn-value references fail in non-entry modules — FIXED as #448, 2026-07-14

Found by the M15 S1b adversarial gate; reproduced identically on the whole-program path that
S1b did not touch, so pre-existing. Inside a NON-ENTRY module, referencing a same-module
function by bare name as a fn-value (`xs.map(dbl)` where `fn dbl` is defined beside it) failed
with `undefined function: 'dbl'`; the entry module accepted the same code.

**Root cause:** the pipeline/reducer callable-resolution paths (`check_stage_fn`,
`resolve_stage_fn`, `resolve_fn`, and the `named_param_hint`/`named_sig`/`check_pipeline`
element-type peeks in `crates/align_sema/src/lib.rs`) looked the callable up in `self.sigs`
by its **bare** name and lowered it to that bare name — but outside the entry module functions
are keyed `module$name` (`mangle_fn`), so both the lookup and the codegen target missed. The
value-expression path (`f := double`, line ~8157) already resolved through `resolve_local_fn`
and was unaffected. **Fix:** every callable position now resolves the bare name through
`resolve_local_fn` (the same helper the direct-call path uses) before the `sigs` lookup and
lowers to that mangled name — so same-module fn-values behave identically in entry and
non-entry modules, and the effect machinery (which keys on the mangled name) accepts a Pure
non-entry `par_map` callee and rejects an Impure one. Whole-program and per-unit checkers agree
(differential test added). Tests: `crates/align_driver/tests/modules.rs` (6 new) +
`per_unit.rs` (1 new).

**Qualified remainder — FIXED as #458, 2026-07-15.** A shared named-function reference now preserves either
a bare name or the complete dotted module prefix, then resolves through the same import / `pub`
visibility contract as a direct `mod.fn(...)` call. Qualified functions work in every named
callable consumer (`map`/`where`/`reduce`/`scan`/`partition`/`any`/`all`/`par_map`/
`sort_by_key`) and as ordinary bound function values (`f := util.dbl; f(x)`). Signature peeks for
untyped literal sources and fold accumulators use the same resolver without emitting premature
diagnostics; checked resolution reports import, visibility, and missing-function errors once.
The leftmost-name shadowing rule is shared with direct calls, so a local `util` keeps value-field
semantics instead of being misread as a module. Dotted modules (`util.math.dbl`) retain the full
prefix. Whole-program and per-unit checking agree, including imported Pure/Impure effect bits at a
direct qualified `par_map` boundary. Tests: `crates/align_driver/tests/modules.rs` and
`per_unit.rs`.

Note: the recorded repro snippet `pub fn doubled(xs) -> array<i64> = xs.map(dbl)` does not
type-check in *either* module — a `map` must end in a reduction (`.sum()`) or an out-param sink
(`map_into`); there is no bare `map`→`array` collect. The bug reproduces cleanly with a valid
terminal, which is what the tests use.

### Separate compilation / ThinLTO — remaining deferrals (design SHIPPED; decision record in Settled)

The separate-compilation + ThinLTO **decision is settled and SHIPPED**; the durable decision record
moved to the Settled section ("Separate compilation + ThinLTO — SHIPPED (M15 + ThinLTO S0–SV)").
What stays genuinely OPEN is a short list of deferred follow-ups — each a future trigger, not a
blocker:

- **Cross-unit `pub` internalization** — v1's fail-closed preserve set keeps every `pub` fn external
  in its ThinLTO object; the win waits for a whole-program-visibility pass.
- **Precise-vs-conservative backend digest evolution** — v1 uses the precise ThinLTO backend digest
  (own prelink ⊕ inbound imports ⊕ outbound exports ⊕ import-source digests); a coarser conservative
  digest that trades hit rate for simpler invalidation is deferred.
- **ThinLTO-aware `explain-opt` / `emit-llvm --stage`** — they stay per-unit-in-isolation (the honest
  zero-cross-unit-opt view) until a cross-unit remark story is designed.
- **`extern "C"` export-of-body + fully-escaping cross-unit function values** — standing deferrals
  (out-param noalias trust chain / heap-owned closure environment), unchanged by ThinLTO.

The companion audits below remain the durable implementation records for this workstream; several
carry active measure-first probes, which is why they stay in Open.

**Cache-first companion audit (2026-07-12):** `impl/10-cache-first-optimization.md` is the durable
detail for this item's artifact identity and invalidation questions. Its confirmed pre-M15
basename-derived shared temporary artifact defect is fixed 2026-07-13 with private staging, atomic
publication, and concurrent build/run/size gates. It also records
the proposed staged CAS + interface/implementation/link-summary hashes, exact toolchain/runtime
keys, deterministic-output validation, and separately labeled measure-first CPU-cache candidates.
The remaining cache correctness constraints are commitments today; the locality
candidates remain probes until their written gates pass.

**Parallel execution/output-IR companion audit (2026-07-12):**
`impl/11-parallel-execution-optimization.md` is the durable implementation record. It confirmed two
P0s absent from the prior queue: `EffectScan` omitted a lifted capturing closure's call edge, allowing
observable I/O inside an accepted Pure `par_map`; and `task_group -> par_map` deadlocked at shared
pool saturation because `par_map` waited after one caller chunk instead of draining its ranges.
Both P0s are fixed 2026-07-13: closure edges plus unknown-target fail-closed propagation close the
effect path, while a shared cursor, caller drain loop, total-range barrier, and forced-worker
watchdog close the progress path. Next implement the already-recorded whole-chunk
specialization. The guide already calls capturing-`par_map` parallelization “implementation in
progress”; this audit pins its read-only context ABI. New measure-first work is wrapping-integer
`par_map(...).sum()` fusion (remove the full intermediate write/read), length-preserving staged
parallel lowering, task claim/completion + queue batching, packed task records, and body/byte-aware
grain. Applying the already-recorded blocking-worker direction to generic `task_group` stays a
later mixed-load gate, not a newly invented idea. No new language syntax is proposed. The same
record catalogs task-error, pool, MIR, and generic parallel-reduce documentation drift; none of
those unsettled descriptions authorizes implicit parallelization of ordinary `reduce`.

**Pipeline/closure/memory/I/O/SIMD companion audit (2026-07-13):**
[`impl/12-pipeline-closure-memory-io-simd-audit.md`](impl/12-pipeline-closure-memory-io-simd-audit.md)
is the durable implementation record. It confirms that normal fused loops, `map_into` alias
metadata, non-escaping capture inlining, JSON/UTF-8/string SIMD, direct regular-file reads, mmap
views, and small/large writer paths already have the intended shape. Its post-`where` callable,
spawn-capture, closure-result region, Unit indirect-call ABI, and buffered-reader `io.copy` P0s are
fixed and regression-pinned in `impl/source-correctness-fixes-2026-07-13.md`; ordinary sequential
effects are settled as Impure-allowed with exact guarded order. Dynamic allocation-size hardening
is complete with checked heap/arena/SoA byte arithmetic and boundary/mutation gates. New
measure-first work included the per-callsite initialized-before-read arena split and exact-final
Base64/hex fill; both shipped 2026-07-16 after their gates. Arena chunks now distinguish raw
`MaybeUninit` backing from lazy-zero backing, while the public/generated ABI and task records remain
conservative; only file-copy, arena-builder-finish, and strict SoA decode use raw storage. The HTTP
batch request-copy removal also shipped 2026-07-16: immutable requests are prebuilt once and the
workers borrow uniquely claimed entries, removing one URL allocation/copy per request while retaining
the bounded claim loop and ordered all-or-Err cleanup. The x86-64 Base64 and hex encoders
runtime-dispatch to AVX2 at their independently measured 32-byte crossovers; their scalar oracles and
exact destinations are retained. The aarch64 NEON halves have since been measured on native Apple
Silicon and activated, each byte-for-byte against its scalar oracle and dispatched only above the
measured length: Base64 at a 48-byte crossover, hex at 16-byte, and — 2026-07-18 — the JSON
string-escape classifier (`write_json_str`) at 16-byte (mostly-clean 6.04x, escape-dense 2.10x,
short 1.11x). Remaining measure-first work is scalar vs SIMD stable compaction. Existing work from
documents 10/11 remains attributed there. The separate native aarch64 UTF-8 portability run also
remains deferred.

**String/array allocation-copy and short-input companion audit (2026-07-13):**
[`impl/13-string-array-allocation-short-input-audit.md`](impl/13-string-array-allocation-short-input-audit.md)
is the durable implementation record. Its UTF-8 range-boundary gap is fixed and regression-pinned;
its settled `str + str` enforcement drift is fixed 2026-07-15; unbound owned-expression
temporaries gained view-aware synthetic owners the same day; arena-free templates then gained scoped
owners and static-only folding, and known-null destructor calls were eliminated. It confirms the
remaining avoidable path/builder/chunks/group staging. It also records the
good existing zero-copy view, fused pipeline, scalar fallback, and array-builder freeze shapes so
later work does not replace them accidentally. UTF-8 short crossover, repeated-needle preparation,
and JSON escape SIMD were measurement-gated and shipped; large constant-local pooling shipped
2026-07-17 at a measured N=32 cutoff (§8.4, memcpy-from-rodata, type-preserving). The document's language-
surface items are deliberately **questions for Claude Code only**: this ledger adopts no new syntax,
type, capacity argument, eager/lazy guarantee, or template ownership rule from the audit.

### External binary-optimization audit (Codex, 2026-07-12) — adoption record

The owner's out-of-repo Codex audit (`~/winhome/Downloads/align-binary-optimization-report-2026-07-12.md`,
audited HEAD `4a8e76c` = **pre-#425**, on arm64 macOS with rustc 1.96.1 / LLVM 22.1.8). Fable
verified the key claims against the code on 2026-07-12. High-quality report; the owner wants
every valid finding addressed. Disposition:

- **Already resolved by #425 (no action):** the LLVM-22 `nocapture` compatibility item (the
  named attr auto-upgrades to `captures(none)`; the A8 gate proves the in-memory attribute is
  honored — the report's "dangerous as-is" is DISPROVEN, though its hardening suggestion is
  adopted below), opaque benchmark seeding (Shape shift A), clang-22 harness unification.
- **CONFIRMED bugs → adopt as the next code wave ("measurement portability", the report's PR1):**
  1. **bench export contract broken by M13 internalization** — `bench/README.md` promises
     no-`main` `pub fn`s are exported from `emit-obj`; #418 internalizes ALL program fns, so
     `bench/run.sh`'s Rust harness link fails (undefined `_sum_sq_pos` etc.). Fix per the
     report: keep default whole-program internalization; add explicit export roots
     (`emit-obj --export foo` driver mechanism); DCE roots + linkage from the same export set;
     re-verify `bench/run.sh baseline/native` runs.
     **DONE (2026-07-13):** a repeatable `--export <name>`/`--export=<name>` flag on
     `emit-obj`/`emit-llvm` only (rejected with a diagnostic elsewhere); pulled out before
     positional-argument parsing so a following value can never be misread as the output-object
     path; fail-closed against the lowered MIR (`align_driver::unknown_exports`, unit-tested) —
     an unknown name is a listed hard error, never a silent no-op. `align_codegen_llvm::declare_fn`
     keeps `external` linkage for a named export root exactly like it already does for `main`
     (keyed on the *source* `Function::name`, independent of the `main`/`align_main` symbol
     split), so linkage and the LLVM DCE-root set come from the same list by construction.
     `bench/run.sh` + every sub-bench (`group_by`, `group_by_reuse`, `json_decode`, `json_soa`,
     `par_map`, `string_builder`) now pass the exact `--export` list their Rust harness's
     `extern "C"` block calls; `bench/binary_size` links no harness and needed no change.
     `bench/README.md` rewritten: `--export` is an object-level C-ABI boundary independent of
     `pub`. Regression test `crates/align_driver/tests/export_roots.rs` (mutation-tight: reverting
     the linkage guard fails it).
  2. **ELF-only link/size tooling breaks macOS builds** — the driver passes
     `--gc-sections/--as-needed/--strip-all` unconditionally (`align_driver/src/lib.rs:182`);
     `alignc size` assumes `readelf`/GNU `nm`; `bench/binary_size` assumes `stat -c`/`mapfile`.
     Fix: linker policy selected by target triple/object format (ELF vs Mach-O `-dead_strip`,
     no `-ldl` on macOS), migrate size inspection to version-matched `llvm-readobj`/`llvm-nm`/
     `llvm-size`. Also adopt: derive runtime native-lib deps from Rust's `native-static-libs`
     instead of hand-written flags.
     **DONE (compiler slice, 2026-07-12):** `ObjectFormat` + `target_object_format()` in
     `align_codegen_llvm` (triple classification stays in codegen; Windows fail-closed);
     format-selected linker policy in `link_objects` via `hygiene_flags`/`support_libs` data
     tables (ELF `--gc-sections`/`--as-needed`/`--strip-all` unchanged; Mach-O `-dead_strip`/
     `-dead_strip_dylibs`, no support libs, post-link external `strip` — `Profile::strip` stays
     the only strip decision point); a `LIBRARY_PATH` hint on gated-library link failures;
     `llvm_tool()` discovery (build prefix → `-22` suffix → bare name); `alignc size` fully on
     `llvm-readobj`/`llvm-nm` for both formats (readelf/GNU-nm removed outright — Mach-O symbol
     sizes derived from address deltas, chained-fixups note, `LC_LOAD_DYLIB` listing); the
     macOS regression net (`macho_link.rs`, format-branched `build_profiles`/`capability_linking`
     with a `can_link` probe for gated libs). **Deferred out of the slice:** (a) **DONE
     (2026-07-13):** `bench/binary_size` script port — `run.sh`/`profiles.sh` now source a shared
     `bench/binary_size/lib.sh` for `filesize` (GNU `stat -c%s` / BSD `stat -f%z` / `wc -c`
     fallback), `llvm_tool` (the PATH half of `align_driver::llvm_tool`'s `<name>-22` → `<name>`
     search), `gated` (`llvm-readobj --needed-libs`, format-general: ELF `DT_NEEDED` sonames /
     Mach-O `LC_LOAD_DYLIB` install names, basename'd before matching), and `stripped` (`llvm-nm`
     empty-stdout signal, not ELF-only `.symtab` grepping); `profiles.sh`'s `mapfile` (bash >= 4)
     replaced with a plain-loop array build for bash 3.2 compatibility. Verified end-to-end on this
     Linux box: both scripts' before/after output is byte-identical to the pre-port readelf/GNU-nm
     version; the Mach-O branch is structurally exercised by the same code path as the compiler-side
     `size.rs` but unverified on real Mach-O hardware, same caveat as the compiler slice above; (b)
     `native-static-libs` derivation — the
     `support_libs(format)` table is the seam it replaces; (c) x86_64-apple-darwin SysV
     acceptance (today fail-closed over-rejects that target for by-value FFI structs — not a bug;
     relax when hardware/CI exists); (d) `-L`/sysroot CLI flags (M15 cross-compilation
     discussion); (e) a Mach-O chained-fixups count in `alignc size` (llvm-readobj 22 exposes
     no way to print it).
  3. **Build profiles don't reach the backend** — TargetMachine is always
     `OptimizationLevel::Default` (`align_codegen_llvm/src/lib.rs:181`); `small`/`tiny` never
     set `optsize`/`minsize` fn attrs; the runtime archive is one variant for all profiles.
     Adopt the report's mapping table as the starting point (dev=None … fast=Aggressive,
     small=`optsize`, tiny=`minsize`+`optsize`); runtime cache key gains profile/panic/LLVM-major.
     **DONE (code slice, 2026-07-13):** `Profile::codegen_opt_level` (dev=`None`, release=`Default`,
     fast=`Aggressive`, small/tiny=`Default`) threaded into `create_target_machine`, so the object
     path's TargetMachine follows the profile; the diagnostic lenses (`emit_llvm_ir` /
     `collect_opt_remarks`) pin codegen=`Default`, no size attrs, pipeline `O2` — the IR-shape suite
     stays byte-identical. `small`/`tiny` gain a single `optsize` / `optsize`+`minsize` fn-attr
     sweep over module *definitions* only (`apply_size_attrs`, `count_basic_blocks() > 0`), a set
     completely disjoint from `apply_rt_contract_attrs`'s declaration-only sweep. small/tiny stay at
     codegen `Default` on purpose (clang parity: lowering the codegen level does not shrink size, it
     only slows code — size is the attrs + `default<Os|Oz>` pipeline). Release object output is
     bit-for-bit unchanged (verified). **Deferred:** the per-profile runtime variant + cache key
     (`(profile, panic-strategy, LLVM-major, rustc-version, runtime-source-hash, target-triple/cpu)`)
     goes to the M14 runtime-bitcode slice + the doc-10 §2 cache layer — there is no clean partial:
     the runtime `.a` is a single cargo-side prebuilt, variants need `alignc` to drive cargo through
     a keyed cache subsystem that does not exist yet, and the `panic=abort` lever is already a Future
     backlog item, the bitcode LLVM-major match belongs to M14.
- **Quick-win fixes (all verified in code; independent small slices):**
  4. **`sort`/`sort_by_key` O(n²)** insertion sort (`align_mir` ~5161, self-documented "first
     cut") — report measured 547× vs Rust stable sort at 100k. Adopt: stable O(n log n) core +
     tiny-N insertion base case + `sort_by_key` decorate-sort-undecorate (keys computed N times,
     not per-comparison — inner comparisons currently recompute `key(arr[j])`).
     **DONE (2026-07-13):** `lower_array_sort` rewritten as a **stable bottom-up merge sort**
     (O(n log n)) with an insertion-sort base case for runs ≤ `SORT_INSERTION_THRESHOLD` (32).
     Kept as **MIR expansion** rather than a runtime function on purpose: the comparison is already
     polymorphic over every scalar *and* `str` key via `BinOp` lowering (`Lt`/`Gt` → `align_rt_str_cmp`),
     so no per-type `align_rt_sort_*` matrix and — critically — **zero `align_runtime` changes**. It
     runs in place over the collected buffer using one same-size heap scratch buffer `tmp` (each pass
     merges `arr`→`tmp` then copies back, so the result always lands in `arr` and the caller's
     drop/arena semantics are unchanged); scratch is transient `HeapAllocBuf` freed by shallow-spine
     `DropValue` (the same discipline `group_by` uses, and correct for `str`-view keys, which are
     Copy `{ptr,len}` borrows — the free never touches the pointed-to bytes). `sort_by_key` is now
     true **decorate-sort-undecorate**: each key is computed exactly once into a parallel `keys`
     buffer (carried alongside the elements through every move), replacing the old per-comparison
     `key(arr[j])` recomputation. Stability comes from taking the left run first on equal keys
     (insertion shifts only on strict `>`; merge takes the right run only on strict `<`).
     Correctness net = `crates/align_driver/tests/sort_merge.rs` (random/sorted/reverse/duplicate/
     all-equal/empty/single, the N = 31/32/33 base-case⇄merge boundaries, and stability in both the
     insertion base case and through several merge passes; i64/f64/char/`str`-key). **Measured**
     (100k pseudo-random i64, `--profile release`, this host): O(n²) main **≈ 1.08 s** →
     merge-sort branch **≈ 9.7 ms** whole-program (identical generation) = **~111×**; isolating the
     sort (subtracting the ~4.8 ms generation) ≈ **220×**.
  5. **tiny `par_map` cold start** — `par_pool()` is called BEFORE the single-chunk threshold
     check (`align_runtime` ~1469), so an 8-element map spawns the worker pool (~69 µs cold vs
     125 ns warm). Adopt: hoist the `count <= PAR_MIN_CHUNK` check above `par_pool()`; same for
     `task_group` n=1.
     **DONE (2026-07-13):** `align_rt_par_map` now checks `count <= PAR_MIN_CHUNK` and runs the
     whole map on the caller *before* calling `par_pool()`, so a tiny map never touches the global
     pool (the pre-existing `nchunks <= 1` fallback stays, reached only when a degenerate worker
     count still collapses a bigger-than-threshold `count` to one chunk). `align_rt_tg_wait` gets
     the same treatment: `par_pool()` is now called only when `n > 1` (`workers.min(n - 1)` is
     always 0 for `n == 1`, so a single-task group never needed the pool either — confirmed by
     reading the code, not just the report). Regression-pinned by a same-process correctness sweep
     across the threshold boundary (`par_map_correct_across_threshold_boundary`) plus a
     process-isolated integration test (`tests/par_map_cold_start.rs`) that checks a new test-only
     introspection hook (`align_rt_test_par_pool_initialized`, not part of the FFI surface) stays
     `false` through both the tiny `par_map` and the single-task `task_group`, then confirms it
     flips `true` once a workload actually crosses the threshold (so the assertions above are not
     vacuous).
  6. **zero-size arena alloc** can take a fresh 64 KiB zeroed chunk (`CHUNK = 64*1024`,
     `align_runtime` ~7095). Adopt: size-0 fast path returning a canonical dangling pointer,
     allocation-counter test. Distinct from the REJECTED arena pool+re-zero — do not conflate.
     **DONE (2026-07-13):** `Arena::alloc` now takes a `size == 0` fast path that returns `align`
     itself (already normalized to a nonzero power of two) cast to a pointer — non-null, trivially
     aligned, and read/written through only in the sense that it never is (a 0-byte allocation
     carries no bytes) — without fetching a chunk or advancing the bump cursor. Distinct from the
     rejected "arena pool + re-zero" idea: no chunk memory is ever reused or pooled, none is
     allocated at all for a 0-byte request. Regression-pinned by
     `arena_alloc_zero_size_never_grows_chunk_count` (many size-0 allocations at several alignments
     keep the chunk count at zero, interleaved with a real allocation to confirm the fast path
     doesn't corrupt subsequent bump-allocator state).
- **Measure-first adoptions (direction yes, gated on numbers):**
  7. **JSON decode double allocation — exact-count scalar path measured and rejected 2026-07-16**
     (Rust `Vec` → `align_rt_alloc` → memcpy, runtime ~2317/~2624). A checked-in balanced
     `array<i64>` probe priced the required lexical count pass before exact allocation and direct
     parse. It won only at 1-8 elements; from 64 it lost, reaching 0.71-0.73x of the existing
     one-pass staged decoder at 1K-1M elements. Retain staging for this path. A C-owned growable
     buffer remains a different ownership experiment, not authorized by this negative result; the
     bigger lever stays decode-fusion (consumer-gated, GPT-5.6 record).
  8. **I/O buffer zero-fill** (`Vec::resize(64KiB, 0)` before `read` overwrite, runtime ~4558/
     ~4995). Adopt `spare_capacity_mut`+`set_len` — but correctness tests (short read/EINTR/
     EOF) FIRST; throughput-only.
     **DONE for reader/lookahead/`io.copy` (2026-07-16):** the direct reader and buffered lookahead
     now read into reserved `MaybeUninit` spare capacity and publish exactly the successful prefix;
     the portable `io.copy` loop inherits the same path, including lookahead-first semantics. EOF
     leaves logical length zero and EINTR retries before publication. The allocation-inclusive fresh
     64 KiB-window probe improved 0/1/4 KiB/full-prefix cases by 20.92x/20.83x/11.49x/1.98x.
     **AUDITED EXTENSION DONE (2026-07-16):** UDP receive and positional `pread` now use the same
     guarded raw window. Datagram truncation still publishes exactly `cap` leading bytes; `pread`
     still preserves fd offset, surfaces short reads/EOF, and retries EINTR.
- **Hardening (adopt):**
  9. **attribute kind-ID fail-loud** — `add_enum_attr` doesn't check
     `get_named_enum_kind_id() == 0` (silent no-op on a renamed/typo'd attr); make it a codegen
     error. Emit `captures(none)` via the modern `captures` kind (raw CaptureInfo value pinned
     against the LLVM 22 headers) instead of relying on auto-upgrade — this likely also FIXES
     the recorded `emit-llvm | llvm-as-22` textual round-trip follow-up (the printer would emit
     `captures(none)`, not the unparseable `ptr none`). Prefer semantic attr assertions over
     full-declaration string pins where practical.
     **DONE (2026-07-13):** the two changes were coupled — investigating the fail-loud check
     revealed the *current* code was already going through the silent-no-op path: on LLVM 22
     `get_named_enum_kind_id("nocapture")` returns `0` (the attribute was removed in favour of
     `captures(...)`), so `create_enum_attribute(0, 0)` emitted the bare, un-reparseable `ptr none`
     shorthand — the very shape that broke the round-trip. Fix: a shared `enum_kind_id(name)` gate
     under `add_enum_attr` / the new valued `add_valued_enum_attr` that **panics** if the name
     resolves to kind id 0. Chosen a panic, not a `CodegenError`: every attribute name is a
     compiler-internal string literal (never user input), so an unknown name is an input-independent
     compiler build defect against the linked LLVM — it fails on *every* compilation, not one
     program — which is the internal-invariant class codegen already handles with `unreachable!` /
     `expect`; a per-program `CodegenError` would falsely blame the user's source, and threading
     `Result` through the ~6 valueless call sites (`mark_nounwind`, `apply_size_attrs`,
     `apply_rt_contract_attrs`, …) buys nothing. The no-capture contract is now emitted as
     `captures(none)` via the `captures` kind (id 92) + value `0` (`CAPTURES_NONE`), pinned against
     LLVM 22.1.8 `llvm/Support/ModRef.h` (`CaptureComponents::None == 0`;
     `CaptureInfo::toIntValue() == (Other<<4)|Ret == 0` for `none()`) and verified empirically via
     `LLVMCreateEnumAttribute`. **Round-trip follow-up: RESOLVED (hypothesis CONFIRMED).** With
     `captures(none)` emitted directly the printer now prints `ptr readonly captures(none)`, which
     `llvm-as-22` accepts — a new tool-gated gate `align_driver::llvm_as_roundtrip`
     (`emitted_ir_round_trips_through_llvm_as`) feeds `alignc emit-llvm` output to `llvm-as` and
     proves it assembles. Semantic pin added (`rt_contract_captures_none_is_present_by_kind_id`):
     queries the param attribute by kind id and asserts the `captures` payload is exactly `0`, per
     "prefer semantic attr assertions"; the remaining declaration string pins were updated to the
     new `ptr readonly captures(none)` spelling. The A8 optimization gate and the full IR-shape
     suite stay green (the contract is semantically preserved — `captures(none)` is a strictly more
     precise no-capture claim than the old kind-0 emission). Fail-loud pinned by a
     `#[should_panic]` test on a bogus attribute name.
- **Already recorded elsewhere (report converges independently — pointers only):** runtime
  staticlib feature-split (existing Open item; the report adds a Mach-O upper-bound observation:
  hello 409,248 B → 33,984 B when a print-only member resolves first — evidence FOR the split;
  probe numbers, not baselines); discarded-Move drop leak (recorded M9 v1 limit);
  structured remarks via libRemarks (09-explain-opt C++-shim deferral — short-term stays
  re-captured LLVM-22 strings, DONE in #425); ThinLTO-limited/runtime-bitcode-is-the-prize +
  same-major opportunity (= the M14 re-scope, independently derived); PGO needs a merged
  multi-workload profile, BOLT is Linux/ELF-only (fold into the M14 PGO/BOLT items when they
  come up). The report's "do not re-propose" list fully matches our rejected/settled records.
- **Doc debt (fix with the adopting slices):** `bench/README.md` export contract vs M13
  internalization (rewrite with the `--export` mechanism) **[DONE 2026-07-13, with item 1]**;
  `docs/impl/05-backend-llvm.md` says
  bool is "i8 when stored" while Slice 5B deliberately pinned SSA+stack `i1` (align the doc with
  the pinned reality; the i8-stored-bool idea stays a bounded experiment per the report's own
  P3 restraint); `docs/guide/ja/16-toolchain.md` missing profiles/`size`/`explain-opt`;
  `draft.md`/`open-questions.md` `str + str` prohibition is now verified: audit 13 corrected the
  guides and records sema/MIR enforcement as C0 implementation drift; the untracked
  `analysis-report-2026-07-02.md` at root is superseded by this report — delete or archive.
- **Rejected report claims (with reasons):** "`nocapture` is dangerous as-is" (disproven — the
  A8 gate proves auto-upgrade preserves the semantic attribute; adopted only as hardening);
  macOS size/speed numbers as baselines (the report itself marks them manual-link probes;
  baselines wait for the portability fix); "second `default<O2>` run" (the report itself
  rejects it — keep the stock pipeline; the trigger was literal-folded non-opaque input, which
  #425 already fixed).

**Sequencing (owner-tunable):** wave 1 = the three CONFIRMED bugs (measurement portability —
also unblocks trustworthy cross-platform numbers for everything later); wave 2 = quick wins
4–6 (+ 9 hardening); wave 3 = 7–8 measured. The M14 LTO ceiling probe is independent and cheap
— run it whenever. M15 separate compilation proceeds on its own track.

### External optimization consultation (GPT-5.6, 2026-07-11) — adoption record

The owner's out-of-repo optimization consultation (8 long responses: LLVM Performance Tips for
Frontend Authors digest, constants/binary-size, core/std review, cache locality, execution-plan
engine, performance philosophy, codegen deep-dive, and a repo-read review) was fully read and
triaged by Fable on 2026-07-11. Three claimed gaps were **empirically confirmed** against the code:
zero linkage/`unnamed_addr` settings in codegen, `emit-llvm` emits only pre-`run_passes` IR, and the
driver links `-lpthread -ldl -lm -lz -lzstd -lcrypto -lssl` unconditionally. Disposition:

- **ADOPTED → roadmap M13** (the pre-LLVM-upgrade codegen-quality wave; actionable detail lives
  there): symbol internalization + `private unnamed_addr` constants; capability-based linking +
  runtime split (+ `--gc-sections`/`--as-needed`); optimized-IR emission + LLVM-remarks→Align
  translation (`explain-opt`); build profiles (`dev/release/fast/small/tiny` over `default<O*>` —
  deliberately NOT a custom pass pipeline); internal-ABI flattening of slice/str/Option/Result +
  argument attributes (`noundef`/`nonnull`/`nocapture`/`readonly`/`writeonly`/`noalias`/
  `dereferenceable`) + effect-summary `memory(...)` attrs + proven-range `nsw`/`nuw` (a new
  `AddProvenNoOverflow`-class MIR distinction; user wrap arithmetic NEVER gets them).
- **ADOPTED as verification tasks → M13 Slice V**: `BuildTarget::Cpu(name)` empty-feature-string
  objdump test; cold-edge `!prof` metadata (measure before shipping); canonical-loop shape
  snapshot tests + the Vectorizers.html-catalog IR-shape suite (these become the LLVM-upgrade
  regression net).
- **ADOPTED direction, consumer-gated (no milestone yet; recorded at their homes):** decode fusion
  (decode+filter+reduce without materialization — the align-LLM trace-aggregation consumer);
  filter/projection pushdown + dense/sparse execution (selection vector / bitset / index list);
  algorithm portfolio + `Exact/AtMost` cardinality in MIR (→ auto `array_builder` capacity);
  Sink/Source as MIR vocabulary (template/encode → writer without intermediate strings — the
  streaming×pipeline backlog's concrete shape); **the pipeline answer to the map-vs-loop gap
  (settled direction 2026-07-11; OWNER-RATIFIED same day):** side-effecting iteration gets
  pipeline vocabulary, NOT a `for` construct — an `each`/Sink terminal
  (`xs.each(fn x { w.write(x)? })` — Impure, deliberately OUTSIDE the fusion/vectorization
  contract, the structured exit) and a `range(n)` pipeline source (kills the loop index
  ceremony; cardinality `Exact(n)` feeds capacity inference). `loop` stays narrow
  (retry/accept/EOF/convergence — the owner endorsed the purification); the consultation's
  "classified loop forms" land as pipeline ops, never as loop-syntax variants. **Owner's
  flexibility clause:** where the pure path would be GROSSLY inefficient, pragmatic structured
  escapes are allowed (efficiency outranks purity at the extremes — e.g. the `for_each_line`
  scoped zero-copy variant when the per-line copy is measured to dominate); the escape must
  stay structured and visible, never a general-purpose `for`; `for_each_line` scoped zero-copy callback (the
  safe form of A7's rejected lookahead view — noted in the A7 record); cache-locality lints
  (useful-byte ratio, pointer-indirection, false-sharing — the M8 frequency-lints family);
  string blob + offset tables / error-message tables / relative-offset metadata (binary-size,
  alignpack-adjacent); performance contracts spelled in draft.md as Guaranteed /
  Target-dependent / Profile-dependent tiers; `f.len()`-in-loop syscall lint; **the AI
  optimization loop** — a machine-readable performance report (`alignc explain-opt --format
  json`: per-loop {fused, vectorized, why-not, suggestions}), a per-build optimization score
  report (allocations / materializations / hot-loop bounds checks / indirect calls / fused
  pipelines — itemized, not a single number), and CI perf-regression gates that fail a PR on
  allocation/fusion/vectorization-count regressions (grows out of M13 Slice 3's explain-opt;
  the count-gates piggyback on the M13 IR-shape suite).
- **Post-LLVM-upgrade (order matters — bitcode compat):** ThinLTO → runtime-as-bitcode (LLVM
  version alignment is the known wall) → instrument PGO → sample PGO / BOLT.
  **Instrument-PGO SETTLED + S0 GO 2026-07-17** (design/S0-evidence/slice-plan = roadmap
  "Instrument-PGO design SETTLED"): ONE new `align_pgo_run_pipeline` shim entry (llvm-sys 221 has
  no PGO surface), opt-in `--pgo-instrument` / `--pgo-use`, a `PgoMode` cache-key component; ELF
  needs `-Wl,--undefined=__llvm_profile_runtime` + the `clang_rt.profile` archive on instrument links.
  **Instrument-PGO arc CLOSED 2026-07-17** (S0 #499 → S1 #500 → S2 #501 → SV; SV record = roadmap
  "Instrument-PGO SV SHIPPED"): SV verification bundle green (determinism both modes, stale/wrong-profile
  matrix, compile-time bound, and a MEASURED ~1.16× payoff on a branch-layout kernel). Settled (f)
  AMENDED at SV: "0%-match = hard error" was falsified (no reliable match signal — tally undercounts via
  inline+DCE, overcounts via `--rt-lto` baked primitives, per-unit cache bypasses a build-level gate), so
  0%/partial match ships as a prominent WARNING (clang parity: performance-only); hard errors stay at the
  reliable layer (bad-magic profdata + Error-severity libLLVM diagnostics). Deferred: sample PGO / BOLT,
  CSPGO, PGO × `--thin-lto` composition.
  **Amended 2026-07-12 (post-#425 two-lens review; full record = roadmap "Post-upgrade wave"):**
  ThinLTO-across-Align-modules is MOOT (one `Program` → one module; the only boundary is the
  runtime FFI), the version wall DISSOLVED (rustc 1.96 = LLVM 22.1.2, same major as the 22.1.8
  toolchain; rustc bitcode verified to merge + LTO cleanly, std does not leak into IR), and the
  measured win surface is narrow (per-element string/hash wrappers only; the numeric core
  already vectorizes with zero in-loop FFI). M14 Slice 1 = a wall-clock ceiling probe
  (≥ 1.15× or record-and-close both items); PGO keeps its place in the order.
  **Probe RESULT 2026-07-13 (full record + tables = roadmap "M14 Slice 1 probe RESULT"): ABOVE
  GATE → proceed to Slice 2.** Median-of-7 over ~1M short strings (znver3, LLVM 22.1.8): `str_eq`
  2.12× (a genuine LTO-visibility win — inlined + constant-target length fast-path fold; holds at
  generic codegen 2.35×), `hash64` 1.63× native but 1.02× generic (win is native-runtime-tuning
  only, cheaper via a per-target-cpu runtime `.a`), `str_cmp` 0.72× (LTO REGRESSES it — the
  per-symbol `rt_contract` guard is mandatory), numeric control 1.00×; link+reopt ≈ 0.25 s. Slice 2
  re-scoped to the inlinable fast-path string primitives, per-symbol guarded.
- **REJECTED (do not re-litigate; reasons):** NaN boxing / general SSO / runtime string interning
  (representation branches + hidden state vs the settled type splits); automatic AoS↔SoA
  conversion (hidden bulk data movement); deterministic map iteration as a default contract
  (map/ordered_map split instead); `llvm.assume` / early intrinsic emission / loop-metadata
  overrides as a general policy (the consultation itself counsels restraint — attributes and
  flags first); custom pass pipelines from day one (measure `default<O*>` first); linked lists as
  std-central collections.
- **Standing context reaffirmed by the owner (2026-07-11):** pre-release breaking changes stay OK
  for the foreseeable future — public repo but sole user; interface/spec changes need no compat
  shims (the CLAUDE.md rule extends indefinitely until stated otherwise).

### Runtime staticlib feature-split — deferred from M13 Slice 2 (OPEN, not blocking)

M13 Slice 2 (capability-based linking, DONE 2026-07-11) landed the coarse win: a program using no
gated feature links none of z/zstd/crypto/ssl, and per-capability gating is precise for the *clean*
cases (`gzip` → `libz`, `zstd` → `libzstd`). What it could NOT make precise is `Crypto`/`Tls`:
`Capability::link_libs` is a monotonic SUPERSET (`Tls ⊇ Crypto ⊇ {compress}`) because the runtime is
one crate → one archive member, and once a candidate library on the link line resolves *some* of
that member's symbols GNU ld stops garbage-collecting the member's *other* external references (so a
crypto-only program still retains `libz`/`libzstd` in its `DT_NEEDED`). The **ideal** fix — a
crypto/tls binary that links `libcrypto`/`libssl` **alone** — needs the runtime **split by feature
area** so each C-library's code lives in its own object/member (then `--gc-sections` + member
granularity isolate cleanly). Options weighed and deferred (none blocking; the superset is always
correct): (a) separate `align_runtime_{core,compress,crypto,http}` crates/staticlibs — the clean
form, but restructures the 18.5k-line single `lib.rs` and the driver's single-archive link; (b)
cargo features producing per-capability builds — the roadmap already flags this "probably too
slow/complex"; (c) forcing the compress/crypto/http modules into distinct codegen units. Revisit
when a crypto/tls binary's extra compress `DT_NEEDED` actually matters (deployment-size or
supply-chain-surface pressure), or fold into a build-profile/packaging slice. Recorded at the M13
Slice 2 roadmap entry.

### Side-effecting iteration & pipeline sinks — `each`/Sink terminal + `range(n)` source

Direction SETTLED + owner-ratified 2026-07-11 (grew out of the map-vs-loop discussion; also the
consultation digest's Sink/Source item above). The gap: today the only pipeline terminals are
reductions (`.sum()` etc.), so "do a side effect per element" (write each row to a `writer`, run
each record through a handler) has no pipeline form and falls to `loop` + a manual index — the
段差 the owner flagged. The resolution is **pipeline vocabulary, never a `for` construct** (`for`
would be a second data-path way and would erase the intent — filter/projection — that lets the
pipeline fuse). Two additions, both **consumer-gated** (first consumers = the align-LLM gateway
per-token write and the trace-processing per-line handler; they do NOT block M13):

- **`each` / Sink terminal** — `xs.each(fn x { w.write(x)? })`, or a typed sink terminal
  (`xs.write_to(w)`). **Impure, and deliberately OUTSIDE the fusion/vectorization performance
  contract** — it is the structured *exit* from the data path, not a fused stage; documenting it
  as contract-external keeps "pipelines are allocation-free/fused" honest. This is the concrete
  surface of the recorded **Sink/Source MIR vocabulary** (`JsonEncode(_, Sink::Writer)`,
  `Template(_, Sink::Response)` — encode/template writing straight to a writer with no intermediate
  `string`; the streaming×pipeline backlog's shape).
- **`range(n)` pipeline source** — `range(n).map(...)`, `range(lo, hi)` — kills the loop-index
  ceremony for count/index iteration and carries cardinality **`Exact(n)`**, feeding the recorded
  `array_builder` auto-capacity work.

`loop` stays purified to control-only (retry / accept / read-until-EOF / convergence) — the owner
endorsed keeping it narrow. **Owner flexibility clause** (see [[owner-design-criteria]]): where the
pure `each`/sink path would be grossly inefficient, a structured escape is allowed (e.g. the
recorded `for_each_line` scoped zero-copy callback when the per-line copy is measured to dominate)
— structured and visible, never a general `for`. Design still open: exact spelling
(`each`/`for_each`/`write_to`), whether `each` returns `Result<(), E>` accumulating the first error
(the `?`-in-body case), and the Sink type set (writer / buffer / builder / response / hash /
size-counter — the consultation's sink list).

### Bare array literal in a value position (general lowering gap)

Surfaced by the `loop` adversarial review (2026-07-10). A bare array literal `[…]` is lowerable only
as a `let` initializer or a pipeline source; in any other **value** position it reaches
`lower_expr`'s `ArrayLit` arm and the `unreachable!` there panics the compiler (exit 101). The
`loop` slice patched the `break` case (`check_break` now rejects a bare-array-literal `break` value
with a sema diagnostic), but the same panic remains reachable via an `if`/`match` **arm** whose
value is a bare array literal (e.g. `x := if c { [1, 2, 3] } else { … }`). Fix options: a general
sema diagnostic rejecting a bare array literal in any free value position (bind it to a local /
`.to_array()` first), or a generalized lowering that materializes it into a fresh frame slot.
Two lower-priority review notes recorded alongside, both **not** defects: (1) a diverging (`break`-
less) `loop` bound to a `str`-annotated `let` can report a type mismatch — consistent with existing
diverging-value-position behavior (`return` in the same spot behaves the same), not loop-specific;
(2) the `break`-escape diagnostic wording is a nit (it reuses the return-escape phrasing shape).

**Separately found (pre-existing, loop-independent):** a plain block expression carrying an owned
`let` in a **value position** — e.g. `take({ s := make_string(); s.len() })` as a call argument —
miscompiles even with no loop (a loop-free program returns a garbage exit code, not `0`), so the
owned local's frame-exit drop / block-value handling is wrong for blocks nested in call-arg / tuple /
operand positions. Present on `main` before the `loop` work; it entangled with the `loop`
per-iteration-drop review (the review's example rides this same broken feature). The `loop` slice's
per-iteration drops are now correct regardless (the drop set is the body's declared-`LocalId` range
∩ `drop_locals`, so a `let` at any nesting/position is captured); the block-in-value-position
miscompile itself is the separate fix needed here.

### Unrecorded spec vacuums — remainder (recorded 2026-07-09; settle-next priority)

From the same audit as the sweep above. Leans are recorded so the next design session can settle
fast; "spec-debt" = no decision needed, just transcribe implemented truth into `draft.md`.

- **`assert`** — no user-level defined-abort-with-message exists. Lean: an `assert(cond)` builtin
  that aborts with source location (same trap class as bounds checks); a test runner is separate
  (toolchain, M12+).
- **`str` character access** — byte-first is deliberate, but "get/iterate Unicode scalars" has no
  answer *and no recorded rejection*. Lean: document as a deliberate omission (bytes + `find` +
  ranges cover the data path); add a scalar iterator only on concrete evidence.
- **Operator precedence table** — only the bitwise/shift slice is documented; the full table
  (unary ops, `as`, postfix `?`, `.`, comparisons vs `&&`/`||`) is spec-debt: transcribe from the
  parser into §5.
- **Stack-overflow behavior** — a "no UB" language must say: defined abort or not. Lean: verify
  the guard-page trap is a clean abort on all supported targets, then spec "aborts".
- **`main` signature set** — three forms appear in examples; the legal set is never enumerated.
  Spec-debt: transcribe from sema.
- **Reserved words + identifier grammar** — no keyword list exists; may a local be named `loop`?
  Are non-ASCII identifiers legal? Spec-debt from the lexer, plus one decision — lean: ASCII-only
  identifiers for v1 (AI/tooling-friendly, matches the English-only source policy).
- **C→Align (reverse FFI)** — general embedding (a C program hosting Align as a library) =
  **non-goal** (owner decision 2026-07-09): Align is an application language; exporting a stable
  C surface drags in closure-ABI + runtime-init questions for no aligned use case. The one real
  use is **callbacks** — passing an Align function to a C API during an Align-initiated FFI call
  (signal handlers; callback-style C libraries). Deferred with a trigger: a concrete std/pkg
  consumer that cannot use a stepped/non-callback C API. Ideal shape when triggered: a
  **top-level non-capturing `fn` only** (a plain function pointer — extern-compatible params, no
  closure environment, no new mechanism).
- **No-shadowing vs. function names** — a local/parameter may currently share a name with a
  top-level function (functions and values are separate namespaces today; fn-values are not yet
  first-class), which the no-shadowing settlement's wording (Settled → "No shadowing") does not
  explicitly cover. Decide whether function names join the shadowing check once fn-values land.

### align-LLM runway — std/language items pulled by the inference-engine north star (recorded 2026-07-09)

The owner's align-LLM spec (**v1.0 FINAL as of 2026-07-11**, out-of-repo; see Future →
"Resource-oriented north star + local LLM inference") is the planned killer app: a GGUF-input,
tiered-memory (VRAM/DRAM/NVMe), PGO-relayout local LLM inference engine, Phases 0–4 of which
(GGUF inspect / expert trace aggregation / cache simulator / alignpack generation) are pure
data-oriented Align programs. **v1.0 confirms the A-list unchanged** — Phase 0 is READY on
A1–A3 (the spec's §13.1 bytes API shipped as A2's method form), Phase 4 = A4, Phase 2 = A6+A7,
the gateway = A5-SSE + A8 — and adds one watch item: `json.encode` is flat-struct-only while
the gateway's OpenAI-compatible chat-completion payloads are nested; deferred until that
consumer arrives (builder/template can hand-build JSON in v1). The engine's tokenizer +
prefix-token hashing needs are covered by borrowed-via-FFI tokenizers and the shipped
`hash64/128`, not new language surface.
Working backward from it yields three lists. Items marked *(general)* are ordinary
fast-systems-programming needs that any Align user hits, not engine-specific.

**A. Build next (the M12 std-wave candidates, lean = build in this order):**

1. **`fs.read_bytes_view`** — a binary mmap view. **DONE.** `read_file_view` UTF-8-validates and
   returns `str`, so a multi-GB binary file (GGUF) cannot be mmap'd today; same mmap path minus
   validation, returning an arena-scoped `bytes`. Cheap; unblocks Phase 0. *(general — any
   binary-file work)* **Shipped:** `fs.read_bytes_view(path) -> Result<bytes, Error>` shares the
   `read_file_view` runtime (one `fs_read_view_impl` with a `validate` flag — regular-file fast
   path, owned-copy fallback, `munmap` at arena end, no `SIGBUS` handler), same arena region rule
   and errno mapping. The new `Scalar::Slice(PrimScalar)` payload lets a `slice` ride a `Result`
   (the borrowed-view sibling of `Scalar::Str`); a `slice<u8>` stays out of `tracks_region`
   (numeric slices remain freely returnable), so a new `region_bearing` predicate routes the
   escape check — a `bytes` view is caught escaping its arena through return / arena-block-value /
   match-arm-unwrap of `Result<slice<u8>, Error>`. `box<slice>` and array-literal-of-slices are
   deferred (rejected) like other views. Tests in `crates/align_driver/tests/m9_fs.rs`
   (byte-exact non-UTF-8 read, multi-page, empty, missing→NotFound, + four escape rejections).
2. **Binary decode/encode surface on `bytes`/`buffer`** — bounds-checked, endian-explicit
   `b.u32_le(off)` / `u64_le` / `f32_le` reads and the matching `buffer` writes
   (`put_u32_le`, …). Core-adjacent (pure computation); GGUF headers and `alignpack`/`alignidx`
   emission are the consumers. *(general — any binary format)* **DONE (design SETTLED + BUILT,
   branch `runway-a2-binary-codec`).** Design confirmed:
   - **Endianness = both LE and BE, explicit `_le` / `_be` suffix; never implicit.** GGUF /
     safetensors / ONNX are little-endian, but network/other formats are big-endian, so both are
     offered — and each `_le`/`_be` is a *distinct operation* (different result), not two spellings
     of one, so this is One-Way-compatible (like `select` vs `where`). A bare `b.u32(off)` that
     defaulted to LE would hide the byte order — rejected. Single-byte `u8`/`i8` carry no suffix.
   - **API.** Read on a `bytes` (`slice<u8>`) view: `bytes.<scalar>(off) -> scalar` for `scalar` in
     `u8, i8, u16_le, u16_be, i16_le, i16_be, u32_le, u32_be, i32_le, i32_be, u64_le, u64_be,
     i64_le, i64_be, f32_le, f32_be, f64_le, f64_be`. Encode on a growable `buffer` (a `mut` local):
     `buffer.put_<scalar>(v) -> ()` (the same 18-name set, `put_`-prefixed) plus
     `buffer.append(data) -> ()` (copy a raw `bytes`/`str`/`string` blob). Read-back via the existing
     `.bytes()` / `.len()`.
   - **Out-of-range policy = abort** (`off < 0` or `off + width > len`), reusing the slice
     range-bounds check — the **same fail-closed policy as `slice[i]`** (not a `Result`): a
     structural over-read is a bug, and a parser checks `.len()` first exactly as it checks a
     slice's length before indexing. (Overflowing `off + width` is caught by the `start > end` arm,
     so no out-of-range address is ever formed.)
   - **copy-out = not needed, deferred.** A decode read returns a **Copy scalar** (it never carries
     the view's region — so it composes freely and needs no owned-bytes), and encode grows an
     **owned `buffer`** whose `.bytes()` view stays in scope. Neither path needs a value to leave its
     arena, so the still-absent owned-bytes/copy-out shape (HANDOFF A1 note) is genuinely
     unnecessary here; it stays deferred until a consumer needs a `bytes` value to escape.
   - **Lowering.** Decode is **inline codegen** (a new `Rvalue::BytesRead` → an alignment-1 load,
     plus an `llvm.bswap` for `_be`; a float loads its bits then bit-casts), not an FFI call — a
     per-scalar call would be an optimizer barrier in a descriptor loop, against the data-oriented
     "lowers well" invariant. Encode is a runtime call (`align_rt_buffer_put` / `_append`) because a
     `buffer` is an opaque growable heap handle (like `.push`). Host little-endianness is assumed
     (x86-64 / aarch64), as elsewhere in the backend. `put`/`append` truncate the buffer to its
     logical `len` before appending, so encoding after a `reader.read` is well-defined. Records:
     `draft.md` §12 "Binary decode and encode"; `crates/align_driver/tests/runway_a2_binary_codec.rs`
     + runtime unit tests in `align_runtime`.
3. **`loop` implementation slice** — the settled design (Settled → "Sequential control") now has
   consumer pressure: streaming parses, the gateway server loop, the runtime scheduler. First
   unimplemented settle to build.
4. **`std.io` seek/pread/pwrite** (offset-addressed access) — `align-pack` is a
   read-at-X-write-at-Y relayout tool; sequential readers can't express it. *(general)*
   **Design SETTLED 2026-07-11 (two-lens review; full record = roadmap M12 Slice A4):** a Move
   `file` = the random-access WRITE handle (`fs.create_rw`/`fs.open_rw`, O_CLOEXEC) with
   `pread(mut buffer, off)` read-back (actual count, 0 = EOF) / `pwrite(bytes, off)`
   (loops-to-full) / `len()`; **no `seek`** (hidden cursor state) and **no read-only
   constructor** (reads stay reader | mmap; `fs.open_ro` = the deferred-with-trigger escape
   hatch); negative offset aborts; `copy_range` deferred.
5. **`std.http` server slice (Slice 4) + streaming (SSE/chunked) response write** — the
   align-gateway is an OpenAI-compatible SSE-streaming local server; Slice 4's design should be
   written against that requirement. **Slice-4 surface SETTLED 2026-07-10** (two-lens design
   review; full record in `http.md` Signatures + slice plan): `serve`/`accept`/`response_builder`/
   `ctx.respond`. The SSE commitment is pinned: streaming lands as a **sibling op**
   `ctx.respond_stream(rb) -> Result<http_stream, Error>` with Move `http_stream.send(chunk)` +
   Drop-terminates (needs a chunked *write* path; the v1 surface already admits it — `.body()` is
   optional). Two recorded prerequisites: **Move-capture-into-spawn** for *concurrent* serving
   (v1 = sequential accept loop, sufficient for the single-GPU gateway; distinct from the deferred
   fully-escaping fn values), and the v1 server's **trusted-network security caveat** (no read
   deadline → slow-loris DoS; recorded in `http.md` Known v1 limitations).
6. **Growable `array<T>`** *(general)* — the missing sibling of `buffer` (which stays
   byte-specialized per the settled `bytes`/`buffer` design): push/append + freeze to owned
   `array<T>`. Becomes acute the moment `loop` lands (accumulate-unknown-count is the natural
   loop output; today only bytes and strings can grow).
   **Design SETTLED 2026-07-11 (same review; full record = roadmap M12 Slice A6):**
   `array_builder<T>()` + `mut`-receiver `push`/`append` (Pure) + consuming `.build() ->
   array<T>` — the third grow-then-freeze member (no views until freeze → realloc-safe by
   construction; growable `array<T>` itself rejected on exactly that view-invalidation
   ground). Zero-copy freeze via a new `align_rt_realloc` (a Rust-`Vec` store was rejected —
   allocator-boundary mismatch with the C-free that frees `array<T>`). Elements v1 = Copy
   scalars + `string`; Copy structs deferred; Move handles excluded.
7. **Streaming line/record reads** *(general)* — `read_line`-class chunked record iteration over
   a reader; multi-GB `expert_trace.jsonl` is the concrete consumer for the already-recorded
   post-M9 "streaming×pipeline integration" backlog item.
   **Design SETTLED 2026-07-11 (two-critic review, Fable synthesis; full record = roadmap M12
   Slice A7):** the buffered READER (`r.buffered()`, the read dual of the buffered writer;
   lookahead is explicitly constructed, drain-before-fd interleaving contract) +
   `r.read_line(mut buffer)` (body-with-terminator-stripped into the buffer, returns
   consumed-incl-terminator, 0 = EOF; grows to a 64 MiB line cap) + the generic
   `bytes.as_str()` validating view (the view sibling of `bytes.to_string()`). Zero-copy
   views into the lookahead and a bespoke buffer `line()` op were both rejected. Perf
   follow-up recorded: json.decode's redundant re-validation of invariant str input.
8. **Arena checkpoint/rollback** *(general)* — already Open (see its entry); the consumer
   arrived: a long-running server loop resetting its arena per request (gateway, or any server).
   **Design SETTLED 2026-07-11 (two-critic review, Fable synthesis; full record = roadmap M12
   Slice A8):** the checkpoint API is REJECTED-with-reopen-trigger (second way + unsound without
   flow-sensitive epochs); `loop { arena {} }` is the safe-subset scoped form, and the slice is a
   pure-runtime thread-local `Box<Arena>` pool (unmap-first, chunks-only, re-zeroed, capped) behind
   a measure-first ship gate (≥ ~1.15× on the gateway shape, else record-and-close).
   **OUTCOME 2026-07-11: MEASURED, BELOW GATE, RECORD-AND-CLOSE — the pool was built to spec and
   benchmarked at ~1.06× over pre-pool (`bench/arena_pool`), short of the ≥ ~1.15× gate, so it was
   reverted (not shipped).** The mandated full-64-KiB re-zero (≈480 ns) dwarfs the ≈32 ns of
   malloc/free pooling removes; the no-re-zero variant is 13.5× (the recorded drop-the-re-zero
   follow-up), so the win lives entirely in that separate slice, not this one. Full record + measured
   matrix = roadmap M12 Slice A8; the feature-gated prototype is preserved in git history.

**B. Design stances to record now (implement with their consumers):**

- **Async = `task_group` + blocking I/O on worker threads; async/await is NOT adopted.**
  Overlapped NVMe reads / PCIe copies / compute are expressible as structured concurrency with
  blocking syscalls on workers — no function coloring (consistent with non-goals'
  "async-everywhere" exclusion). `io_uring` remains a runtime implementation detail behind
  `std.io`, never a language surface.
- **Shared mutable state across tasks — lean: channels (CSP), not user-facing atomics.** The
  engine's cache/scheduler state is genuinely shared-mutable, outside today's
  by-value-capture model. A channel is One-Way-compatible (one visible mechanism, Go precedent);
  raw atomics stay sealed unless channels are measured insufficient.

**C. Explicit non-adoption boundaries (do NOT let the engine pull these into the core):**

- **No `MemoryTier`/pinned/VRAM in the language.** Engine memory tiers are std/pkg **Move
  handles** (opaque FFI-backed resources — the `std.crypto`-over-OpenSSL pattern applied to VRAM
  buffers). The language memory model stays value/arena/heap; the killer app is a *consumer* of
  Align, not a shareholder in its core semantics.
- **`f16`/`bf16` stay Future.** Dequantization is the borrowed kernel's job (ggml); the engine
  adds no pressure to pull half-precision arithmetic forward.

### Expression-depth cap (128) vs the full-pipeline stack ceiling (~40) — M12+

**Recorded 2026-07-07 (surfaced by the std.crypto Slice-3 frame regression, #386).** The #296
front-end cap `MAX_EXPR_DEPTH = 128` governs parse-time rejection, and front-end-only checking
handles depth ~120 comfortably — but the **full parse→sema→MIR→codegen pipeline** overflows a
default 2 MiB thread stack at `+`-chain depth **~41** (measured on main, debug profile; the gap is
pre-existing, not introduced by any std slice). A machine-generated 41–128-deep expression
type-checks under the cap yet can crash the compiler on a small stack. Candidates: lower the
effective cap to the measured ceiling with margin; run lowering/codegen on a worker thread with an
explicit larger stack; make MIR lowering iterative for left-associative chains. Standing
mitigation convention (adopted during M11 crypto): recursive functions (`check_expr`,
`lower_expr`, the sema walkers) must not gain match arms with inline locals — arm bodies go in
`#[inline(never)]` free helpers, and wide MIR `Rvalue` payloads are boxed (debug builds reserve
every arm's locals per frame, so one fat arm taxes every recursion level).

### Module / import system — design SETTLED (2026-06-25), implementation in progress
**The last big language-core gap.** Today `module`/`import` are *parsed* into `File.module`/`File.imports` but otherwise **ignored** (single-file compilation; `core.*`/`std.*` are compiler builtins). Decided:

- **core stays builtin (language-intrinsic), and so does std for now.** core members are intrinsically compiler-magic — `core.json`/`core.template` need compiler-generated static field tables (`non-goals.md`: "compile-time story is builtin-driven static data only"), `map`/`where`/`reduce` fuse into one MIR loop, `core.vec`/`core.mask` lower to SIMD. They are language semantics wearing a library name, not hand-writable library code. **std** bottoms out in `align_rt_*` calls today; it becomes real Align-over-FFI library code only **after FFI** (post-M8), so it stays builtin until then.
  - **Preconditions for std-in-Align (recorded 2026-07-04, external design-note review adoption):** writing std in Align requires (a) function multiversioning — first candidate: compiler-automatic multiversioning of hot/pipeline fns + capability predicate builtins (NOT user-facing target-feature attributes; Align minimizes annotations), (b) arena-aware allocation from library code, (c) native soa layout access. Until then the C-ABI boundary costs (no inlining, fragile arena calls from Rust, hardcoded soa layouts) are the accepted tradeoff of the builtin approach.
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

### Lazy multi-source pipeline (`zip`) — SHIPPED 2026-07-15

**Recorded 2026-07-14.** The shipped pipeline fuses work derived from one input element but has no
canonical runtime-array/slice shape for `out[i] = f(a[i], b[i], c[i])`. Retain one lazy `zip`
pipeline source as the preferred direction, not `map2`/`map_with` or numbered `zip2`/`zip3`
siblings: equal runtime lengths (mismatch abort before iteration), increasing-index evaluation,
no allocated tuple array, and per-index tuple values expected to disappear in SSA. First slice is
Copy-scalar arrays/slices; `map_into` proves its output disjoint from every source without claiming
that sources are mutually disjoint. A directional clang-22 ceiling probe for
`a + b*c` measured a fused loop 1.44-1.75x faster than two passes with an already-allocated
temporary across 256..8,388,608 `f32` elements. The real ship gate is an Align consumer plus
equal-LLVM IR/assembly: one allocation-free vector loop, no tuple storage, and exact
length/effect/trap/alias behavior on x86-64 and arm64. Runtime-length `dot` may reuse the machinery,
but ordered floating-point reduction must not be silently reassociated. Full measurement and design
gate: `impl/12-pipeline-closure-memory-io-simd-audit.md` §4.3.

**Implemented 2026-07-15.** `zip(a, b, ...)` is a pipeline-only lazy source for two or more Copy
primitive-scalar arrays/slices. It emits one equal-length-guarded loop, per-index SSA tuples, no
tuple storage, and existing stage/reducer trap semantics. `map_into` proves the destination
disjoint from every source using one input-vs-output scope and makes no source-source no-alias
claim. The dedicated regression gate covers runtime/static mismatch, fusion/allocation/SIMD,
guarded traps, and both alias directions. Strided/Move/parallel forms remain consumer-deferred.

### Deep pipeline stage scaling — DONE / MEASURED 2026-07-15

**Recorded 2026-07-15; implemented and measured the same day.** The shared 1/2/4/8/16/32-stage
fixture covers arithmetic maps, branchless reducing `where`, capturing lambdas, and the deliberately
branchy general-callable-after-`where` case. A mandatory 2 MiB-stack integration test proves one
fused MIR loop, no intermediate/closure allocation, no residual simple-stage calls, legal SIMD
through depth 32, and successful object emission. The same-target O2 harness uses runtime input and
equal-LLVM C controls. On Ryzen 9 5950X / LLVM 22.1.8 all 24 native and x86-64-v2 points stayed
within 7.1% of control; depth-32 ratios were 0.981-1.011 native and 1.000-1.005 baseline. No
Align-specific depth cliff was found. The portable baseline's higher per-stage cost for long serial
dependency chains matched C and is therefore useful-work/code-shape cost, not pipeline abstraction
overhead. Compile time and sampled peak RSS are recorded separately. The broader accepted-expression
depth versus compiler-stack gap remains open above; a larger compiler stack is not a runtime
optimization. Full result: `impl/12-pipeline-closure-memory-io-simd-audit.md` §4.5 and
`bench/deep_pipeline/`.

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

### Arena checkpoint / rollback — SETTLED 2026-07-11: API rejected-with-reopen-trigger; scoped reuse instead
A lightweight `cp := arena.checkpoint()` / `arena.rollback(cp)` for `O(1)` bulk-free of everything allocated since a checkpoint, for long-running loops (event loops, packet/stream parsers) that must keep a flat memory footprint while reusing the same blocks. (Digested from `work/proposals/library-foundations.md` §3 + the `http-optimization.md` §5 streaming-parse story; the consumer arrived 2026-07-09 — the gateway per-request reset.)
**Settled (full record = roadmap M12 Slice A8):** the imperative API is REJECTED for v1 — it is a
second way (`loop { arena {} }` already expresses per-iteration bulk-free, verified working) and it
is unsound without flow-sensitive epoch tracking in the escape checker (a post-checkpoint view used
after rollback dangles — the MIR-dataflow follow-up). The scoped form restricts to the safe subset;
what ships is a pure-runtime **thread-local `Box<Arena>` pool** (unmap_all-first, 64 KiB-chunks-only,
re-zeroed on reuse, size-capped) behind a **measure-first gate**. The one inexpressible shape —
data-dependent checkpoint depth (speculative/backtracking parsers) — is the recorded **reopen
trigger**: revisit iff the MIR-dataflow escape checker lands AND a measured parser consumer appears
that recursion + pooling cannot serve. (The old "after MMv2" gate is moot — MMv2 completed.)

### Build system / package layout — pkg-foundation model (SETTLED 2026-07-20 — F0 v1 landed)

> **SETTLED 2026-07-20.** F0 v1 shipped: the two import-edge rules (D7 `internal` path rule + D8
> pkg-layering) are enforced in `align_driver::load_units` (`check_pkg_import_edge`), with the spec
> text landed in `draft.md` §17 "Packages" + §18.3, the `language-spec.md` digest, and the
> `design-notes.md` "package philosophy" rationale. Driver tests: `pkg_foundation.rs` (D7 within-package
> OK / outside-package + sibling-package rejected; D8 pkg→project rejected, pkg→pkg + pkg→std + project→pkg
> OK). The remaining Dn items (the fetch tool D11, compiled-library distribution D12, the `alignc deps`
> capability report) stay deferred/Future as recorded below — none blocks. The design record is preserved
> verbatim below.


Visibility (`pub`), import, and module are decided (`impl/02-frontend.md`); M15 shipped per-unit
interfaces + objects + the incremental cache — explicitly motivated by "compiled-library
distribution for the future pkg ecosystem". This entry is the design for that ecosystem's
foundation. **Target: consumer-gated** — implement when the first shared library exists (e.g. an
align-LLM component extracted for reuse, or the first third-party dependency). Nothing blocks on it.

**Consumer-gate OPENED 2026-07-20 (owner directive: framework-first).** The first shared library is
`pkg.web` — the zero-copy REST framework (Fiber-referenced; design at `impl/pkg-design/web.md`) —
built deliberately (not extracted); the v1 implementation scope below (the two import-edge rules +
spec text) is phase **F0** of `impl/15-pkg-web-plan.md`, the plan of record for the pkg-foundation
+ `pkg.web` sequence. This entry moves PROPOSAL → Settled when F0 lands with the spec text.

**Thesis: the package layer adds two path rules and zero new compiler concepts.** A "package" is a
*distribution-layer* unit — the subtree a tool (or a human) vendors under `pkg/` — and the compiler
never learns what one is. Resolution, visibility, effects, escape, capabilities: all carry over
unchanged from the settled module system. This is the M15 "driver-discovered unit graph, NO
manifest" decision extended to its conclusion: the *package* graph is also discovered from imports +
the filesystem, and a build is hermetic on the source tree alone.

**D1 — the first import segment is a trust tier.** `core` (language) / `std` (OS boundary) / `pkg`
(third-party) / anything else (this project). `core`/`std` are already builtin-reserved (never
resolved to files); `pkg` is hereby blessed as the third-party area. A file's import header thus
shows not just *what* it reaches but *whose code* it trusts — "nothing hidden" extended to
provenance. No new syntax.

**D2 — packages resolve by the EXISTING filename convention; no new resolution rule.** Today
`import a.b` → `<entry-dir>/a/b.align` (module decl must match). So `import pkg.router` →
`pkg/router.align` and `import pkg.db.postgres` → `pkg/db/postgres.align` — this works **today**
with zero compiler change (verified end-to-end 2026-07-19: a `pkg/router.align` root module + a
`pkg/router/util.align` submodule with an in-package absolute import, called from `main`, compiles
and runs on the current `alignc` unchanged). A package = its root module file `pkg/<name>.align` plus (optionally)
its submodule tree `pkg/<name>/…`; a single-module package is one file; a namespace directory
(`pkg/db/`) is owned by nobody and shared by its residents. No search paths, no `-I`, no env vars,
no registry lookup at compile time — what is on disk is what compiles.

**D3 — call sites stay fully qualified (`pkg.router.route(...)`); no import aliases.** Already the
module rule (`util.math.fn(...)`). An alias (`import x as y`) would hide provenance at the call
site — rejected for the same reason `where`/`filter` synonyms are. The trust tier from D1 is
therefore visible at every use, not only in the header.

**D4 — a package's own imports are absolute; the repo layout mirrors the vendored layout; vendoring
IS copying.** Inside `pkg/router/middleware.align`, a sibling import is written
`import pkg.router.util` — the same absolute path a consumer would write. Consequence: a package
author develops in a workspace whose own `pkg/<name>/` holds the package (plus root-level
example/test entry files), and publishing = sharing that subtree; a consumer vendors it by copying
it to the same place. No source rewriting on vendor (rewriting is hidden magic), no "develop layout"
vs "installed layout" split. The compiler cannot tell vendored code from hand-written code — by
design: your dependencies are ordinary source in your tree, fully greppable/auditable (maximally
AI-friendly: the whole dependency closure is in-context).

**D5 — ONE version of a package per build, by construction.** `pkg/<name>/` can exist once, so the
diamond problem is resolved by whoever populates the tree, not by a version solver; type identity
stays unambiguous (two versions of `pkg.foo.Point` can never coexist). An incompatible major
version is a **new name at publish time** (`pkg.router2`) — the Go `/v2` convention without the
special-cased path segment. No semver resolver, no MVS, no lockfile-driven builds: version
*selection* is a fetch-tool/human concern that ends before the compiler starts.

**D6 — the compiler stays manifest-free; hermetic builds.** `alignc build` reads `.align` files,
full stop. Dependency *names* are derivable from source (`grep 'import pkg\.'` — no manifest to
drift, same argument as M15's unit graph). Only *sources/versions* need recording, and that record
(`align.lock` at the project root: name → URL + rev + content hash, written and read **only** by
the future fetch tool) is a tool artifact invisible to the compiler. There is deliberately **no
build-configuration language** — `alignc build <entry>` + the M15 cache IS the build system; a
multi-binary workspace is one entry file per binary sharing the project root.

**D7 — `internal` path rule (the one new visibility rule).** An import whose path contains a
segment `internal` is legal only if the importer's module path starts with the path prefix up to
the `internal` segment's parent: `pkg/router/internal/pool.align` (`pkg.router.internal.pool`) is
importable from `pkg.router.*` only. Pure path rule (Go-proven), zero syntax, no package-boundary
metadata needed. Without it every module is forever public API and narrowing later is a break; with
it, `pub` keeps meaning "visible to my importers" and the path decides who may import at all. Also
applies outside `pkg/` (a project may hide its own internals from... nothing today, but the rule is
uniform and future-proof for compiled distribution).

**D8 — layering is enforced: a module under `pkg/` may import only `core` / `std` / `pkg`.** A
vendored package importing the consuming project's modules would compile in one tree and nowhere
else (and inverts the dependency arrow). Cheap path check at import resolution; keeps §18's
layering (core → std → pkg → project) a compiler-checked fact instead of a convention.

**D9 — one visibility model.** Module-level `pub` + the D7 path rule. No `pub(pkg)` / export lists
/ re-export machinery — a second visibility granularity is exactly the complexity budget Align
refuses (and D7 makes it unnecessary: hide a module by path, hide an item by omitting `pub`).

**D10 — dead modules cost nothing; no exclusion config.** The BFS compiles only modules reachable
from the entry's imports, so a vendored package's tests/examples/benches are simply never touched.
No "exclude" lists, no test-vs-src manifest keys.

**D11 — the fetch tool is deferred; manual vendoring is the v1 mechanism.** Copying a package's
subtree into `pkg/` is a complete, working dependency mechanism on day one (and stays the
ground-truth even after a tool exists — the tool only automates the copy + records provenance in
`align.lock`). A minimal `alignc pkg add <git-url>` verb, registry infrastructure, and signing are
all deferred until real consumers exist; none changes the compiler-side model above.

**D12 — compiled-library distribution: Future, already enabled by M15.** `InterfaceSummary` ships
generic `pub` bodies as source, carries three-valued effect bits and the capability (link-lib) set,
and has a versioned bounds-checked codec — so an interface + per-unit-objects bundle (a closed-source
package) is a packaging exercise, not a redesign. Deferred with record until a consumer demands it;
source-first remains the default distribution (auditable, cache makes recompiles cheap).

**Soundness carries over unchanged.** The DAG rule + bottom-up interface checking + body-blind
escape analysis + type-derived Move/Copy + interface effect bits were all designed
restriction-first in M15; a package boundary is just a module boundary, so cross-package inference
is exactly cross-module inference. Capabilities already flow: a pkg dep using `std.crypto` surfaces
its `-l` libs through the existing interface→link-union path.

**Future tooling (recorded, not v1):** a per-package capability report (`alignc deps`: package →
the `std.*`/`unsafe`/`extern` surface it reaches — derivable from imports + interfaces; the audit
story for AI-vendored dependencies), and a lint gating `unsafe`/FFI inside `pkg/` behind an
explicit allow.

**Not adopted (with reasons):** a compile-time-read manifest (drift; second source of truth); a
version resolver in the compiler (one-version-by-construction makes it moot); import aliases (hide
provenance); source rewriting on vendor (hidden magic); registry-first distribution (vendoring is
the primitive; a registry only feeds it); build scripts / config languages (the entry file + cache
is the build system); `pub(pkg)` granularity (D9).

**v1 implementation scope (when the consumer-gate opens):** ① the D7 `internal` path rule + ② the
D8 pkg-layering rule (both are import-edge checks in `load_units`/sema), ③ spec text — `draft.md`
§17 (the two rules) + §18.3 (replace the placeholder with this model), `language-spec.md` digest,
`design-notes.md` rationale. Everything else already works or is deferred above.

### FFI (foreign function interface) — v1 COMPLETE (keystone for the library strategy)
Detailed design of C / Rust / Zig interoperability. Because Align is AOT-via-LLVM with no GC, an external C call is a direct LLVM `call` at native speed (no pinning / stack-switch / marshaling), and an Align `slice`/`str`/`bytes` hands its raw pointer straight to C. **This gates a deliberate library strategy: "own the memory wrappers, borrow the mathematical engines"** — `std.compress` wraps `libzstd`/`zlib-ng`, `pkg` DB drivers wrap `libpq`/`sqlite`, etc., rather than re-implementing assembly-tuned algorithms in Align. So FFI's design should land before those `std`/`pkg` libraries are built, even though it stays out of the v1 *language* core. (Digested from `work/proposals/ffi-optimization.md`, `compression-strategy.md`, `rdb-optimization.md`.)

**First slice SHIPPED (2026-07-01):** `extern "C"` bodyless declarations + `unsafe`-gated direct calls; FFI-safe scalars (int/float) + `raw` + `()` return; libc/libm resolve with no extra `-l`. See the `unsafe`/`raw` Settled entry above for the full record.

**`layout(C)` struct ABI — slice 1 SHIPPED (2026-07-01):** a `layout(C)` attribute (composes with `align(N)`) pins a struct to a stable, C-compatible flat layout (decl order, natural alignment, no reordering — Align's default, which the marker *locks* and opts into FFI). Only a `layout(C)` struct may be moved through a `raw` pointer — `raw.store`/`raw.load` widened to accept a struct value (no new IR variant; the existing `Scalar::Struct` flows through `RawLoad`/`RawStore`, codegen does an unaligned aggregate load/store). Fields must be int/float. This is the **pointer-based** FFI pattern (hand C a buffer, read/write structs in it).

**FFI views — SHIPPED (2026-07-01):** a `str`/`slice`/`bytes` view is FFI-safe as an extern **parameter**, lowered to its data pointer (C `char*`/`void*`); the length is passed separately by the caller (`s.len()`) — the C `(ptr, len)` idiom, no hidden arg (`is_ffi_safe_param`; codegen `ffi_param_type` + an `extern_params` map that coerces the `{ptr,len}` arg to element 0). Slice element must be an int/float scalar (`slice<str>`/`slice<Struct>` rejected — no settled C element layout). Not a valid return type (a bare pointer has no length → returns stay scalar-only); not NUL-terminated (length-based C fns only).

**External library linking — SHIPPED (2026-07-01):** an `extern "C" link("name")` clause names a library to link (`-lname`); sema validates + dedupes into `hir`/`mir::Program.link_libs`, and the driver's `link_executable` appends `-l<name>` after the objects/runtime (libc/libm stay auto-linked). The name is charset-validated (`[A-Za-z0-9._+-]`) and passed as a single `-l<name>` argv (no flag/shell injection). `ast::ExternBlock.link`.

**FFI v1 — COMPLETE (2026-07-01).** The shipped surface: `extern "C"` decls + `unsafe`-gated calls; scalar/`raw`/`()` signatures; `layout(C)` struct-by-pointer (`raw.load`/`store`); `str`/`slice`/`bytes` views (data-pointer + separate length); `link("name")` external libraries. That is a coherent, tested v1 — the `std`/`pkg` C-engine wrapper strategy (zstd/sqlite/…) can be built on it (own the memory wrappers, borrow the engines, pass buffers by pointer+len).

**Measured optimization follow-up (2026-07-14) — recommended, consumer-gated.** The direct call and
view ABI are already at the useful floor; retain three generic additions rather than inventing a
second FFI ABI: (1) extend the guarded, per-symbol `--rt-lto` mechanism to optional pkg-provided
matching bitcode only after a real fine-grained foreign call clears the ~1.15x wall-time gate;
(2) make a loop-contained extern that actually blocks vectorization/fusion visible in `explain-opt`,
with shaped-op/batch/LTO suggestions rather than a blanket warning; (3) when the first backend pkg
needs a persistent context/buffer, design one pkg-definable opaque Move resource with exactly-once
Drop so `raw` and explicit destroy do not leak through the safe API. Foreign `readonly`/`noalias`/
`nounwind`/Pure contracts remain deferred behind a concrete missed-optimization IR: a false contract
is a miscompile or race, so visible-body inference or an audited generated wrapper is preferred over
casual user annotations. Whole-backend LTO, automatic batching, an FFI fastcall, and LLM-specific
surface are not adopted. Full rationale, gates, and the measured 2.95x-positive/0.72x-negative LTO
evidence: `impl/14-llm-inference-focus-audit.md` §7.

**Deliberately out of FFI v1** (draft §15 "Not in FFI v1", decided 2026-07-01 — defer over ship-half-right):
- **A struct by value** — SHIPPED for **x86-64 SysV (Linux) only** (`feat/ffi-byvalue-sysv`). A `layout(C)` struct ≤ 16 bytes is passed/returned in registers via the SysV AMD64 classification (each eightbyte INTEGER→`i64` slot / SSE→`double` slot; a two-register value returns as an `{T0,T1}` aggregate). The compiler emits exactly clang's coerced IR, so a call is binary-compatible with a real C callee — proven by a compiled-C-helper harness (`crates/align_driver/tests/ffi_byval.rs`) that links a `cc`-built by-value callee and round-trips every eightbyte pattern ({i32,i32}/{i64,i64}/{f64,f64}/{f32,f32} packed/{i32,f32} merge/mixed {i64,f64} return). This is the one FFI corner a wrong per-target rule *silently miscompiles*, so it is structurally fenced: **codegen refuses on any non-SysV target** (diagnostic: pass by pointer instead) rather than guessing; a **> 16-byte MEMORY-class struct is rejected** (redundant with struct-by-pointer); and — the subtle one — a struct argument that would **fall to memory under register pressure** is rejected too. SysV's all-or-nothing rule passes a struct in registers only if every eightbyte fits in the class registers left after preceding args, else the whole struct goes `byval` on the stack; clang implements that reclassification in its frontend, and a flattened `{i64,i64}` at the exhaustion boundary makes LLVM split the struct across the last register and the stack (verified round-trip corruption vs a clang `byval` callee), so those signatures are refused rather than miscompiled (reorder the struct earlier, or pass by pointer). In every accepted case the struct fits in registers and per-eightbyte flattening is byte-identical to clang's own flattened parameter form. Still deferred: AAPCS64 (other arches), and the MEMORY-class `byval`/`sret` path (added only when a concrete wrapper needs a large by-value struct).
- **`bool` / `char` as FFI types** — use the integer types (C `_Bool` = `u8`, `char` = `i8`/`u8`, `char32_t` = `u32`; a `wchar_t` is platform-sized — pick the matching integer width). Align `char` is a 32-bit Unicode scalar (**not** a C `char`), so admitting it would invite the wrong mapping; `bool` stays out for the same one-unambiguous-way reason (and dodges the `i1`-`zeroext` ABI subtlety). Note: there is no `bool as int` cast today, so a `bool` reaches C as `if b { 1 } else { 0 }`.
- **`raw.ptr_cast<T>`** — a *typed* reinterpret has nothing to reinterpret to while `raw` (opaque bytes) is the only pointer type; it earns meaning once FFI grows typed/external pointers.

### REST-gateway runway — `Option<T>`/`array<T>` struct fields + `core.json` nested/optional targets (filed 2026-07-18)

**Target: the first post-M15 core/std consumer wave. Consumer: the OpenAI-compatible REST gateway
(the align-LLM runway A5 surface — `std.http` serve + SSE shipped in M11/M12 and waiting on exactly
this).** An OpenAI `chat/completions` exchange is nested and optional-heavy on both sides:

```text
request:   { model: str, messages: array<Message>, temperature?: f64, stream?: bool, ... }
response:  { id: str, choices: array<Choice>, usage: Usage }   // Choice { message: Message, ... }
SSE delta: { choices: array<DeltaChoice> }                     // DeltaChoice { delta: Delta, ... }
```

Two independent gaps block declaring/decoding/encoding these today, in different layers:

1. **Language: the struct field-type whitelist.** `is_field_ok` (`align_sema`) admits only
   `Int`/`Float`/`Bool`/`Char`/`Str`/`String`/nested `Struct` as struct fields — `Option<T>` and
   `array<T>` fields are rejected at declaration. The OpenAI shapes cannot even be *declared*,
   regardless of what `json.decode` supports.
2. **`core.json`: the decode-target whitelist.** The verified matrix (`impl/core-design/json.md`)
   covers flat structs and top-level arrays only; nested-struct / Option-field / array-field
   targets are recorded there as "design work before code". `encode` is flat-struct-only.

**Status (updated 2026-07-18): Slices A + B + C SHIPPED — the full OpenAI chat-completions
request/response round-trips through `core.json`. Follow-ups: B's `Option<struct>` ENCODE, C's
`array<scalar>` field decode + owned-element arrays. NEXT: the owner-directed JSON-completeness push
(enum/union payloads finish the gateway's `content` union; then `JsonValue`/map + streaming/validate
close the rest — no "this JSON shape works, that one doesn't" gap).**

**Plan — three slices, each shippable and ideal-form on its own:**

- **A. Nested-struct fields in `core.json` (json-only; no language change). — SHIPPED 2026-07-18.**
  Nested `Struct` fields are already legal, so recursive field-descriptor tables on decode +
  recursive `encode` through the builder path were pure `core.json` work. Delivered: the runtime
  `JsonField` carries a kind-4 `JsonSubTable` pointer (nested descriptors + PHF + store size), and
  both `parse_object` (slow path) and `write_field_indexed` (Mison speculative path) recurse — a
  nested field is one record-level colon whose value the record-splitter leaves at a deeper bracket
  depth, so the flat colon-ordinal speculation is undisturbed (json.md P1/P2 honored). `encode`
  recurses through `json_object_parts` with an extended field `path`; `IndexField` was generalized
  from a single `field` to a `Vec<u32>` path (reusing the existing `elem_field_ptr`/
  `phys_field_indices` nested-path machinery) so a fixed struct-array element with a nested field
  encodes uniformly — no partial support. `struct_has_str` recurses so a nested-`str` struct is
  region-tied to the input recursively. **Also fixed a pre-existing stale-cache miscompile this
  slice would have extended (the #514/#517 class):** a decode target struct's field name/type feeds
  only the codegen descriptor, not the surrounding MIR, so a field RENAME at the same slot (or a
  nested struct's field change) left `impl_hash` unchanged → the warm object cache served a stale
  object decoding the OLD key (reproduced end-to-end). The `JsonDecode*` MIR rvalues now bake a
  recursive `json_schema_sig` (names + types + `layout(C)`/`align`, nested expanded) printed into
  the MIR; pinned by `cache_codegen.rs` gate 2b (flat + nested). Tests: `m5.rs`
  (`json_decode_encode_nested_struct_roundtrip`, `json_decode_nested_struct_array_mison`), runtime
  descriptor-level (`json_decode_nested_struct_single`/`..._array_mison`), example
  `examples/json_nested.align`. (The broader `layout(C)`-toggle / field-reorder offset-cache concern
  for NON-json struct field access is untouched here and remains a separate pre-existing question.)
- **B. `Option<T>` struct fields (language) + optional-field decode/encode (json). — SHIPPED
  2026-07-18 (decode + encode of scalar/str/nested-struct Options; `Option<struct>` ENCODE is the
  one recorded follow-up).**
  - Field support DONE: `is_field_ok` admits `Option<T>`; the layout pair `ty_size_align` (sema) ↔
    `option_struct_type`/`field_abi_align` (codegen) agree on the `{ i8 tag, payload }` layout —
    `layout_parity` extended with every payload kind + reorder + `layout(C)`. `struct_acyclic`
    recurses through `Option<Struct>` (a `Node { next: Option<Node> }` is still rejected);
    `struct_has_str`/`tracks_region`/`ty_may_borrow` recurse through Options (region soundness).
    **v1 restriction (owner-directed clean boundary): an `Option` field's payload must be NON-OWNED**
    (scalar / `str` view / plain-data struct). `Option<string>` and `Option<Move-struct>` are
    rejected at declaration (pass 0b-2) — an owned Option payload needs a conditional "free iff Some"
    drop-as-a-field path with no consumer yet, and `Scalar::Struct.is_move()` is table-free so it
    would mis-classify the struct as non-Move and leak. This adds ZERO owned-drop surface (json only
    ever fills view/scalar/plain-struct Options) and covers the whole consumer.
  - **Null policy SHIPPED as settled: missing key → `None`; JSON `null` → `None`; type mismatch →
    `Err`; a required (non-`Option`) field still `Err`s when missing.** `encode` omits a `None`
    field entirely (never `"k": null`). One absence representation (One way); `decode(encode(x))`
    round-trips by construction (pinned by `json_option_field_decode_encode_roundtrip`).
  - **Decode mechanism:** the runtime `JsonField` gains `opt_tag` (`-1` = required, else the
    `Option` tag byte offset); an optional field is not required by `all_required_seen`, and its
    payload writer (the single-sourced `write_value`, shared by the slow + Mison paths) writes at the
    payload slot then sets the `Some` tag — `null`/missing leave the zeroed `None`. **Encode
    mechanism:** an `Option`-bearing object switches to a trailing-comma layout — every present field
    emits `"name":value,` and one `align_rt_builder_pop_comma` before `}` drops the dangling comma
    (`{"a":1}` / `{}`); the `TemplatePart::OptionField`/`PopComma` pieces carry it (a pure-required
    object keeps the original static layout — zero regression). **Deferred (the one follow-up):**
    `Option<struct>` ENCODE (a conditional nested object rendered from the payload value) — decode
    supports it; scalar/str Option encode ships. Tests: `m5.rs`
    (`json_decode_option_fields_null_policy`, `json_decode_option_struct_field_in_array`,
    `json_encode_option_fields_omit_none`, `json_option_field_decode_encode_roundtrip`),
    `layout_parity` Option cases.
- **C. `array<T>` struct fields (language) + `array<Struct>` decode/encode (json). — SHIPPED
  2026-07-18.** The `messages: array<Message>` / `choices: array<Choice>` shape — the full OpenAI
  request/response now round-trips (`json_full_openai_response_shape_roundtrip`). `is_field_ok`
  admits `array<T>`; the field owns ONE heap AoS buffer freed by the struct's `Drop`
  (`drop_struct_fields`'s new array arm). **v1 element restriction (non-owned, like Slice B): a
  scalar / `str`-view / plain-data (non-Move) struct** — `array<string>` / `array<Move-struct>`
  rejected at declaration (their per-element deep free is a later slice). `struct_acyclic` does NOT
  recurse through `array<Struct>` (a heap indirection, so `Node { children: array<Node> }` trees
  are legal). **Decode:** a new descriptor kind 5 (sub = element schema); the runtime
  `decode_struct_array_value` parses the JSON sub-array into an owned AoS via `parse_object` per
  element (nested/`Option` element fields recurse), writing `{ptr,len}` to the field. **Encode:** a
  dynamic length can't unroll, so a `StructArrayField` template piece calls the runtime
  descriptor-driven encoder `json_encode_struct_array`/`json_encode_object` (reusing the DECODE
  descriptors — symmetric, handles nested/Option/str/scalar). **Memory-safety:** the decode `Err`
  path frees any AoS buffers already written into the partial struct (`drop_decoded_owned`, the
  runtime dual of `drop_struct_fields`) — pinned by `json_array_field_error_path_frees_buffer`
  (alloc==free). **Known constraint:** a Move struct (owns an array) can't be a `Result`/`Option`
  Ok payload that crosses a function boundary (pre-existing) — decode + use it in the same scope.
  Deferred: `array<scalar>` field decode (`array<i64>`), `array<string>`/owned-element arrays.
  `soa<T>` keeps excluding Move-fielded structs (the settled owned-columns deferral stands). Tests:
  m5 (`json_array_struct_field_decode_read_and_roundtrip`, `json_full_openai_response_shape_roundtrip`,
  `json_empty_array_struct_field`), runtime (`json_decode_array_struct_field_and_encode`,
  `json_array_field_error_path_frees_buffer`).

**Acceptance (the consumer is the test):** decode a real `chat/completions` request (messages +
optional params), encode a real response and an SSE `delta` chunk through `respond_stream`, in an
`examples/` OpenAI-compatible server. Deferred out of this runway: enum-payload targets (the
string-or-parts multimodal `content` union — v1 restricts `content` to `str`),
`json.scan`/`json.token` streaming, `validate<T>`.

**Direction notes recorded with this filing (not slice work; framing for later):**
- **Server form:** the gateway is a `match (method, path)` dispatch app — for a fixed-path API
  this is the ideal form (every route visible in source and to the compiler; 405/404 are explicit
  arms), not a stopgap awaiting a router. No router is on this runway's critical path.
- **`std.http` protocol floor, consumer-gated:** query-string parse + percent-decode
  (`ctx.query(name) -> Option<str>` — RFC 3986 has one correct answer → std, the sibling of
  `ctx.headers().get(name)`) and an SSE event-framing helper (WHATWG-defined) join `std.http` when the
  gateway or a successor actually needs them; server keepalive lands invisibly per the existing
  plan. The boundary rule: protocol (spec-defined, one correct answer) may enter std;
  convention/policy may not.
- **`pkg.router`/`pkg.web` stay pkg and stay deferred** (draft §18.3: DB drivers and web
  frameworks are not core/std; `:id` segment conventions, middleware chains, `ctx.json()` sugar
  are policy, not protocol). Double-gated on (a) fully-escaping fn values (storing handlers in
  structs/arrays) and (b) the build-system / package-layout / dependency-resolution design
  above. Re-evaluate on real reuse pressure from shipped apps — extraction over invention.
  **SUPERSEDED 2026-07-20 (owner re-decision): framework-first.** `pkg.web` — the **zero-copy,
  blazing-fast REST framework** (the owner's restored brief: primary reference Go's Fiber /
  fasthttp zero-allocation philosophy; router reference the httprouter radix-tree lineage) — is
  built deliberately FIRST; the gateway/LLM apps are merely its first consumers, explicitly later.
  Gate (b) opens via the pkg-foundation proposal's F0 scope; gate (a) was probed narrower than
  recorded — the router needs **non-capturing** fn values as struct fields / array elements
  (Copy/`Static`, no environment; capturing escaping closures stay deferred — middleware-lite runs
  without them). Design: `impl/pkg-design/web.md` (performance contract + surface + W1–W7). Plan
  of record: `impl/15-pkg-web-plan.md` (F1 field-eligibility widening ∥ F0 pkg foundation → F3
  framework slices → F4 the LLM gateway app later).

### JSON completeness — DESIGN SETTLED 2026-07-18 (owner-approved; the implementation source of truth)

**Owner directive (2026-07-18): make `core.json` *holistically complete* — no more piecemeal "this
JSON shape is supported, that one isn't."** The design below was settled with the owner (three
forks decided 2026-07-18: lazy document view / shape-directed unions / catalog trimmed). It is the
source of truth for the implementation slices J1–J6; spec text lives in draft §14 + §18.1.

**"Complete" = three tiers, plus a catalog with no dangling entries:**

```text
Tier 1  typed decode/encode, full matrix   (schema known  — the §14 identity, completed)
Tier 2  json.doc lazy document view        (schema unknown — zero-copy, arena-backed)
Tier 3  json.scan streaming rows           (larger than memory — pipeline source)
```

**T1a — unions: shape-directed sum-type mapping (SETTLED).** A JSON `oneOf` (the OpenAI multimodal
`content: str | array<Part>`) maps to an Align sum type; the variant is selected by the JSON
value's **shape class** — `Str` (`"`), `Number` (digit/`-`), `Bool` (`t`/`f`), `Object` (`{`),
`Array` (`[`) — an O(1) dispatch on the first structural byte, no backtracking. **Compile-time
restriction (the Align move — restriction buys determinism):** a union-decodable enum has every
variant carrying exactly one payload, each payload mapping to one shape class, all classes
**pairwise distinct** — two object-payload variants (or `i64 | f64`, both `Number`) are a compile
error naming the clash. Tag-only variants are rejected (a string-enum name mapping is future).
`null` is deliberately NOT a class: absence belongs to `Option` (`Option<Content>` composes for
nullable unions) — one absence representation. Encode is the inverse image: the live variant's
payload encodes **bare** (no wrapper key), so round-trip holds by construction. Object-vs-object
discrimination (internally-tagged `{"type": …}`) is NOT a second rule — that shape is expressed
today as a single struct with `Option` fields; revisit only on real pressure. **Language
prerequisite (the bulk of the work): enum `str` + owned payloads.** Today `enum_payload_ok` allows
only primitive scalars / `str`-free plain structs because enums are neither region-tracked nor
dropped. The union work extends enums: region-tracked iff any variant payload tracks a region
(`str` view payloads — `tracks_region`/`region_of`/`ty_may_borrow` grow `Ty::Enum` arms); Move
iff any payload is Move (`array<T>` payloads — drop switches on the tag and frees the live
payload; MoveCheck/`null_moved_source`/`drop_struct_fields` grow enum arms; element restriction =
Slice C's non-owned rule). The Gate-1 sibling-pass sweep applies in full.

**T2 — dynamic JSON: `json.doc`, a lazy document view (SETTLED — NOT a serde-style value tree).**
A `serde_json::Value` heap tree (per-node allocation, pointer-chasing) contradicts Nothing hidden
+ data-oriented and would drag in recursive enums and a map type; rejected. Instead the simdjson
"on-demand" model, built on the existing SIMD structural index: `d := json.doc(s)?` inside an
`arena {}` parses once into an arena-backed tape (`Result` — malformed input is an `Err`).
Navigation after the parse is **total, Missing-propagating** (`?` is Result-only, so per-step
`Option` unwrapping would be unusable): `d.get("k")` / `d.at(i)` always return a `json.doc`; a
missing member / out-of-range index yields `kind() == Missing`, which propagates through further
navigation, so absence surfaces exactly once — as `None` from a leaf accessor
(`as_str/as_i64/as_f64/as_bool -> Option<…>`). `kind() -> json.kind { Object, Array, Str, Number,
Bool, Null, Missing }` distinguishes JSON `null` from absence when asked; both yield `None` from
`as_*`. Keys-as-data (object-as-`map`) is `d.key(i) -> Option<str>` + `d.at(i)` over an object's
ordered members — NO map type enters the language. `d.len()` is 0 on a non-container; `d.elems()
-> array<json.doc>` bump-materializes child handles so ordinary pipelines run over a document
level (the Align idiom instead of an iterator protocol). `json.doc` is a Copy view handle
region-tied to min(input, arena); nothing escapes the arena un-cloned. Implementation details
recorded for the slice: `.at(i)`/`.get(k)` are linear at one nesting level (tape sibling-skip
offsets make each hop O(1); use `elems()` for whole-level loops); an escaped string in `as_str()`
unescapes into the arena (bump, bulk-freed — the one allocating accessor, documented).

**T3 — streaming: `json.scan` (SETTLED → SHIPPED as J5, #546 + #547).** NDJSON /
top-level-array streaming typed by the binding annotation (`rows: json.scanner<Row> :=
json.scan(view)` — the schema-selector residual resolves the same way `decode` does; never a
turbofish). The scanner is a **pipeline source only** (fused terminals:
`rows.where(.active).map(…)…`), which sidesteps per-row escape/invalidation entirely — a row view
borrows the current chunk and dies with the stage. Measured basis: streaming projected scan beat
materializing 2.7–2.9× at 1–5M rows. **SHIPPED:** `Ty::JsonScanner(Row)` (a Copy `{ptr,len}` input
view, region-tracked; no arena — rows decode into a per-step stack slot, `str` fields borrow the
input) + the full streaming reducer family `sum`/`count`/`reduce`/`any`/`all`/`min`/`max` → each
`Result<T, Error>` (a malformed row → `Err`, byte-identical to `decode`'s `Error.Code(1)`), full stage
set (`.field`/`.where(.field)`/`.where(pred)`/`.map`), one scanner covering both a JSON array and NDJSON.
Materializing terminals over a stream are rejected in sema. draft.md §18.1 + language-spec.md already
described it exactly (design ran ahead), so no spec-sync was needed. **With J5, the JSON-completeness
arc J1–J5 is COMPLETE.**

**Catalog trimmed (SETTLED — dangling entries removed, not left "unimplemented"):**
`json.validate<T>` **deleted** (decode-and-discard IS validation with zero-copy costs; one way);
`json.token` **deleted** (doc + scan cover the realistic cases; no consumer — build it only if one
appears, as a Future note); `json.field_table<T>` **deleted** (a compiler-internal artifact, not
API). §18.1's core.json surface becomes exactly: `decode`, `encode`, `doc`, `scan`. This also
closes the no-turbofish settled item's "schema-selector residual" — `scan` is the one survivor
and it types from the binding annotation.

**T1b — matrix fill (impl, no new design): COMPLETE.** top-level scalar/bool targets (`x: i64 :=
json.decode(s)?` — SHIPPED, #539), `array<scalar>` struct fields (SHIPPED, #538), `Option<struct>`
ENCODE (the B follow-up — SHIPPED, #540). Rule: any composition of supported constructors closes; the
v1 non-owned boundaries stay explicit (`array<string>` waits for owned-element drop).
**`array<Option<T>>` DEFERRED (not a JSON gap — a language-type gap).** The T1b sketch listed it as a
"supported-constructor composition", but it is NOT one: an owned `array<T>`'s element is a
[`PrimScalar`] (the deliberately **non-recursive, `Copy`** subset — see `Scalar::DynArray`'s doc), and
`Option<T>` is a composite, not a `PrimScalar`. So `array<Option<T>>` is un-representable in the type
system today (it is rejected everywhere, not just in JSON — `[Some(1), None]` fails to type). Closing it
needs a dedicated **composite-element owned-array type** (`array<Option<prim>>` / eventually
`array<array<T>>`) threaded through the whole pipeline (a new `Ty`/`Scalar` variant, layout, drop,
region, decode/encode) — a real language-surface addition, not a JSON matrix-fill, and low-value (a
`[1,null,3]` sparse numeric array is rare in the gateway/align-LLM shapes the shipped `array<scalar>`
already covers). Per "ideal form or defer — no compromise", it waits for the composite-element-array
language feature rather than a special-cased JSON-only hack. **So JSON completeness now advances to J4.**

**Slices:** **J1a — SHIPPED: enum `str` payloads with region tracking** (the design's "region
tracking pending" prerequisite). `enum_payload_ok` / pass 0c admit a `str` view and a `str`-bearing
plain-data (non-Move) struct payload; the enum is region-tracked iff any variant payload is —
`tracks_region` / `region_of(EnumValue)` / `ty_may_borrow` grew precise `Ty::Enum` arms (the enums
table is now threaded into `EscapeCheck` / `MoveCheck` / the MIR `Builder`, and `ty_may_borrow`
takes it as a param). A scalar-only enum stays Copy / freely returnable; a `str`-bearing enum
cannot escape the region backing its view (construction, match-result, and match-binding escapes
all caught). Never Move (a `str` borrows) — owned payloads are J2. Tests: `enum_match.rs` (J1
section). **J1b-1 — SHIPPED (#532): enum as a struct field** (`is_field_ok`/`struct_has_str`/
`ty_size_align`/`struct_acyclic`/`field_abi_align` grew a `Ty::Enum` arm; `enum_types` build before
struct bodies; `layout_parity` pins the enum-field layout). **J1b-2a — SHIPPED: top-level
shape-directed union decode/encode** over str/number/bool/object payloads. A union-decodable enum =
every variant one payload, each mapping to a shape class (Str/Number/Bool/Object), pairwise distinct
(compile-checked — `check_union_decodable` + `union_shape_class`). Runtime `JsonUnion` descriptor
(per-variant payload arm + shape-class→arm + arm→enum-tag tables) + first-byte dispatch
(`decode_union_value`/`align_rt_json_decode_union`); encode writes the live payload bare
(`json_encode_value` factored out of `json_encode_object`, `align_rt_json_encode_union`). New
`Rvalue::JsonDecodeUnion` + `TemplatePiece::UnionValue` swept through every exhaustive HIR/MIR pass
(region_of gives input-region for a str-bearing union, Static for scalar-only); `json_union_schema_sig`
baked into the MIR for cache invalidation. Tests: `m5.rs` J1b section. **J1b-2b — SHIPPED: union as a
struct field** (`Message { content: Content }`) — a JSON descriptor **kind 6** whose `sub` is the
`JsonUnion` (reused decode+encode); `field_width`/`write_value` (all decode paths) + `json_encode_value`
grew a kind-6 arm; sema `decode_struct_fields_ok`/`json_object_parts` grew enum-field arms (both check
union-decodability so `emit_json_union` never sees a bad enum); `json_schema_sig_into` expands a union
field (cycle-safe). Composes with nested / `Option` / `array<Struct>` fields — the full
`Chat { messages: array<Message> }` shape round-trips. Tests: `m5.rs` J1b-2b (field decode/encode,
object-payload + Option coexist, array<Message>, non-union-enum-field rejected). **J2a — SHIPPED: enum
owned `array<T>` payloads + tag-switched drop (the language prerequisite for the multimodal union).**
A sum type may now carry an owned `array<T>` payload (`Content { Text(str), Parts(array<Part>) }`),
which makes the **enum a Move type**: its `Drop` switches on the tag and frees the live variant's
owned buffer (`drop_enum` in codegen; `DropFlagInit` zeroes the aggregate → null-safe on a moved-out /
unconstructed path). `enum_is_move` (any payload `is_move()`) is the new classifier; it is threaded
into `is_owned_droppable` / `needs_drop_flag` / `ty_is_move` / `ty_capture_is_move` (the enum arm) —
NOT into `struct_is_move` (22 sites); instead a **Move enum struct field is rejected** (a non-Move enum
field stays allowed, J1b). Pass 0c admits `array<scalar>`/`array<str>`/`array<plain-struct>` (one flat
free — the Slice-C element rule: `array<string>` / `array<Move-struct>` deferred; a non-representable
element like `array<array<T>>` is a clean error, never a panic). `enum_payload_ok` (generic enums)
grew the same arm. MoveCheck consumes an owned payload at construction (like a struct literal) and
`null_moved_source` nulls the source slot (no double-free); the region/escape arms (`region_of` folds
to `Static` for an owned-array payload → freely returnable; `str`-payload enums stay arena-bound) are
J1a's, unchanged. **v1 confinement (fail-closed):** a Move enum is a bare local / param / return only —
rejected as a struct field, `Option`/`Result` payload (the wrap site), fixed-`array` element, lambda
capture, box/tuple/task payload (some pre-existing) — each with a clean "later slice" diagnostic, so no
owned enum leaks past a drop mechanism that can't tag-switch it. **Match-binding an owned payload
works** — `lower_match_enum` already nulls the scrutinee on a bound arm (`null_moved_source`, extended
with the enum arm), so the binding moves the buffer out and the scrutinee's drop frees null, the same
path `Result`/`Option` use for their Move payloads (no special sema handling). The owned `array<Struct>`
payload is exercised end-to-end in J2b (JSON). Tests: `enum_match.rs` J2 section (construct/move/return/if-join drop-clean, 7 deferral
rejections, the nested-array non-panic). **J2b — SHIPPED: the union Array shape-class arm.** A JSON
`[` now dispatches to a union's owned `array<Struct>` variant (shape class Array=4, O(1) first-byte),
so the full multimodal **`Content { Text(str), Parts(array<Part>) }`** union decodes both shapes
(`"hi"`→Text, `[{…}]`→Parts) and encodes the live payload **bare** — `decode(encode(x))` round-trips
byte-identically. The whole decode/encode/drop-cleanup pipeline was **already** kind-5-aware from Slice
C (`json_payload_tag_sub` → `emit_json_union` arm; runtime `json_shape_class('[')`=4,
`decode_union_value`/`write_value`/`field_width`, `encode_union_at`/`json_encode_value`), so the change
is minimal: sema `union_shape_class` `Scalar::DynStructArray => Some(4)`; `check_union_decodable`'s
class table `[_;4]` → `[_; JSON_SHAPE_CLASSES]` (an Array arm would else panic OOB) + recurse into the
array **element** struct (decodable check); MIR `json_union_schema_sig_into` expands the element
struct's schema (`[]{…}`) so an element-field rename invalidates the cache (#514/#517 class, pinned by
`cache_codegen` gate 2c); runtime `drop_decoded_union` frees the materialized AoS on the
trailing-garbage error path (the one new leak surface — the Align side has no bound value to drop on an
`Err`; pinned by an `alloc-count` new==free gate). v1 boundary: `array<scalar>` union payloads
(`Scalar::DynArray`) stay rejected ("no shape class") — no descriptor arm yet, deferred to J3. Tests:
`m5.rs` J2b (decode-by-shape, encode-bare + round-trip, trailing-garbage no-leak, two-Array clash,
Move-element rejected). **The union itself closes here; the remaining gateway shapes —
`Message { content: Content }` (a Move-enum struct field) and `Chat { messages: array<Message> }` (an
`array<Move-struct>`) — are J3 compositions** (both need the deferred owned-enum-struct-field / owned
array element, not new union work). **J3a — SHIPPED: the multimodal union as a Move-enum struct
field** (`Message { content: Content }`). A Move enum (an owned `array<Struct>` payload variant) is now
a legal struct field — it makes the enclosing struct **Move**, so the full multimodal
`content: str | array<Part>` union composes into a record and decodes/encodes both shapes, round-tripping
byte-identically. **Classifier:** `struct_is_move`/`struct_is_move_rec`/`ty_owns_buffer_rec` grew an
`enums` param + a `Ty::Enum` arm (`enum_is_move`) — threaded through every caller (mir `Builder`,
codegen, sema's `ty_is_move`/`is_owned_droppable`/`ty_capture_is_move`/`reject_move_struct_payload`/
`enum_payload_ok`), so **every** Move-ness question (MoveCheck, drop-flag, escape, Result/Option-payload
rejection) sees a struct-with-Move-enum-field as Move in lockstep. The pass-0c-2 rejection was lifted.
**Drop:** `drop_struct_fields`'s new `Ty::Enum` arm frees the live variant via the tag-switched
`drop_enum`; `DropFlagInit` zeroes the aggregate → null-safe on a moved-out / unconstructed path.
**Match-move:** `match m.content { Parts(ps) => … }` moves the owned payload out of the field — extended
`null_moved_source`'s depth-1 `Field` arm (a Move-enum field, mirroring the `string`-field case) + made
`NullStructField` codegen type-aware (zero the whole `{tag,payloads}` enum aggregate, not just a 16-byte
slice), so the struct's exit `Drop` frees null there — single-free (the same use-consumes-the-enum
semantics a bare-local scrutinee already has). **JSON:** decode/encode were already kind-6-aware (J1b-2b);
only the runtime `drop_decoded_owned` grew a **kind-6** arm (`→ drop_decoded_union`) to free the union's
owned payload on the trailing-garbage error path. **`array<Move-struct>` rejection relocated to a new
post-0c pass (0c-3):** an element struct can be Move *only* through a not-yet-resolved enum field, so the
0b-2 check (enums empty) would let `Chat { messages: array<Message> }` slip through with a leaking flat
free — moved after 0c where `struct_is_move` is enum-accurate; `array<Message>` is now cleanly rejected
(the `array<Move-struct>` deep-free is the next J3 slice). **v1 confinements (unchanged, now Move-enum
accurate):** a Move struct (owns via its enum field) is confined exactly like any owned struct —
rejected as a `Result`/`Option` Ok/Err payload across a function boundary (Slice-C constraint, so a
decode target uses `?`), and reassigning a Move-enum field leaks the old buffer (only a direct `string`
leaf is drop-of-old'd today — the SAME pre-existing gap `array<T>` fields have, not new). `/align-self-review`
(Gate 1 Move-reason sweep; Gate 3 no-panic) + `/code-review` high run. Tests: `enum_match.rs`
(construct/match-move/drop-clean), `m5.rs` J3 (both-shape decode/encode+round-trip, match-move no double-free,
trailing-garbage no-leak, `array<Message>` rejected). **J3b — SHIPPED: `array<Move-struct>` struct
fields (the owned-element deep free), closing `Chat { messages: array<Message> }`.** An `array<Struct>`
field (and a standalone `array<Struct>` local) whose element is Move — a `string`/owned-array/Move-enum
field, transitively — is deep-freed via a shared codegen `deep_free_struct_array` helper (a runtime loop
over `len` recursively dropping each element, then freeing the AoS) called from both
`drop_struct_fields`'s array arm AND `Stmt::Drop`'s standalone-local arm (the Gate-1 sibling hole); the
runtime decode error path deep-frees each element (`drop_decoded_owned` kind-5, gated by a new
`sub_owns_buffers` walk) and the mid-array partial (`decode_struct_array_value`'s `cleanup_partial`)
before bailing. The J3a pass-0c-3 rejection is lifted; `array<string>` (bare-string element) stays
deferred at 0b-2. **The OpenAI chat gateway now closes end-to-end** (`Chat` round-trips byte-identically).
v1 limits: `json.encode` of a bare `array<Move-struct>` and pipelines over such a field stay restricted
(decode→encode passthrough works). Tests: `m5.rs` (full gateway round-trip, standalone-local drop,
`array<string>`-element rejection), runtime alloc-count deep-free gates (a shared `ALLOC_COUNT_LOCK`
serializes the count-asserting tests). **T1b (part 1) — SHIPPED: `array<scalar>` struct fields** (`array<i64>` / `array<f64>` / `array<bool>` —
the align-LLM embeddings / token-id shapes). A new JSON descriptor **kind 7** (the field slot is width 16;
the element scalar's kind/width/sign pack into the tag's upper bits). Decode parses via the shared
per-scalar `write_value` (same range/sign/float-width checks per element); encode emits `[e0,e1,…]` via a
runtime loop (`ScalarArrayField` template piece → `align_rt_json_encode_scalar_array`). Composes with J3b
(a scalar-array field inside an `array<Move-struct>` element). Drop: the owned buffer flat-frees
(`drop_struct_fields`'s `DynArray` arm on success; `drop_decoded_owned` kind-7 on the decode error path;
`sub_owns_buffers` gained kind 7). Also fixed a pre-existing #514/#517 stale-cache bug: MIR `ty_name`
rendered a bare `"array"` for `DynArray`, so a `array<i64>`→`array<f64>` element change didn't invalidate
the decode cache (now renders the element). `array<str>` (borrowed element) / `array<char>` deferred. v1
limits: `.sum()`/pipelines over an owned scalar-array field and `json.encode` of a bare `array<scalar>`
stay restricted. Tests: `m5.rs` T1b, `cache_codegen` gate2d, runtime alloc-count. **T1b (part 2) — SHIPPED: top-level (bare) scalar decode targets** (`x: i64 := json.decode("42")?` for
int / float / bool). Parses the WHOLE input as one JSON number / bool; the value is `Copy` (copied out,
not a view), so the result is `Static` / returnable. New HIR/MIR `JsonDecodeScalar` → runtime
`align_rt_json_decode_scalar` (via the shared per-scalar `write_value` — same range/sign/float-width
checks; trailing non-whitespace → `Err`). Bare `str` (input-borrowing view) / `char` deferred. (The
top-level `array<scalar>` target already existed — MMv2 slice 8c.) **T1b (part 3) — SHIPPED: `Option<struct>` ENCODE** (the Slice-B follow-up; decode already supported it).
`Some` → the nested object via the runtime descriptor-driven encoder (a new `OptionStructField` template
piece + `align_rt_json_encode_object` FFI); `None` → the field omitted (trailing-comma + `PopComma`).
Composes recursively; the payload struct must be non-Move (`Option<Move-struct>` stays rejected —
Slice-B). Also fixed a pre-existing #514/#517 stale-cache bug: `json_schema_sig` folded an `Option<struct>`
field to a bare `"Option"`, so a decode-only payload field rename didn't invalidate the cache — now
recurses into the payload (and renders `Option<scalar>` width). Tests: `m5.rs` T1b, `cache_codegen`
gate2e. **T1b is now COMPLETE** — `array<Option<T>>` is DEFERRED as a language-type gap (an owned array
of a composite element needs a dedicated `Ty`/`Scalar`, not a JSON matrix-fill; see the T1b entry above).
**J4 `json.doc` — SLICE 1 SHIPPED** (the schema-unknown lazy document view MVP): a Copy `{tape,node}`
handle (`Ty::JsonDoc` / `Scalar::JsonDoc`, region-tied to min(input, arena)); `json.doc(s)?` parses
ONCE into an arena-backed tape (`Result<json.doc, Error>` — malformed = `Err`, requires an enclosing
`arena {}`); total Missing-propagating navigation `d.get(k)` / `d.at(i)` (always a `json.doc`, Missing
= `node < 0`); `d.kind()` → the builtin **`json.kind`** sum type (`Object/Array/Str/Number/Bool/Null/
Missing`, matched by bare variant name); and the four leaf accessors `as_str`/`as_i64`/`as_f64`/
`as_bool` → `Option` (`as_str` is a zero-copy input view, escaped strings unescape into the arena — the
one allocating accessor). Runtime = a `simdjson`-style flat node array (per-node sibling-skip offsets;
recursive validate-and-emit build) in `align_rt_json_doc_*`. Schema-UNKNOWN, so no `json_schema_sig`
cache key (the tape is generic — nothing to stale). Threaded through every exhaustive HIR/MIR pass
(region_of = min(input, arena) for the doc / receiver-region for `get`/`at`/`as_str` / `Static` for
`kind`/`as_i64/f64/bool`; `tracks_region` + `ty_may_borrow` + `borrow_sources` = the input roots).
Tests: `m5.rs` (navigate + kind-match + leaf accessors, malformed→Err, arena gate, view-escape
rejection), runtime `json_doc_*`. **J4 SLICE 2 SHIPPED:** `d.len()` (member/element count, 0 on a
non-container) + `d.key(i) -> Option<str>` (i-th object key in document order — objects-as-ordered
data); with `at(i)` these iterate a doc array by recursion (no `loop` needed). The builtin type names
`json.doc` / `json.kind` are now **nameable** in annotations (resolved directly in `resolve_type` before
the import/`pub` check), so a `fn f(d: json.doc)` helper / a `k: json.kind` binding work. Tests: `m5.rs`
`json_doc_len_and_key_iterate_via_recursion` (recursive sum over a doc array + key-order), runtime
`json_doc_len_and_key`. **J4 SLICE 3 SHIPPED — J4 COMPLETE:** `d.elems() -> slice<json.doc>`
materializes one level (each Array element, or each Object member **value**) as an arena-backed
`slice<json.doc>` **once** (O(n) build, O(1) index — vs `at(i)`'s O(i) re-walk). The key realization:
`Ty::Slice(Scalar)` already takes a full `Scalar` and `Scalar::JsonDoc` already exists, so
`slice<json.doc>` = `Ty::Slice(Scalar::JsonDoc)` needs **no new array type** — it reuses the existing
slice `.len()` + `xs[i]` machinery (`check_index`'s Move-element rejection doesn't fire — a `json.doc`
is a Copy 16-byte handle, no double-free; `region_of(Index)` binds the element to the slice). The slice
is nameable as a param type (`fn f(xs: slice<json.doc>)`), so a level is walked by recursion.
Runtime `align_rt_json_doc_elems` bump-allocates a `DocHandle` buffer in the arena (checked_mul,
null-safe) and writes the `{ptr,len}` header; region = arena.shorter(doc). New HIR/MIR `JsonDocElems`
threaded through every exhaustive pass (region = arena-tied like `to_array`; not local-backed).
Tests: `m5.rs` `json_doc_elems_materialize_and_iterate` (materialize → index → recursive sum) +
`json-doc-elems-escape` (returning the slice out of the arena rejected, #297), runtime
`json_doc_elems_materializes_a_level`. **Follow-up (not required for J4):** full `.map`/`.where`
**pipeline fusion** over a `slice<json.doc>` (closures taking `json.doc`) — index + len + recursion
already cover level iteration. →
**J5** `json.scan` — **COMPLETE** (#546 slice 1 `sum`/`count`, #547 slice 2 `reduce`/`any`/`all`/`min`/
`max`; streaming typed rows, `Result<T,Error>` terminals). **J6 spec sync — not needed:** draft.md §18.1
+ language-spec.md already described `json.scan` exactly (design ran ahead; the implementation matched
it), so the authoritative spec is already consistent. **The JSON-completeness arc J1–J5 is COMPLETE.**
sweep (draft §14 two-tier framing done at design time; per-slice updates as they land). Each slice
ships ideal-form or defers per CLAUDE.md.

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

**Pinned-memory arena option (Future, recorded 2026-07-04, external design-note review adoption):**
an arena flavor allocating page-locked (pinned) memory, enabling zero-copy DMA to GPUs (`cuMemcpy` et
al.). Same shape as the huge-page option above. GPU integration itself needs NO language surface:
every vendor (CUDA Driver API, ROCm/HIP, Vulkan, Metal, WebGPU-native) exposes a C ABI, and Align's
soa/array layouts match GPU buffer layouts bit-for-bit — plain FFI suffices (consistent with
non-goals: no GPU syntax).

Placement: `std.io` (OS boundary, `draft.md` §18.2), implemented in the Rust runtime
with a portable fallback; cross-platform mmap via a crate (e.g. `memmap2`). Revisit
around the string/JSON milestone (M5) and std build-out.

**Status update (2026-07-03, M9 std design):** the v1/reference portable fixed-buffer loop for
`io.copy` is scheduled as `impl/07-roadmap.md` M9 Slice 2 (memory-bounded, `O(buffer)`, tested).
The fast paths above (`sendfile`/`splice`/`io_uring`/Direct I/O) stay **post-M9**, added later
without an `io.copy` signature change.

**Correctness correction (audit 2026-07-13, FIXED):** the v1 loop was memory-bounded but not a
byte-exact oracle from every valid reader state. After `read_line`, a buffered reader could hold unread
lookahead while its fd offset was already ahead; `io.copy` read the fd directly and skipped those
bytes. It now drains through the shared reader path before fresh fd reads. The permanent byte/count
gate is in `impl/12` §3.6; every future syscall fast path must retain the empty-lookahead precondition.

**Status update (2026-07-03, M9 Slice 3 DONE):** the **`mmap` scan path** landed as `fs.read_file_view`
(`draft.md` §18.2), the one place a view's backing is bound to a region (the enclosing arena;
`munmap` at arena end). Two guardrail decisions were resolved concretely in v1:
- **Special / untrustworthy-size files → owned copy, not an error.** `fstat` gates `mmap` to a
  regular file with a nonzero size; a character device / FIFO / `/proc` file (`st_size` 0 or a lie)
  and any zero-length file take an **owned arena-copy fallback** (read the real bytes into arena
  memory, return a view of that). The record said "avoid" such files — v1 reads them correctly at a
  changed cost class (a copy, not zero-copy) rather than erroring; the view stays arena-bound either
  way, so no API/region change.
- **`SIGBUS` on post-`mmap` truncation → documented limitation, no handler.** The mapping length is
  fixed at `mmap` time from the `fstat` size; if the file is truncated afterward, touching the lost
  pages raises `SIGBUS`. v1 installs **no** `SIGBUS` handler — a process-global signal handler is
  exactly the hidden global side effect Align forbids ("nothing hidden"), and per-mapping recovery
  needs `sigsetjmp`/`siglongjmp` out of v1 scope. Concurrent truncation of a mapped file is the
  caller's contract to avoid (a known limit, not UB in the language sense).

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
  Lives in core.str / core.math. Reference pointer (builder/fmt, recorded 2026-07-04, external
  design-note review adoption): Dragonbox float formatting.
- Compile-time template compilation (Askama/Sailfish class): parse the template at build time,
  emit direct buffer-write code. core.template's compiler-generated static field tables already
  take this shape; recorded as confirming prior art (reference pointer, 2026-07-04, external
  design-note review adoption).
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
  zero-alloc itoa/atoi (an extension of the existing fast atoi/itoa lever). Reference pointer
  (UTF-8, recorded 2026-07-04, external design-note review adoption): Lemire 2021 (<1 inst/byte);
  already cited at its fix-PR call site, noted here only for the consolidated list.
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
Provenance: surfaced by an external idea review (2026-07-02); verified. **Reference pointer
(recorded 2026-07-04, external design-note review adoption):** Faro & Külekci's SIMD-filter
substring-search family (EPSM class) — confirms the shipped approach (simple wide compare-and-filter beats
Boyer-Moore-style skipping on modern hardware); no code change implied.

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
  (Update 2026-07-09: the `loop` expression is now design-settled — Settled → "Sequential control" —
  so this lint gains its firing surface once `loop` is implemented.)
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
  std.compress (FFI wrappers over libzstd/zlib-ng — gated on FFI). Reference pointer (std.encoding,
  recorded 2026-07-04, external design-note review adoption): Lemire Base64-at-memcpy-speed.
- pkg (frameworks/ecosystem, kept out of core/std): HTTP/3 client+server, socket tuning
  (TFO/REUSEPORT/thread-per-core), RDB drivers (Postgres/MySQL/SQLite), the API-server
  blueprint. DB ecosystem delegation is already Settled above.
```
These ride on the core capabilities (arena, views, FFI, task_group, zero-copy I/O); they
are downstream consumers, scheduled after the core + std foundations, and are recorded here
only so the vision is not lost when `work/proposals/` is discarded.

### std.rand (Future)

**Design direction (recorded 2026-07-04, external design-note review adoption):** seed from the OS
(`getrandom`/`urandom` — never raw `RDRAND`/`RNDR`: not in the x86-64-v2/armv8-a baseline, `SIGILL`
on older silicon), then a fast portable PRNG (PCG or Xoshiro256++ class, pure bit ops, SIMD-friendly
at baseline). Non-cryptographic per `draft.md` §18.

**Status update (2026-07-04, M10 scope decision):** this direction is now the M10 Slice 2 design —
full signatures settled in `draft.md` §18.2 (`rand.seed`/`seed_with`/`r.next`/`r.range`/`r.shuffle`/
`r.sample`, a Copy `rng` value), scheduled in `impl/07-roadmap.md` M10. No longer purely a Future
direction; implementation is next.

### std.crypto (Future)

**Hard requirement (recorded 2026-07-04, external design-note review adoption):** all secret-dependent
code paths MUST be constant-time (no secret-dependent branches or memory addressing; CMOV/bitwise
only) regardless of speed — the one domain where Align's branchless-for-vectorization machinery
becomes a correctness requirement, not a perf choice.

**Status update (2026-07-04, M10 scope decision):** stays deferred to M11+ — out of M10 scope
specifically because the constant-time requirement above needs verification, not just
specification, before implementation (`impl/07-roadmap.md` M10).

**Status update (2026-07-07, M11 — engine decision SETTLED):** the FFI engine is **OpenSSL
libcrypto (EVP), floor ≥ 3.2, always-linked `-lcrypto`** (the compress always-link precedent) —
superseding the design doc's original "libsodium recommended". Decided from two independent
reviews (security lens + dependency lens) that converged: libcrypto covers every required
primitive natively in one trust surface (HKDF + Argon2id via `EVP_KDF` — libsodium 1.0.18-class
has no HKDF, which would force a self-hosted HKDF seam or a second engine), its AES-GCM is
constant-time on supported targets without libsodium's hardware API-gating (AES-NI/PCLMULQDQ path,
CT vpaes fallback; T-table AES only on exotic targets — recorded as an unsupported-platform note),
and it is a universal system lib (libsodium isn't even linkable on a default install). **blake3 is
deferred with record** (no system engine provides it; self-hosting violates the borrow-the-engine
rule; aliasing BLAKE2b under the name is forbidden). The AEAD wrapper's mandatory all-or-nothing
shape under EVP (internal buffer, SET_TAG before Final, `OPENSSL_cleanse` on failure, single
opaque error) is specified in `impl/std-design/crypto.md` P2. Slice 1 (`constant_time_equal` +
`crypto.random`, engine-independent) started the same day.

**Status update (2026-07-11, M13 capability linking):** the linking and version-floor portions of
the engine decision above are superseded. The driver now adds `-lcrypto` only when a used Crypto or
TLS capability requires it; engine-independent `crypto.random` and `constant_time_equal` do not
request it. Most EVP-backed operations work with OpenSSL 3.0. Only `crypto.argon2id` requires the
`ARGON2ID` provider added in OpenSSL 3.2, and reports its absence as `Error.Code`.

**Status update (2026-07-07, evening): std.crypto COMPLETE (PRs #384–#388)** — and the hard
requirement above was met as *verification*, not specification: `constant_time_equal`'s
branchless property was confirmed by disassembling both shipped profiles (no content-dependent
branch, no memcmp idiom; vectorized OR-reduction + `black_box` barrier + lone `sete`), and the
AEAD all-or-nothing path was traced line-by-line with `OPENSSL_cleanse` confirmed live in the
optimized artifact. Full shipped-feature record in the roadmap's M11 section. Deferred with
record: blake3, zeroize-on-drop key buffers (P6), nonce-generating seal convenience (P3),
`OSSL_set_max_threads`, fixed-size `array<u8; N>` returns.

### std.ndslice — strided multi-dimensional views (Future)

**Recorded 2026-07-04 (external design-note review adoption).** A std-layer (not syntax) strided 2D+
view over contiguous storage — `{ptr, rows, cols, stride}` with `img[y,x]` / `.roi(x,y,w,h)` as
library methods — the OpenCV `cv::Mat` / numpy `ndarray` shape. Rationale: signal/image processing
wants pitch/ROI views; `soa` already gives planar (RRR GGG BBB) layout — the superior form
interleaved formats must convert to. Belongs M10+, after a concrete consumer.

**Measured qualification 2026-07-14:** runtime row pitch did not prevent LLVM from vectorizing a
fixed 3x3 stencil, so keep this as a minimal std view rather than new indexing/layout syntax. The
valuable consumer-side work is a dedicated dynamic stencil/window-dot operation carrying
known-stride, legal traversal-order, alias, and interior/border facts: an output-lane implementation
was about 8-9x faster than the natural dynamic nested loop, while cache-friendly legal loop
interchange was about 31x faster than column-strided traversal in the measured controls. Treat
separable kernels as an explicit API property because the two-pass transform changes ordered
floating-point evaluation. See `impl/12-pipeline-closure-memory-io-simd-audit.md` §4.2. Build remains
consumer-gated; these results do not justify an image-specific feature or a general loop DSL.

### Zero-copy serialization (Cap'n Proto / FlatBuffers class) (Future)

**Recorded 2026-07-04 (external design-note review adoption).** Align's region system makes zero-copy
deserialization *safe* — a wire-format view is region-tied to its mapping arena exactly like
`fs.read_file_view` (the #339 machinery above is already the substrate). Recorded as a distinctive
future capability; concretize on demand.

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

**Measured engine-focus audit 2026-07-14:** `impl/14-llm-inference-focus-audit.md` adds no language
surface. It prioritizes execution-order model/tier layout over blanket page advice; a backend-chosen
KV descriptor/layout (ordered CPU fallback probes found complementary K-score and V-accumulation
layouts worth roughly 5-14x, with an explicit K-append tradeoff); and conditional stable MoE expert
grouping (worth roughly 1.3-1.6x only for the measured large/diverse-route batch, neutral at batch 1
or when a four-expert hot set already fits cache). Quantized matvec/attention/`matmul_id` remain
borrowed-backend operations first. The real engine must profile cold/warm, prefill/decode, context,
batch, route skew, cache/tier residency, and bytes moved before selecting any policy.

**Mining-adjacent tooling** (profiler / autotuner / energy-aware scheduler / pool client) shares the
"make cost visible" instinct but is a **weaker** north star: ASIC/electricity economics dominate, mature
GPU miners are hard to beat, and the hot loop is tiny/specialized. Acceptable as a side experiment;
**not** a core language driver. Do not optimize the language around speculative profitability.
