# MIR: Intermediate Representation, Desugaring, and Optimization

Implementation model for `align_mir`. MIR is the **backend-agnostic core** (`00-overview.md`).
Align's semantics—desugaring, fused pipelines, explicit ownership operations, tasks, and
target-independent vector operations—are represented here, while `MIR → LLVM`
(`05-backend-llvm.md`) is restricted to lowering them.

Role boundaries:

```text
typed HIR (03)  the tree as written, annotated with Ty / Region / move / Effect
   │  ① desugaring (lowering)  expand ? / else / template / selectors / projection chains
   │  ② MIR construction       CFG + explicit alloc / error-edge / parallel nodes
   │  ③ optimization            fusion / mask branchless / dead clone & heap elimination / const pool
   ▼
MIR (optimized)  → to codegen
```

Design principle: **nothing hidden** (`draft.md` §3.2). allocation / error path / parallel unit (chunk) remain as **explicit nodes** in MIR, read by both lint (`draft.md` §16) and codegen.

The pseudocode below explains the model; it is not a second definition of the Rust enum. The
authoritative concrete node inventory is `crates/align_mir/src/lib.rs`, and `alignc emit-mir`
shows what a program actually lowered to. Historical optimization proposals are labelled as such.

---

## 1. The Shape of MIR

A CFG (a set of basic blocks) per function. Each block is a sequence of statements + a trailing terminator. Close to SSA form (each value defined once; reassignment yields a new value), but assignment to a `mut` place is an explicit store.

```text
Function { name, params: [(value, Ty, ParamMode)], ret, slots, blocks[] }
Block    { params, stmts[], term }

stmt =
  Let(v, rvalue)                       // v = computation / call / allocation
  Store(slot, operand)                 // assignment to storage
  StoreField / StoreIndex / PtrStore   // explicit aggregate or buffer writes
  ArenaEnd / TgWait / TgEnd            // explicit lifetime and join points
  Drop / DropValue / DropElem…         // explicit ownership release
  ClearDropFlag                         // ownership transferred to native/raw

term =
  Goto(bb)
  Branch(cond, bb_then, bb_else)
  Return(operand?)
  Unreachable
```

Calls, arena/heap allocation, enum inspection, collection operations, vector operations,
`ParMapParallel`, and task creation are concrete `Rvalue` variants. A source `match`, `?`, or loop
therefore becomes ordinary blocks and `Branch`/`Goto`; there is no separate `Switch`, `TryEdge`,
`Loop`, or `ParLoop` terminator in the current representation. This keeps control flow uniform
while preserving allocations, drops, task boundaries, and parallel materialization as explicit
operations for codegen and inspection.

Each value/place keeps its HIR-derived `Ty` and (for views) `Region`. codegen **does not recompute** types (anti-rewrite).

### 1.1 Borrow and resource operations

The library boundary adds generic MIR operations, never package-specific variants:

```text
ResourceFromRaw { resource_def, raw }
  // unsafe; establishes independent ownership
ResourceFromRawBorrowed { resource_def, raw, parent_ref }
  // unsafe; establishes Move child plus parent-generation dependency
ResourceBorrow { resource_def, owner_place }
  // Copy ResourceRef
ResourceViewFromRaw { resource_def, owner_ref, ptr, len, view_kind, validation_plan }
  // unsafe boundary; checked owner-tied str/slice view result
ResourceRaw { resource_def, resource_ref }
  // unsafe; non-owning extraction
ResourceIntoRaw { resource_def, owner_place }
  // unsafe; standalone owned resource root only; consumes + clears Drop flag
CloneIn { value, region_handle, plan }
  // explicit recursive region copy
RegionBuilderNew/Push/Build { ... }
  // visible region allocation/copy operations
StaticQueryDescriptor { artifact_id }
  // producer-owned immutable descriptor
```

Ordinary `Drop`/`DropValue` consults the resource definition's producer-owned Drop thunk; no
`DbConnDrop`, `RegexDrop`, or sibling type family is added. `ResourceIntoRaw` accepts only an
initialized standalone resource local or by-value resource parameter owned by the function. HIR
rejects fields, elements, projections, borrowed/out parameters, and temporaries, so MIR never needs
field-level cleanup ownership. The operation leaves the accepted root uninitialized on every
continuing path, exactly like another Move transfer.

