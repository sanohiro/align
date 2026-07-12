# Pipeline, closure, memory, I/O, and SIMD audit

Status: **RECORDED 2026-07-13; partially implemented 2026-07-13.** The spawn-capture lifetime gap
(§3.3), closure-result environment region gap (§3.4), Unit indirect-call ABI defect (§3.5), and
buffered `io.copy` data loss (§3.6) are fixed and regression-pinned; the other
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

1. A reducing pipeline speculatively executes every stage after `where`, and the reducer itself, on
   rejected elements. Pure does not mean non-trapping or guaranteed to terminate. Division by zero
   after a false predicate aborts today.
2. The implementation documents say ordinary pipeline callables are Pure, while the normative spec
   constrains only `par_map` and the compiler accepts Impure sequential stages. Optimizations cannot
   assume a premise the language has not settled.
3. ~~`spawn` could retain a view backed by an inner arena until after that arena was freed.~~
   **FIXED 2026-07-13**, including wrapped and nested-closure captures. The related first-class
   closure-result escape is also fixed by including the callee environment's region in an indirect
   call result.
4. ~~An indirect `() -> ()` call was emitted with an `i32` return type while its thunk was `void`.~~
   **FIXED 2026-07-13.**
5. ~~`io.copy` read the fd directly and skipped bytes already held in a buffered reader's
   lookahead.~~ **FIXED 2026-07-13.**

Separately, generated dynamic allocation byte counts use unchecked signed multiply/add. A concrete
safe-source exploit was not established, so this is **REQUIRED hardening**, not a confirmed P0.

After those are closed, the highest-value performance refinements from this audit are:

1. Split arena allocation into **proven-initialized-before-read / uninitialized** and conservative
   **zeroed** paths. A fresh chunk logically initializes at least 64 KiB even for a tiny request;
   existing reuse measurements show that mandatory full re-zeroing dominates that microbenchmark.
2. Fill exact-size Base64 and hex output directly into its final allocation, then add the already
   planned runtime-dispatched Base64 SIMD backend; hex SIMD is a separate new measure-first probe.
3. Evaluate SIMD block compaction for materializing `where`/`partition`, separately from reducing
   `where`; do not preserve the current unsafe speculative-execution trick merely to get vector code.
4. Remove a redundant URL/request copy in `http.get_many`, and complete the already-planned I/O
   uninitialized-buffer and `io.copy` syscall fast paths.

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

### 3.1 CONFIRMED P0 — `where` does not guard later stages or the reducer

`lower_array_reduce` accumulates predicates into a Boolean mask, but keeps evaluating subsequent
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

The false predicate rejects `0`, but `reciprocal(0)` still runs and reaches the division-by-zero abort.
The same class includes checked indexing, explicit fatal operations, allocation/OOM, and
nontermination. **Pure means no observable I/O/shared mutation; it does not imply total,
non-trapping, non-allocating, or speculatable.** LLVM's own legality rules make the same distinction.

Required immediate correction:

1. Once a `where` mask is false, branch around every later stage and the reducer call for that
   element. Stages before the first `where` still execute, preserving source order.
2. Retain branchless identity/select only where every operation executed on an inactive lane is
   locally proven safe. Use a summary such as `CanExecuteOnInactiveLane`; do not mechanically map a
   broad function effect bit to LLVM's `speculatable` attribute.
3. The proof is a conjunction: Pure/no observable side effect, no trap/abort, will return, no
   allocation whose OOM is observable, and valid provenance/bounds for every inactive-lane memory
   access. Initially infer it only for a small whitelist such as wrapping integer arithmetic,
   bitwise operations, comparisons, and loads whose address is independently proven in bounds.
   Division, checked indexing, allocation, opaque calls, and loops fail closed.
4. A later backend may use target masks or LLVM VP operations, but masked execution must suppress the
   unsafe operation, not merely discard its result.

Regression gate:

- false `where` followed by integer division by zero and by out-of-range indexing does not execute
  those operations;
- a `map` before `where` still executes at its existing semantic position;
- generic `reduce`, `any`, and `all` functions are guarded too;
- a whitelisted, total arithmetic `where(...).sum()` retains the vectorized positive shape;
- mutation that changes the guard back to a result-only `select` fails the trap tests.

### 3.2 CONFIRMED DOC/LEGALITY CONFLICT — ordinary pipeline effects are unsettled

