# Runtime and core/std Bootstrap

Implementation overview of the `align_runtime` ABI and the core/std bootstrap boundary. This closes
the source→executable design used by `05-backend-llvm.md`.

Policy:

```text
keep the runtime thin       no GC. only the "minimum the language requires"—arena / parallel / panic / mutable buffer, etc.
ownership is on the compiler lifetimes & release points are settled in MIR (03/04). the runtime allocates/frees exactly as told
core is close to the language  write it in Align itself as much as possible. drop to the runtime (C ABI) only for the minimal lowest layer
std is the OS boundary       std is a thin wrapper over OS syscalls. built on core
```

Current per-module surfaces live in `core-design/` and `std-design/`.
The [runtime ABI ledger](20-runtime-abi-ledger.md), typed registry and declaration golden
own the exact native signatures; the sections below summarize their roles.
The original bootstrap order is retained in §10.

---

## 1. Runtime structure

`align_runtime` is a thin native Rust library exposing a C ABI. It uses the Rust standard library
where the shipped runtime needs OS integration, allocation, synchronization, or the persistent
worker pool. The driver links it with the object (`01`/`05`).

```text
align_runtime
  entry helpers  argv construction / error reporting for the compiler-generated main wrapper
  arena          bump allocator + bulk reset
  heap           explicit heap (thin wrapper over the malloc family)
  par            data-parallel runtime (work-stealing or chunk splitting)
  task           I/O concurrency (task_group)
  buffer/builder mutable byte buffer
  panic          abort + message
  intrinsics     memcpy/memset etc.; SIMD helpers via RUNTIME CPU-feature dispatch
```

**SIMD in the library is runtime-dispatched** (`open-questions.md` "Build targets & portability"):
because the default build targets a portable per-arch baseline, the wide-SIMD speedups (JSON / UTF-8
/ string scan, bulk copy) come from the *library* detecting the host CPU at run time and falling back
safely — so one binary uses AVX2 / NEON when present across a varied cloud/Docker fleet.

How the dispatch actually works (important detail): runtime adaptation requires **function
multi-versioning**, not a single portable function. A SIMD routine is compiled in several variants
(e.g. `#[target_feature(enable = "avx2")]` and a baseline one) and a thin entry selects among them
via `is_x86_feature_detected!`. **`std::simd` does not by itself produce a runtime-adaptive binary** —
it lets us write each variant's body *portably* (one source compiling to SSE/AVX/NEON per the active
target features), but the per-feature variants + the runtime selector are still explicit. In practice
the cheapest path is to lean on crates that already do this internally (`memchr` etc.); we add our own
multi-versioned routine only where no crate fits. Either way **no hand-written per-architecture
intrinsics**, and x86-64 + aarch64 are covered from one source.

`// OPEN:` whether to pin static linking, and how far to depend on libc (`05 §10`). Whether to hit the OS directly (syscalls) or go through libc is a decision shared with std.

---

## 2. Value ABI (the contract between compiler and runtime)

Match the MIR/codegen layout (`05 §1`). Views and finished arrays use pointer/length
pairs; mutable builders and buffers are opaque runtime handles.

```text
slice<T>     { T* ptr, i64 len }
array<T>     { T* ptr, i64 len }
str          { u8* ptr, i64 len }
string       { u8* ptr, i64 len }         // owned bytes
builder       Builder*                  // opaque growable state
buffer        Buffer*                   // opaque mutable byte storage
arena handle  Arena*   (opaque pointer. its contents are runtime-private)
```

Native signatures specify whether a header passes by value, through separate
pointer/length arguments, or through an output cell. Capacity belongs to mutable
runtime state, not to the finished `array<T>` header.

---

## 3. arena allocator

The center of Align's memory model (`draft.md` §6.4). bump allocator + bulk reset at the block exit.

```text
Arena* align_rt_arena_begin(void)
void*  align_rt_arena_alloc(Arena*, i64 size, i64 align)
void   align_rt_arena_reset(Arena*)        // bulk-release all allocations. no individual free
void   align_rt_arena_end(Arena*)          // return the arena itself
```

Allocation advances a pointer within a chunk, acquiring another chunk when needed.
`alloc` rounds up to the requested alignment. `reset` releases retained file mappings
and chunks while keeping the arena handle; `end` also releases the handle.

codegen correspondence (`05 §4`):

```text
arena { .. }  →  a = arena_begin(); ...body (alloc is arena_alloc(a,..))...; arena_reset(a)/end(a)
```

Because typecheck has verified that a view inside the arena does not escape (`03 §7`), the runtime tracks no lifetimes at all.

Named `arena a {}` and explicit region placement are implemented. Their lifetime
and allocation rules are in [core-design/arena-heap.md](core-design/arena-heap.md).