`ResourceFromRawBorrowed` carries the exact imported/local resource declaration identity, the raw
handle, and a checked `ResourceRef` for the parent generation. The result is a Move resource whose
cleanup must precede any invalidating move, mutable borrow, or Drop of that parent generation.
That dependency is a typed HIR/MIR fact and is preserved through moves, aggregates, calls, and
interfaces; it is not reconstructed from raw values in LLVM.

`ResourceViewFromRaw` is the explicit unsafe-to-safe boundary for package-owned native buffers.
Its `validation_plan` performs checked signed-to-size conversion, size arithmetic, null/zero-length
handling, alignment for non-byte views, and UTF-8 validation for `str` before the operation returns
a safe `Option<str>`/`Option<slice<T>>`. A successful view carries the exact `owner_ref` generation.
Failure returns `None` without creating a view. MIR-to-LLVM lowers these checks and the resulting
fat-view representation mechanically; codegen may not invent, omit, or weaken them.

`Borrow` and `BorrowMut` parameters use a non-null pointer to caller-owned storage in the internal
ABI. A recursively Move pointee also passes access to the caller's path-local cleanup bit. They are
never materialized as an owning copy in the callee. The callee may read through a shared borrow; a
mutable borrow additionally permits replacement/mutation allowed by the checked source. Neither
mode emits function-exit cleanup for an unchanged pointee: ownership remains with the caller.
Replacement is different. MIR emits the ordinary `DropValue(old_place, DropPlan)` guarded by the
caller's current cleanup bit before storing the new value, then stores the new cleanup bit. Thus an
old string/array/resource is dropped exactly once and a later caller cleanup sees only the
replacement. `BorrowMut` carries non-alias facts proven by recursively scanning every peer
parameter mode in checked HIR, but LLVM attributes are emitted only where their exact ABI meaning
is sound.
Owner-generation invalidation is complete in HIR before MIR construction and is retained in debug
provenance; LLVM does not decide borrow legality.

`Fn`/`FnTy` stores `[(ParamMode, Ty)]`, not only `[Ty]`, plus the target's
`ReturnBorrowSummary`/`ReturnRegionSummary` and `ReturnCleanupAbi`. A named function converted to a function value retains
`Out`, `Borrow`, and `BorrowMut`; `CallFnValue` lowers each operand with the identical direct-call
ABI and applies the stored result provenance. Concrete closure targets resolve target-relative
capture-slot summaries through the selected environment; those roots travel when the function
value moves and constrain the result. Function-value joins require exact mode equality, union
parameter sets, and preserve the selected target's capture metadata. An unresolved higher-order parameter uses every compatible view/region
input rather than `None`, including embedded provenance in a by-value Move operand. The call
snapshots that provenance before its ordinary move/null and attaches it to the result. No
indirect-call shim copies a borrowed owner, changes a parameter mode, or drops result provenance.

A recursively Move return lowers as the normal return value plus one dynamic cleanup bit.
`ReturnCleanupAbi::DynamicBit` is part of direct/indirect/interface ABI identity. Every return edge
forwards its selected path-local bit; callers store it beside the returned value. In particular an
arena-owned `Ok` and an individually owned `Err` may return different values of that bit through
the same `Result` ABI. `ReturnRegionSummary` supplies lifetime roots only and never reconstructs the
ownership bit.

A resource Drop operation names the resource-declaring producer's generated hidden support thunk,
whose symbol and ABI fingerprint come from the imported resource summary. The thunk calls the
`pub` raw-only hook in the package's `internal` module. MIR never attempts to resolve or import that
source hook from a consumer unit.

---

## 2. Desugaring (lowering)

HIR sugar is expanded into the CFG here—the things the frontend/typecheck did not expand (`02`/`03`).

### 2.1 `?` (Result propagation)
Expand `expr?` into testing the tag, extracting the ok value, and returning early on failure.
The following names are explanatory pseudocode; the concrete MIR uses result-inspection rvalues
plus `Branch`.

```align
data := fs.read_file(path)?;
```
```text
t0 = call fs.read_file(path)        : Result(String, E)
is_ok = ResultIsOk(t0)
Branch(is_ok, ok, err)
ok:  data = t0.ok_value
err: r = make Err(convert(t0.err))  // E -> the function's E'
     Return(r)
```
codegen can place the cold edge in a separate/low-priority section.

