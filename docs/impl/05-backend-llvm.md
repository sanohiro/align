# Backend: MIR → LLVM

Implementation model for `align_codegen_llvm`. It commits to **pure lowering**—Align's semantic
decisions (desugaring, fusion, SIMD legality, ownership/region) are already represented by MIR
(`04-mir.md`), and here we mechanically lower MIR to LLVM IR. Types and explicit runtime operations
are carried in MIR, so we **do not recompute** them (anti-rewrite, `00-overview.md`).

```text
MIR (optimized)  →  LLVM IR  →  object (.o)  →  [driver links] → executable
                                                  + align_runtime (06)
```

The implementation uses LLVM 22 through Rust's `inkwell` bindings. Remaining portability and
optimization-policy questions are collected in §10.

---

## 1. Type correspondence (Ty → LLVM type)

Map MIR's `Ty` (`03 §1`) one-to-one to LLVM types.

```text
Bool              i1 (i8 when stored)
Int(w, signed)    iW            (sign distinguished by the operation)
Float(32|64)      float | double
Char              i32 (Unicode scalar)
Unit              void for a function return; it has no ordinary SSA value
Vec(n, T)         <n x T'>      ← maps directly to LLVM vector type
Mask(T)           <n x i1>
Bitset            iN / [iW]
fixed array<T,N>  [N x T]                        inline, contiguous
owned array<T>    { T* ptr, i64 len }            owned, contiguous
Slice(T, _)       { T* ptr, i64 len }            view (Region does not surface in the type)
Str               { i8* ptr, i64 len }           (+ meta is separate, §6)
String/Buffer     { i8* ptr, i64 len }            owned headers
Builder           pointer to runtime builder state
Named(struct)     %struct.S = type { each field }   (layout is §2)
Named(sum)        { i32 tag, payload fields... } tagged aggregate
Option(T)         { i8 tag, T payload }          tag 0=None, 1=Some
Result(T,E)       { i8 tag, T ok, E err }        tag 0=Ok, 1=Err; inactive owned payloads are zero
Fn(..)            function pointer (+ environment pointer if there is a capture)
```

`Region` **does not appear in LLVM**. Safety is already verified in HIR (`03 §7`); codegen receives only the concrete value (an arena pointer, etc.). This is the final destination of "do not surface lifetimes".

---

## 2. struct layout

The default is **AoS** (row-major, the value-type-centric `draft.md`). **SoA**, which helps for data parallelism, is treated as a transform over arrays.

```text
AoS   array<User> = contiguous User → { User* , len, cap }      (row-major, default)
SoA   soa<User>   = one contiguous column per field → {id[], name[], active[], score[]}
```

**Field order within a struct is unspecified for a non-`layout(C)` struct** (SETTLED,
`open-questions.md` "Default struct layout: field reordering"). Codegen — the *one* place struct
layout is computed (the `set_body` in `align_codegen_llvm`) — lays fields out in **descending
alignment** (ties keep declaration order, a stable sort) to eliminate padding, matching Rust's
default (`{ a: i8, b: i64, c: i8 }` → 16 bytes, not 24). Source access is by name, so the reorder is
invisible; codegen keeps a **logical→physical field-index map** (`field_perm[struct_id][logical]`)
that *every* field-index consumer routes through — struct-field GEPs (`field_path_ptr`,
`elem_field_ptr`, AoS `IndexFieldPtr`, `NullStructField`, `DropElemField`, the `drop_struct_fields`
walk), byte-offset sites (`offset_of_element` for `json.decode` field tables, `group_by`/dict key &
value offsets, `GatherColumnI64`), and the `soa` gather's struct-aggregate insert. `sizeof`/alignment
follow automatically because they read back the built LLVM struct type. A `layout(C)` struct uses the
**identity map** (declaration order, natural alignment, no reordering) — its byte layout is the
FFI / `raw` / `json`-encode / by-value boundary and must not move. `soa` *column* order stays in
declaration order (a separate, self-consistent column layout, independent of the AoS field order).

**SETTLED (`open-questions.md` "Memory layout — `soa<T>`"): the layout is chosen by an explicit type,
not by automatic whole-program inference.** `soa<T>` is a first-class columnar collection (peer to
`array<T>`); the compiler lowers field access / pipeline stages over it to per-column contiguous
storage (fields naturally SIMD-aligned, `align(N)` when needed — `draft.md` §3.4). A pipeline that
touches a subset of fields (`users.where(.active).pay.sum()`) then streams only those columns. The
choice is visible (predictable performance, "nothing hidden"); the *field-wise lowering under the
type* is the automatic part. Crossing a byte-layout boundary (FFI, `json` encode/decode, by-value)
**materializes to AoS explicitly**. (This closes the earlier "automatic decision vs. annotation"
question in favor of annotation.) Uses the `Layout::Soa` seam.

