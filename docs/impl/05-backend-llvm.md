# Backend: MIR → LLVM (draft)

Design sketch for `align_codegen_llvm`. It commits to **pure lowering**—Align's semantic decisions (desugaring, fusion, SIMD-ization, region) are already done in MIR (`04-mir.md`), and here we just mechanically lower MIR to LLVM IR. Types, Region, and parallel units are carried in MIR, so we **do not recompute** them (anti-rewrite, `00-overview.md`).

```text
MIR (optimized)  →  LLVM IR  →  object (.o)  →  [driver links] → executable
                                                  + align_runtime (06)
```

The implementation takes Rust's LLVM bindings (`inkwell`) as the baseline. `// OPEN:` version-pinning strategy (how to absorb LLVM version dependence).

This document is a **draft**. Open items are at the end + inline `// OPEN:`.

---

## 1. Type correspondence (Ty → LLVM type)

Map MIR's `Ty` (`03 §1`) one-to-one to LLVM types.

```text
Bool              i1 (i8 when stored)
Int(w, signed)    iW            (sign distinguished by the operation)
Float(32|64)      float | double
Char              i32 (Unicode scalar)
Unit              {} (empty) / void (return)
Vec(n, T)         <n x T'>      ← maps directly to LLVM vector type
Mask(T)           <n x i1>
Bitset            iN / [iW]
Array(T)          { T* ptr, i64 len, i64 cap }   owned, contiguous
Slice(T, _)       { T* ptr, i64 len }            view (Region does not surface in the type)
Str               { i8* ptr, i64 len }           (+ meta is separate, §6)
String/Buffer/Builder  owned header struct
Named(struct)     %struct.S = type { each field }   (layout is §2)
Named(sum)        { iT tag, [payload bytes] }    tagged union
Option(T)         null representation for types that can be made non-null, otherwise { i1, T }   // OPEN: representation TBD
Result(T,E)       { i1 is_ok, union{T,E} }
Fn(..)            function pointer (+ environment pointer if there is a capture)
```

`Region` **does not appear in LLVM**. Safety is already verified in HIR (`03 §7`); codegen receives only the concrete value (an arena pointer, etc.). This is the final destination of "do not surface lifetimes".

---

## 2. struct layout

The default is **AoS** (declaration order, natural alignment, the value-type-centric `draft.md`). **SoA**, which helps for data parallelism, is treated as a transform over arrays.

```text
AoS   array<User> = contiguous User → { User* , len, cap }      (row-major, default)
SoA   soa<User>   = one contiguous column per field → {id[], name[], active[], score[]}
```

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
`s: soa<T> := json.decode(d)?` reuses this: decode to a temporary AoS (the array length is unknown
until parsed), `transpose_to_soa`, then free the AoS temp. Known-schema field-skip decode (parse only
the used columns) and `str`/owned columns are later slices.

---

## 3. Functions, CFG, cold path

- MIR `Function` → LLVM function. `Block` → LLVM basic block (nearly one-to-one).
- Terminator correspondence:

```text
Goto         br
Branch       conditional br
Switch       switch
Return       ret
TryEdge      conditional br. attach cold metadata to err_bb
Loop         br structure of header/body/exit (vectorized in §5)
ParLoop      to a call of the runtime's parallel API (§7)
```

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

MIR is already fused and has shaped element-independent loops into "vector width W + remainder" (`04 §4`). codegen lowers this **deterministically** to vector instructions—rather than "hoping" for LLVM's auto-vectorization, we build the IR with vector types ourselves.

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
- **no-alias** (`out`, `03 §6`) → `noalias` attribute on the pointer argument. Make explicit to LLVM the basis for dependence-free vectorization.
- aligned load/store when already aligned.