### 2.2 `else` unwrap
Turn `lhs else rhs` into an Option/Result branch. If `rhs` diverges (`return`), keep it as is; if it supplies a value, merge with the then side.

```align
user := find_user(id) else return Error.NotFound;
port := get_env("PORT") else { 8080 };
```
```text
Branch(has_value(lhs), bind_block, rhs_block)
```

### 2.3 Field selector `.ident`
The `.active` in `xs.where(.active)` is already reified into a closure in HIR (`03 §4`). In MIR it is inlined, leaving no call (a prerequisite for fusion).

### 2.4 Projection chains
Desugar `users.where(.active).score.sum()` into a **single loop** (the fusion body is §3). The intermediate `array<i32>` (the result of `.score`) is **not** created.

### 2.5 template / html / json
Decomposed at compile time into static parts and value parts (`draft.md` §13).

```align
msg := template "Hello {name}, score={score}";
```
```text
b = builder()
b.write_static("Hello ")     // known length (string meta, 03)
b.write_value(name)
b.write_static(", score=")
b.write_int(score)
msg = b.to_string()
```
`html`/`json` insert context-specific escaping (`write_html_escaped` etc.) on the value parts. If the total length of the static parts is known, the builder's initial capacity is preallocated (1 `Alloc`).

### 2.6 match
Lowered to tag tests and a chain/tree of ordinary `Branch` terminators. An owning scrutinee uses
the existing `EnumPayload` extraction in the selected arm; a borrowed-place scrutinee uses the
checked `BorrowedProjection` path and projects the active payload pointer in place. The latter
never emits an aggregate extraction, creates a Move binding cleanup bit, or nulls the source. The
borrowed path is admitted only for the exact stable place and recursive payload grammar owned by
`docs/impl/26-borrowed-sum-projection-plan.md`; malformed metadata fails validation rather than
falling back to the owning extraction. Exhaustiveness is already guaranteed by typecheck (`03`).

---

## 3. Loop Fusion (Align's flagship)

The core of "writing it the normal way makes it easy for the compiler to optimize" (`draft.md` §1). Collapse a chain of `map`/`where`/`scan`/reduction into **one loop**, eliminating intermediate arrays.

### Targets and rules
```text
map(f)       per-element transform. passes through to the next stage
where(p)            only elements satisfying the predicate pass to the next stage (can be masked, §4)
Project(field)      extract an element's field (no intermediate array)
reduce/sum/min/max/count/dot/any/all  terminal. fold into an accumulator
```

Consecutive map/where/project are **producer-consumer fused** into a single loop body, and the
terminal reduction closes the loop. `Effect=Pure` (`03 §8`) is a prerequisite for transformations
that reorder, speculate, erase, or parallelize calls. Ordinary sequential callables may be Impure;
they may share one loop only when guarded source order and exactly-once evaluation are preserved.
Pure is necessary but not sufficient for inactive-lane execution because a Pure call can trap,
allocate, or fail to terminate. See `12-pipeline-closure-memory-io-simd-audit.md` §3.2.

```text
total := users.where(.active).score.sum();
=>
acc = 0
Loop over i in users:
  u = users[i]
  if u.active:                 // where → branch or mask
    acc += u.score             // .score projection fuses into a load, no array created
total = acc
```

### Array expressions (no temporary arrays)
`a = (b + c) * d - e` (`draft.md` §9) becomes a loop that writes the per-element expression tree to the output array in one pass. No temporary arrays for intermediates like `b+c`.

```text
Loop over i:
  a[i] = (b[i] + c[i]) * d[i] - e[i]
```

With an `out` argument (no-alias, `03 §6`), the input and output are guaranteed to be separate regions, so vectorization can proceed without dependence checks.

### Fusion boundaries (`// OPEN:` details)
```text
fuse       consecutive map/where/project + terminal reduction when source order/count is preserved
don't fuse sort / group_by / partition (involve whole-collection rearrangement), reordered side effects, inter-element dependence (part of scan)
```
`sort` etc. cut the fusion point, with separate loops before and after.

---

## 4. SIMD / mask lowering

Carry vec/mask in MIR as **first-class** (`draft.md` §9), in a form that codegen can deterministically lower to vector instructions.

### masks and guarded inactive lanes
Safe primitive `where` reductions can lower to a mask + predicated identity operation
(suited to SIMD/GPU); materialization uses stream compaction. The shipped reducing lowering extended
that mask too far, but the 2026-07-13 correction now guards every general callable and
post-`where` inactive-lane-unsafe computation. Field operations plus builtin
`sum`/`count`/`min`/`max` retain mask + `select`; materializing terminals continue to skip rejected
elements.

