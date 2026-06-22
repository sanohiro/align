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

### Type declaration syntax
**Decision: keyword-less.** Contains `ident: Type` → struct; `ident`/`ident(...)` → sum type, disambiguated by content. Fields/variants are `,`-separated.
Record: `draft.md` §4, `impl/02-frontend.md`

### Purity model
**Decision: compiler inference (no explicit marks).** Effects (Pure/Impure) are inferred from the body, and `par_map` etc. require Pure closures.
Record: `impl/03-types.md` §8

### Ownership syntax
**Decision: ownership is a property of the type, not a keyword.** `array<T>`/`string`/`buffer`/heap are Move; primitives/small structs/`slice` (view) are Copy. No `owned` modifier is introduced. Lifetimes are inferred and lifetime syntax is not surfaced.
Record: `impl/03-types.md` §6–§7

### SIMD exposure (basic policy)
**Decision: `vec<N,T>` + auto-vectorization as the baseline.** Make mask first-class.
(Whether to place explicit SIMD intrinsics in std is open, see below)
Record: `draft.md` §9, `impl/04-mir.md` §4, `impl/05-backend-llvm.md` §5

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
SSO is **not** adopted (its own Settled entry above). Element indexing is implemented: `recv[index]` (array/slice/owned array → scalar) and `arr[index].field` (a struct-array element's field), both bounds-checked. Still open / separate tracks (not part of this decision): tuples / multi-value returns (for `partition`), `array<slice<T>>` (for `chunks`), `array<Struct>.clone()`, a bare whole-struct element value `users[i]` (no field), and `out` params + `noalias` (below).
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
scalars **and `str`** (a `str`-bearing tuple is region-tracked, region-tied to the view's source,
and arena-`str` tuples are escape-checked). Owned (`string`/`array<T>`) elements (which make a
tuple Move + need element-wise drop) + `partition`/`chunks`/`min_with_index` are the additive
follow-ups (see the Open note). Record: `draft.md` §5 (Types → Tuple), `impl/02-frontend.md` §8,
`impl/03-types.md`, `impl/07-roadmap.md`.

### Type-argument syntax: no turbofish (expression position)
**Decision (2026-06-22): there is no expression-position type-argument syntax.** A call's type parameters are recovered by inference — from a value argument (`json.encode(u)`) or from the expected type propagated from context, including back through `?` (`u: User := json.decode(d)?`). When neither supplies the type it is a hard error directing the user to annotate the binding; an explicit `f<T>(x)` / `f::<T>(x)` form is **not** adopted. Rationale: keeps "one way" (the binding annotation is the single place a type is written), removes the `<` vs comparison parse ambiguity at expression position outright (the reason Go uses `f[T](x)` and Rust `::<>`), and is friendlier to generate. The headline case — `draft.md` §19's `json.decode<array<User>>(data)` — therefore becomes `users: array<User> := json.decode(data)?`; the checker already takes `decode`'s target from the expected `Result<T,_>` and emits an annotate-the-binding error otherwise (no code change needed — only the spec/comment caught up). **Residual (still open):** a *schema-selector* builtin whose type appears in neither arguments nor result (`json.validate<T>`, `json.field_table<T>`); narrow, unimplemented, and may fold into `decode`. This rule scales to general generics (below): a return-only type parameter is supplied by the binding annotation, never a turbofish. Record: `impl/02-frontend.md` §8 (generics `<` vs comparison), `draft.md` §18 (core.json), `language-spec.md` (JSON).

---

## Open (to be decided)

Each item is tagged with a target milestone for resolution (`impl/07-roadmap.md`).

### Generics (minimal system) — before M4
Structural-constraint inference vs explicit bounds (trait-style). Unit of monomorphization implementation. Value generics for `vec<N,T>`. Required to write core in Align itself (`impl/03-types.md` §9, `impl/06-runtime-std.md` §10). Note: the *call-site* surface is already settled — no expression-position type arguments (see "Type-argument syntax: no turbofish" under Settled); a return-only type parameter is supplied by the binding annotation.

### Error type design — M2
single `Error` / typed errors / error categories. Includes the `E → E'` conversion rule for `?` and the exit-code mapping (`impl/03-types.md` §5, `impl/06-runtime-std.md` §9).

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

### `out` parameters + `noalias` — fold into the post-MMv2 aliasing work
`out` params (`draft.md` §7) are a no-alias optimization. The compiler's `EscapeCheck`/`MoveCheck` (Memory Model v2) already track which views may overlap; an `out` param / non-aliasing view lowers to LLVM `noalias` metadata, letting loop vectorization skip runtime overlap checks. **Schedule right after MMv2** (it is a direct extension of the analysis being written there), not in a separate far-future phase. (Digested from `work/proposals/optimization-milestones.md` §1.2, `toolchain-optimizations.md` §5; see also `08-memory-model-v2.md` §11 "out parameters".)

### SoA conversion trigger
Whether to automate the decision to lay out `array<T>` as SoA, or use annotation. Impact on the array ABI (`impl/05-backend-llvm.md` §2). (Subsumed by "SoA layout" above; kept as the open auto-vs-annotation sub-question.)

### Tuples / multi-value returns — design SETTLED (see Settled); implementation in progress
The *design* is settled (first-class anonymous tuples; multi-value return = returning a tuple —
see "Tuples / multi-value returns" under Settled). The **foundation is implemented**: the
`(T, U, …)` type, literals, destructuring `(a, b) :=`, positional `.N`, tuple params/returns, for
primitive scalars **and `str`** (region-tracked). What remains is purely additive *implementation*,
not design: **owned** (`string`/`array<T>`) tuple elements (make a tuple Move; reuse the
owned-aggregate + drop machinery from MMv2 slice 8 for element-wise drop / move-out), then the
`partition` (`(array<T>, array<T>)`) and `chunks` (`array<slice<T>>`) terminals that consume them,
and `min_with_index`-style `(value, index)` reductions.

### Arena checkpoint / rollback — std arena API, after MMv2
A lightweight `cp := arena.checkpoint()` / `arena.rollback(cp)` for `O(1)` bulk-free of everything allocated since a checkpoint, for long-running loops (event loops, packet/stream parsers) that must keep a flat memory footprint while reusing the same blocks. The runtime arena already bump-allocates; this exposes a reset-to-mark on top. (Digested from `work/proposals/library-foundations.md` §3; used by the streaming-parse story in `http-optimization.md` §5.)

### Build system / package layout
Visibility (`pub`), import, and module are decided (`impl/02-frontend.md`). What remains is the design of the build system, package layout, and dependency resolution.

### FFI (foreign function interface) — after M8 (keystone for the library strategy)
Detailed design of C / Rust / Zig interoperability. Because Align is AOT-via-LLVM with no GC, an external C call is a direct LLVM `call` at native speed (no pinning / stack-switch / marshaling), and an Align `slice`/`str`/`bytes` hands its raw pointer straight to C. **This gates a deliberate library strategy: "own the memory wrappers, borrow the mathematical engines"** — `std.compress` wraps `libzstd`/`zlib-ng`, `pkg` DB drivers wrap `libpq`/`sqlite`, etc., rather than re-implementing assembly-tuned algorithms in Align. So FFI's design should land before those `std`/`pkg` libraries are built, even though it stays out of the v1 *language* core. (Digested from `work/proposals/ffi-optimization.md`, `compression-strategy.md`, `rdb-optimization.md`.)

### Details (settled during implementation)
```text
- Numeric literal suffix set and default-type lint
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

```text
Backend / codegen lowering (MIR -> LLVM, source unchanged):
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
- panic=abort build + strip .eh_frame: drop the Rust-std unwinder (smaller binary, cleaner
  I-cache). The no-unwind CFG itself is already Settled; this is the build-flag half.

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