The column buffer is **column-major with per-column alignment padding**: column `j` begins at
`align_up(start_{j-1} + len*size_{j-1}, size_j)`, so mixed-width columns (`bool` then `i64`) stay
naturally aligned for any `len`. A column read is `Rvalue::IndexColumn`; a column write (during
construction) is `Stmt::StoreColumn` — both share one `soa_column_offset` codegen helper.
Construction is `.to_soa()`: `Rvalue::SoaAlloc` arena-bump-allocates the buffer (total size = the
offset walk to the last column + its `len*size`, aligned to the widest field), then a fused loop
scatters each AoS element's fields into their columns (`StoreColumn`), yielding the `{ptr,len}` view.
The allocation uses a checked mirror of the offset walk: negative counts, product/addition wrap,
and byte totals above the signed `i64` allocator ABI abort before allocation.
`s: soa<T> := json.decode(d)?` takes a separate direct-fill rail: the runtime first counts rows,
arena-allocates the final column layout, then parses values directly into their columns. There is no
AoS intermediate and no transpose. Primitive and zero-copy `str` columns are supported; owned and
nested columns remain deferred. `.to_soa()` itself still uses the transpose loop described above.

JSON field dispatch is O(1): codegen bakes a **compile-time perfect-hash table** from the (known)
field names (`build_phf` finds a collision-free seed + power-of-two size; emits a `[i32]`
slot→index global beside the descriptor table), and the runtime hashes each key to a slot + one
confirming name compare instead of a linear scan. `phf_len = 0` (empty/1-field, or no table found)
falls back to the scan, so it is a pure speedup. Both ends call the **one** canonical `wyhash` (the
shared `align_hash` crate — same hash as the `hash64` builtin), so the codegen-built table and the
runtime probe route a field name identically *by construction* (the paired pinned tests are now a
canary against an accidental algorithm edit, not the mechanism that keeps them in sync). (Known-schema field-skip decode is deferred — the perf is already had by declaring a
narrow struct, since unknown keys are skipped; see `open-questions.md`.)

---

## 3. Functions, CFG, cold path

- MIR `Function` → LLVM function. `Block` → LLVM basic block (nearly one-to-one).
- Every **Align-generated** function is marked **`nounwind`** (`mark_nounwind`): Align never unwinds
  (errors are `Result` values; a fatal fault `abort`s — see "Panic / unwinding" in
  `open-questions.md`), so this is always sound and lets LLVM drop exception edges / unwind tables and
  inline more aggressively. The external `align_rt_*` declarations are **not** marked (ordinary Rust
  fns). Pure-function `memory(none)`/`readonly` is *not* emitted — Align's purity is "no I/O effect"
  and permits allocation, so it doesn't imply LLVM `readonly` (deferred; `open-questions.md`).
- Terminator correspondence:

```text
Goto         br
Branch       conditional br
Return       ret
Unreachable  unreachable
```

Source `match`, `?`, and loops have already become `Branch`/`Goto` CFG in MIR. Calls,
`ParMapParallel`, and allocation are rvalues rather than terminators.

### cold path (error)
The failure edge of `?` (`04 §2.1`) is cold. In LLVM:

```text
- attach llvm.expect / branch weights to the br branching to err_bb, making the ok side fall-through
- place the body of err_bb at the function tail (or a cold section)
- lean toward noinline for calls on the failure path
```

This keeps the normal path's I-cache clean (`draft.md` §10).

---

## 4. allocation lowering

Materialize MIR's explicit `Alloc` (`04 §5`).

```text
Alloc(Arena(id), layout)   → pointer returned by align_rt_arena_alloc(arena_ptr, size, align)
arena block exit            → align_rt_arena_reset(arena_ptr)   (bulk, no individual free)
Alloc(Heap, layout)        → align_rt_heap_alloc(...)  / align_rt_heap_free at the Drop point
Alloc(Stack, layout)       → alloca
```

The arena pointer is acquired via the `align_rt_arena_begin()` equivalent at the arena block entry, and carried around as a block-scoped value (function argument/local). The detailed runtime ABI is in `06-runtime-std.md`.

---

## 5. Loops and vectorization (the crux of Align's performance)

MIR is already fused and carries the **width-agnostic** vectorizable properties of each
element-independent loop (`04 §4`) — it never fixes a vector width. **Choosing the width is the
backend's job, chosen per target.** The current, working form emits clean IR (contiguous access,
branchless `where`, `noalias`) and hands it to LLVM's `-O2` vectorizer, which picks the width from the
target: this is the right split, not a fallback — MIR stays portable and each target gets its own
strategy (fixed width + a scalar remainder on NEON/AVX; `<vscale x N x T>` + active-lane predication on
SVE/RVV). On a fixed-width target the loop lowers to:

```text
vector body   load <W x T> → VecOp/Mask → store. pointer advances by W
remainder     handle the leftover scalarly
```

```text
total := scores.sum_where(scores > 80);   (MIR: VecCmp + MaskedReduceAdd)
=>
loop:
  v   = load <W x f32>, p
  m   = fcmp ogt <W x f32> v, splat 80.0     ; <W x i1>
  sel = select <W x i1> m, v, zeroinitializer
  acc = fadd <W x f32> acc, sel
  p  += W
; reduce: llvm.vector.reduce.fadd(acc) + remainder
```

- **mask** → LLVM `<W x i1>` and `select` (branchless, `04 §4`).
- **dot / sum / min / max** → `llvm.vector.reduce.*`.
- **no-alias** (`out`, `03 §6`) → scoped `!alias.scope`/`!noalias` metadata on the `map_into` fused
  loop's source load and `dst` store (a slice is passed by value as `{ptr,len}`, so its buffer
  pointer is not a standalone param to carry a `noalias` *attribute* — the scoped metadata is the
  equivalent per-access form). One fresh domain + `in`/`out` scope pair per loop; gated on the
  sema-proven `dst`-disjoint-from-source precondition. Makes explicit to LLVM the basis for
  dependence-free vectorization — verified to drop the loop's runtime overlap guard at `-O2`.
- aligned load/store when already aligned.

### Choosing the width (a backend, per-target choice)
```text
explicit vecN<T>   N is fixed in the type → the LLVM vector width directly (the fixed escape hatch)
inferred loops     no width in MIR → the backend chooses it per target:
                     fixed-width ISA (AVX/NEON)  a portable per-arch baseline + a scalar remainder
                     scalable ISA (SVE/RVV)      <vscale x N x T> + active-lane predication (no fixed W)
```
**SETTLED (`open-questions.md` "Build targets & portability") — for fixed-width ISAs:** the default
targets a portable per-arch baseline (`x86-64-v2` / `armv8-a`, i.e. 128-bit); `--target-cpu native` /
higher baselines are opt-in. This keeps one binary runnable across a varied cloud/Docker fleet.
**Wide SIMD on that fleet comes from runtime CPU-feature dispatch in the library layer** (`06 §1`),
not from raising the generated-code baseline — one binary picks AVX2/NEON at runtime and falls back
safely. Runtime-multiversioning the generated loops themselves (an ifunc-style v2 + v3 selector) is a
possible future refinement, deferred. This is a *fixed-width-ISA* policy, not a universal 128-bit cap:
a scalable ISA is handled by predicated scalable codegen instead, which is why MIR stays width-agnostic
(`04 §4`).