---

## 4. heap

```text
void* align_rt_alloc(i64 size)
void  align_rt_free(void* ptr)
i32   align_rt_raw_try_alloc(i64 size, void** out)
```

MIR's `Drop` determines ordinary release points; codegen calls the appropriate
typed cleanup or `align_rt_free`. Source `raw.alloc` and `raw.free` require
`unsafe` and use the same flat allocator family. `raw.try_alloc` uses the
status/output-cell entry to return allocation refusal as `Result<raw, Error>`;
the caller still owns initialization and release. [Plan236](236-fallible-raw-carriers.md)
fixes its status and size rules.

---

## 5. Data parallelism (par)

The target that MIR's `ParMapParallel` (`04 §6`) lowers to (`05 §7`).

```text
void* align_rt_par_map(
  void* capture_ctx, void* in_buf, i64 count,
  i64 in_stride, i64 out_stride,
  i64 work_weight,
  void (*kernel)(void* capture_ctx, void* in_buf, void* out_buf, i64 start, i64 end))

{ void* ptr, i64 len } align_rt_par_map_filter(
  void* capture_ctx, void* in_buf, i64 count,
  i64 in_stride, i64 out_stride,
  i64 work_weight,
  void (*count_kernel)(void* capture_ctx, void* in_buf, void* counts, i64 start, i64 end),
  void (*scatter_kernel)(void* capture_ctx, void* in_buf, void* out, i64 start, i64 end))

i64 align_rt_par_map_reduce(
  void* capture_ctx, void* in_buf, i64 count,
  i64 in_stride, i64 result_stride,
  i64 work_weight,
  void (*kernel)(void* capture_ctx, void* in_buf, void* partial,
                 i64 start, i64 end))
```

- Allocate one owned output buffer, split its disjoint element ranges across the process-resident
  `ParPool`, and return the buffer pointer. Small inputs run on the caller to avoid pool overhead.
- `work_weight` is a compiler-generated bounded hint (`1`, `2`, or `4`), not a source-level tuning
  knob. The runtime combines it with `in_stride + out_stride` (or `in_stride + result_stride` for
  reduction) to derive a conservative caller-only floor. Invalid values fall back to `1`; the common
  scalar shapes retain the measured 65,536-element floor, narrower shapes may require more elements,
  and wider shapes may reach the same byte budget with fewer.
- The kernel is a generated typed loop around a Pure function (`03 §8`), so output ranges do not
  race. It receives a call-scoped immutable capture context, the complete bases, and one disjoint
  `[start,end)` range; it loads captures once and calls the known body directly per element. A
  primitive-scalar length-preserving `map` chain uses the same ordered loop.