```align
m := scores > 80;
total := scores.sum_where(m);
```
```text
m   = VecCmp(gt, scores, splat(80))   // mask<...>
acc = MaskedReduceAdd(scores, m)       // no branch
```

### Vectorizable properties (width is a backend concern)
**Invariant: MIR carries only the *properties* that let a loop vectorize; it never fixes a vector
width.** A fused element-independent loop stays width-agnostic in MIR, tagged with the facts codegen
needs:

```text
element independence   no inter-element dependence (scan is the exception)
Effect = Pure          (03 §8)
noalias                out-derived disjointness (03 §6)
trip count             loop length / bound
reduction monoid       identity + associative combine (for a reducer terminal)
access plan            contiguous / known-stride
predicate chain        the where/mask conditions, folded to a mask
```

Turning those into a *width* is **permanently the backend's concern** (`05 §5`): fixed width + a
scalar remainder on NEON/AVX, or `<vscale x N x T>` with active-lane predication on SVE/RVV — one MIR
shape, a per-target strategy. Baking a concrete `W` into MIR would make the shape vary per target
class and break "MIR is the backend-agnostic core" (this file's intro), so it is deliberately absent.
`select(mask, a, b)` / `dot` / `sum_where` are kept as dedicated rvalues.

---

## 5. arena / region encoding

The region checked in `03 §7` is converted into actual allocation/release here.

> **Implemented (Memory Model v2).** Free-standing owned values use a per-binding `Drop` (a
> `DropFlagInit` null-inits the slot; a moved-out source is nulled at the move site so the exit
> `Drop` is a no-op `free(null)`). An owned payload inside an `Option`/`Result` is dropped by
> freeing each owned field's buffer. Inside an `arena {}` the same values are bump-allocated and
> bulk-freed (no per-binding `Drop`). See `08-memory-model-v2.md`.

```text
arena {}        →  group of Alloc(.., Arena(id)) + a bulk release at the block exit
                   no individual Drops emitted (arena is bump + bulk reset)
arena out {}    →  the same ArenaBegin/ArenaEnd; `out` is the existing handle as `region`
Heap            →  Alloc(.., Heap). Drop at the release point derived from the move check
Stack/Value     →  on the stack. Drop at scope end
```

```text
arena {
  data := fs.read_file(path)?;     // Alloc(Arena(a))
  users := json.decode(...)?;      // Alloc(Arena(a))  (zero-copy view points into data)
  process(users)?;
}
// exit: bulk reset of arena(a) (no individual frees)
```

Because the `Alloc` node carries a region, lints like "allocation inside a loop" and "unnecessary heap" (`draft.md` §16) detect them by scanning this MIR. Escapes are already rejected in HIR (`03 §7`), so MIR can assume safety.

An imported function with a `region` parameter receives the caller's arena handle as an ordinary
internal ABI operand. Its allocations remain `ArenaAlloc { handle, ... }`, and its returned
ownership/region fact is already fixed by `ReturnRegionSummary`. There is no ambient current-arena
lookup. Region-builder chunk allocations and the final compacting allocation are distinct MIR
operations so `emit-mir`, allocation lints, and benchmarks can observe the documented one-pass
cost.

---

## 6. Parallel nodes (`par_map` / `task_group`)

The implemented data-parallel operation is a dedicated materializer:

```text
direct scalar/AoS source.par_map(f)
                                → Rvalue::ParMapParallel { src, func, stages: [], captures, capture_tys, elem_in, elem_out, work_weight }
direct chunk source.chunks(n).par_map(f)
                                → Rvalue::ParMapParallel { src: array<slice<T>>, func, stages: [], captures, capture_tys, elem_in: slice<T>, elem_out, work_weight }
staged scalar/AoS map/filter/project chain
                                → Rvalue::ParMapParallel { src, func, stages, captures, capture_tys, elem_in, elem_out, work_weight }
recognised invariant `str.contains` filter chain
                                → Rvalue::ParMapParallel { stages: [..., FilterStrContains], captures, ... }
direct stage-free integer par_map(f).sum()
                                → Rvalue::ParMapReduce { src, func, captures, capture_tys, elem_in, elem_out, work_weight }
direct stage-free integer chunks(n).par_map(f).sum()
                                → Rvalue::ParMapReduce { src: array<slice<T>>, func, captures, capture_tys, elem_in: slice<T>, elem_out, work_weight }
unsupported filtered par_map    → sequential pipeline fallback, preserving capture semantics
source.chunks(n)                 → Rvalue::Chunks (a collection/view operation, not a loop hint)
task_group                       → TgBegin; SpawnTask…; TgWait/TgWaitResult; TgEnd
```

`par_map` requires a Pure callable (`03 §8`); the dedicated node lets codegen call the runtime's
parallel-map API with a generated whole-range kernel. Direct-source Copy captures are lowered once
into a call-scoped immutable context; the kernel loads them and passes them as direct trailing
arguments. The kernel contains the typed counted loop and direct calls for primitive scalar, `str`,
or Copy-struct map/filter values plus the terminal body. AoS projection stages extract the logical
field after the backend layout permutation; `where(.field)` stages use the same count/prefix/
scatter pair as callable filters, so output order stays source order. The recognised invariant
`where(fn s { s.contains(NEEDLE) })` form over `str` elements is represented by a
compiler-generated `FilterStrContains` stage with one literal/free-variable `str` capture; codegen
calls the existing `str_contains` ABI in both stable-filter passes. The runtime schedules
disjoint `[start,end)` ranges. A chunk source loads each borrowed `slice<T>` header as one range
element; the owned chunk-header buffer remains materialized by `Rvalue::Chunks` and is dropped after
the synchronous consumer. The specialized `ParMapReduce` node covers direct, stage-free integer
`par_map(f).sum()` for scalar and chunk sources; its generated kernel folds each range with plain
wrapping integer addition and publishes one partial result per range, so no transformed result
array is materialized. SoA projections, richer string-search expressions, floating-point sums, and
arbitrary reducers remain on their existing paths. This does not add a generic `ParLoop(reduce=…)` node or silently
parallelize ordinary reductions.

Each parallel node also carries a compiler-generated `work_weight` in `{1, 2, 4}`. The post-lowering
MIR pass classifies the callable from its local statement count, branches, and opaque calls; staged
chains add their stage weights and cap the result at `4`. An unavailable imported body defaults to
`1`, so separate compilation fails closed toward the conservative scheduler floor. This is a coarse
runtime scheduling hint, not a source annotation or a nanosecond estimate; codegen passes it with
the input/output byte widths to the runtime.

---

## 7. Optimization model (historical proposed order)

```text
1. inline      small functions / selector closure expansion (set up the prerequisites for fusion)
2. fuse        fuse map/where/project + reduction, fuse array expressions (§3)
3. mask        where→mask, branchless-ization (§4)
4. vectorize-tag    tag element-independent loops with their vectorizable properties (§4); width is chosen later, in the backend
5. mem         dead clone elimination / unnecessary heap → stack / arena promotion
6. const       const string pooling, constant folding, use of string meta
7. simplify    cleanup of unreachable (cold) code, common subexpressions
```

This list is the design decomposition, not a claim that the compiler currently has seven separately
scheduled MIR-to-MIR pass objects. When a transformation is implemented during lowering, its
observable contract is still the same: no hidden intermediate collection, guarded effects, and an
explicit ownership/parallel operation in the emitted MIR.

---

## 8. Debug output

`alignc emit-mir` (`01`) displays MIR as text. To allow comparing before/after fusion, support emitting inter-pass snapshots. This is a means for humans/AI to confirm "is the optimization working", and it underpins Align's predictability (`design-notes.md`).

---

## 9. Remaining design refinements

```text
- precise rules for fusion boundaries (how far to do partial fusion of scan / group_by)
- SIMD width is permanently a backend concern (§4): MIR stays width-agnostic and carries only vectorizable properties; the open part is what property set the backend needs to pick fixed-width+remainder vs. scalable predication
- finalizing the optimization pass order and whether iteration (fixpoint) is needed
- how far to push MIR toward SSA / handling of mut places
```

Monomorphization is already settled: it happens before MIR construction (`03 §9`). A parallel
reduction is a possible future feature, not an unimplemented branch of the current MIR.
Borrow/resource/region/static-Query lowering is also settled but not yet implemented; its
mandatory sequence and exhaustive-pass verification are in
`17-library-boundary-prerequisites.md` §§8–10.
