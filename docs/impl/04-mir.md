# MIR: Intermediate Representation, Desugaring, and Optimization (draft)

Design sketch for `align_mir`. MIR is the **backend-agnostic core** (`00-overview.md`). Align's semantics—desugaring, loop fusion, SIMD-ization, arena/region encoding—are all settled here, and `MIR → LLVM` (`05-backend-llvm.md`) is restricted to pure lowering.

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

This document is a **draft**. Open items are at the end + inline `// OPEN:`.

---

## 1. The Shape of MIR

A CFG (a set of basic blocks) per function. Each block is a sequence of statements + a trailing terminator. Close to SSA form (each value defined once; reassignment yields a new value), but assignment to a `mut` place is an explicit store.

```text
Function { params, ret, regions, blocks[] }
Block    { stmts[], term }

stmt =
  Let(v, rvalue)               // v = rvalue (pure computation)
  Store(place, operand)        // write to a mut place
  Alloc(v, kind, layout)       // ★explicit allocation node (kind: Arena(id) | Heap | Stack)
  Drop(place)                  // release point for an owned value (from the move check)
  Call(v?, callee, args, eff)  // eff: Pure/Impure. used for parallel/I/O analysis

term =
  Goto(bb)
  Branch(cond, bb_then, bb_else)
  Switch(operand, [(val,bb)...], default)   // from match
  TryEdge(ok_bb, err_bb)       // ★the ?'s ok/failure split (err_bb is cold)
  Return(operand?)
  Loop(header, body, exit)     // structured loop (unit of fusion analysis)
  ParLoop(chunk, body, reduce?)// ★parallel loop (par_map/reduce). unit is chunk
```

`★` marks the explicit nodes that make "nothing hidden" work. Their kind is preserved after optimization, referenced by codegen and lint.

Each value/place keeps its HIR-derived `Ty` and (for views) `Region`. codegen **does not recompute** types (anti-rewrite).

---

## 2. Desugaring (lowering)

HIR sugar is expanded into the CFG here—the things the frontend/typecheck did not expand (`02`/`03`).

### 2.1 `?` (Result propagation)
Expand `expr?` into extracting the ok value + an early-return on failure, marking the failure edge **cold** (`draft.md` §10).

```align
data := fs.read_file(path)?;
```
```text
t0 = call fs.read_file(path)        : Result(String, E)
TryEdge(ok, err)                    // err is cold
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
Switch(tag(lhs), some=>bind, none=>rhs_block)
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
Lowered to a `Switch` terminator + variant decomposition (`Field`/payload extraction). Exhaustiveness is already guaranteed by typecheck (`03`).

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

---

## 6. Parallel nodes (par_map / task_group)

Data parallelism stays as `ParLoop`; I/O concurrency stays as a Call.

```text
par_map(f)   ParLoop(chunk=default/specified, body=the fused body of f, reduce=none)
parallel reduce  ParLoop(.., reduce=associative accumulator)   // combine partial sums
chunks(n)    set the ParLoop's chunk size to n
task_group   spawn=async Call, wait=join point. ? applies to each spawn result
```

The body of `ParLoop` already requires `Effect=Pure` (`03 §8`). Because the parallel unit is explicit in MIR, codegen can hand it straight to the runtime's (`06`) parallel API. `// OPEN:` how to guarantee/express the associativity of reduce (monoid specification vs. restricting to known reductions).

---

## 7. Optimization passes (proposed order)

```text
1. inline      small functions / selector closure expansion (set up the prerequisites for fusion)
2. fuse        fuse map/where/project + reduction, fuse array expressions (§3)
3. mask        where→mask, branchless-ization (§4)
4. vectorize-tag    tag element-independent loops with their vectorizable properties (§4); width is chosen later, in the backend
5. mem         dead clone elimination / unnecessary heap → stack / arena promotion
6. const       const string pooling, constant folding, use of string meta
7. simplify    cleanup of unreachable (cold) code, common subexpressions
```

Each pass is MIR→MIR. lint diagnostics reuse the analysis results of 2/3/5/6 (no separate implementation). `// OPEN:` finalizing the pass order and whether fixpoint iteration is needed.

---

## 8. Debug output

`alignc emit-mir` (`01`) displays MIR as text. To allow comparing before/after fusion, support emitting inter-pass snapshots. This is a means for humans/AI to confirm "is the optimization working", and it underpins Align's predictability (`design-notes.md`).

---

## 9. Open items (to be settled)

```text
- precise rules for fusion boundaries (how far to do partial fusion of scan / group_by)
- SIMD width is permanently a backend concern (§4): MIR stays width-agnostic and carries only vectorizable properties; the open part is what property set the backend needs to pick fixed-width+remainder vs. scalable predication
- expressing associativity for par reduce (monoid specification vs. restricting to known reductions)
- finalizing the optimization pass order and whether iteration (fixpoint) is needed
- how far to push MIR toward SSA / handling of mut places
- whether monomorphization happens before or after MIR construction (linked with 03 §9)
```

Once settled, reflect into `draft.md` (the relevant feature) and this document.