> Status note: the default build now targets the **portable per-arch baseline** (`x86-64-v2` on
> amd64, `generic`/`armv8-a` on arm64) via `BuildTarget` in `align_codegen_llvm`; `--target-cpu
> native` opts into the host CPU. The backend still builds **scalar** IR and leans on the LLVM `-O2`
> pipeline (SLP / loop vectorizer) for the actual SIMD. Branchless `where` is implemented for the
> inactive-lane-safe reducing suffix: MIR folds predicates into a mask and emits identity-select for
> `sum`/`count` (`0`) and `min`/`max` (`+∞`/`−∞`). `min`/`max` further lower to the
> `select(cur `cmp` acc, cur, acc)` idiom (`llvm.{s,u}{min,max}` / `llvm.{min,max}imum`) so the whole
> loop is branch-free and vectorizes: e.g. `xs.where(p).min()` over a `slice<i32>` emits `pminsd`
> over a `pcmpgtd` mask on x86-64-v2 (verified via `objdump`; before, the per-element branch blocked
> it entirely). General callable suffixes/reducers and materializing terminals use real skip-branches:
> the former may trap or have effects, and the latter must not append a rejected element.
>
> **Correctness fix (2026-07-13):** reducing lowering used to speculate a reducer's callable and
> every stage after `where` on rejected elements. Pure does not imply total/non-trapping, so
> `where(false).map(divide_by_zero).sum()` aborted. Identity `select` now stays branchless only for
> field operations plus builtin `sum`/`count`/`min`/`max`; every general post-`where` callable is
> guarded and never executes on a rejected element.
> See [`12-pipeline-closure-memory-io-simd-audit.md` §3.1](12-pipeline-closure-memory-io-simd-audit.md#31-fixed-2026-07-13--where-guards-later-callables-and-callable-reducers).
> `maskN<T>` remains the explicit hand-written value mask, and `dot` has no masked pipeline form.

> **Why the identity-select shape matters beyond perf.** Selecting each reducer's identity for a
> masked-out lane (`min` → `+∞`, `max` → `−∞`, `dot` → `0`, matching
> `sum`/`count` → `0`) makes *every* reduction **predication-ready**: a masked-out lane contributes the
> identity and cannot change the result. Generic `reduce` is the one exception — its user-supplied
> function has no known identity (`init` is the starting accumulator, not an identity), so its
> computation is guarded. `any`/`all` predicates are likewise guarded. That distinction is what
> makes the form semantics-correct and still predication-ready for scalable tails (`04 §4`).

---

## 6. Strings, builder, const pool

- **string literals**: bytes as an LLVM global constant. A `str` value is `{ptr,len}`. Compile-time meta (len/hash/ascii, `draft.md` §12, `03`) is embedded as constants and used for `write_static` lengths and hash comparisons.
- **const string pool** (`draft.md` §12): identical literals/JSON field names/HTTP header names are coalesced into a single global (deduplication).
- **builder**: the runtime's mutable buffer. In the `template` desugaring (`04 §2.5`), `write_static` becomes memcpy + known length, and `write_value` becomes a per-type formatting call.

---

## 7. Parallelism (`ParMapParallel` → runtime)

MIR's dedicated non-capturing `ParMapParallel` materializer (`04 §6`) goes to the runtime's
parallel-map API.

```text
ParMapParallel { src, func, elem_in, elem_out }
  → synthesize one element thunk
  → align_rt_par_map(in_buf, count, in_stride, out_stride, thunk)
  → owned array<elem_out>

task_group → align_rt_tg_begin / tg_alloc / tg_register / tg_wait / tg_end
```

The thunk loads one input element, calls the named Pure Align function, and stores one output
element. Capturing or staged `par_map` forms use the sequential pipeline fallback before codegen.
There is no generic parallel-reduce lowering in the current surface. The ABI is in `06`.

---

## 8. Target, optimization, output

```text
- build the TargetMachine for the host (or a specified triple). obtain the data layout and reflect it in the §2 layout
- LLVM optimization: since fusion/vectorization is done on the Align side, leave the
  lower-level optimizations (instcombine, regalloc, peephole, etc.) to LLVM. don't duplicate high-level transforms
- output: object (.o). the driver links it with align_runtime into an executable (01/06)
- alignc emit-llvm outputs the IR as text (for verification/debug, 01)
```

`// OPEN:` how far to use the LLVM pass pipeline (a single O2-equivalent pass vs. selecting the necessary passes). Decide empirically within the range that does not conflict with Align's optimizations.

---

## 9. Debug info, panic

```text
- generate DWARF/CodeView line info from Span (align_span). introduce at least step-debug-capable level in stages across M
- traps such as divide-by-zero (03/draft §5): to a runtime abort (align_rt_panic). message + location
- overflow defaults to wrap, so no check is emitted (optionally insert a check in dev builds only)
```

---

## 10. Settled backend choices and remaining refinements

### Settled (M0; upgraded to LLVM 22 post-M13): inkwell / LLVM version and linking method
Use LLVM 22 via `inkwell 0.9` (feature `llvm22-1`), with `llvm-sys` 221. `llvm-sys` is pinned to
**dynamic linking** (`prefer-dynamic` feature + `LLVM_SYS_221_PREFER_DYNAMIC=1` in
`.cargo/config.toml`); `llvm-config-22 --shared-mode` still reports `shared`. Unlike the Debian
llvm-19 era (shared-only — no static components such as `libPolly.a`, so dynamic linking was
mandatory), the apt.llvm.org llvm-22 packages ship the static archives and Polly is no longer a
separate `--libs` component, so a static build would work; dynamic linking is kept deliberately (it
links smaller and matches the rustc-side LLVM). In M0 the generated `main` is the C entry (called by
crt0), and the driver links the object with `cc`. (History: M0 shipped on LLVM 19 / `llvm19-1` /
`LLVM_SYS_191_PREFER_DYNAMIC`; the LLVM 19 → 22 upgrade checkpoint landed after M13 — see
`07-roadmap.md`.)

```text
- the scope of multi-ISA support: the vector width is a backend, per-target choice (§5) — MIR stays width-agnostic (04 §4) — so the open part is how far to carry scalable-ISA (SVE/RVV) predicated codegen, not whether MIR fixes a W (common with 04 §9)
- the scope of adopting the LLVM optimization pipeline (non-overlap with Align's optimizations)
- by which M and how far to raise the precision of debug info
- linking: static runtime, and how far to depend on libc (linked with 06)
```

`Option`/`Result` use tagged aggregates, and SoA is selected by an explicit `soa<T>` type; neither
is an open backend decision.