- `align_rt_par_map_filter` is the stable materialization path for callable primitive-scalar
  `where` stages. Its count kernel writes one survivor count per range; the runtime prefix-sums those
  counts in input order, allocates the exact result, and invokes the scatter kernel with one stable
  byte offset per range. The map/filter chain is Pure and is intentionally evaluated in both passes;
  the result is `{null,0}` when no element survives. SoA projections, richer string-search
  expressions, unsupported aggregate layouts, and other unsupported layouts remain on the sequential
  path. A
  compiler-recognised invariant `str.contains` filter uses the same stable count/scatter path. A
  materialized `chunks` source uses the ordinary range kernel with `slice<T>`
  element headers. Immediate stage-free chunk sources selected for range
  materialization instead derive each header in the kernel without allocating
  the header array ([Plan32 §11](32-post-xml-consolidation-plan.md#11-o0--virtual-chunks-for-direct-explicit-parallelism)).
- `align_rt_par_map_reduce` is the specialized direct, stage-free integer
  `par_map(f).sum()` path. It allocates one 8-byte-aligned partial slot per claimed range rather
  than an element-sized output array, invokes the same caller-draining range scheduler, and combines
  the `result_stride`-wide wrapping integer partials in range order. `result_stride` is 1, 2, 4, or
  8 bytes. The kernel must store exactly one integer partial at its supplied `partial` pointer;
  worker panics are joined and resumed before partials are read. Scalar and `chunks` sources share
  this entry point; immediate stage-free chunks derive their headers in the kernel,
  while materialized chunk values retain their owned header array. This is
  not a generic parallel reduction contract: staged pipelines, floating-point sums, and arbitrary
  reducers remain outside this entry point.

---

## 6. I/O concurrency (task_group)

```text
TaskGroup* align_rt_tg_begin()
void*      align_rt_tg_alloc(TaskGroup*, i64 size, i64 align)
void       align_rt_tg_register(TaskGroup*, tramp, thunk, env, result_slot, error_slot, cleanup)
void*      align_rt_tg_wait(TaskGroup*)   // lowest-index error slot, or null
void       align_rt_tg_end(TaskGroup*)
```

I/O-wait concurrency (`draft.md` §11). Spawn snapshots captures into the task-group region; `wait`
joins registered tasks using the persistent shared pool and reports the failure from the lowest spawn index.
The caller also claims work, so nested groups can progress without a free worker.
Owned result slots supply a cleanup thunk and an initialized flag: `get()` transfers
one owned result, and group exit drops untaken results after joining. Cleanup remains
registered across waits. Copy results allow repeated reads. The result and capture
domains are fixed by [Plan228](228-task-owned-results.md). There is no async/await.

---

## 7. buffer / builder

The foundation for string output and template desugaring (`04 §2.5`).

```text
Builder* align_rt_builder_new(Arena* a, i64 capacity)  // optional arena destination
void    align_rt_builder_write(Builder*, u8* ptr, i64 len)   // static part (memcpy)
void    align_rt_builder_write_int(Builder*, i64)
void    align_rt_builder_write_f64(Builder*, f64)
{ ptr, len } align_rt_builder_finish(Builder*)       // consuming arena-view finish
{ ptr, len } align_rt_builder_into_string(Builder*)  // consuming independently owned finish
```

`template "Hello {name}"` lowers its literal parts to `builder_write` and values
to per-type writers. Known output bounds can supply initial capacity. Nonescaping
headers also have stack-storage entries; the growable byte storage remains explicit
in the runtime. Plain language templates, JSON encoding and `pkg.template` HTML
escaping retain their separate documented output policies.

---

## 8. panic / traps

```text
noreturn void align_rt_div_fail(void)
noreturn void align_rt_bounds_fail(i64 index, i64 len)
noreturn void align_rt_alloc_size_fail(void)
```

Operation-specific failure entries print a diagnostic and abort. Integer arithmetic
wraps on overflow in every profile; division by zero, failed bounds checks and invalid
allocation extents use explicit failure paths. Fatal errors have no unwind or catch
boundary. Source debug information is separate from these native signatures.

---

## 9. Entry point

```text
i32 main(i32 argc, char** argv):     // compiler-generated wrapper, not a runtime export
  reject any non-UTF-8 argument (including argv[0]) with Invalid report/exit 2
  args = allocate owned array<str> headers borrowing process-lifetime argv bytes
  r = user_main(args)               // pub fn main(args) -> Result<(), Error>
  match r:
    Ok      => return 0
    Err(e)  => report(e); return non-zero
```

The wrapper calls `align_rt_args_build` for an argv-taking entry and
`align_rt_report_error` for a failed builtin-Error result. It maps `main`'s return
to the exit code; unit success is zero. The accepted signatures and exit mappings
are in [core-design/option-result.md](core-design/option-result.md).

---

## 10. core / std bootstrap

```text
core  foundation close to the language philosophy (draft.md §18.1)
      option/result, array/slice/chunks, vec/mask/bitset,
      map/reduce/scan/partition/sort, str/string/bytes/buffer/builder,
      arena, json, template, hash, math
std   the OS boundary (draft.md §18.2)
      io/fs/path/process/env/time/net/cli/encoding/compress/rand/crypto/http
```

### Policy on the implementation language
- **Write core in Align itself as much as possible**. To make MIR's fusion (`04 §3`) work for `map`/`where`/`reduce`, the direction is to define them not as specially-handled builtins but as **normal Align generic functions + intrinsic hooks**. The lowest layer (SIMD scan, the core of hash) drops to runtime intrinsics.
- **Write std as a thin wrapper** over the runtime + OS syscalls, in Align. `fs.read_file` etc. call the runtime's I/O primitives.

### Bootstrap order (historical; all listed milestones completed)
```text
M0-M1  minimal runtime (start/arena/panic) + builtin print only. almost no core/std
M2     core.option / core.result. std.fs.read_file (a concrete example of ?)
M3     wire core.arena to the language feature
M4     core.array / slice / reduce family (the verification target for fusion)
M5     core.str/string/builder, core.json, core.template
M6     core.vec / mask
M7     parallel (par_map on the core side / task on std)
M8+    std expansion (path/env/time/net/...). pkg is out of scope (draft.md §18.3)
```

The current builtin/runtime boundary is documented by the core-design inventories and the shipped
M4–M12 records in `07-roadmap.md`; consumer-driven intrinsics remain preferable to speculative
self-hosting work.

---

## 11. Open items (to be settled)

```text
- static linking / scope of libc dependence / whether direct syscalls are allowed (common with 05 §10)
- the boundary for writing core in Align (how far to drop to intrinsics)
```

Generic parallel reduction is a future surface, not an unfinished runtime promise. Builtin `Error`
display and `main` exit-code correspondence are settled in `core-design/option-result.md`.
