# Parallel execution and generated-IR optimization audit

Status: **RECORDED 2026-07-12; correctness P0s implemented 2026-07-13.** The lifted-closure effect
edge and higher-order unknown-target fail-closed gate (§4), plus caller-draining nested scheduler
progress (§5), are fixed and regression-pinned. Performance work remains open. This is the durable parallel-runtime
and generated-IR companion to [`10-cache-first-optimization.md`](10-cache-first-optimization.md).
It separates already-recorded work from new findings, records two confirmed correctness blockers,
and gives a cache-coherence-first implementation order. Audit baseline: commit
`ad7e4c8b57ad`, arm64 macOS, eight logical workers, LLVM 22.1.8, rustc 1.96.1.

Labels in this document are deliberate:

- **CONFIRMED P0** — reproduced on the audit baseline; correctness or forward progress is broken.
- **REQUIRED** — an invariant the fix must preserve.
- **ALREADY PLANNED** — useful context, but not a new proposal from this audit.
- **PROPOSED** — the preferred implementation direction; settle details in the slice review.
- **MEASURE-FIRST** — plausible, but it must pass a written gate before adoption.
- **DOC DRIFT** — implementation and durable documents disagree; do not infer semantics from the
  stale description.

The main conclusion is intentionally conservative: **no new source syntax is needed for the next
parallel-performance wave.** Align's Pure `par_map`, value capture, ordered results, and structured
`task_group` already expose enough information to remove nearly all normal-path locks internally.

---

## 1. Decision summary

Do the work in this order:

1. ~~Close the capturing-closure effect hole.~~ **FIXED 2026-07-13**, including a conservative
   higher-order unknown-target gate until `FnTy` carries effects.
2. ~~Make `par_map` work-first and caller-draining.~~ **FIXED 2026-07-13** with a shared range
   cursor, caller drain loop, total-range completion barrier, and watchdog gate.
3. Implement the **already-planned** whole-range kernel so LLVM sees the loop body and the
   per-element indirect thunk disappears.
4. Extend that kernel with a read-only capture context, removing the current sequential fallback
   for capturing `par_map`.
5. Replace `task_group`'s per-task mutex/wake path with batched claims and a low-lock completion
   latch; batch queue submission too.
6. Fuse integer `par_map(...).sum()` so the parallel operation never writes and rereads its full
   intermediate array; this is the strongest new CPU-cache candidate.
7. Parallelize length-preserving staged pipelines first. Gate `where` separately because stable
   compaction needs counts and prefix offsets. Before widening, require every fused prior stage to
   be Pure; the current checker validates only the terminal `par_map` function.
8. Choose grain from bytes and estimated body work, not a fixed element count alone. Evaluate
   separate CPU and blocking-I/O execution domains only after the common work-first fix.

The cache-first principle applies directly: the important resource is often not the instruction
count but ownership of shared cache lines. One atomic range claim per coarse block is acceptable;
one atomic plus one mutex acquisition plus one wake attempt per tiny task is not.

The cache/artifact C0 sequence in document 10 remains the build-system priority. Both parallel P0
prerequisites are now closed; later purity or scheduler changes must retain their mutation and
watchdog gates before performance widening.

---

## 2. Language contract that optimizations must preserve

| Construct | Settled source-visible contract | Optimization consequence |
|---|---|---|
| `par_map(f)` | Parallelism is explicit; `f` must be inferred Pure | The runtime may schedule internally, but it may not admit hidden I/O/shared mutation |
| captures | By value; non-escaping pipeline captures become extra parameters | A parallel kernel may receive immutable capture data without a user lock |
| result order | Output index corresponds to input index | Workers write disjoint indexed ranges; unordered output is not an acceptable shortcut |
| `chunks(n)` | Explicit unit of coarse data parallelism | Keep it as the source-level control; do not add scheduler/grain knobs casually |
| `task_group` | Structured scope; all tasks join; `get()` is after `wait()` | No detached jobs may retain a task region or result slot |
| spawned task | May be Impure for blocking I/O; capture is by value | Scheduler must tolerate blocking and nested constructs without shared user mutation |
| ordinary `reduce` | No visible parallel construct is present | It must not silently begin spawning threads |