The effect pass walks ordinary stage call edges, but `check_parallelism` validates only the terminal
`par_map` function ([lines 2413-2420](../../crates/align_sema/src/lib.rs#L2413)). The implementation
type/MIR documents say ordinary `map`/`where`/reducer callables require Pure, but `draft.md`'s
normative side-effect rule constrains `par_map` only, the language summary does not forbid sequential
effects, and the compiler accepts them. Rejecting all existing Impure sequential pipelines would
therefore be a language change, not a performance bug fix.

The following checks and runs successfully, printing `1`, `2`, then `0`:

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

The output is `1`, `2`, then `0`: direct evidence for section 3.1's speculation bug. It is not by
itself authority to ban `noisy` from an ordinary sequential pipeline.

Required settlement before optimization widening:

1. **Recommended compatibility default:** allow Impure sequential callables, preserve exact source
   order/evaluation with real guards, and use inferred effects only as fusion/vectorization legality.
   Require Pure for every stage moved into an explicit `par_map` range.
2. Alternatively, if Align deliberately wants all data-processing callables Pure, first change
   `draft.md` and `language-spec`, define diagnostics/migration, then enforce it consistently for
   `map`, `where`, `reduce`, `scan`, `partition`, `any`/`all`, and `sort_by_key`.
3. Settle `sort_by_key` before its planned decorate-sort-undecorate rewrite: an Impure key function's
   call count is currently O(n^2), while decoration calls it once per element. Either require Pure or
   normatively define exactly-once key evaluation.

Section 3.1 is independent: even after choosing Pure-only, a Pure callable may abort and cannot be
speculated without the stronger inactive-lane proof.

The already-recorded missing lifted-closure effect edge in document 11 remains a P0. Adding only that
edge is insufficient for a higher-order function:

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

`apply` contains an indirect target that the name-only effect graph cannot recover. The already-planned
effect bit on function types is therefore a prerequisite for parallel capture contexts and sound
optimization legality. Until that lands, unknown function values must be conservatively Impure at a
Pure/parallel boundary; this does not require rejecting a guarded sequential call.

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

### 3.7 REQUIRED HARDENING — check allocation byte arithmetic before allocator work

Codegen currently forms byte sizes with ordinary signed LLVM multiply/add for `ArenaAlloc`,
`HeapAllocBuf`, and `SoaAlloc` before passing the result to the runtime
([allocation lowering around lines 4682-4729](../../crates/align_codegen_llvm/src/lib.rs#L4682)). A
wrap can turn a positive logical count into a small or non-positive allocation; the generated loop then
assumes the returned buffer covers the original count. Most safe producers are bounded by an existing
source allocation, but unsafe/FFI lengths and widening output types make that an observation, not a
proof.

Reject a negative signed count first, then use checked unsigned `count * stride` and checked
aligned-add, with a cold abort before allocation on overflow. The runtime parallel-map allocation
already demonstrates checked multiplication. Pin the
largest fitting count, one-over-limit, widening element size, SoA padding overflow, and zero-count
cases before changing arena initialization policy.

---

## 4. Sequential pipeline: current output quality

### 4.1 SHIPPED / GOOD — fusion and allocation shape

| Terminal/shape | Current lowering | Assessment |
|---|---|---|
| `map/where/.../sum`, `count`, `min/max`, `reduce`, `any/all` | One counted loop; scalar accumulator; no intermediate allocation | Correct high-level shape; section 3.1 must fix unsafe speculation |
| `to_array` / `scan` | One output allocation sized to the source upper bound, one fused fill loop | Good single-allocation shape; `where` uses a real skip branch |
| `map_into(out)` | No output allocation; one length-preserving loop into caller storage | Best materializing path; scoped input/output alias metadata removes overlap checks |
| `to_soa` | One contiguous aligned arena buffer, one fused transpose | Good representation; wide schemas may benefit from already-planned blocking |
| `partition` | Two upper-bound output buffers, one pass | Predictable but write-stream/RSS heavy; measure compaction before changing |
| `sort` / `sort_by_key` | Materialize, then insertion-sort | Fusion boundary is correct, but the current O(n^2) algorithm/key recomputation is a serious **ALREADY PLANNED** replacement (stable O(n log n) plus decorate-sort-undecorate) |
| `scan` | Loop-carried dependency | Correctly scalar in the general case; lack of vectorization is not a missed LLVM flag |

The upper-bound allocation used by filtering does not necessarily touch every page: malloc-backed
unused capacity is generally demand-paged, and right-sizing group outputs already measured as a
no-op. Do not add a second count pass merely to make capacity equal length. It would evaluate a Pure
or Impure predicate twice, alter effects/trap timing, and add memory traffic.

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

### 4.3 Correct way to recover SIMD after the `where` fix

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

### 6.2 Highest-value new refinement — initialized-before-read arena allocation classes

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

### 6.3 PROPOSED — exact-size codecs should allocate once

Base64 and hex encode into a Rust `Vec`, then `owned_str_from_vec` performs a second allocation and
copies into Align-owned storage
([encoding implementation](../../crates/align_runtime/src/lib.rs#L5375)). Their output sizes are known:

```text
base64 padded   4 * ((input_len + 2) / 3)
base64url       (4 * input_len + 2) / 3
hex             2 * input_len
```

All additions and multiplications in these integer formulas must use checked arithmetic. Then
allocate the final Align string once and fill it directly. This removes one allocation and one
full-output copy before any SIMD work. Then implement the
**ALREADY PLANNED** Lemire-class Base64 runtime dispatch behind the same ABI. Hex SIMD is a separate
new **MEASURE-FIRST** extension and must beat the simple scalar exact-destination loop:

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
| I/O `Vec::resize(..., 0)` removal | Reader/`io.copy` **ALREADY PLANNED**; UDP/pread are a new audited extension; use spare-capacity/raw-write discipline |
| JSON Vec->malloc final copy | **ALREADY PLANNED** measure-first |
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
| `io.copy` | one 64 KiB allocation per call; final short write may enter writer buffer | portable fixed 64 KiB shared-reader/shared-writer loop | Memory-bounded and **byte-correct after buffered lookahead** (fixed §3.6) |
| `file.pread/pwrite` | one synchronous positional syscall/loop | caller-selected buffer size; pwrite handles partial writes | Correct, but blocks the calling OS thread; no batch/vectored surface today |
| `http.get_many` | per-request allocations matter | bounded dedicated blocking threads overlap latency; input-order slots | Correct concurrency shape; request construction has one removable copy |

Small writes should go through the explicit buffered writer or a builder followed by one write. The
measured difference from flushing each tiny print is hundreds-fold; adding SIMD to the formatting loop
cannot compensate for a syscall/flush per record.

### 7.2 Blocking is visible, but the generic pool is not yet I/O-safe enough

The I/O APIs are synchronous and therefore block an OS thread. This is consistent with Align's
no-async-runtime decision. `task_group` is the explicit way to overlap independent blocking calls, and
`http.get_many` already uses a bounded blocking-specific claim loop with strong measured scaling.

Generic task-group I/O still shares the CPU-sized `ParPool` with `par_map`. Document 11 already records
the saturated nested deadlock and the required caller-draining work-first scheduler, followed by a
measure-first CPU/blocking execution-domain split. That is the answer to “are callers forced to wait?”:

- a single synchronous dependency must wait;
- independent calls can be overlapped explicitly;
- current generic overlap can occupy every CPU worker and must not be widened until the document-11
  progress invariant is fixed;
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

The **ALREADY PLANNED** zero-fill removal covers reader/lookahead and `io.copy`; this audit extends the
same proof obligation to UDP receive and `pread`. Use `spare_capacity_mut`/`MaybeUninit` or raw pointers
and set the initialized logical length only after a successful syscall. Never form an initialized Rust
slice covering the unwritten tail of a short read.

### 7.4 New small allocation/copy cleanup in `http.get_many`

The batch first creates `Vec<String>`, then each worker clones `urls[i]` into a new `HttpRequest` even
though `http_client_perform` only borrows the request. Prebuild one owned `HttpRequest` per URL and let
the uniquely claiming worker borrow `requests[i]`. This removes one String allocation/copy per request
without a lock or surface change.

Measure 1/8/64/1K URLs with short and long URLs, zero-latency loopback and 10 ms latency. Require at
least 10% improvement/allocation reduction in the zero-latency small-response case and no more than 3%
latency/throughput regression in the network-bound case. Batch cursor/slot atomics and false sharing
belong to the document-11 scheduler methodology, not this small cleanup.

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

### 8.2 P1 — exact-destination SIMD Base64 and hex

This is the strongest unimplemented byte-kernel candidate. Lemire-class Base64 SIMD was already on
the encoding backlog; exact-final-allocation fill for Base64/hex and hex SIMD are new here. Combine
the Base64 work, and any hex backend that passes its own gate, with section 6.3 so SIMD does not merely
accelerate a buffer that is then copied in full.

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
- O(n^2) `sort`/`sort_by_key` replacement and one-time key decoration;
- Linux `io.copy` sendfile/splice/io_uring-class fast paths, scoped mmap views, and mmap advice/prefetch;
- Base64 SIMD as an encoding backlog item.

New here are the `where` speculation reproduction and legality split, the ordinary-stage normative
effect conflict, two now-fixed closure-region UAFs, Unit indirect-call ABI mismatch,
buffered-`io.copy` data loss, allocation byte-overflow hardening requirement, per-callsite
initialized-before-read arena proof/gate, exact-final-destination codec fill, hex SIMD and macOS
copy-path probes, HTTP request-copy removal, and sequential SIMD stream compaction.

The document-11 lifted-closure effect hole and nested scheduler deadlock remain P0 and must be solved in
the same correctness wave. The new higher-order effect finding strengthens its planned function-type
effect solution; it does not replace it with a second mechanism.

---

## 10. Implementation sequence

### Slice P0a — pipeline semantic legality

- branch-guard all post-`where` work not proven safe on inactive lanes;
- settle ordinary sequential effects normatively; preserve accepted Impure call order unless/until the
  language spec changes;
- close lifted/higher-order effect holes and require Pure for every stage moved into `par_map`;
- introduce conservative inactive-lane legality only after the correct branch baseline;
- update the backend/design text that currently equates Pure with safe speculation.

**Completion:** false predicates suppress every later potentially trapping operation; named,
capturing, and higher-order Impure functions cannot enter a parallel body; guarded sequential effects
match the settled contract; safe primitive positive cases retain vectorization.

### Slice P0b — closure lifetime and ABI

- [x] require spawn captures to outlive the task-group region (2026-07-13);
- [x] include callee/environment region in an indirect call's result (2026-07-13);
- [x] emit Unit indirect calls as `void` (2026-07-13);
- wire the documented fallible spawn-lambda expected `Result` type;
- retain document 11's closure-effect and scheduler-progress P0s in the same release gate.

**Completion:** all UAF repros are rejected, dynamic Unit targets have matching LLVM signatures, and
mutation tests prove every new gate.

### Slice P0c — buffered copy correctness

- [x] drain `Reader` lookahead before the fd loop (2026-07-13; future syscall fast paths retain the gate);
- [x] add a `read_line -> io.copy` byte/count regression test (2026-07-13; injected-error expansion remains useful).

**Completion:** the portable implementation is a byte-exact oracle from every reader state.

### Slice C0 — allocation arithmetic hardening

- checked count/stride multiplication and SoA aligned-add before allocation;
- huge/zero/widening tests at heap, arena, and parallel allocation sites.

**Completion:** no generated output loop can observe a buffer smaller than its logical extent because
of integer wrap.

### Slice P1 — cache traffic and visible loops

- implement and benchmark arena initialized-before-read/uninitialized vs conservative-zeroed separation;
- direct-fill exact codec destinations, then evaluate already-planned Base64 SIMD and the separate
  measure-first hex SIMD probe;
- execute document 11's range-kernel and low-lock scheduler sequence;
- land the already-planned I/O uninitialized-read-buffer work.

### Slice P2 — earned throughput fast paths

- syscall-dispatched `io.copy` after the portable buffered-reader oracle is fixed;
- HTTP batch request-copy removal;
- SIMD stream compaction only if its selectivity matrix passes;
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

No below-gate mechanism remains shipped in parallel with the reference implementation. Record the
result here and close it, as was done for arena pooling with mandatory re-zero.

---

## 12. Claude Code handoff checklist

1. Read `HANDOFF.md`, `CLAUDE.md`, documents 10-12, then the relevant M14/M15 and post-M9 roadmap
   entries.
2. Re-run every P0 reproduction at current HEAD before editing; line numbers may move, function names
   and semantic gates are authoritative.
3. Fix pipeline/closure/I/O correctness and allocation-size hardening before widening SIMD or parallel execution.
4. Keep `Pure`, `CanExecuteOnInactiveLane`, and ownership/region facts distinct. Never infer one from another.
5. Preserve the pipeline's direct, width-independent loop; do not introduce iterator/closure runtime
   allocation on the hot path.
6. Reuse the shipped scalar or portable implementation as the oracle for SIMD and syscall fast paths.
7. Keep existing planned work attributed to documents 10/11/open-questions; update one status ledger
   rather than duplicating designs.
8. Attach before/after optimized IR, benchmark data, mutation tests, and the exact fast-path selection
   rule to each implementation PR.
