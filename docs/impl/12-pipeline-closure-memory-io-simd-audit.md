# Pipeline, closure, memory, I/O, and SIMD audit

Status: **RECORDED 2026-07-13; partially implemented 2026-07-13.** Post-`where` callable execution
(§3.1) is corrected and ordinary sequential effects are settled (§3.2). The spawn-capture lifetime
gap (§3.3), closure-result environment region gap (§3.4), Unit indirect-call ABI defect (§3.5), and
buffered `io.copy` data loss (§3.6), and dynamic allocation byte arithmetic (§3.7) are also fixed
and regression-pinned; the other
correctness and performance items remain open. This is the durable follow-up to
[`10-cache-first-optimization.md`](10-cache-first-optimization.md) and
[`11-parallel-execution-optimization.md`](11-parallel-execution-optimization.md); the corrective
wave is summarized in [`source-correctness-fixes-2026-07-13.md`](source-correctness-fixes-2026-07-13.md).
It answers five
questions together because the profitable changes cross their boundaries: pipeline legality decides
whether LLVM may vectorize; closure representation decides whether the loop stays visible; allocation
policy decides how much memory is touched; and blocking I/O decides whether parallel workers make
progress. Audit baseline: commit `5a12ea3cc0ed`, arm64 macOS, LLVM 22.1.8, rustc 1.96.1.

Labels in this document are deliberate:

- **CONFIRMED P0** — reproduced or directly demonstrated in generated IR; correctness, memory safety,
  or ABI correctness is broken.
- **REQUIRED** — an invariant that must be established before the associated optimization widens.
- **SHIPPED / GOOD** — the current implementation already has the intended shape.
- **ALREADY PLANNED** — useful boundary information, not a new proposal from this audit.
- **PROPOSED** — the preferred implementation direction.
- **MEASURE-FIRST** — plausible, but must pass the written gate before adoption.
- **DOC DRIFT** — implementation and durable documents disagree.

No new source syntax is proposed here. In particular, this audit does not add implicit parallelism,
an async runtime, a custom allocator, or a fixed SIMD width to pipeline MIR.

---

## 1. Decision summary

The normal sequential pipeline is already strong: reductions are one fused loop with no intermediate
array, materializing terminals allocate once, `map_into` reuses caller storage with sound scoped alias
metadata, and the five C-comparison kernels have the same vectorization shape as C under the same
LLVM. Capturing non-escaping pipeline lambdas also optimize well: captures are direct extra arguments,
the lambda inlines, and loop-invariant values are hoisted. Do not replace this with a runtime iterator
or heap closure.

The audit nevertheless found correctness blockers and one normative effect-contract conflict that
come before further SIMD or parallel widening:

1. ~~A reducing pipeline speculatively executed every stage after `where`, and the reducer itself,
   on rejected elements.~~ **FIXED 2026-07-13:** general callable suffixes are guarded; safe field +
   builtin reducer suffixes retain mask/select.
2. ~~The ordinary sequential effect contract conflicted across normative and implementation docs.~~
   **SETTLED 2026-07-13:** Impure is allowed with exact guarded input/stage order; `par_map` remains
   Pure-required. `sort_by_key` key evaluation stays separately open.
3. ~~`spawn` could retain a view backed by an inner arena until after that arena was freed.~~
   **FIXED 2026-07-13**, including wrapped and nested-closure captures. The related first-class
   closure-result escape is also fixed by including the callee environment's region in an indirect
   call result.
4. ~~An indirect `() -> ()` call was emitted with an `i32` return type while its thunk was `void`.~~
   **FIXED 2026-07-13.**
5. ~~`io.copy` read the fd directly and skipped bytes already held in a buffered reader's
   lookahead.~~ **FIXED 2026-07-13.**

6. ~~Generated dynamic allocation byte counts used unchecked signed multiply/add.~~ **HARDENED
   2026-07-13:** heap, arena, and SoA allocation paths reject negative counts, checked-operation
   overflow, and results above the signed allocator ABI maximum before allocator work.

After those are closed, the highest-value performance refinements from this audit are:

1. ~~Split arena allocation into **proven-initialized-before-read / uninitialized** and conservative
   **zeroed** paths.~~ **SHIPPED 2026-07-16.** Fresh uninitialized chunks avoid the 64 KiB blanket
   zero; fresh conservative chunks retain lazy/calloc zeroing, and reused raw chunks zero only the
   requested range.
2. ~~Fill exact-size Base64 and hex output directly into its final allocation.~~ **SHIPPED
   2026-07-16.** Next add the already planned runtime-dispatched Base64 SIMD backend; hex SIMD is a
   separate new measure-first probe.
3. Evaluate SIMD block compaction for materializing `where`/`partition`, separately from reducing
   `where`; do not preserve the current unsafe speculative-execution trick merely to get vector code.
4. ~~Complete the already-planned reader/`io.copy` uninitialized-buffer work and remove the
   redundant URL/request copy in `http.get_many`.~~ **SHIPPED 2026-07-16.** Syscall-dispatched
   `io.copy` remains a separate throughput fast path.
5. ~~Add the measured total-order stable-sort fast path: avoid unused tiny scratch, detect a fully
   ordered input, and skip comparison-merging adjacent runs whose boundary is already ordered.~~
   **SHIPPED 2026-07-16 (the `w64` shape — see §4.1):** all three refinements are in
   `lower_array_sort`, total-order keys only, correctness-verified (`sort_adaptive.rs`). The
   ordered-boundary check is applied only from pass 2 (`width >= 64`); a drift-immune, control-
   corrected sweep localized the first cut's ≈ 7 % random/reverse regression to the pass-1 check and
   the fix keeps every negative workload within ≈ 2 % while retaining the ordered-input wins.

The priority is cache traffic, not novelty. A global replacement allocator, blanket `alwaysinline`,
and compiler-internal SIMD over enum-heavy MIR are not recommended.

---

## 2. What was inspected and how

The audit followed source/HIR through MIR, LLVM IR, and the runtime rather than inferring performance
from the surface syntax. Evidence included:

- reduction, collect, scan, `partition`, `map_into`, SoA, and parallel lowerings in
  [`align_mir`](../../crates/align_mir/src/lib.rs);
- effect, escape, region, closure-lifting, and pipeline checks in
  [`align_sema`](../../crates/align_sema/src/lib.rs);
- closure thunks, indirect calls, allocation byte arithmetic, alias metadata, and LLVM optimization in
  [`align_codegen_llvm`](../../crates/align_codegen_llvm/src/lib.rs);
- arena, heap, buffer, codec, string/JSON SIMD, file, stream, HTTP, and task runtime paths in
  [`align_runtime`](../../crates/align_runtime/src/lib.rs);
- optimized-IR shape tests and the five Align/C twins in
  [`bench/clang_ir_compare`](../../bench/clang_ir_compare/README.md);
- existing head-to-head and arena evidence in [`bench/README.md`](../../bench/README.md) and
  [`bench/arena_pool`](../../bench/arena_pool/README.md).

Fresh probes emitted raw and optimized LLVM for capturing pipeline lambdas, known first-class
capturing closures passed to a higher-order function, and dynamically selected Unit-returning function
values. The local stable compiler was also queried directly: rustc 1.96.1 still rejects `std::simd`
with `E0658 portable_simd`. Consequently the current repository's `std::arch` + runtime dispatch,
`memchr`, and LLVM auto-vectorization strategy remains the correct stable toolchain strategy.

---

## 3. Correctness blockers and required hardening found while auditing performance

### 3.1 FIXED 2026-07-13 — `where` guards later callables and callable reducers