Sources: [`draft.md` parallelism](../../draft.md#11-parallelism),
[`design-notes.md` lambda philosophy](../design-notes.md#the-lambda-philosophy), and
[`non-goals.md` runtime/async exclusions](../non-goals.md#no-runtime-magic).

Consequences for the language surface:

- Do **not** add ordinary `thread`, `mutex`, or scheduler atomics to make these optimizations work.
  Pure bodies plus disjoint outputs are stronger information than a general shared-memory API.
- Do **not** add `schedule(dynamic)`, `workers`, `affinity`, or `grain` parameters now. The existing
  `chunks(n)` is the explicit escape hatch; defaults belong to the compiler/runtime cost model.
- Do **not** make ordinary `reduce` implicitly parallel. A future explicit parallel reduction is
  compatible with the philosophy, but it is already recorded and its associativity/floating-order
  contract remains unsettled.
- Keep ordered `par_map`. An unordered variant would add a second mechanism and weaken
  reproducibility merely to simplify a runtime implementation.

---

## 3. Current lowering and runtime shape

### 3.1 `par_map` is narrower than the design document says

The implementation lowers to the parallel runtime only when all of these are true:

```text
no prior pipeline stages
no captures
scalar / slice / chunks-compatible source
```

Otherwise it appends a normal `map` stage and executes the sequential collect loop
([MIR lowering](../../crates/align_mir/src/lib.rs#L2874-L2921)). In particular:

- a capturing `par_map` is sequential;
- `map(...).par_map(...)` is sequential even though it preserves length;
- `where(...).par_map(...)` is sequential;
- unsupported aggregate source shapes are sequential.

The parallel case is a single `Rvalue::ParMapParallel`, not the fused `ParLoop` described by
`04-mir.md` ([MIR node](../../crates/align_mir/src/lib.rs#L446-L449)). That opaque node carries a
source, function name, and input/output element types, but no explicit range body, captures, or
cost summary.

### 3.2 Generated IR crosses an indirect call once per element

Codegen emits a private helper of shape:

```text
void element_thunk(ptr input_element, ptr output_element)
```

The helper loads one element, calls the Align function, and stores one result
([thunk generation](../../crates/align_codegen_llvm/src/lib.rs#L3320-L3365)). The runtime owns the
counted loop and calls that function pointer for every element
([runtime loop](../../crates/align_runtime/src/lib.rs#L1457-L1467)). Consequently LLVM cannot see
the runtime loop and the map body in one optimization unit. Even when the user function inlines
into the helper, the hot loop still contains one indirect call per element.

The output is freshly allocated and the runtime return is already marked `noalias`, but that fact
does not compensate for hiding the counted loop behind the runtime ABI.

### 3.3 The shared pool and barriers

`ParPool` is one process-lifetime `Mutex<VecDeque<Box<dyn FnOnce()>>>` plus a `Condvar`
([pool](../../crates/align_runtime/src/lib.rs#L1376-L1418)). Every submit locks the global queue;
every worker locks it again to pop a job.

`align_rt_par_map` now:

1. allocates the complete output;
2. initializes the pool;
3. partitions into at most `workers` contiguous chunks, with `PAR_MIN_CHUNK = 32768` **elements**;
4. creates one shared range cursor and total-range completion barrier;
5. submits helper drain loops and runs the same drain loop on the caller;
6. waits until every claimed range publishes completion, then re-raises any recorded panic.

The already-planned tiny-call fix moves step 2 after the single-chunk decision. It does not address
nested progress, per-element IR, or warm steady-state contention.

`align_rt_tg_wait` reuses the same pool but has a different execution rule. Runners claim one task
at a time with `AtomicUsize::fetch_add`; the caller runs the same loop until the cursor is drained.
After **every task**, the runner locks a shared `TgBarrier`, increments `done`, selects the
lowest-index error/panic, and calls `notify_all`
([wait loop](../../crates/align_runtime/src/lib.rs#L7319-L7395)). Only the caller waits on that
condvar.

Codegen separately allocates each spawned task's capture environment, result slot, and optional
error slot, then registers one descriptor
([spawn codegen](../../crates/align_codegen_llvm/src/lib.rs#L4283-L4342)). Small result slots are
therefore densely bump-allocated; adjacent workers can write different words of one cache
line.

---

## 4. FIXED 2026-07-13: capturing and higher-order calls fail closed at `par_map`

### Reproduction

At the audit baseline this program was accepted as Pure at `par_map(worker)`, although the capturing closure performs
observable output:

```align
fn worker(x: i64) -> i64 {
  k: i64 := 100
  f := fn y: i64 {
    print(y)
    y + k
  }
  return f(x)
}

fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
```

`alignc check` reports success and emitted MIR contains both `par_map[worker]` and the lifted
`worker$lambda0` with a call to `print`. A second probe used a 7.88 MiB byte slice, so the fixed
threshold selected eight real runtime chunks; the supposedly Pure parallel body executed the
captured closure's output 1,905 times. This is not only a small-input sequential artifact.

### Cause

At the audit baseline `EffectScan` added a call-graph edge for `ExprKind::FnValue(name)`, closing the previously fixed
non-capturing function-value laundering hole. For
`ExprKind::Closure { lifted, captures }`, however, it visits only capture expressions and discards
the `lifted` function name ([effect scan](../../crates/align_sema/src/lib.rs#L2529-L2544)). A later
indirect call through a local does not recover that target. `worker` is therefore marked Pure even
though its lifted closure is Impure.

### Shipped correction and gate

- [x] Add the lifted closure function as an `EffectScan` call edge while continuing to visit captures.
  The conservative behavior matches the existing `FnValue` rule.
- [x] Add the capturing counterpart beside
  `impure_fn_via_fn_value_rejected_in_par_map` in `analysis_coverage.rs`.
- [x] Treat an indirect target whose effect is absent from `FnTy` as unknown and propagate that fact
  separately from observable I/O; reject it only at a later Pure/`par_map` boundary. Sequential HOF
  calls remain legal. A future function-type effect bit recovers pure-HOF precision.
- [x] Retain direct, transitive, inline-lambda, and non-capturing function-value purity tests.
- [x] Mutation gates: removing either the closure edge or unknown-indirect mark fails its dedicated test.

No scheduler or IR optimization should widen parallel execution until this soundness hole is
closed. The language promise is stronger than “usually race-free”: accepted Pure parallel bodies
must not reach observable side effects through a representation corner.

---

## 5. FIXED 2026-07-13: `task_group -> par_map` caller-draining progress

### Reproduction

On the eight-worker audit host, each spawned task ran a 40,000-element `par_map`. That is above
`PAR_MIN_CHUNK` and creates two inner chunks.

| Shape | Result |
|---|---|
| 1 task calling inner `par_map` | completes |
| 8 tasks calling inner `par_map` | completes in about 0.4-0.9 s |
| 9 tasks (`workers + 1`) calling inner `par_map` | no output; reproducible 5-10 s timeout |
| top-level `par_map -> par_map` control | completes in about 0.5 s |

The result was independently reproduced through the public runtime ABI and through an Align
program. At the audit baseline there was no `task_group -> par_map` regression test. The current “parallel path”
driver test uses only 12 elements, below the runtime threshold, so it proves the MIR call shape but
not multi-worker execution.

### Deadlock cycle

For 9 tasks, `tg_wait` submits 8 runner jobs and runs a ninth task on the external caller. The eight
pool workers all enter task bodies. Each task's inner `par_map` then:

1. queues its non-caller chunk onto the same `ParPool`;
2. runs chunk 0;
3. waits for the queued chunk.

Every pool worker is now waiting, while all jobs that could release those waits are behind them in
the pool queue. The external caller waits too. `task_group`'s caller-draining rule protects nested
`task_group -> task_group`, but `par_map` does not share that progress invariant.

The top-level `par_map -> par_map` control succeeds only because a top-level map submits at most
`workers - 1` helpers, accidentally leaving one pool worker to drain inner work. That spare worker
is not a correctness guarantee.

### Required work-first invariant

> A thread waiting for a structured parallel operation must be able to make that operation finish
> without requiring an idle pool worker.

The fix changes `par_map` to a shared range descriptor:

```text
next_range       AtomicUsize
remaining_ranges AtomicUsize
input/output/count/kernel/context
rare panic state + one parker
```

Pool helpers and the calling thread run the same loop: claim a coarse range, execute it, publish
completion, repeat until the cursor is exhausted. The caller must drain, not execute one fixed
range and immediately wait. A helper picked up after draining sees an exhausted cursor and exits
without touching input, output, or a call-scoped capture context. The return path waits for every
range to publish completion before exposing or freeing the output.

Each queued helper must own an `Arc` (or equivalent) to the scheduler-only descriptor/cursor. The
external call may finish while late no-op helpers are still queued, so that descriptor must outlive
their cursor check even though the input/output/context do not. The exhausted-cursor branch must
occur before loading any raw data pointer, and dropping the last scheduler reference must not
dereference those raw addresses.

Initialize `remaining_ranges` to the **total** range count, not a dynamically published claim
count. A worker may claim and then be descheduled before any second “active” counter update; a
dynamic claimed count can therefore let the caller return early. Every successful claim installs a
completion guard that decrements exactly once, including an unwind path. Return only after the
claim cursor is exhausted and `remaining_ranges == 0` with Acquire visibility.

Catch unwind around caller-executed ranges as well as helper ranges. Record a panic payload,
continue draining/joining, then resume it on the external caller after no valid claim can still
touch the output/context. A caller unwind that escapes immediately would invalidate a stack capture
context while queued helpers still hold raw addresses.

The implementation uses an `Arc<ParMapWork>` with an atomic range cursor and a mutex/condvar state
initialized to the total range count. Pool helpers and the caller both run `drain_par_map`; each
range is caught independently, decrements completion exactly once, and the lowest-index panic is
re-raised only after the join. Late helpers check the exhausted cursor before reading inert raw
addresses from the scheduler descriptor. A child-process unit test forces two workers, launches
`workers + 1` task-group tasks whose maps each have two ranges, and completes under a ten-second
watchdog. Breaking the drain loop after one range reproduces the timeout and fails the gate.

This fix is separate from the already-planned `n == 1`/tiny-call fast path. It establishes the
forward-progress requirement for all multi-worker calls.

---

## 6. Boundary with already-planned parallel work

| Item | Status before this audit |
|---|---|
| Cheap-`par_map` cost lint, backed by the 0.24-0.81x result | **ALREADY PLANNED** |
| Whole-chunk specialization / defunctionalisation to remove the per-element thunk | **ALREADY PLANNED** |
| Capturing `par_map` parallelization | **ALREADY INDICATED** as “implementation in progress”; this audit pins the context ABI |
| Pool initialization after the tiny/single-chunk decision; `task_group n == 1` fast path | **ALREADY PLANNED** |
| Persistent `ParPool` for `task_group` and caller-draining nested `task_group` | **IMPLEMENTED** |
| Caller-draining shared range claims for nested `par_map` | **IMPLEMENTED 2026-07-13** |
| `task_group` + blocking workers / a dedicated HTTP blocking pool | **ALREADY RECORDED/IMPLEMENTED FOR I/O CONSUMERS**; generic `task_group` application is new |
| Deterministic parallel-reduction candidates and generic associativity question | **ALREADY RECORDED; UNSETTLED** |
| Profile-guided performance diagnostics | **ALREADY RECORDED** |
| Structured deadline/cancellation | **ALREADY RECORDED** |
| Channels as the possible shared-state surface; general atomics still deferred | **ALREADY RECORDED** |
| GPU offload for Pure `par_map`/reduction | **FUTURE BACKLOG** |

Primary records are the cheap-body evidence and defunctionalisation note under
the “Codex perf / I/O / LLM research sweep” in [`open-questions.md`](../open-questions.md),
the quick-win queue in the same file, and the M7/post-M8 sections of `07-roadmap.md`.

The two P0 defects, length-preserving staged parallelization, integer transform-reduce fusion,
work-aware grain, queue batching, low-lock completion, packed task records, false-sharing
mitigation, and generic shared-pool interference were not implementation tasks before this audit.
The capture goal and blocking-I/O pool direction existed; this record makes their generic ABI,
progress, and measurement requirements concrete instead of claiming the ideas as new.

---

## 7. Generated-IR direction

### 7.1 Whole-range kernel — already planned, now made concrete

Replace the per-element callback ABI:

```text
void element(ptr input_element, ptr output_element)
```

with one generated kernel call per claimed range:

```text
void range(ptr context, ptr input, ptr output, i64 start, i64 end)
```

The generated `range` function contains the counted loop, typed GEPs, the fused map body, and
direct calls. Strides and element layouts are compile-time constants. LLVM can then:

- inline the user function or lifted non-escaping lambda into the loop;
- vectorize/unroll the loop and hoist loop-invariant captures;
- see the output as fresh, disjoint, and write-only;
- see input as read-only while leaving input/context aliasing conservative unless proven;
- pay one scheduler callback per coarse range instead of one indirect call per element.

Use LLVM 22's semantic `captures(none)` spelling on pointer parameters that the kernel does not
retain. The fresh output may additionally be `noalias` + `writeonly`; input and context may be
`readonly`, but input must not be `noalias` because a Copy capture can contain a view of the same
storage.

The intended IR gate is structural: no indirect call may remain in the hot element loop for a
directly known `par_map` body. Vectorization remarks and generated assembly are secondary evidence.
Runtime bitcode/LTO may help other runtime calls, but it is not required to expose a loop that the
compiler itself can emit.

The MIR should grow an explicit parallel-kernel node/body rather than preserving the current opaque
`ParMapParallel` call forever. It needs, at minimum, the fused body, capture operands, source/result
layout, grain/cost summary, ordering, and ownership/drop information. That restores the intended
`ParLoop` boundary without pretending the not-yet-implemented parallel reduction is settled.

### 7.2 Parallel capture context — prior goal, concrete ABI now pinned

The source semantics already say a non-escaping pipeline lambda uses captures as parameters. Extend
the range kernel with one immutable context pointer:

1. codegen builds a typed call-scoped record from captured Copy values;
2. the synchronous runtime call receives its address and never retains or dereferences it after a
   successful range claim;
3. the generated range kernel loads invariant fields once and passes them as direct extra
   arguments/inlined values;
4. mark the context `readonly` + `captures(none)` where LLVM's proven contract permits it.

This is not a heap closure environment and needs no new syntax. It fulfills the existing
captures-as-parameters design for the parallel lowering instead of falling back to sequential.
Copy captures may themselves contain borrowed/view aggregates, so retain the ordinary region and
liveness checks and do not infer that input and context are mutually `noalias`. Owned/Move captures
remain rejected until their existing ownership question is solved.

The late-runner proof from the work-first scheduler is load-bearing: a queued runner that starts
after the synchronous call has drained must inspect only scheduler-owned state, observe no range to
claim, and exit without touching the caller's context.

### 7.3 Length-preserving staged pipelines — new implementation task

Prior `map`/projection stages preserve length and can be fused directly into the same generated
range kernel:

```text
source.map(f).map(g).par_map(h)
  -> one ordered parallel range loop containing f -> g -> h
```

There is no semantic reason for the current sequential fallback. Start with scalar/Copy stages and
the same layout set already supported by direct parallel lowering. Ordered output/drop behavior
must match the sequential collect loop.

**REQUIRED widening precondition:** today `EffectScan` adds prior `map`/`where` functions only as
ordinary call-graph edges; `parmaps` records only the terminal function. Consequently
`xs.map(impure).par_map(pure)` can be accepted and is safe only because the staged path is currently
sequential. Before any stage executes inside a parallel range, require every prior callable stage
of that `ArrayParMap` to be Pure (including lifted/capturing forms), with named and capturing
negative tests. Alternatively, keep an Impure prefix materialized and sequential; never widen it
silently.

`where(...).par_map(...)` is a separate gate because output length is unknown. Two viable stable
algorithms are:

- two-pass: count survivors per range, prefix-sum counts, allocate exact output, rerun the
  predicate/maps and scatter to disjoint final offsets;
- one-pass local compaction: compact into per-range scratch, prefix-sum lengths, then copy ranges to
  final offsets.

Do not use one global atomic output cursor: it introduces a hot cache line and loses stable order.
Selectivity, predicate cost, scratch bytes, extra reads, and memory bandwidth determine the winner;
keep the current sequential path until a crossover is measured.

### 7.4 Transform-reduce fusion — new highest-value cache candidate

The current shape:

```align
total := xs.par_map(f).sum()
```

allocates and writes `N * sizeof(R)` bytes, joins, then reads the same bytes in a second serial
loop. The map's parallelism is already explicit, so a specialized range kernel can compute an
integer partial sum while computing each transformed value **without materializing the values**.
After join, combine the small indexed partial array.

Start with wrapping integer `sum`. Align defines integer overflow as two's-complement wrap, so
addition is associative modulo the integer width and regrouping across worker/range counts is
bit-exact. Integer `min`/`max` may follow once their empty-input contract and emitted shape are
pinned. Exclude floating sum, arbitrary reducers, and reducers whose errors/ordering are observable.
Apply the rewrite only when the `par_map` array is a directly consumed temporary with no other use
or alias. Generated partial additions must remain plain wrapping IR operations — never `nsw`/`nuw`.

This is distinct from silently parallelizing ordinary reduction:

```text
xs.map(f).sum()       stays a sequential fused pipeline
xs.par_map(f).sum()   already exposes parallel execution; the compiler removes its intermediate
```

The range kernel still executes every `f` exactly once, preserving traps/abort behavior of a Pure
body. It writes only one isolated partial per range instead of every transformed element. Inspect
and resume any joined panic before reading partial slots that may be uninitialized. Gate on:

- output allocation count goes from one to zero;
- result-buffer writes plus the second read disappear from IR and hardware-byte accounting;
- empty input returns the existing zero identity; signed/unsigned wraparound and every worker/range
  count are bit-identical;
- peak RSS, memory bandwidth, LLC misses, and wall time improve on a memory-bound positive case;
- heavy compute bodies and the non-fused `par_map` control regress no more than the global gate.

Use cache-separated per-runner/range partials only after checking the target/cache-line tradeoff;
there are only `O(ranges)` of them, so isolation is much cheaper than padding every task result.

### 7.5 Explicit-parallel producer materialization elision — later

Recognize shapes such as:

```align
xs.chunks(n).par_map(chunk_summary).sum()
```

without turning ordinary `sum`/`reduce` into a hidden parallel operation. The producer is already
explicitly parallel. A later MIR pass may keep per-chunk results in indexed slots and perform the
same serial, index-ordered terminal fold, eliminating a general owned intermediate and its drop
path. Do not reassociate floating operations or combine results in completion order. A truly
parallel final reduction remains the separate, already-recorded language-semantics question.

---

## 8. Low-lock runtime direction

### 8.1 Batch `task_group` claims and completion

Document 10 already records this as a CPU-cache probe; this section pins the parallel-runtime
shape. For a large group, let each runner claim a small contiguous block:

```text
grain = max(1, ceil(task_count / (workers * oversubscription)))
oversubscription candidate = 4..8
```

Keep grain 1 for small groups and measure highly skewed blocking I/O. A contiguous batch provides:

- roughly `O(workers * oversubscription)` cursor RMWs instead of `O(tasks)`;
- sequential task-descriptor reads;
- adjacent result slots usually written by the same runner, reducing false sharing.

Accumulate normal completions locally and publish once per runner/batch. A low-lock latch can use:

- an Acquire/Release atomic remaining count for normal completion;
- an atomic minimum error index, then read that task's `err_slot` after join;
- a mutex only for a real panic payload;
- one `notify_one` on the final transition, protected against lost wakeups by the parker mutex.

Preserve deterministic lowest-index error/panic selection until the language document explicitly
settles otherwise. Preserve current precedence too: after all tasks join, any panic is resumed even
if an ordinary task error was also recorded. Publish result/error-slot writes before a Release
completion decrement and perform an Acquire join/fence before reading them; a Relaxed atomic
minimum is sound only when that completion latch supplies the happens-before edge. Never return
before all result-slot writes are visible and all tasks have joined.

### 8.2 Batch pool submission

Add `submit_many`/runner-batch publication so one operation locks the global queue once, extends it,
and wakes the needed number of workers. Today both producer and consumer take the queue mutex once
per job. Queue batching is a smaller, safer first step than immediately replacing the queue with a
custom lock-free deque.

A dedicated bulk fork-join descriptor or bounded MPMC ring may be justified later, but only after
queue lock telemetry. The operation count is at most roughly the worker count once range kernels
replace element jobs; a complex work-stealing runtime must earn its permanent cost.

### 8.3 False sharing: schedule first, pad last

Task result/error slots are densely region-allocated. Blanket cache-line padding would multiply
region memory for tiny scalar tasks and encode a target-specific line size. First apply contiguous
block claims and separate genuinely hot scheduler counters from read-mostly descriptor data. Only
if cache-to-cache/HITM evidence remains should a cache-padded slot layout or per-runner result area
be prototyped.

`par_map` output ranges are already disjoint and contiguous; only range boundaries can share a
line. Align range starts/ends to element/cache-line boundaries when it does not create a tiny tail,
but do not add per-element padding.

### 8.4 Nested budget and the already-recorded blocking-pool direction

Work-first caller draining is the correctness foundation. After it lands, measure two further
policies:

- **Nested budget:** when called from a pool worker or another parallel region, submit fewer helper
  runners and let the current thread do more work. This limits late no-op jobs and queue growth.
- **Separate execution domains:** Pure CPU `par_map` and blocking-I/O `task_group` currently share
  one CPU-sized FIFO pool. Long I/O runners can starve CPU helpers; after the P0 fix the caller still
  completes, but the map may collapse to sequential speed. If mixed-load benchmarks prove this
  material, apply the already-recorded `task_group + blocking workers` direction generically:
  retain a CPU-sized data pool and a bounded blocking-task pool. `std.http.get_many` already uses a
  dedicated bounded blocking-I/O claim loop because the CPU-sized pool was measured as the wrong
  shape for network overlap; what remains new here is the generic scheduler application and its
  cross-construct progress proof.

One source-level parallel model does not require one physical queue. Both pools would activate only
behind explicit `par_map` or `task_group`, so no hidden source construct is introduced. This is a
larger resource-policy decision and stays after batching/latch work.

`task_group` permits Pure CPU-heavy tasks as well as blocking I/O, so the construct's effect alone
cannot classify workload dominance. Any blocking pool must stay bounded and be gated on CPU-heavy
task groups plus nested cross-pool calls; preserve caller-draining progress in both domains.

NUMA first-touch and affinity are low-priority, target-dependent probes. Default thread pinning can
hurt laptops, asymmetric cores, and shared hosts; do not expose or enable it without a concrete
server workload and multi-socket evidence.

### 8.5 Pack each spawned task record — measure first

For a fallible captured task, codegen currently calls `tg_alloc` for the capture environment,
again for the result, again for the error, copies the capture environment, then calls
`tg_register`. Codegen knows every size and alignment. It can lay out one typed
`{ captures, result, error }` record, allocate once, and register descriptor offsets/pointers.

This removes runtime calls and improves descriptor/env locality, but it is not automatically a
cache win: densely packed records can put result words written by different runners on one line.
Sequence it after block claiming, compare one-record vs current allocation on tiny and I/O-heavy
tasks, and retain separate allocations if the false-sharing cost exceeds the call/locality win.

---

## 9. Work- and byte-aware grain selection

`PAR_MIN_CHUNK = 32768` elements treats a byte transform and a 128-byte aggregate with an expensive
body as the same amount of work. The compiler knows more than the runtime:

```text
estimated work = element_count * MIR body cost
memory pressure = element_count * (input_bytes + output_bytes + staged bytes)
```

Attach a small static cost class/weight to the parallel MIR node. It need not predict nanoseconds;
it only needs to distinguish trivial arithmetic, memory-dominated copying, opaque calls, and heavy
non-vectorizable bodies. Combine that with bytes/range and worker count at runtime. Keep the
already-planned cheap-body lint as user guidance; the runtime threshold is the safe execution
fallback for a construct whose parallel intent is already visible.

Use several coarse ranges per worker only when load imbalance justifies it. The range-kernel ABI
makes 4-8x over-decomposition plausible because it pays one indirect call per range, but uniform
numeric maps should retain large contiguous ranges for prefetching and cache locality. Record the
chosen range count in benchmark telemetry so a regression is explainable.

No source annotation is proposed. `chunks(n)` remains the explicit form when the programmer knows
the natural unit better than the compiler.

---

## 10. Documentation drift to settle

### Task error selection

- `draft.md` and the settled design entry say `wait()?` propagates the “first” error.
- an older implementation plan says any failing task is acceptable;
- current runtime and tests select the failing task with the **lowest spawn index**.

Recommended settlement: define “first” as lowest spawn index. It is deterministic, matches the
implementation, and batching can preserve it with a local/atomic minimum. Completion-order error
selection would make reruns nondeterministic for negligible normal-path benefit.

### Pool implementation

Parts of `07-roadmap.md` still describe `std::thread::scope`/one OS thread per task, and
`06-runtime-std.md` still calls pool lifetime open. The current implementation uses the persistent
shared `ParPool` with caller-participating `task_group` and `par_map` runners. The P0 scheduler
slice updated both descriptions on 2026-07-13; retain this invariant in later scheduler edits.

### MIR and reduction

`04-mir.md` describes `par_map` as a real `ParLoop`; current MIR uses opaque
`ParMapParallel`. The same section sketches parallel reduction, while no complete source-visible
associativity/floating-order rule exists. Restore the explicit kernel body for maps, but do not let
that editorial cleanup silently declare generic parallel reduction settled.

---

## 11. Benchmark and regression gates

### Correctness and progress

- Capturing Impure closure through a function value is rejected from `par_map`; mutation removes
  the call edge and fails the test.
- An Impure named or capturing prior `map`/`where` stage is never widened into a parallel range;
  the accepted/rejected contract is pinned before staged parallelization.
- A child-process test runs `workers + 1` task-group tasks, each calling a multi-range `par_map`, and
  completes under a watchdog. Never let a deadlock regression hang the entire test runner.
- Cover worker counts 1, 2, and the host/default degree through a test-configurable pool.
- Retain nested `task_group -> task_group`; add `par_map -> par_map`, `par_map -> task_group`, and
  pool-saturated caller-progress controls.
- Preserve ordered output, all-task join, lowest-index error, panic propagation, drop counts, and
  capture-context lifetime.
- Force a caller-range panic while helpers are queued; all valid claims join, the scheduler `Arc`
  remains valid for late no-op jobs, and only then is the panic resumed.

### Generated IR and throughput

- Emitted whole-range kernel contains no indirect call inside the element loop for a direct body.
- Cheap arithmetic positive case vectorizes after specialization; a deliberately opaque body is
  the negative control.
- Transform-reduce fires only for a directly consumed temporary; wrapping partial adds carry no
  `nsw`/`nuw`, and panic is inspected before partial slots are read.
- Sweep input/output element bytes, body cost, element count around the threshold, and range count.
- Record sequential, old parallel, and new parallel results separately; report cold pool and warm
  steady state.

### Scheduler/cache-coherence

Sweep:

```text
tasks        P, 2P, 8P, 1K, 64K
body         no-op, ~1 us, 100 us, 1 ms, blocking I/O
distribution uniform, one slow task, alternating, heavy-tailed
nesting      depth 1/2/4/8 and mixed task_group/par_map
result slot  1/8/16/64/128 bytes
producers    1/2/P concurrent callers
```

Record ns/task, throughput, p50/p99, queue depth, enqueue/dequeue/late-runner counts, cursor and
completion RMW counts, mutex wait/hold, parks/wakes/context switches, cache-to-cache traffic where
available, CPU utilization, and peak task-region bytes.

Initial adoption gates:

- all progress tests complete;
- short-task throughput improves by at least 1.2x for latch/batching work;
- heavy uniform tasks and large maps regress no more than 3%;
- skew/blocking p99 regresses no more than 5%;
- nested queue depth is bounded by active operations/workers, not task fanout;
- padded layouts, split pools, and affinity each require their own additional positive workload.

Use the balanced AB/BA and cache-hierarchy methodology from document 10. Do not attribute an
order-warmed cache to the scheduler change.

---

## 12. Implementation sequence

### Slice P0 — soundness and forward progress

- [x] Add the missing lifted-closure effect edge, higher-order fail-closed gate, and negative tests (2026-07-13).
- [x] Replace fixed `par_map` chunk waiting with caller-draining shared range claims (2026-07-13).
- [x] Add a forced-multi-worker `task_group -> par_map` child-process watchdog gate (2026-07-13).

**Completion:** no Impure closure reaches a Pure map, and every structured operation can complete
with zero idle pool workers.

### Slice P1 — range IR and captures

- Introduce the explicit parallel range-kernel MIR shape.
- Generate one typed loop body per range; erase per-element indirection.
- Pass immutable capture context and parallelize current capturing cases.
- Pin IR/vectorization and ordered/drop behavior.

**Completion:** direct and capturing scalar maps use the same parallel range path; no indirect call
is inside a directly known hot loop.

### Slice P2 — low-lock task execution

- Batch queue publication.
- Add adaptive contiguous claims.
- Replace per-task barrier mutex/wake with runner-local accumulation and final-transition latch.
- Measure false sharing before padding.
- Probe one-allocation packed task records after block claiming is stable.

**Completion:** normal completion has no per-task mutex acquisition; deterministic errors and
nested progress remain pinned.

### Slice P3 — widening and cost model

- Fuse wrapping-integer `par_map(...).sum()` and eliminate its full intermediate array.
- Require every callable prior stage in a parallelized `ArrayParMap` to be Pure, then parallelize
  length-preserving staged pipelines.
- Add body/byte-aware grain.
- Probe stable parallel compaction for `where` separately.
- Probe explicit-parallel producer/ordered-terminal materialization elision.

### Slice P4 — resource policy, only if earned

- nested helper budget/idle hints;
- CPU vs bounded blocking pool split;
- NUMA/affinity only for a demonstrated multi-socket consumer.

---

## 13. Claude Code handoff checklist

1. Read `HANDOFF.md`, `CLAUDE.md`, documents 10 and 11, then the M7/post-M8 roadmap entries.
2. If touching parallel code, close both P0 reproductions before performance work.
3. Do not count whole-range specialization, the cheap-body lint, tiny-pool fast path, or generic
   parallel-reduction discussion as new findings; their prior records remain authoritative.
4. Preserve the caller-draining invariant across every construct and every later scheduler rewrite.
5. Keep capture context call-scoped and prove late queued runners cannot dereference it.
6. Keep normal `reduce` sequential unless an explicit source-visible parallel contract is settled.
7. Record benchmark gates and below-gate closures in this file; do not leave an unearned scheduler
   mechanism in parallel with the old one.