### Target width W
```text
from vec<N,T>   N becomes the LLVM vector width directly
inferred loops  the default build's safe baseline width (amd64 x86-64-v2 / arm64 NEON = 128bit);
                wider (AVX2 256bit, …) only under an opt-in --target-cpu
```
**SETTLED (`open-questions.md` "Build targets & portability"):** the default targets a portable
per-arch baseline (`x86-64-v2` / `armv8-a`), so inferred-loop W is the baseline width (128-bit);
`--target-cpu native` / higher baselines are opt-in. This keeps one binary runnable across a varied
cloud/Docker fleet. **Wide SIMD on that fleet comes from runtime CPU-feature dispatch in the library
layer** (`06 §1`), not from raising the generated-code baseline — one binary picks AVX2/NEON at
runtime and falls back safely. Runtime-multiversioning the generated loops themselves (emitting v2 +
v3 variants behind an ifunc-style selector) is a possible future refinement, deferred.

> Status note: the default build now targets the **portable per-arch baseline** (`x86-64-v2` on
> amd64, `generic`/`armv8-a` on arm64) via `BuildTarget` in `align_codegen_llvm`; `--target-cpu
> native` opts into the host CPU. The backend still builds **scalar** IR and leans on the LLVM `-O2`
> pipeline (SLP / loop vectorizer) for the actual SIMD. In particular,
> `where` / conditional reductions currently lower to a **per-element branch** (`Term::Branch` in
> `align_mir`) — a naive placeholder, **not** the intended final form. The branchless mask + `select`
> lowering above (`where(p).sum()` → masked reduce; materialize via stream-compaction) is M6 work
> (`07-roadmap.md`). The design is fixed: **`where` is branchless**; no per-element `if` is part of
> the source semantics. (`mask<T>` is the explicit hand-written form of the same.)

---

## 6. Strings, builder, const pool

- **string literals**: bytes as an LLVM global constant. A `str` value is `{ptr,len}`. Compile-time meta (len/hash/ascii, `draft.md` §12, `03`) is embedded as constants and used for `write_static` lengths and hash comparisons.
- **const string pool** (`draft.md` §12): identical literals/JSON field names/HTTP header names are coalesced into a single global (deduplication).
- **builder**: the runtime's mutable buffer. In the `template` desugaring (`04 §2.5`), `write_static` becomes memcpy + known length, and `write_value` becomes a per-type formatting call.

---

## 7. Parallelism (ParLoop → runtime)

MIR's `ParLoop` (`04 §6`) goes to the runtime's parallel API.

```text
ParLoop(chunk, body)          → align_rt_par_for(items, chunk, body_fn, ctx)
ParLoop(.., reduce)           → allocate a partial-result array → run in parallel → combine reduce serially/tree-wise
task_group spawn/wait         → align_rt_task_spawn / align_rt_task_wait
```

The `body` is the fused body from MIR, **carved out as a separate function**, and a function pointer + capture environment (`ctx`) is passed to the runtime. Because the parallel unit comes from MIR, codegen makes no parallelism decisions. The ABI is in `06`.

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

## 10. Open items (to be settled)

### Settled (M0): inkwell / LLVM version and linking method
Use LLVM 19 via `inkwell 0.9` (feature `llvm19-1`). Debian's llvm-19 is in shared mode
(`llvm-config --shared-mode` = shared, not bundling static components such as `libPolly.a`), so
`llvm-sys` is pinned to **dynamic linking**: `llvm-sys`'s `prefer-dynamic` feature +
`LLVM_SYS_191_PREFER_DYNAMIC=1` in `.cargo/config.toml`. In M0 the generated `main` is the C entry
(called by crt0), and the driver links the object with `cc`. The upgrade strategy (tracking future LLVM versions)
remains to be examined.

```text
- finalize the LLVM representation of Option/Result (null-ization vs. tagged, niche optimization)
- trigger for the SoA transform (automatic vs. annotation) and its impact on the array<T> ABI
- deciding the vector width W and the scope of multi-ISA support (common with 04 §9)
- the scope of adopting the LLVM optimization pipeline (non-overlap with Align's optimizations)
- by which M and how far to raise the precision of debug info
- linking: static runtime, and how far to depend on libc (linked with 06)
```

Once settled, reflect into `draft.md` (the relevant feature) and this document.