At the audit baseline `lower_array_reduce` accumulated predicates into a Boolean mask but kept evaluating subsequent
`map`/`where` functions and a user reducer/predicate for every source element. Only the contribution
to the accumulator is discarded with `select` ([lowering around lines 4033-4102](../../crates/align_mir/src/lib.rs#L4033)).
At the audit baseline the backend guide explicitly described this as deliberate because pipeline
functions are Pure; that paragraph is corrected alongside this record
([backend section](05-backend-llvm.md#5-loops-and-vectorization-the-crux-of-aligns-performance)).

This program aborts instead of returning `5`:

```align
fn nonzero(x: i64) -> bool = x != 0
fn reciprocal(x: i64) -> i64 = 10 / x

fn main() -> Result<(), Error> {
  print([0, 2].where(nonzero).map(reciprocal).sum())
  return Ok(())
}
```

The false predicate rejected `0`, but `reciprocal(0)` still ran and reached the division-by-zero abort.
The same class includes checked indexing, explicit fatal operations, allocation/OOM, and
nontermination. **Pure means no observable I/O/shared mutation; it does not imply total,
non-trapping, non-allocating, or speculatable.** LLVM's own legality rules make the same distinction.

The correction is deliberately split by local legality:

1. If a callable stage follows the first `where`, or the terminal is generic `reduce`/`any`/`all`,
   each predicate branches rejected elements directly to the loop continuation. Stages before the
   first `where` still execute in source order.
2. Field projection/predicates and builtin `sum`/`count`/`min`/`max` are locally safe for the loaded
   source element, so they keep the identity-select mask path and its vector shape.
3. Future widening requires a stronger `CanExecuteOnInactiveLane` proof; it must not mechanically
   map the broad Pure effect bit to LLVM `speculatable`.

Regression gate:

- false `where` followed by integer division by zero and by out-of-range indexing does not execute
  those operations;
- a `map` before `where` still executes at its existing semantic position;
- generic `reduce`, `any`, and `all` functions are guarded too;
- a whitelisted, total arithmetic `where(...).sum()` retains the vectorized positive shape;
- mutation that changes the guard back to a result-only `select` fails the trap tests.

All gates are implemented in `branchless_where.rs`; `vectorize_shapes.rs` retains the v2/v3 masked
sum and masked-min positive shapes.

### 3.2 SETTLED 2026-07-13 — ordinary sequential callables may be Impure

The effect pass walks ordinary stage call edges, but `check_parallelism` validates only the terminal
`par_map` function ([lines 2413-2420](../../crates/align_sema/src/lib.rs#L2413)). At the audit
baseline the implementation type/MIR documents said ordinary `map`/`where`/reducer callables require
Pure, while `draft.md` constrained `par_map` only and the compiler accepted Impure sequential calls.
The settled specification now matches that implementation behavior and defines its exact order.

At the audit baseline the following printed `1`, `2`, then `0`:

```align
fn never(x: i64) -> bool = false
fn noisy(x: i64) -> i64 {
  print(x)
  return x
}

// `noisy` is after a predicate that rejects every element.
fn main() -> Result<(), Error> {
  total := [1, 2].where(never).map(noisy).sum()
  print(total)
  return Ok(())
}
```

It now prints only `0`: `noisy` is after a predicate that rejects every element. The old output was
direct evidence for section 3.1's speculation bug, not authority to ban sequential effects.

The compatibility default is now normative: sequential `map`/`where`/`reduce`/`scan`/`partition`/
`any`/`all` callables may be Impure. They execute in input-index and stage order, exactly once for
each element that reaches them; a false `where` suppresses the suffix. `any`/`all` do not
short-circuit. Inferred effects restrict transformations rather than source acceptance. Explicit
`par_map` remains Pure-required.

`sort_by_key` is separated into its own Open item. The implementation now decorates in input order
and calls the key exactly N times, but the source contract has not yet made that behavior normative.
The effect graph can fail closed at Pure boundaries, but no rejection lands before the key-evaluation
contract itself settles.

Section 3.1 is independent: even after choosing Pure-only, a Pure callable may abort and cannot be
speculated without the stronger inactive-lane proof.

The document-11 lifted-closure edge and the higher-order unknown-target case are fixed. The latter
reproduction was:

```align
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)

fn main() -> Result<(), Error> {
  f := loud
  ys := [1, 2, 3].par_map(fn x { apply(f, x) })
  print(ys.sum())
  return Ok(())
}
```

`apply` contains an indirect target that the name-only effect graph cannot recover. `EffectScan` now
propagates unknown-indirect separately from observable I/O and rejects it at a Pure/parallel boundary;
ordinary sequential HOF calls remain legal. The already-planned effect bit on function types now
recovers precision for known-Pure HOFs rather than blocking soundness.

### 3.3 FIXED 2026-07-13 — `spawn` captures outlive the task-group region

At the audit baseline this shape passed `check`; emitted MIR ordered `spawn_task`, `arena_end`, then
`tg_wait`:

```align
fn main() -> Result<(), Error> {
  task_group {
    arena {
      n := 7
      v := template "hello {n}"
      t := spawn(fn { print(v) })
      0
    }
    wait()
  }
  return Ok(())
}
```

The task environment snapshots only the view's `{ptr,len}`; it does not own the arena backing.
`EscapeCheck` now keeps the active task-group regions as an innermost-last stack. At each `spawn`,
every region-bearing capture must outlive the innermost group. Static, frame, and outer-arena
captures remain accepted; a capture tied to any inner arena is rejected before MIR lowering. The
checked HIR currently permits only a direct `FnValue` or `Closure`, but a whole-expression fallback
keeps this pass fail-closed if `spawn` later accepts a local or block function expression.

`task_group.rs` pins the direct reproduction and captures wrapped in a struct, tuple, `Option`,
`Result`, and another closure; a parenthesized lambda reaches the same gate because grouping is
erased by the parser. Positive gates cover frame/static and outer-arena captures. Separate tests pin
the current literal-only surface for local and block function expressions. Removing the outlives
check makes the negative matrix pass checking again, so the regression net exercises the lifetime
gate rather than an adjacent restriction.

### 3.4 FIXED 2026-07-13 — closure-call results include the closure environment's region

At the audit baseline this shape passed `check` and used `v` after `arena_end`:

```align
fn main() -> i32 {
  v := arena {
    n := 7
    s := template "hello {n}"
    f := fn { s }
    f()
  }
  return v.len() as i32
}
```

`region_of(CallFnValue)` used to fold only the explicit arguments; a zero-argument call was
consequently `Static`, even though its result could borrow the captured environment. It now seeds
that fold with the callee's region, then shortens it with every argument. A regression test rejects
the zero-argument arena-capture reproduction above. A future function-type return-borrow summary can
recover precision; the safe default is no longer `Static`.

This is distinct from the existing test that prevents the closure value itself from escaping an
arena: here the closure is called inside the arena and only its returned view escapes.

### 3.5 FIXED 2026-07-13 — Unit-returning indirect-call ABI mismatch

At the audit baseline a Unit-returning function and its function-value thunk were declared as LLVM
`void`, but `Rvalue::CallIndirect` asked `scalar_type(Unit)` for a value type and constructed an
`i32`-returning call. This complete source emitted the mismatch in raw IR (a two-target runtime
selection kept it in optimized IR as the negative/devirtualization control):

```align
fn noop() {}

fn main() -> i32 {
  f := noop
  f()
  return 0
}
```

At the audit baseline, raw IR contained the incompatible pair:

```llvm
define private void @"noop$fnval"(ptr %env)
...
%r = call i32 %selected(ptr %env)
```

Opaque pointers prevented the verifier from rejecting the mismatch. A dynamically selected target kept
the bad call after optimization. The relevant sites were the Unit-aware function declaration
([`declare_fn`](../../crates/align_codegen_llvm/src/lib.rs#L2852)), the closure thunk
([around line 1900](../../crates/align_codegen_llvm/src/lib.rs#L1900)), and the non-Unit-aware indirect
call ([around line 6501](../../crates/align_codegen_llvm/src/lib.rs#L6501)).

The fix follows the spawn-trampoline precedent: ordinary Unit `CallIndirect` now constructs a
`void(env, args...)` function type, emits a void call, and returns no MIR value to store. The raw-IR
test pins `call void` and rejects `call i32`; an integration test selects between two Unit targets
from runtime argv and executes both paths.

### 3.6 FIXED 2026-07-13 — `io.copy` preserves buffered-reader lookahead

`Reader` may hold unread bytes in `buf[start..filled]` after `read_line`. At the audit baseline,
ordinary `reader.read` drained that lookahead before reading the fd, but `align_rt_io_copy` read
`Reader.fd` directly. The fd offset had already advanced past the lookahead, so this sequence lost
data:

```align
import std.fs
import std.io

pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  line := buffer(1024)
  consumed := r.read_line(line)?
  w := fs.create(args[2])?
  copied := io.copy(r, w)?
  return Ok(())
}
```

`align_rt_io_copy` now goes through the shared `align_rt_io_reader_read` path for every chunk. That
path drains `buf[start..filled]` first, advances `start`, and only then reads fresh fd bytes; the
shared writer path and returned byte total are unchanged. The regression test uses `AB\nCDEFG`,
consumes the first line, then proves that copy returns `5` and writes exactly `CDEFG`. Any future
sendfile/splice path must retain the same precondition: it may start only after lookahead is empty.

### 3.7 HARDENED 2026-07-13 — checked allocation byte arithmetic before allocator work

Codegen currently forms byte sizes with ordinary signed LLVM multiply/add for `ArenaAlloc`,
`HeapAllocBuf`, and `SoaAlloc` before passing the result to the runtime
([allocation lowering around lines 4682-4729](../../crates/align_codegen_llvm/src/lib.rs#L4682)). A
wrap can turn a positive logical count into a small or non-positive allocation; the generated loop then
assumes the returned buffer covers the original count. Most safe producers are bounded by an existing
source allocation, but unsafe/FFI lengths and widening output types make that an observation, not a
proof.

Codegen now rejects a negative signed count, uses `llvm.umul.with.overflow` /
`llvm.uadd.with.overflow`, and separately rejects a negative result. The last check is required
because allocator calls take signed `i64`: a mathematically valid unsigned result in
`2^63..=2^64-1` would otherwise arrive as a non-positive byte count. `ArenaAlloc` and
`HeapAllocBuf` share the checked multiply; `SoaAlloc` uses an allocation-only checked offset walk
covering every column product, column end, alignment bump, and final total. Normal post-allocation
column reads/writes retain the existing offset helper and do not acquire duplicate guards. Every
failure calls the noreturn `align_rt_alloc_size_fail` before allocator work.

Raw and O2-folded MIR gates pin the largest fitting 16-byte count, one-over-limit, negative and
zero counts, widening `str` elements, and SoA padding overflow. Removing the signed-result check
reproduces `align_rt_alloc(i64 -9223372036854775808)` and fails the boundary gate; replacing the SoA
alignment checked-add with ordinary add fails the exact intrinsic-count gate.

---

## 4. Sequential pipeline: current output quality

### 4.1 SHIPPED / GOOD — fusion and allocation shape

| Terminal/shape | Current lowering | Assessment |
|---|---|---|
| `map/where/.../sum`, `count`, `min/max`, `reduce`, `any/all` | One counted loop; scalar accumulator; no intermediate allocation | Correct high-level shape; section 3.1 callable speculation fixed |
| `to_array` / `scan` | One output allocation sized to the source upper bound, one fused fill loop | Good single-allocation shape; `where` uses a real skip branch |
| `map_into(out)` | No output allocation; one length-preserving loop into caller storage | Best materializing path; scoped input/output alias metadata removes overlap checks |
| `to_soa` | One contiguous aligned arena buffer, one fused transpose | Good representation; wide schemas may benefit from already-planned blocking |
| `partition` | Two upper-bound output buffers, one pass | Predictable but write-stream/RSS heavy; measure compaction before changing |
| `sort` / `sort_by_key` | Materialize, stable bottom-up merge sort with 32-element insertion runs; decorate keys once; **adaptive total-order fast paths (below), SHIPPED** | Good worst-case/stability shape; ordered-run adaptation (`w64`) SHIPPED for total-order keys — ordered-input wins, negatives within ≈ 2% — see §4.1 |
| `scan` | Loop-carried dependency | Correctly scalar in the general case; lack of vectorization is not a missed LLVM flag |

The upper-bound allocation used by filtering does not necessarily touch every page: malloc-backed
unused capacity is generally demand-paged, and right-sizing group outputs already measured as a
no-op. Do not add a second count pass merely to make capacity equal length. It would evaluate a Pure
or Impure predicate twice, alter effects/trap timing, and add memory traffic.

**Adaptive stable-sort probe (2026-07-14, Ryzen 9 5950X, clang 22 O3, one pinned core):** the current
MIR algorithm was reproduced as 32-element stable insertion runs followed by bottom-up stable merges,
one same-size scratch allocation, and a full copy-back after every pass. A candidate retained that
shape but (1) returned before scratch allocation when the whole input was already ordered and
(2) copied an adjacent run pair without comparison-merging when its boundary was already ordered.
The benchmark included copying the fixed source into the working array, used balanced AB/BA order,
and repeated the medians twice:

| `u64` input state | 1,024 elements | 100,000 elements | 1,000,000 elements |
|---|---:|---:|---:|
| already sorted | 10-18x | 21-35x | 22-30x |
| only a tail swap | 3.1-3.7x | 2.9-3.1x | 2.6-2.8x |
| 1% adjacent swaps | 3.8-4.0x | 3.1-3.3x | 2.8-3.1x |
| random / reverse / 16-value cardinality | 0.97-1.03x | 0.98-1.02x | 0.98-1.02x |

A whole-input precheck alone is rejected: at 1,024 elements the tail-swap control regressed 17-19%
because it scanned the entire input and then paid the unchanged sort. The combined ordered-boundary
path recovers that work and keeps the same stable O(n log n) worst case. Adopt it as a **P1 measured
backend/MIR refinement**, with no source/API change, only for keys with a total order (integers,
characters, and byte-ordered strings). IEEE floats with NaNs must retain the current merge path unless
a separate exact legality proof is carried; `!(right < left)` is not a transitive boundary proof in
the presence of NaN. For `sort_by_key`, run the check after exactly-once input-order decoration so key
effects/evaluation count do not change.

The current implementation also allocates its element scratch (and `sort_by_key` key ping scratch)
before sorting even when `len <= 32`, although no merge pass can read it. Avoiding that unused
allocation measured 2.3x at two elements and 1.4x at eight; 16-32 elements were small/noisy. Delay
only the unused ping buffers, retain the insertion run, and gate this mechanical cleanup by zero
scratch allocations plus the document-13 short-size matrix rather than claiming a broad throughput
win.

**SHIPPED 2026-07-16 (the `w64` shape; total-order keys only).** All three refinements are in
`lower_array_sort` ([`crates/align_mir/src/lib.rs`](../../crates/align_mir/src/lib.rs)):
`sort_key_order` classifies the key scalar (fail-closed — every non-total scalar keeps the plain
merge; float structurally cannot take any new path), a whole-input ordered early exit before scratch
allocation, an ordered run-boundary straight-copy, and delayed `len > 32`-gated ping-buffer
allocation. Correctness is fully covered by
[`crates/align_driver/tests/sort_adaptive.rs`](../../crates/align_driver/tests/sort_adaptive.rs) (a
packed strictly-ascending differential/stability oracle across every input state and structural size
boundary, str keys, an impure-key evaluation-count pin, float/NaN behavior + a MIR gate, the
ping-scratch-behind-the-gate and width-gate MIR gates, and leak/double-free coverage). The measurement
probe is [`bench/adaptive_sort`](../../bench/adaptive_sort/README.md).

**Root-cause of the first-cut regression, and the fix (`w64`).** The first implementation applied the
ordered-boundary check on *every* merge pass and measured a real ≈ 7% regression on random/reverse.
An isolation sweep — one compiler emitting the pre-change baseline and each refinement independently
via `ALIGN_SORT_ADAPTIVE`, compared with a **drift-immune** median-of-adjacent-ratios harness plus an
identical-code control (WSL2 has no CPU-frequency control, so block-sequential timing is corrupted by
±25% between-block drift; the control quantifies the residual cross-kernel i-cache bias) — localized
the cost precisely: the ordered-run-boundary check on **pass 1** (`width == 32`), which has the most
run pairs and the least straight-copy benefit, is pure overhead on merge-heavy inputs. The
delayed-scratch refinement is throughput-neutral (its apparent cost was 100 % measurement bias:
control == real), and the whole-input precheck is free on out-of-order inputs (it exits at the first
inversion). Skipping the boundary check below `2 * SORT_INSERTION_THRESHOLD == 64`
([`SORT_BOUNDARY_MIN_WIDTH`]) removes the regression while keeping the wins, which come mostly from
higher passes. Drift-immune, control-corrected `before/after` on `sort_u64` (before =
`ALIGN_SORT_ADAPTIVE=off`; three runs, `taskset`-pinned):

| `u64` input state | 100,000 | 1,000,000 |
|---|---:|---:|
| already sorted | 3.5x | 3.5-3.6x |
| only a tail swap | 1.13-1.16x | 1.13x |
| 1% adjacent swaps | 1.16-1.20x | 1.13-1.14x |
| random | 1.00x | 1.00x |
| reverse | 0.99x | 0.99x |
| 16-value cardinality | 1.00x | 1.00x |

For **plain `sort_u64`** all three negative workloads are within ≈ 2 % of baseline (gate met) and
already-sorted / tail-swap / 1 %-swap keep material wins. `sort_by_key` adds a large already-sorted
win via the precheck (4.6-15.6x — the decorate cost dominates the tiny scan); `sort_str` (byte-lex
key) is 10.9x already-sorted and within ≈ 1 % on random/reverse. The delayed-scratch cleanup is proven
separately (a `len <= 32` sort allocates only the materialize buffer — plain: 1, keyed: 2 — versus
2 / 4 before). Worst case stays a stable O(n log n) merge; the NaN/total-order caveat holds (float
keys excluded).

**One keyed negative workload is over the 3 % line (recorded, pending a keyed-specific decision):**
`sort_by_key` on a ≤ 16-distinct-value key at 100,000 elements measures a **stable ≈ 3.4-3.7 %
regression** (corrected 0.963 / 0.966 / 0.963x across three runs; identical-code control 0.996-0.999x,
so it is real, not measurement bias). The same key at 1,000,000 elements is fine (≈ 1.00x), and every
other keyed workload (random/reverse at both sizes, low-cardinality at 1M) is within ≈ 2 %. Cause: the
keyed straight-copy must copy **two** buffers (elements + decorated keys), so refinement 2 has less
upside for keyed sorts, while a 16-value key makes the pass-2+ boundary decision a coin flip
(mispredict) that the copy no longer offsets — plain low-cardinality has the same tie pattern but a
one-buffer copy, so it stays ≈ 1.00x. Open decision (not taken unilaterally): raise the keyed boundary
width threshold above `w64`, or skip refinement 2 for keyed sorts (which would forfeit the keyed
tail-swap / 1 %-swap wins, ≈ 1.13-1.16x at 100k, that vanish anyway by 1M).

### 4.2 SHIPPED / GOOD — vectorization parity with C

The maintained comparison compiles five Align/C twins through the same LLVM. On the recorded x86-v3
run, all five match on loop-vectorized status, widest lane count, horizontal reduction, and the
load-bearing absence/presence of overlap checks:

- `map(...).sum()` and branchless `where(...).sum()` both form 256-bit integer reductions;
- `map_into` vectorizes without a runtime overlap guard because the disjointness proof reaches LLVM;
- hash fold and general scan remain scalar on both sides because they have real loop-carried state.

This is the right bar: compare equal algorithms under equal LLVM, not surface-language line count. The
remaining observed difference is loop interleave/unroll depth (clang emits more vector arithmetic per
body in the two reductions). Treat that as **MEASURE-FIRST**: only tune the pass pipeline or add a
targeted loop hint if a throughput benchmark shows a stable win across arm64 and x86-64. Equal width
with fewer unrolled operations is not by itself a bug.

The maintained wall-clock suite agrees with the IR result: a flat fused numeric pipeline is roughly at
Rust parity because both become the same native loop; the large wins come from layout and traffic
(column-only SoA scans around 7-12x over a Rust AoS control and filtered SoA aggregation around 3.5x
in the recorded runs). Cheap `par_map` is the important negative control: its current per-element
runtime thunk loses to the sequential/vectorized loop, which is why document 11's whole-range kernel
is already the priority rather than “more threads” or manual SIMD at the call site.

**Native machine-code recheck (2026-07-14, Ryzen 9 5950X, LLVM 22 release/O2):** exported opaque
slice kernels confirm that the vector IR reaches real AVX2 instructions, not merely vector-looking
IR. `map(x*2).sum()` uses 256-bit `vpaddq` loads/accumulators plus a horizontal reduction;
`where(x>0).sum()` uses `vpcmpgtq` + `vpand` + `vpaddq`; and `map_into` uses YMM loads, `vpaddq`, and
YMM stores. For 1,048,576 cached `i64` elements, those first two kernels were 1.40x and 2.25x faster
than an equal clang-22 O3 control compiled with loop/SLP vectorization disabled (median of nine,
32 calls/sample).

The associative wrapping-product reduction also vectorizes and measured 2.31x over its scalar
control. AVX2 has no packed 64-bit integer multiply, so LLVM correctly synthesizes it from several
`vpmuludq`/shift/add instructions; this is a real speedup on this host but remains target-cost-model
work, not a promise that every vector IR operation maps to one instruction. The semantic negative
controls also reach the expected machine shape: prefix `scan` is scalar because of its loop-carried
dependency, and ordered `f64` sum uses scalar `vaddsd` because reassociation would change IEEE
results. At the explicit fixed-vector layer, `vec4<i32>` dot lowers to one `vpmulld` plus horizontal
`vpaddd`, while `fma(vec4<f32>,...)` lowers to one `vfmadd213ps`.

**Dynamic-window convolution probe (2026-07-14, same host/profile):** this is the important
loop-nest qualification to the positive result above. A directly written safe 3-tap loop
(`dst[i] = src[i]*3 + src[i+1]*5 + src[i+2]*7`) already auto-vectorizes across output positions:
YMM 4-lane i64 / 8-lane f32 loads and stores, safe vector-prefix bounds, scalar tail, and a runtime
overlap check. Over 1,048,576 outputs it took about 0.21 ms (i64) / 0.10 ms (f32), versus about
0.46 / 0.36 ms for scalar controls and about 0.21 / 0.10 ms for equal clang auto-vectorized
controls. The ordinary loop path is therefore already good when the tap count is statically exposed.

A truly dynamic kernel was then written as the natural nested loop: for each output, reduce
`src[i+k] * kernel[k]` over runtime `k`. LLVM vectorizes the inner i64 dot, but repeats vector setup,
bounds reasoning, and a horizontal reduction for every output; it leaves ordered f32 scalar. A
semantics-preserving AVX2 ceiling instead put adjacent **outputs** in lanes and advanced `k` in the
original order, keeping one vector accumulator per output block. It used multiply+add rather than
FMA, and matched the scalar f32 bytes exactly. For 65,536 outputs:

| runtime taps | i64 current / output-lane | speedup | f32 current / output-lane | speedup |
|---:|---:|---:|---:|---:|
| 3 | 0.140 / 0.034 ms | 4.1x | 0.085 / 0.011 ms | 7.7x |
| 8 | 0.266 / 0.075 ms | 3.6x | 0.223 / 0.020 ms | 11.2x |
| 16 | 0.519 / 0.155 ms | 3.3x | 0.442 / 0.037 ms | 12.0x |
| 64 | 0.818 / 0.572 ms | 1.4x | 2.052 / 0.209 ms | 9.8x |

This promotes dynamic convolution/window-dot to a **P1 measured design candidate**, but not to a
generic loop-pass tweak: LLVM does not reliably choose the profitable outer/output-lane
vectorization from the nested scalar source. Give MIR an explicit convolution/window-dot vocabulary
only after the surface question (`convolve(kernel, out)` versus virtual
`windows(kernel.len()).map(dot(kernel)).map_into(out)`) is settled. Its acceptance gate must pin
empty/oversized kernels, exact output length, source/output non-aliasing, scalar tails, wrapping
integer arithmetic, byte-exact ordered FP without implicit FMA, and equivalent AVX2/NEON paths.

**Strided-2D/cache probe (2026-07-14, Ryzen 9 5950X, clang 22 O3, one pinned core):** a
768x1536 `f32` image with runtime row pitch (`width + 32`) shows that a minimal strided view does not
itself block optimization. A directly written fixed 3x3 interior stencil took 0.48-0.58 ms and its
machine code used YMM loads/multiply/add across adjacent output columns. Expressing the same taps as
a runtime nested loop took 6.28-6.45 ms; an explicit eight-output AVX2 kernel preserving tap order
and avoiding implicit FMA took 0.72-0.79 ms and was byte-identical. The missing optimization is again
dynamic output-lane vectorization, not a need to make row pitch part of the language syntax.

The dynamic square-kernel results reinforce the same boundary:

| kernel | natural nested loop | output-lane SIMD | explicit separable two-pass |
|---:|---:|---:|---:|
| 3x3 | 6.24-6.41 ms | 0.73-0.80 ms (8.0-8.7x) | 0.47-0.52 ms (12-13x) |
| 7x7 | 29.9-30.4 ms | 3.21-3.36 ms (8.9-9.5x) | 0.75-0.76 ms (39-40x) |
| 15x15 | 136.9-138.5 ms | 17.5-17.6 ms (7.8-7.9x) | 1.48-1.68 ms (82-93x) |

The separable path is deliberately **not** an automatic compiler transform: it requires the user or
kernel API to state separability and changed the ordered `f32` result slightly (maximum observed
absolute difference `1.79e-7`). It belongs in an explicit library/kernel operation, not hidden
algebraic inference.

Cache order was even more important in a 4096x4096 `u32` column-sum control (64 MiB active data).
Walking each column with a roughly 16 KiB row stride took 67.1-69.5 ms. Loop interchange retained the
per-column reduction order while streaming rows contiguously and took 2.17-2.24 ms (about 31x);
64-column tiling took 4.54-4.92 ms (about 14-15x). Therefore generalize the measured P1
convolution/window-dot candidate to a consumer-gated strided 2D/N-D stencil operation and carry
known-stride, traversal-order legality, alias, and interior/border facts in MIR. Prefer legal loop
interchange first and backend-chosen tiling only when per-output state or another constraint needs
it. Keep ROI/border policy and separability explicit in the library operation. Do not add
image-specific syntax or a general polyhedral loop system from this result.

### 4.3 SHIPPED 2026-07-15 — lazy multi-source `zip`

The current pipeline is fundamentally single-source. It can fuse arbitrary work derived from one
element, and `map_into` can write one disjoint destination, but it cannot directly express the
common pure shape `out[i] = f(a[i], b[i], c[i])` over runtime arrays/slices. An application can
materialize intermediate arrays or fall back to an indexed `loop`; the former adds avoidable memory
traffic, while the latter leaves the pipeline's canonical counted-loop, equal-length, alias, and
fusion facts behind. Residual addition, gating, AXPY-class work, multiple-column transforms, and a
future runtime-length dot are all consumers. This is a core dataflow gap, not an LLM algorithm.

**Directional ceiling probe (2026-07-14, Ryzen 9 5950X, clang 22 O3 `-march=native`, balanced
AB/BA, median of nine):** compute `out[i] = a[i] + b[i] * c[i]` either as one fused loop or as two
loops with `tmp[i] = b[i] * c[i]`. Allocation was outside the timed region, so the comparison gives
the staged form every benefit except its unavoidable extra write/read traffic:

| `f32` elements | fused ns/element | staged ns/element | fused speedup |
|---:|---:|---:|---:|
| 256 | 0.048 | 0.070 | 1.44x |
| 4,096 | 0.110 | 0.173 | 1.58x |
| 65,536 | 0.141 | 0.209 | 1.49x |
| 1,048,576 | 0.169 | 0.294 | 1.75x |
| 8,388,608 | 0.584 | 0.947 | 1.62x |

Retain a **consumer-gated P1 core design** with this preferred semantic shape:

```align
zip(a, b, c)
  .map(fn v { v.0 + v.1 * v.2 })
  .map_into(out)
```

- `zip` is a lazy pipeline source, never an allocated array of tuples. Each tuple is an SSA value
  assembled for one index and should disappear after inlining.
- Every input has the same runtime length; mismatch aborts before the loop, matching `map_into`'s
  existing length-contract policy. Evaluation remains in increasing input-index order.
- First slice: two or more arrays/slices of Copy scalar elements. Tuple-valued storage, Move
  elements, strided/indexed inputs, and parallel `zip` stay out until separate consumers prove them.
- `map`/`where`/reducers reuse their existing effect and inactive-lane legality. `map_into` must prove
  the destination disjoint from **every** source and emit destination-vs-source alias scopes, but
  must not claim that source inputs are disjoint from one another.
- A runtime-length `dot` can later consume the same multi-source loop machinery. Do not silently
  turn ordered floating-point `zip(...).map(mul).sum()` into a reassociated dot; an explicit fast
  dot needs its own numeric contract.

Acceptance requires an Align implementation to show one allocation-free counted loop, no tuple
storage, expected SIMD on x86-64 and arm64, exact mismatch/effect/trap order, and parity with a
manually fused equal-LLVM C control. Compare against both staged materialization and an indexed-loop
control so the feature is adopted for canonical expression/fusion, not because the control was
artificially weak. Do not add `map2`/`map_with`, `zip2`/`zip3`, or an automatic multi-array fusion
heuristic as parallel mechanisms; settle one `zip` source spelling with the first real consumer.

**Implementation result (2026-07-15):** the preferred spelling above is shipped. Checked HIR keeps
`zip` as a pipeline-only source carrying two or more Copy primitive-scalar arrays/slices and an
interned tuple type. MIR evaluates sources left-to-right, checks every length against the first
before constructing the terminal loop, loads one element from each source at the shared index, and
assembles only an SSA tuple. Existing stage/reducer control flow therefore preserves increasing
index, effects, and the post-`where` trap guard without a second pipeline engine. `map_into` checks
the destination against every source root. All runtime source loads use the same input-vs-output
alias scope, which permits source-source aliasing and never claims mutual disjointness.

`zip_pipeline.rs` pins a three-source runtime result, one counted MIR loop, no allocation, optimized
LLVM vectorization, a guarded division after `where`, runtime mismatch abort, static mismatch and
surface diagnostics, repeated-source legality, and destination-vs-any-source rejection. The tuple
never becomes array storage. v1 deliberately requires named arrays/slices, fixed literals, or
sub-slices; Move/borrowed text elements, nested zip, strided inputs, and parallel zip remain deferred.

### 4.4 Correct way to recover SIMD after the `where` fix

Do not choose between semantics and SIMD. Split the problem by legality:

```text
stages before first where        execute normally; vectorizable when independent
predicate                        compute a mask
inactive-lane-safe suffix        may use select/masked vector execution
other suffix/reducer calls       execute only on true lanes/elements
```

For reducing builtins, LLVM may still if-convert safe primitive arithmetic. For a general callable,
emit the correct branch and let profile/vectorization remarks explain the limitation. A later
target-aware masked-call/VP design is valid only when it truly suppresses traps on inactive lanes.

MIR should continue carrying width-independent facts: contiguous access, independence, alias scope,
reduction identity, effect, and the new `CanExecuteOnInactiveLane` legality summary. Fixed `vecN<T>` remains the
explicit kernel escape hatch; the ordinary pipeline should not bake in AVX2/NEON widths.

### 4.5 SHIPPED / MEASURED — deep-stage scaling is part of the pipeline contract

Pipeline fusion removes intermediate arrays and extra memory passes, but it does not make added
work free. For `N` input elements and `S` stages, the intended runtime shape is one counted loop
doing `O(N*S)` useful stage work, with no additional abstraction cost that grows non-linearly with
`S`. The shallow C-parity kernels in §4.2 established the starting baseline; the shipped depth gate
now checks that inlining, vectorization, register allocation, and code size remain healthy for a
deeply chained pipeline without claiming that added semantic work is free.

The shared fixture sweeps `S = 1, 2, 4, 8, 16, 32` for four representative families:

1. pure arithmetic `map` stages ending in `sum`;
2. inactive-lane-safe `where` plus builtin reduction, which should retain the branchless mask path;
3. scalar-capturing inline lambdas, proving captures stay as hoisted direct arguments; and
4. a general callable after `where`, whose required skip branch is a correctness control rather
   than a vectorization expectation.

It uses runtime-provided input so LLVM cannot constant-fold the pipeline. Every depth compares with
a semantically identical equal-LLVM C control. That independent backend ceiling is stronger than a
noncanonical hand-written Align data loop, which would share both the frontend and backend while
contradicting the language's pipeline-owns-data-path rule. Publish both absolute throughput and cost
per performed stage operation; a longer pipeline necessarily performs more arithmetic, so raw
latency alone is not evidence of abstraction overhead.

The gate inspects optimized IR as well as time:

- exactly one data loop and no intermediate collection or closure-environment allocation;
- no residual stage calls for the simple arithmetic and scalar-capture cases;
- the same vectorization decision, width, and reduction shape as the equal-LLVM control whenever
  the semantics permit vectorization;
- no unexplained depth knee from spills, failed inlining, excessive code growth, or lost loop
  interleaving; if one appears, retain the simplest failing depth as a regression fixture before
  changing lowering or optimization policy; and
- compile time and peak memory recorded separately from runtime throughput.

Compiler robustness remains a related but distinct gate. Method chains build nested AST receivers and
the compiler still has the accepted-depth versus 2 MiB-stack gap recorded in `open-questions.md`
under "Expression-depth cap". The integration test runs the depth sweep through `check`, MIR,
optimized LLVM, and object emission on a controlled 2 MiB-stack worker. A compiler stack overflow
is a build robustness defect, not evidence that the generated pipeline is slow, and increasing the
compiler stack must not be reported as a runtime performance fix.

**Recorded result (2026-07-15, Ryzen 9 5950X, LLVM/clang 22.1.8):** the 24-point native and
x86-64-v2 O2 sweeps stayed within 7.1% of their equal-LLVM C controls. At depth 32 the
Align/control ratios were 0.981-1.011 native and 1.000-1.005 baseline. MIR retained one cyclic
component per kernel, no intermediate allocation formed, all simple/capturing stage calls inlined,
and every legal family retained its vector reduction through depth 32. Native named/capture cost per
stage remained nearly flat; v2 showed a matching Align/C increase for long dependency chains, so it
is useful-work/code-shape cost rather than language abstraction overhead. With cache disabled, the
full fixture took 0.017 s to check, 0.017 s to emit MIR, 0.204 s to emit optimized LLVM, and 0.510 s
to emit a release object; sampled peak RSS was 52,152 / 46,788 / 70,292 / 77,844 KiB respectively.
The mandatory 2 MiB-stack integration gate completed in 0.64 s. Reproduction and the complete
baseline are in `bench/deep_pipeline/` and `crates/align_driver/tests/deep_pipeline.rs`.

---

## 5. Closure representation and inlining

### 5.1 SHIPPED / GOOD — non-escaping pipeline capture

An inline pipeline lambda is lifted to an internal function. Its element is the first argument and
captured Copy values are trailing value arguments. `stage_call_args` lowers capture reads in the loop
([lines 3816-3825](../../crates/align_mir/src/lib.rs#L3816)); LLVM inlines the internal lifted function
and hoists invariant capture loads.

A fresh optimized-IR probe of:

```align
fn run(xs: slice<i64>, k: i64) -> i64 =
  xs.map(fn x { x * k }).sum()
```

contained no lambda call, closure object, environment allocation, or indirect call. `k` was a loop
invariant in the vector body. This is exactly the representation Align's closure philosophy promises.
Do not replace capture arguments with a heap environment for ordinary pipelines.

Large Copy aggregates can make an ABI copy visible before inlining, so add an IR/throughput sweep for
8/16/32/64/128-byte captures. Prefer scalar replacement or a read-only call-scoped context only when
that sweep proves a surviving copy; do not penalize the common scalar capture preemptively.

### 5.2 SHIPPED / GOOD — known first-class closures usually devirtualize

A first-class capturing closure uses a frame-local environment and a fat `{thunk, env}` value. The
thunk loads captures and directly calls the lifted function
([thunk generation](../../crates/align_codegen_llvm/src/lib.rs#L1921),
[environment construction](../../crates/align_codegen_llvm/src/lib.rs#L6468)). A probe passing a known
capturing closure into a higher-order `apply` function optimized to one direct arithmetic expression:
the environment alloca, thunk, indirect call, and higher-order wrapper all disappeared.

That means blanket `alwaysinline`, hand-authored `readonly`, or a new closure ABI is not the first
optimization. Internal/private functions already receive the stock O2 inliner and FunctionAttrs.
Add a generated-IR regression that pins devirtualization for a known target, while retaining a
dynamically selected two-target case as the legitimate indirect-call negative control.

### 5.3 PROPOSED — direct-lower a spawn literal

`spawn(fn { ...captures... })` currently builds a frame closure environment, copies it into a task
environment, then reaches the lifted function through a runtime trampoline and closure thunk:

```text
frame env -> task-region env memcpy -> trampoline -> indirect thunk -> lifted function
```

The design record already describes the better representation: a spawn node carrying the lifted
target and captures, with a typed per-site trampoline. That removes frame staging, the redundant
environment copy, and the inner indirect call. This is **DOC DRIFT / previously intended work**, not
a reason to invent another closure form. It should follow the capture-lifetime P0 and packed task
record/low-lock scheduler work in document 11, because those changes touch the same record ABI.

Gate on generated IR (one task-region record, direct lifted call), tiny-task allocation/call counts,
fallible/Unit/non-Unit returns, panic/error ordering, and capture drop/lifetime tests. Do not mark the
optimization complete while the currently documented fallible spawn-lambda spelling is rejected by
type inference; that separate drift must be settled in the same review.

### 5.4 DOC DRIFT — fallible spawn-lambda inference is not wired

The settled task design shows a fallible spawned closure using `?`, but the current `check_spawn`
calls `lift_lambda(..., expected_ret=None)`. This complete shape is rejected with “`?` can only be
used in a function that returns a Result”, incomplete `Ok` inference, and a Unit result:

```align
fn get() -> Result<i64, Error> = Ok(7)

fn main() -> Result<(), Error> {
  task_group {
    t := spawn(fn {
      x := get()?
      Ok(x)
    })
    wait()?
    print(t.get())
  }
  return Ok(())
}
```

Thread the enclosing/expected task `Result` type into lambda lifting, then gate `Result<R, Error>`
inference, `?`, `wait()?`, `Task<R>.get`, error propagation, and non-fallible/Unit controls. Settle
this behavior before direct spawn-literal lowering so the optimized ABI is tested against the full
documented closure surface.

### 5.5 REQUIRED — effects belong on function types

Name-based call graphs work for direct calls but cannot soundly summarize an unknown function value.
Before capturing `par_map` is made truly parallel, carry the inferred effect on `fn` types and require
Pure at every explicit parallel boundary. Use the same fact as optimization legality for sequential
fusion, subject to the normative settlement in section 3.2. A closure's effect includes its lifted
body and the effect requirements of every higher-order parameter it invokes. Unknown/FFI function
pointers fail closed at a Pure boundary.

This also improves optimization legality: a Pure + inactive-lane-safe direct target may
inline/vectorize; Pure but trapping code may inline but cannot be executed on false lanes; an Impure
call may remain in an exactly ordered/guarded sequential loop but cannot be speculated or moved into a
parallel range. These are three distinct facts, not one overloaded bit.

---

## 6. Allocator and cache audit

### 6.1 Heap allocator: keep libc until evidence says otherwise

The owned heap family is a thin `malloc/free/realloc` ABI
([runtime around lines 7582-7628](../../crates/align_runtime/src/lib.rs#L7582)). Platform allocators
already have optimized small-size bins/thread caches and mmap-class large allocation paths. The
array-builder grows geometrically and freezes its compatible allocation without copying. Replacing
this globally with mimalloc or a custom slab would add deployment, FFI, and cross-platform cost without
a measured workload.

Prefer reducing allocation count and touched bytes. Reconsider a concurrent allocator only after
task/runtime benchmarks attribute a material fraction to allocator lock contention; compare the
platform allocator and at least one mature alternative under identical thread counts and RSS gates.

### 6.2 SHIPPED 2026-07-16 — initialized-before-read arena allocation classes

The arena is a correct bump allocator after a chunk exists, but every fresh chunk is created as:

```rust
vec![0u8; max(64 * 1024, need + align)]
```

([`Arena::alloc`](../../crates/align_runtime/src/lib.rs#L7095)). A one-byte first allocation therefore
requests a logically zero-initialized chunk of at least 64 KiB. Whether the allocator/OS physically
touches every page is platform-dependent and must be measured. The existing arena-pool experiment
isolates the explicit reuse/re-zero cost on its recorded Ryzen 5950X / WSL2 host:

| Shape | ns/request | Relative to current |
|---|---:|---:|
| current arena | 555.8 | 1.00x |
| pooled + mandatory full re-zero | 523.9 | 1.06x — below gate, correctly reverted |
| pooled, no re-zero upper bound | 41.2 | 13.5x |
| Rust bumpalo reset | 19.2 | 28.9x |
| malloc/free control | 23.7 | 23.5x |

The no-re-zero direction and 13.5x upper bound were already recorded with the rejected pool. The new
contribution here is the callsite-by-callsite initialization proof, a two-class internal ABI, and its
safety/performance gate; do not count the general idea of removing redundant zeroing twice.

The earlier “JSON missing fields require blanket zeroing” warning is stale: current known-schema
struct-array/SoA decoders reject a record unless every declared field was seen, and successful paths
write every semantic field. The call-site audit indicates a narrower initialization contract is
possible, but it must be proved per use:

- file-view fallback, template finish, clone/new, task capture, and builder finish immediately copy or
  store over the requested used bytes;
- `to_array` writes every returned element; unused upper-bound tail is outside the returned length;
- `to_soa` writes every field of every returned row;
- successful task result/error slots are written before read;
- JSON currently begins from zeroed storage, but strict successful decode writes all declared fields;
  inter-column/struct padding is not a semantic field and must never be read as initialized data.

Preferred implementation:

1. Represent chunk backing as uninitialized storage without ever creating a Rust `&[u8]` over bytes
   that have not been initialized.
2. Expose internal `alloc_uninit` and `alloc_zeroed` operations. The latter zeroes exactly the requested
   bytes, not the whole spare chunk.
3. Route a site to `uninit` only when every byte/field that can be semantically read, copied, hashed,
   compared, passed to FFI, or visited by Drop is initialized first. Unused capacity, filter tails,
   and layout padding may remain uninitialized only while no initialized reference/slice or bulk
   operation ever covers them. Keep every unproved/partial-init site on `alloc_zeroed`.
4. Use MIR initialization/drop facts, not function-name guesses, to maintain the proof.
5. Only after the split passes memory-safety gates, re-evaluate a capped thread-local chunk pool. The
   previous pool result remains closed for the blanket-zero policy.

**Implementation result.** Chunk backing is now an `ArenaChunk::Uninit(Vec<MaybeUninit<u8>>)` or
`ArenaChunk::Zeroed(Vec<u8>)`; no Rust byte slice covers raw capacity. `alloc_uninit` and
`alloc_zeroed` share bump/alignment logic. A fresh conservative chunk deliberately keeps `Vec<u8>`
so the platform allocator can retain lazy/calloc zero pages; a zeroed allocation in an uninitialized
chunk memsets only its requested range. The public/generated ABI and task-group records remain
conservative. The runtime routes only three proved overwrite sites to uninitialized storage:
file-view fallback copy, arena builder finish, and strict successful SoA JSON decode. Error paths do
not publish their raw allocation, and SoA padding is never a semantic field or bulk-read range.

The checked-in allocation-inclusive median-of-nine probe includes 1/8/48 B, 1 KiB, the 2.5 KiB
gateway shape, 64 KiB, 1/64 MiB, plus a median-of-nine p99 panel for 48 B/1 KiB/64 KiB task-shaped
conservative records. Overwrite paths improved 13.42-13.52x through 48 B, 10.78x at 2.5 KiB, 1.91x
at 64 KiB, and 1.99x at 1 MiB; 64 MiB stayed at parity. Conservative medians stayed within 1%, and
the task-shaped p99 panel stayed within the 5% gate. An intermediate exact-memset design was rejected
before shipment because it faulted every page of a fresh 64 MiB conservative allocation while the
old lazy-zero Vec did not.

Arena adoption gate:

- sweep 0/1/8/48 bytes, the 2.5 KiB gateway request, P/1K/64K tiny task records, and 1 MiB/64 MiB
  `to_array`/SoA outputs;
- record calls, bytes zeroed, minor faults, peak RSS, L1/LLC/TLB counters, ns/op, and p99;
- gateway improves at least 1.15x; large initialized-before-read paths regress no more than 3%; task p99 no more
  than 5%;
- strict JSON missing/duplicate/error paths, every error/drop-null path, and a sanitizer or equivalent
  uninitialized-read suite pass;
- mutations that publish a logical length before its element write, or bulk-copy/hash an unwritten
  tail/padding region, fail the safety gate.

The already-planned zero-size arena fast path is independent and should land first.

### 6.3 SHIPPED/PARTIAL 2026-07-16 — exact-size codecs plus x86 Base64/hex SIMD are live

At the audit baseline, Base64 and hex encoded into a Rust `Vec`, then copied that complete output
into a second Align-owned allocation. Their output sizes are known:

```text
base64 padded   4 * ((input_len + 2) / 3)
base64url       (4 * input_len + 2) / 3
hex             2 * input_len
```

The shipped scalar encoders compute the equivalent formulas with checked group/tail arithmetic,
allocate one exact `align_rt_alloc` payload, and initialize it through `MaybeUninit<u8>` without
constructing a reference that falsely claims fresh `malloc` bytes are initialized. Empty output is
the canonical `{null,0}`. Differential gates cover every length through 65 bytes, 256, 4096, all byte
values, both alphabets and padding forms, and overflow. This removes one allocation and one
full-output copy before any SIMD work.

The allocation-inclusive median-of-nine probe covered 0-65 bytes, 1 KiB, 1 MiB, and 64 MiB. Every
short case improved: Base64 1.19-1.65x, Base64url 1.18-1.71x, and hex 1.16-2.01x. At 64 MiB the gains
were 1.71x, 1.70x, and 1.86x respectively after the final chunked hot loop removed per-byte bounds
checks. The scalar direct destination is therefore the retained oracle.

The x86-64 runtime now dispatches inputs of at least 32 bytes to a two-lane AVX2 byte-shuffle
encoder and keeps shorter or non-AVX2 inputs on that scalar oracle. Both paths write the same exact
destination. A baseline-NEON implementation is present and cross-compiles for
`aarch64-unknown-linux-gnu`, but production aarch64 dispatch deliberately remains scalar until the
same checked-in crossover probe runs on native hardware; do not guess an ARM threshold. This is
separate from, and does not close, the already-deferred native aarch64 UTF-8 portability run.

The balanced median-of-nine x86 probe measured the following scalar/candidate ratios:

| case | Base64 | Base64url |
|---|---:|---:|
| 1..=64 selected-point geometric mean, allocation-inclusive | 1.05x | 1.05x |
| 32 bytes, allocation-inclusive | 1.22x | 1.21x |
| 1 KiB, allocation-inclusive | 4.12x | 4.09x |
| 1 MiB, allocation-inclusive | 5.22x | 5.27x |
| 64 MiB, allocation-inclusive | 1.52x | 1.55x |
| 1 MiB core input throughput | 11.47 GB/s | 11.64 GB/s |

Differential tests compare the architecture backend directly with scalar for every length through
4096, both alphabets/padding forms, every input alignment modulo 32, and a page-aligned 4096-byte
input. The AVX2 loop's readable-byte guard forbids overread; the scalar tail owns the final 0..27
bytes.

Hex was evaluated separately rather than inferred from Base64. The shipped x86 path maps 32 input
bytes into 64 lower-case bytes with nibble lookup, lane-local unpack, and one cross-lane reorder;
short/non-AVX2 inputs retain the scalar exact-destination loop. The independent NEON candidate maps
16 input bytes into 32 with table lookup plus `vzip1`/`vzip2`; it cross-compiles, but aarch64
production dispatch remains scalar pending the native run. The balanced x86 adoption probe measured:

| case | scalar / shipped hex |
|---|---:|
| 1..=64 selected-point geometric mean, allocation-inclusive | 1.21x |
| 32 bytes, allocation-inclusive | 1.98x |
| 1 KiB, allocation-inclusive | 10.84x |
| 1 MiB, allocation-inclusive | 12.04x |
| 64 MiB, allocation-inclusive | 1.36x |
| 1 MiB core input throughput | 27.58 GB/s |

Every length through 4096, every input alignment modulo 32, and a page-aligned 4096-byte input match
the scalar oracle byte-for-byte. Thus hex clears the previously stated independent gate:

- scalar reference for short input and unsupported targets;
- AVX2/byte-shuffle backend on x86-64, baseline NEON on aarch64; later ISA variants only when tested;
- one dispatch outside the hot loop, with differential scalar-equivalence tests.

Gate 0-64 bytes, 1 KiB, 1 MiB, and 64 MiB; require byte-for-byte and rejection parity, overflow tests,
at least 1.10x on a memory-bound large case, and no more than 3% regression on the short-input suite.
Do not use unstable `std::simd` in the stable runtime.

### 6.4 Other allocation dispositions

| Candidate | Disposition |
|---|---|
| owned temporary buffer donation | **ALREADY PLANNED** in document 10; wait for ownership/liveness proof |
| blocked wide AoS->SoA construction | **ALREADY PLANNED** in document 10 |
| packed spawn env/result/error record | **ALREADY PLANNED** in document 11; measure false sharing after block claiming |
| one `Arc<TgWaitState>` instead of separate task/cursor/barrier Arcs | New small follow-up; measure after scheduler P2 because the work-first descriptor may subsume it |
| I/O `Vec::resize(..., 0)` removal | Reader/`io.copy` plus audited UDP/pread extension **SHIPPED 2026-07-16** with one shared spare-capacity/raw-write discipline |
| JSON Vec->malloc final copy | Exact-count `array<i64>` direct fill **REJECTED 2026-07-16**: an extra lexical count pass fell to 0.71-0.73x at 1K-1M elements; retain one-pass staging unless a different ownership mechanism avoids the second parse |
| C-realloc-backed template Builder with zero-copy string freeze | Plausible P3; current evidence says per-write FFI, not final copy, dominates |
| upper-bound filter allocation right-sizing | Do not retry alone; untouched pages are lazy and prior right-sizing measured no win |
| SSO / hidden default arena / automatic global custom allocator | Rejected or unsupported by evidence; do not reopen here |

Allocator return `noalias` and hygiene attributes are already audited. Runtime-bitcode/LTO is the
existing route for removing hot ABI call overhead; do not add unsound `nofree` to bump allocation,
whose chunk-vector growth may free old metadata.

---

## 7. Blocking I/O and transfer-size audit

### 7.1 Current path matrix

| Operation | Small-transfer behavior | Large-transfer behavior | Assessment |
|---|---|---|---|
| `fs.read_file` regular known-size file | open/metadata/read path; exact final allocation | exact allocation + direct `read_exact`; SIMD UTF-8 validation; measured about 1.8x over Vec+copy at 128 MiB | **GOOD**; add a small-file crossover benchmark, not a second implementation by intuition |
| `fs.read_file_view` / bytes view | any nonzero regular file may mmap; zero/special/failure falls back to arena copy | same arena-scoped mmap path; no payload copy | **GOOD** copy avoidance, not nonblocking I/O: string view immediately UTF-8-scans/faults pages; bytes view may fault later |
| buffered `writer` | accumulates into 64 KiB, amortizing syscalls | flushes then writes a chunk >=64 KiB directly, avoiding double copy | **GOOD** for both sizes; `print` remains the deliberately slow debug rail |
| buffered `reader.read_line` | `memchr` finds newline; one payload append per lookahead span | 64 KiB lookahead; long lines may append several spans and reallocate while growing | **GOOD** baseline; scoped zero-copy line callback is already planned if copying dominates |
| `io.copy` | one raw-capacity 64 KiB allocation per call; final short write may enter writer buffer | portable fixed 64 KiB shared-reader/shared-writer loop | Memory-bounded, no pre-read zero-fill, and **byte-correct after buffered lookahead** (fixed §3.6) |
| `file.pread/pwrite` | one synchronous positional syscall/loop | caller-selected buffer size; pwrite handles partial writes | Correct, but blocks the calling OS thread; no batch/vectored surface today |
| `http.get_many` | prebuilt immutable requests copy each URL once | bounded dedicated blocking threads overlap latency; input-order slots | **GOOD**; redundant per-worker URL clone removed 2026-07-16 |

Small writes should go through the explicit buffered writer or a builder followed by one write. The
measured difference from flushing each tiny print is hundreds-fold; adding SIMD to the formatting loop
cannot compensate for a syscall/flush per record.

### 7.2 Blocking is visible, but the generic pool is not yet I/O-safe enough

The I/O APIs are synchronous and therefore block an OS thread. This is consistent with Align's
no-async-runtime decision. `task_group` is the explicit way to overlap independent blocking calls, and
`http.get_many` already uses a bounded blocking-specific claim loop with strong measured scaling.

Generic task-group I/O still shares the CPU-sized `ParPool` with `par_map`. Document 11 records the
now-fixed saturated nested deadlock and caller-draining work-first scheduler, followed by a
measure-first CPU/blocking execution-domain split. That is the answer to “are callers forced to wait?”:

- a single synchronous dependency must wait;
- independent calls can be overlapped explicitly;
- current generic overlap can occupy every CPU worker, but the document-11 caller-draining
  invariant guarantees structured forward progress;
- no source-level async/await is required to use Linux io_uring, kqueue for readiness-capable
  descriptors, or bounded blocking workers/platform file APIs under the same explicit operation.

`mmap` changes copying and paging, not the synchronization contract. `read_file_view` validates UTF-8
by scanning the complete mapping during the call and may fault/read every page; `read_bytes_view`
defers faults until consumption. `madvise`/`posix_fadvise` prefetch/sequential hints are
**ALREADY PLANNED / MEASURE-FIRST**, not currently emitted.

Do not add one thread per tiny transfer or an unbounded blocking pool. Record queue depth, active
blocking calls, CPU helper starvation, p50/p99, and file/network throughput under mixed workloads.

### 7.3 Already-planned large-transfer work

Now that section 3.6 makes buffered lookahead byte-exact, keep the portable `io.copy` loop as the oracle,
then dispatch by descriptor kind:

- Linux file->socket/pipe: `sendfile`/`splice`; file->file where valid: `copy_file_range`;
- macOS file->socket: platform `sendfile`; file->file: `fcopyfile`/equivalent only where metadata and
  offset semantics match the reference;
- otherwise retain the fixed buffer.

Every fast path must handle partial progress, EINTR/EAGAIN, SIGPIPE/EPIPE, current reader/writer
offsets, buffered lookahead/output, and fallback after partial transfer without duplication. Linux
sendfile/splice/io_uring-class dispatch is **ALREADY PLANNED**; macOS sendfile/fcopyfile design and
validation are a new extension from this audit. Require throughput/RSS results for 0 B, 1 B, 4 KiB,
64 KiB boundaries, 1 MiB, and multi-GiB sparse/real files.

The reader/lookahead and `io.copy` zero-fill removal **SHIPPED 2026-07-16**. Their buffers now reserve
raw capacity, pass only a spare-capacity pointer to `read(2)`, and call `set_len` with exactly the
successful byte count. A short read or EOF therefore never creates an initialized Rust slice over
the unwritten tail; EINTR retries while the logical length remains zero, and `io.copy` still drains
buffered lookahead through the shared reader path. The checked-in allocation-inclusive 64 KiB-window
probe improved fresh 0/1/4 KiB/full reads by 20.92x/20.83x/11.49x/1.98x.

The audited UDP receive and positional `pread` extension **SHIPPED 2026-07-16**. All three buffer
syscall paths now share `Buffer::prepare_uninit_window`, including the release-build capacity guard.
`recvfrom` publishes only its returned prefix while retaining kernel datagram truncation semantics;
`pread` now calls POSIX `pread(2)` directly instead of forming a full initialized `&mut [u8]` for
`FileExt::read_at`, preserving the caller's file offset, short-read/EOF behavior, and EINTR retry.

### 7.4 SHIPPED 2026-07-16 — small allocation/copy cleanup in `http.get_many`

The batch first creates `Vec<String>`, then each worker clones `urls[i]` into a new `HttpRequest` even
though `http_client_perform` only borrows the request. Prebuild one owned `HttpRequest` per URL and let
the uniquely claiming worker borrow `requests[i]`. This removes one String allocation/copy per request
without a lock or surface change.

Measure 1/8/64/1K URLs with short and long URLs, zero-latency loopback and 10 ms latency. Require at
least 10% improvement/allocation reduction in the zero-latency small-response case and no more than 3%
latency/throughput regression in the network-bound case. Batch cursor/slot atomics and false sharing
belong to the document-11 scheduler methodology, not this small cleanup.

The batch now constructs one immutable `HttpRequest` per borrowed URL before entering the scoped
workers. A uniquely claimed index borrows `requests[i]`; the former intermediate `Vec<String>` and
worker-local URL clone are gone. This removes exactly one String allocation and payload copy per URL
without changing the claim cursor, input-order response slots, shared keepalive pool, lowest-index
error selection, or run-to-completion cleanup.

The checked-in balanced median-of-nine construction probe covers 1/8/64/1K URLs at 32 and 2,048
bytes. The short 1/8 cases improved 62.7->50.1 ns (1.25x) and 393.9->333.3 ns (1.18x), while the
larger request-vector initialization made the isolated 64/1K construction controls 0.94x/0.92x;
the allocation reduction remains exactly N per N-URL batch. End-to-end old/new loopback controls at
1/8/64/1K URLs stayed flat or improved through 64 and moved 9.5->9.7 ms at 1K with zero injected
latency (2.1%); the corresponding 10 ms-latency results were 10.5/11.1/42.4/656.3 ms before and
10.5/11.2/42.4/655.8 ms after, all inside the 3% network-bound gate. The negative construction
result is retained rather than generalized into a claim that prebuilding is faster at every batch
size; the shipped reason is the per-entry allocation/copy removal with end-to-end non-regression.

Vectored `preadv/pwritev` or a batch positional-I/O surface may be valuable for alignpack/database
consumers, but there is no current repeated-block workload proving a language/API addition. Keep it as
a consumer-driven design question; do not hide many user-visible independent operations inside an
ordinary scalar call.

---

## 8. SIMD opportunity map

### 8.1 Already SIMD-accelerated — do not duplicate

| Workload | Current mechanism | Disposition |
|---|---|---|
| known-schema struct-array/SoA JSON structural indexing | simdjson-style 64-byte stage 1; AVX2+pclmul on x86, NEON on arm64, scalar oracle | **SHIPPED / GOOD** for its live consumers |
| UTF-8 validation | Lemire lookup method; AVX2/NEON/scalar differential tests | **SHIPPED / GOOD** |
| JSON quote/backslash scan | short scalar prefix, then `memchr2` runtime dispatch | **SHIPPED / GOOD** |
| substring contains/find/rfind | `memchr::memmem` with mature runtime dispatch | **SHIPPED / GOOD** |
| `read_line` newline search | `memchr` | **SHIPPED / GOOD** |
| ordinary numeric pipeline loops | width-agnostic MIR + LLVM loop vectorizer | **SHIPPED / GOOD** for legal independent shapes |
| zlib/zstd/OpenSSL-backed compression/crypto kernels | external optimized engines | Keep borrowing audited engines; self-hosted glue such as constant-time equality is a separate audit surface |

For the known-schema indexed decoders above, remaining scalar token/value work should not be
hand-vectorized without a profile. The structural index and UTF-8 passes already remove the bulk byte-scan cost; stage-2 gathers, number
conversion, schema lookup, and output writes can dominate. Likewise, short HTTP headers and CLI flags
usually lose to SIMD setup. Preserve the scalar prefix/crossover discipline.

### 8.2 P1 — x86 Base64/hex SIMD shipped; native ARM gates remain

Exact-final-allocation fill and runtime-dispatched x86-64 AVX2 backends for Base64 and hex are shipped
in section 6.3. Their cross-compiled NEON backends still need native aarch64 correctness and crossover
runs before production dispatch is enabled. Every backend writes the retained direct destination, so
SIMD cannot merely accelerate a buffer that is then copied in full.

Use architecture intrinsics plus a scalar oracle on stable Rust. Cache CPU feature selection outside
the inner loop. Differentially test every tail length, alphabet, padding form, invalid byte position,
noncanonical trailing bits, and page-boundary input. Report useful input/output GB/s separately from
allocator-inclusive call time.

### 8.3 P2 MEASURE-FIRST — SIMD compaction for materializing filters

`where(...).to_array()` and `partition` use a real per-element branch and append survivor(s). That is
semantically correct but can become branch/memory bound at unpredictable selectivity. A block
compaction kernel can:

1. evaluate the predicate for a block and form a lane bitmask;
2. popcount it to reserve a contiguous output span;
3. compact selected values into that span while preserving source order.

AVX-512 compress-store and SVE compact map naturally; AVX2/NEON need shuffle tables or narrower
sub-blocks. Keep a scalar path for short data, expensive/opaque predicates, unsupported element types,
and selectivity extremes where branch prediction wins. A post-`where` callable still must run only for
survivors; do not vectorize it by reintroducing section 3.1's speculation bug.

This is distinct from document 11's stable **parallel** compaction, though they can share per-block
counts/prefix metadata later. Gate scalar vs block compaction across 0/1/10/50/90/99/100% selectivity,
predictable and random masks, 1/4/8/16-byte elements, and 1 KiB through 256 MiB. Require at least 1.15x
on a named positive case, no more than 3% geometric-mean regression, exact order/drop/trap behavior,
and improved branch-miss or bandwidth evidence. Otherwise record-and-close it.

**MEASURED 2026-07-16 — the structural AVX2 kernel passes; production consumer integration remains
gated.** The checked-in ignored `stable_compaction_probe` gives scalar and SIMD the same precomputed
byte keep-mask, then compares the complete initialized output prefix and survivor count before every
timed case. Its AVX2 candidate uses ordered `pshufb` sub-blocks for one-byte elements and
`vpermd` control tables for 4/8/16-byte elements. Empty blocks skip the input and output entirely,
full blocks copy directly, and one-survivor blocks copy that lane directly; these cases are necessary
to avoid the low-selectivity regression of an unconditional full-width shuffle/store.

On an AMD Ryzen 9 5950X (`x86_64-unknown-linux-gnu`, rustc 1.96.0, LLVM 22.1.2), balanced median-of-five
runs covered all 168 combinations required above at 1 KiB, 1 MiB, and 256 MiB. The full matrix
geometric mean was **3.08x core** and **2.62x allocation-inclusive**. At 256 MiB the one-byte/random-50%
named positive case improved 9.61x core and 8.86x allocation-inclusive; source throughput for the SIMD
core was 2.36 GB/s. Per-width allocation-inclusive geometric means at 256 MiB were 3.66x, 1.96x,
1.22x, and 0.98x for widths 1/4/8/16. The worst core observation was approximately 0.99x. The
allocation-inclusive panel also contained a 0.66x outlier at width 16, predictable 0%: both sides make
the same untouched upper-bound virtual allocation, while their reusable-output core ratio was 1.00x,
so this is retained as allocation/fault noise rather than evidence for a different materializer.
The global geometric-mean and named-positive gates pass, and the recorded SIMD bandwidth establishes
the structural candidate.

Do **not** enable production dispatch from this result alone. The probe intentionally excludes
predicate evaluation and mask formation, while the current `where(...).to_array()` consumer does not
yet expose such a mask. The next gate must lower one real, total, SIMD-vectorizable primitive predicate
to predicate + mask + ordered direct materialization, benchmark that complete consumer against the
current fused scalar loop, and retain scalar execution for opaque/expensive predicates and any shape
that misses the crossover. It must also prove inactive-lane trap suppression and exact predicate/drop
order. Native aarch64 work remains deferred; no x86-only production path was introduced by this probe.

**CONSUMER GATE MEASURED 2026-07-16 — REJECTED, RECORD AND CLOSE.** The checked-in ignored
`stable_compaction_consumer_probe` puts a representative total primitive predicate (`i64 > 0`), AVX2
four-lane mask formation, and ordered direct materialization in the same timed kernel. Its scalar
control is the current inlined predicate/branch/append shape. Predictable and random
0/1/10/50/90/99/100% distributions at 1 KiB, 1 MiB, and 256 MiB compare the complete survivor prefix
and count before timing and black-box the output storage so intermediate stores cannot disappear.

The best candidate (empty-block skip, one-survivor direct copy, full-block direct store, otherwise
identity-table `vpermd`) produced a 42-case geometric mean of **1.93x core** and **1.44x
allocation-inclusive**. Size-panel geometric means were 2.80x/1.87x at 1 KiB, 2.14x/1.53x at 1 MiB,
and 1.21x/1.05x at 256 MiB. A named positive 1 MiB random-50% case reached 2.94x core and 2.81x
allocation-inclusive. However, the 1 MiB predictable/all-survivor case was only **0.63x core** and
0.49x allocation-inclusive; the same all-survivor input generated by the random panel reproduced the
loss at 0.72x/0.52x. Removing the full-mask branch and always applying the identity permutation did
not fix it (0.62x worst) and reduced the full-matrix core mean to 1.52x, so that variant was discarded.

This fails the no-regression gate despite positive geometric means. Do not specialize
`where(i64 > constant).to_array()` or add a selectivity prepass: evaluating a general predicate twice
would change call count/order, and a primitive-only sampler plus second implementation is not earned by
this result. Retain the scalar fused loop. Reconsider compaction only for a future consumer that already
owns a mask, or for an ISA/operation with a materially different compress primitive; native aarch64
work remains deferred.

### 8.4 Numeric parsing, formatting, and other byte loops

- JSON integer parsing is already a tuned one-pass scalar conversion after SIMD structural discovery.
  Treat wide decimal conversion as a separate benchmark; do not assume it beats scalar on normal
  short JSON numbers.
- Float parsing/formatting and Dragonbox-class work are already backlog items. Prefer a mature,
  semantics-matched algorithm or crate over handwritten target intrinsics.
- `str.eq`/comparison ride optimized slice operations; case-insensitive header comparison, trim, and
  reverse key scans are mostly short. Add length histograms/profile data before SIMD.
- Hash/group probing has separate SwissTable/control-byte plans. Do not add vector code to the current
  slot layout without first adopting the layout that makes group probes contiguous.

### 8.5 P2 — remove or diagnose redundant materialization only with the right proof

The explicit shape `xs.map(f).to_array().sum()` currently allocates, writes, rereads, and frees an
array. The best source-level answer is the existing fused spelling `xs.map(f).sum()`. Add a structural
rewrite lint so the allocation is visible and repairable even before ownership-based materialization
elision exists:

```text
redundant materialization before `sum`; use `xs.map(f).sum()`
```

A later MIR rewrite may remove an unobserved temporary, but only when ownership/drop/trap order is
identical. Do not silently erase a named or aliased materialization. This complements, rather than
duplicates, document 10's uniquely-owned buffer donation.

After the effect/inactive-lane summaries exist, `count` can also dead-eliminate value-only maps after
the last filter because their values do not affect the count. That is legal only for callables proven
total and safe to erase: Pure alone is not enough, since deleting a division-by-zero or nonterminating map
would change behavior. Gate with a positive primitive-map IR test and negative trap/allocation calls.

### 8.6 Compiler IR processing is not a good SIMD target today

The compiler's MIR itself is enum- and graph-heavy: control-flow traversal, type/effect propagation,
and instruction construction do not form long homogeneous numeric arrays. SIMD there would add
complexity while affecting compile time, not output throughput. Use algorithmic improvements,
deterministic maps, compact IDs/bitsets, arenas, and parallel compilation first.

For **code generated from MIR**, the opposite is true: keep bulk loops explicit and width-independent
so LLVM sees contiguous loads/stores, trip counts, alias facts, reductions, and direct/inlined bodies.
The document-11 whole-range `par_map` kernel is the critical example: it removes the runtime's
per-element indirect callback and exposes the loop to LLVM. That existing plan has higher value than
adding portable-SIMD operations to compiler passes.

### 8.7 Classical bit tricks — preserve semantics, not folklore

The techniques catalogued in older optimization guides remain relevant, but not as a blanket
source-to-source "bit hack" pass. Separate them by what survives modern targets:

| Technique family | Current policy |
|---|---|
| saturating/overflowing arithmetic, rotate, popcount, leading/trailing-zero count, byte swap, min/max, widening/narrowing, multiply-high, and explicit average rounding | Represent the operation's exact semantics in core/MIR when a real consumer exists, then lower through LLVM intrinsics or target selection. Do not expand it early into shifts, masks, and branches. |
| branchless select, carry/borrow tests, strength reduction, unrolling, and software pipelining | Emit ordinary SSA/control-flow plus the strongest legal alias/effect/range facts. Let LLVM choose by target, but retain machine-code gates for hot shapes because idiom recognition and cost models are not infallible. |
| SWAR packing several byte lanes into one scalar word, packed-scalar multiply, and large lookup tables | Keep only inside a measured leaf kernel when real SIMD is unavailable or the scalar word is itself the natural representation. These forms can obstruct auto-vectorization, increase cache traffic, and encode endian/rounding assumptions. They are not language semantics. |
| approximations such as reciprocal tables, reduced precision, or fast inverse square root | Never substitute implicitly. They change numerical semantics and belong behind an explicit approximate operation with an accuracy contract and an end-to-end workload. |
| manual prefetch, alignment padding, tiling, and cache-oblivious/block algorithms | Still important, but primarily memory-layout/algorithm decisions rather than bit tricks. Adopt only from cache-miss/bandwidth evidence; the backend already handles routine instruction scheduling and constant arithmetic. |

The branchless case is deliberately conditional. A predictable branch can be cheaper because it
avoids loading/computing the unchosen side. A data-dependent unpredictable branch or a SIMD lane
choice can be much cheaper as a mask/select. Selection is legal only when evaluating both sides is
safe; section 3's inactive-lane rule remains load-bearing.

**Representative probe (2026-07-14, Ryzen 9 5950X, clang/LLVM 22, `-O3 -march=znver3`, median of
nine):** over 1 MiB of bytes, a semantic unsigned saturating-add loop lowered to `vpaddusb` and took
27.3 us, while an equivalent source expansion through widen/add/clamp/pack took 59.5 us (2.17x
slower). Align already takes the good path: `slice<u8>.map(x.saturating_add(100)).map_into(out)`
forms `llvm.uadd.sat.v32i8` and native `vpaddusb`. This is evidence for preserving semantic
intrinsics, not for adding a new saturating feature.

For a mixed-width `u8` condition selecting `i32` values, clang's ordinary ternary loop chose a
per-lane conditional-load shape on AVX2. The equivalent XOR/mask expression vectorized to
`vblendvps`: 1M random selections took 2.55 ms versus 0.111 ms (23.0x), and the all-true control took
0.246 ms versus 0.111 ms (2.21x). This is a specific cost-model miss, not a universal branchless
rule. Preserve explicit MIR `select` for proven-safe lane choices and inspect native code for named
hot kernels rather than teaching users the XOR identity.

Conversely, the classic four-byte SWAR floor-average expression measured at parity with the natural
byte loop (26.8 versus 26.1 us per 1 MiB); LLVM vectorized the natural loop. Scalar rotate and
population count became single `rol` and `popcnt` instructions from semantic expressions/builtins.
Therefore do not add a general SWAR rewrite. A future core numeric/bit-operation audit should cover
the first row's missing semantic primitives, but each new surface still needs a concrete codec,
hash, bitset, image/DSP, or parsing consumer and scalar/vector differential tests.

---

## 9. Boundary with documents 10 and 11

Do not count the following as new findings from this audit:

- whole-program/runtime bitcode LTO, PGO/BOLT, build profile/target attributes, and artifact caching;
- owned temporary buffer donation and blocked wide AoS->SoA construction;
- `par_map` work-first progress, whole-range specialization, capture context, staged length-preserving
  parallelization, integer transform-reduce, byte/work-aware grain, task claim/completion batching,
  queue batching, packed task records, and false-sharing measurement;
- generic CPU vs blocking-I/O pool split;
- zero-size arena allocation, reader/`io.copy` zero-fill removal, JSON final-copy measurement;
- the shipped O(n log n) `sort`/`sort_by_key` replacement and one-time key decoration;
- Linux `io.copy` sendfile/splice/io_uring-class fast paths, scoped mmap views, and mmap advice/prefetch;
- native aarch64 Base64 and hex NEON activation as separate encoding backlog items.

New here are the `where` speculation reproduction and legality split, the ordinary-stage normative
effect conflict, two now-fixed closure-region UAFs, Unit indirect-call ABI mismatch,
buffered-`io.copy` data loss, allocation byte-overflow hardening requirement, per-callsite
initialized-before-read arena proof/gate, exact-final-destination codec fill, hex SIMD and macOS
copy-path probes, HTTP request-copy removal, and sequential SIMD stream compaction.
The measured total-order adaptive stable-sort path and tiny unused-scratch removal are also new here.

The document-11 nested scheduler deadlock and lifted/higher-order effect holes are fixed. #465
completed the function-type representation: concrete callables carry the inferred fact, while an
unresolved higher-order parameter stays fail-closed rather than introducing a second source
mechanism.

---

## 10. Implementation sequence

### Slice P0a — pipeline semantic legality

- [x] branch-guard all post-`where` work not proven safe on inactive lanes (2026-07-13);
- [x] settle ordinary sequential effects normatively with exact guarded order (2026-07-13);
- [x] close lifted/higher-order effect holes at current Pure boundaries (2026-07-13);
- introduce conservative inactive-lane legality only after the correct branch baseline;
- [x] update backend/design text that equated Pure with safe speculation (2026-07-13).

**Completion:** false predicates suppress every later potentially trapping operation; named,
capturing, and higher-order Impure functions cannot enter a parallel body; guarded sequential effects
match the settled contract; safe primitive positive cases retain vectorization.

### Slice P0b — closure lifetime and ABI

- [x] require spawn captures to outlive the task-group region (2026-07-13);
- [x] include callee/environment region in an indirect call's result (2026-07-13);
- [x] emit Unit indirect calls as `void` (2026-07-13);
- wire the documented fallible spawn-lambda expected `Result` type;
- [x] retain document 11's closure-effect and scheduler-progress P0s in the same release gate (2026-07-13).

**Completion:** all UAF repros are rejected, dynamic Unit targets have matching LLVM signatures, and
mutation tests prove every new gate.

### Slice P0c — buffered copy correctness

- [x] drain `Reader` lookahead before the fd loop (2026-07-13; future syscall fast paths retain the gate);
- [x] add a `read_line -> io.copy` byte/count regression test (2026-07-13; injected-error expansion remains useful).

**Completion:** the portable implementation is a byte-exact oracle from every reader state.

### Slice C0 — allocation arithmetic hardening

- [x] checked count/stride multiplication and SoA aligned-add before allocation (2026-07-13);
- [x] huge/zero/widening tests at heap, arena, and SoA allocation sites (2026-07-13; parallel-map's
  independent runtime check remains pinned).

**Completion:** no generated output loop can observe a buffer smaller than its logical extent because
of integer wrap.

### Slice P1 — cache traffic and visible loops

- [x] implement and benchmark arena initialized-before-read/uninitialized vs conservative-zeroed
  separation (2026-07-16; safety classes + lazy-zero preservation + adoption probe);
- [x] build lazy multi-source `zip` with its first real consumer; one allocation-free,
  tuple-storage-free vector loop and exact length/alias/effect semantics (2026-07-15);
- [x] direct-fill exact codec destinations and independently gated x86-64 Base64/hex SIMD
  (2026-07-16; native aarch64 activation remains deferred);
- add the total-order ordered-input/run-boundary stable-sort path and delay merge-only scratch until
  a merge pass can execute; retain the current path for float/NaN keys;
- execute document 11's range-kernel and low-lock scheduler sequence;
- [x] land the already-planned I/O uninitialized-read-buffer work (2026-07-16; reader lookahead,
  direct read, and portable `io.copy`; short-read/EOF/lookahead gates + adoption probe).

### Slice P2 — earned throughput fast paths

- syscall-dispatched `io.copy` after the portable buffered-reader oracle is fixed;
- [x] HTTP batch request-copy removal (2026-07-16; prebuilt immutable requests, one URL allocation/copy removed per entry);
- [x] measure structural SIMD stream compaction across the full selectivity/width matrix (2026-07-16;
  kernel passes; the complete `i64 > 0` predicate-to-mask consumer was then measured and rejected for
  a 0.63x named regression, so production integration is closed);
- reduction interleave hints/pass tuning only if equal-LLVM throughput evidence passes;
- direct spawn-literal lowering after lifetime/record ABI work is stable.

### Slice P3 — consumer-driven only

- vectored positional-I/O surface;
- Builder zero-copy string freeze;
- custom allocator, NUMA/affinity, or additional ISA versions.

---

## 11. Cross-cutting benchmark and regression gates

Every performance slice must publish:

- input distribution and size, target triple/CPU/features, LLVM/rustc versions, warm/cold state;
- allocation calls, allocated and actually touched/zeroed bytes, peak RSS and faults;
- scalar/reference vs new path, balanced AB/BA runs, p50/p99 and geometric mean;
- generated optimized IR/assembly for the load-bearing loop;
- target controls on arm64 and x86-64, with baseline and native/v3 where relevant;
- a negative workload that should not select or benefit from the fast path.

Correctness precedes the speed gate:

- pipeline stage order, trap suppression after `where`, wrapping integer behavior, ordered compaction;
- capture lifetime, drop count, error/panic order, Unit/non-Unit/fallible closure ABI;
- buffered lookahead then copy, short read/write, EINTR, partial transfer, EOF, invalid UTF-8/codec
  input, and allocation overflow;
- scalar/SIMD differential fuzzing, tail lengths, misalignment, and page boundaries.

For the flagship pipeline path, also retain the deep-stage sweep from §4.5. Shallow shape parity
does not close a regression that appears only after LLVM's inlining, vectorization, or register
allocation budget crosses a stage-count threshold.

No below-gate mechanism remains shipped in parallel with the reference implementation. Record the
result here and close it, as was done for arena pooling with mandatory re-zero.

---

## 12. Claude Code handoff checklist

1. Read `HANDOFF.md`, `CLAUDE.md`, documents 10-12, then the relevant M14/M15 and post-M9 roadmap
   entries.
2. Re-run every P0 reproduction at current HEAD before editing; line numbers may move, function names
   and semantic gates are authoritative.
3. Preserve the completed pipeline/closure/I/O and allocation-size gates before widening SIMD or parallel execution.
4. Keep `Pure`, `CanExecuteOnInactiveLane`, and ownership/region facts distinct. Never infer one from another.
5. Preserve the pipeline's direct, width-independent loop; do not introduce iterator/closure runtime
   allocation on the hot path.
6. Reuse the shipped scalar or portable implementation as the oracle for SIMD and syscall fast paths.
7. Keep existing planned work attributed to documents 10/11/open-questions; update one status ledger
   rather than duplicating designs.
8. Attach before/after optimized IR, benchmark data, mutation tests, and the exact fast-path selection
   rule to each implementation PR.
